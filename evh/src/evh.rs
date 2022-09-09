// Copyright (c) 2022, 37 Miners, LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::types::{
	ConnectionInfo, Event, EventHandlerContext, EventHandlerData, EventHandlerImpl, EventIn,
	EventType, EventTypeIn, Handle, ListenerInfo, ReadWriteInfo, Wakeup, WriteState,
};
use crate::{
	ClientConnection, ConnData, ConnectionData, EventHandler, EventHandlerConfig, ServerConnection,
	ThreadContext, WriteHandle,
};
use bmw_deps::errno::{errno, set_errno, Errno};
use bmw_deps::rand::random;
use bmw_err::*;
use bmw_log::*;
use bmw_util::*;
use std::cell::{Ref, RefCell};
use std::os::raw::c_void;
use std::rc::Rc;

#[cfg(target_os = "linux")]
use crate::linux::*;
#[cfg(target_os = "macos")]
use crate::mac::*;
#[cfg(windows)]
use crate::win::*;

#[cfg(target_os = "windows")]
use bmw_deps::bitvec::vec::BitVec;
#[cfg(target_os = "windows")]
use bmw_deps::wepoll_sys::{epoll_create, EPOLLIN, EPOLLONESHOT, EPOLLRDHUP};

#[cfg(target_os = "macos")]
use bmw_deps::kqueue_sys::{kevent, kqueue, EventFilter, EventFlag, FilterFlag};

#[cfg(target_os = "linux")]
use bmw_deps::bitvec::vec::BitVec;
#[cfg(target_os = "linux")]
use bmw_deps::nix::sys::epoll::{epoll_create1, EpollCreateFlags, EpollEvent, EpollFlags};

const READ_SLAB_SIZE: usize = 512;
const READ_SLAB_PTR_OFFSET: usize = 504;

const HANDLE_SLAB_SIZE: usize = 42;
const CONNECTION_SLAB_SIZE: usize = 90;

const WRITE_STATE_FLAG_PENDING: u8 = 0x1 << 0;

info!();

pub fn create_listeners(
	size: usize,
	addr: &str,
	listen_size: usize,
) -> Result<Array<Handle>, Error> {
	create_listeners_impl(size, addr, listen_size)
}

impl Default for Event {
	fn default() -> Self {
		Self {
			handle: 0,
			etype: EventType::Read,
		}
	}
}

impl Default for EventIn {
	fn default() -> Self {
		Self {
			handle: 0,
			etype: EventTypeIn::Read,
		}
	}
}

impl Default for EventHandlerConfig {
	fn default() -> Self {
		Self {
			threads: 6,
			sync_channel_size: 10,
			write_queue_size: 100_000,
			nhandles_queue_size: 1_000,
			events_per_batch: 100,
			max_events_in: 1_000,
			max_events: 100,
			housekeeping_frequency_millis: 1_000,
			read_slab_count: 1_000,
			max_handles_per_thread: 1_000,
		}
	}
}

// Note about serialization of ConnectionInfo: We serialize the write state
// which is a LockBox. So we use the danger_to_usize fn. It must be deserialized
// once per serialization or it will leak.
impl Serializable for ConnectionInfo {
	fn read<R>(reader: &mut R) -> Result<Self, Error>
	where
		R: Reader,
	{
		match reader.read_u8()? {
			0 => Ok(ConnectionInfo::ListenerInfo(ListenerInfo::read(reader)?)),
			1 => {
				let id = reader.read_u128()?;
				let handle = Handle::read(reader)?;
				let accept_handle: Option<Handle> = Option::read(reader)?;
				let write_state: Box<dyn LockBox<WriteState>> =
					lock_box_from_usize(reader.read_usize()?);
				let first_slab = reader.read_usize()?;
				let last_slab = reader.read_usize()?;
				let slab_offset = reader.read_u16()?;
				Ok(ConnectionInfo::ReadWriteInfo(ReadWriteInfo {
					id,
					handle,
					accept_handle,
					write_state,
					first_slab,
					last_slab,
					slab_offset,
				}))
			}
			_ => Err(err!(
				ErrKind::CorruptedData,
				"Unexpected type in ConnectionInfo"
			)),
		}
	}
	fn write<W>(&self, writer: &mut W) -> Result<(), Error>
	where
		W: Writer,
	{
		match self {
			ConnectionInfo::ListenerInfo(li) => {
				writer.write_u8(0)?;
				li.write(writer)?;
			}
			ConnectionInfo::ReadWriteInfo(ri) => {
				writer.write_u8(1)?;
				writer.write_u128(ri.id)?;
				ri.handle.write(writer)?;
				ri.accept_handle.write(writer)?;
				writer.write_usize(ri.write_state.danger_to_usize())?;
				writer.write_usize(ri.first_slab)?;
				writer.write_usize(ri.last_slab)?;
				writer.write_u16(ri.slab_offset)?;
			}
		}

		Ok(())
	}
}

impl ReadWriteInfo {
	fn clear_through_impl(
		&mut self,
		slab_id: usize,
		slabs: &Rc<RefCell<dyn SlabAllocator>>,
	) -> Result<(), Error> {
		debug!("clear through impl")?;
		let mut next = self.first_slab;
		let mut slabs = slabs.borrow_mut();
		loop {
			if next == usize::MAX {
				break;
			}

			let next_slab = usize::from_be_bytes(try_into!(
				&slabs.get(next)?.get()[READ_SLAB_PTR_OFFSET..READ_SLAB_SIZE]
			)?);
			debug!("free {}", next)?;
			slabs.free(next)?;

			if next == slab_id {
				self.first_slab = next_slab;
				if next_slab == usize::MAX {
					self.last_slab = usize::MAX;
				}
				break;
			}
			next = next_slab;
		}

		Ok(())
	}
}

impl ThreadContext {
	fn new() -> Self {
		Self {}
	}
}

impl EventHandlerContext {
	fn new(
		tid: usize,
		events_per_batch: usize,
		max_events_in: usize,
		max_events: usize,
		max_handles_per_thread: usize,
		read_slab_count: usize,
	) -> Result<Self, Error> {
		if max_events > events_per_batch {
			return Err(err!(
				ErrKind::Configuration,
				"max_events must be less than or equal to events_per_batch"
			));
		}
		let events = array!(events_per_batch, &Event::default())?;
		let events_in = array!(max_events_in, &EventIn::default())?;

		#[cfg(target_os = "macos")]
		let mut ret_kevs = vec![];
		#[cfg(target_os = "macos")]
		for _ in 0..max_events {
			ret_kevs.push(kevent::new(
				0,
				EventFilter::EVFILT_SYSCOUNT,
				EventFlag::empty(),
				FilterFlag::empty(),
			));
		}

		#[cfg(target_os = "linux")]
		let mut filter_set: BitVec = BitVec::with_capacity(max_handles_per_thread + 100);
		#[cfg(target_os = "linux")]
		filter_set.resize(max_handles_per_thread + 100, false);

		#[cfg(target_os = "windows")]
		let mut filter_set: BitVec = BitVec::with_capacity(max_handles_per_thread + 100);
		#[cfg(target_os = "windows")]
		filter_set.resize(max_handles_per_thread + 100, false);

		#[cfg(target_os = "windows")]
		for i in 0..(max_handles_per_thread + 100) {
			filter_set.set(i, false);
		}

		#[cfg(target_os = "linux")]
		for i in 0..(max_handles_per_thread + 100) {
			filter_set.set(i, false);
		}

		let _handle_slabs = slab_allocator!(
			SlabSize(HANDLE_SLAB_SIZE),
			SlabCount(max_handles_per_thread)
		)?;
		let _connection_slabs = slab_allocator!(
			SlabSize(CONNECTION_SLAB_SIZE),
			SlabCount(max_handles_per_thread)
		)?;
		let read_slabs = slab_allocator!(SlabSize(READ_SLAB_SIZE), SlabCount(read_slab_count))?;
		let handle_hashtable = hashtable_box!(Slabs(&_handle_slabs))?;
		let connection_hashtable = hashtable_box!(Slabs(&_connection_slabs))?;

		#[cfg(target_os = "linux")]
		let epoll_events = {
			let mut epoll_events = vec![];
			epoll_events.resize(max_events, EpollEvent::new(EpollFlags::empty(), 0));
			epoll_events
		};

		Ok(EventHandlerContext {
			connection_hashtable,
			handle_hashtable,
			events,
			events_in,
			events_in_count: 0,
			tid,
			now: 0,
			#[cfg(target_os = "linux")]
			filter_set,
			#[cfg(target_os = "windows")]
			filter_set,
			#[cfg(target_os = "macos")]
			kevs: vec![],
			#[cfg(target_os = "macos")]
			ret_kevs,
			#[cfg(target_os = "macos")]
			selector: unsafe { kqueue() },
			#[cfg(target_os = "linux")]
			selector: epoll_create1(EpollCreateFlags::empty())?,
			#[cfg(target_os = "linux")]
			epoll_events,
			#[cfg(windows)]
			selector: unsafe { epoll_create(1) } as u64,
			read_slabs,
			_handle_slabs,
			_connection_slabs,
			callback_context: ThreadContext::new(),
		})
	}
}

impl WriteState {
	fn set_flag(&mut self, flag: u8) {
		self.flags |= flag;
	}

	fn unset_flag(&mut self, flag: u8) {
		self.flags &= !flag;
	}

	fn is_set(&self, flag: u8) -> bool {
		self.flags & flag != 0
	}
}

impl WriteHandle {
	fn new(
		handle: Handle,
		id: u128,
		wakeup: Wakeup,
		write_state: Box<dyn LockBox<WriteState>>,
		event_handler_data: Box<dyn LockBox<EventHandlerData>>,
	) -> Self {
		Self {
			handle,
			id,
			wakeup,
			write_state,
			event_handler_data,
		}
	}
	pub fn close(&self) -> Result<(), Error> {
		todo!()
	}
	pub fn write(&mut self, data: &[u8]) -> Result<(), Error> {
		let data_len = data.len();
		let len = {
			let write_state = self.write_state.rlock()?;
			if (**write_state.guard()).is_set(WRITE_STATE_FLAG_PENDING) {
				0
			} else {
				write_bytes(self.handle, data)
			}
		};

		if len < 0 {
			return Err(err!(
				ErrKind::IO,
				format!("writing generated error: {}", errno())
			));
		}
		let len: usize = len.try_into()?;
		if len < data_len {
			self.queue_data(&data[len..])?;
		}
		Ok(())
	}
	pub fn trigger_on_read(&self) -> Result<(), Error> {
		todo!()
	}

	fn queue_data(&mut self, data: &[u8]) -> Result<(), Error> {
		let was_pending = {
			let mut write_state = self.write_state.wlock()?;
			let guard = write_state.guard();
			let ret = (**guard).is_set(WRITE_STATE_FLAG_PENDING);
			(**guard).set_flag(WRITE_STATE_FLAG_PENDING);
			(**guard).write_buffer.extend(data);
			ret
		};
		if !was_pending {
			let mut event_handler_data = self.event_handler_data.wlock()?;
			let guard = event_handler_data.guard();
			(**guard).write_queue.enqueue(self.id)?;
		}
		self.wakeup.wakeup()?;

		Ok(())
	}
}

impl<'a> ConnectionData<'a> {
	fn new(
		rwi: &'a mut ReadWriteInfo,
		tid: usize,
		slabs: Rc<RefCell<dyn SlabAllocator>>,
		wakeup: Wakeup,
		event_handler_data: Box<dyn LockBox<EventHandlerData>>,
	) -> Self {
		Self {
			rwi,
			tid,
			slabs,
			wakeup,
			event_handler_data,
		}
	}

	fn clear_through_impl(&mut self, slab_id: usize) -> Result<(), Error> {
		self.rwi.clear_through_impl(slab_id, &self.slabs)
		/*
		let mut next = self.rwi.first_slab;
		let mut slabs = self.slabs.borrow_mut();
		loop {
			if next == usize::MAX {
				break;
			}

			let next_slab = usize::from_be_bytes(try_into!(
				&slabs.get(next)?.get()[READ_SLAB_PTR_OFFSET..READ_SLAB_SIZE]
			)?);
			slabs.free(next)?;

			if next == slab_id {
				self.rwi.first_slab = next_slab;
				if next_slab == usize::MAX {
					self.rwi.last_slab = usize::MAX;
				}
				break;
			}
			next = next_slab;
		}

		Ok(())
				*/
	}
}

impl<'a> ConnData for ConnectionData<'a> {
	fn tid(&self) -> usize {
		self.tid
	}
	fn get_connection_id(&self) -> u128 {
		self.rwi.id
	}
	fn get_handle(&self) -> Handle {
		self.rwi.handle
	}
	fn get_accept_handle(&self) -> Option<Handle> {
		self.rwi.accept_handle
	}
	fn write_handle(&self) -> WriteHandle {
		WriteHandle::new(
			self.rwi.handle,
			self.rwi.id,
			self.wakeup.clone(),
			self.rwi.write_state.clone(),
			self.event_handler_data.clone(),
		)
	}
	fn borrow_slab_allocator<F, T>(&self, mut f: F) -> Result<T, Error>
	where
		F: FnMut(Ref<dyn SlabAllocator>) -> Result<T, Error>,
	{
		let slabs = self.slabs.borrow();
		f(slabs)
	}
	fn slab_offset(&self) -> u16 {
		self.rwi.slab_offset
	}
	fn first_slab(&self) -> usize {
		self.rwi.first_slab
	}
	fn last_slab(&self) -> usize {
		self.rwi.last_slab
	}
	fn clear_through(&mut self, slab_id: usize) -> Result<(), Error> {
		self.clear_through_impl(slab_id)
	}
}

impl EventHandlerData {
	fn new(write_queue_size: usize, nhandles_queue_size: usize) -> Result<Self, Error> {
		let connection_info = ConnectionInfo::ListenerInfo(ListenerInfo { handle: 0, id: 0 });

		let evhd = EventHandlerData {
			write_queue: queue_sync_box!(write_queue_size, &0)?,
			nhandles: queue_sync_box!(nhandles_queue_size, &connection_info)?,
			stop: false,
		};
		Ok(evhd)
	}
}

impl<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
	EventHandlerImpl<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
where
	OnRead: FnMut(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	OnAccept: FnMut(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	OnClose: FnMut(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	HouseKeeper:
		FnMut(&mut ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
	OnPanic: FnMut(&mut ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
{
	pub(crate) fn new(config: EventHandlerConfig) -> Result<Self, Error> {
		let mut data = array!(config.threads, &lock_box!(EventHandlerData::new(1, 1,)?)?)?;
		for i in 0..config.threads {
			data[i] = lock_box!(EventHandlerData::new(
				config.write_queue_size,
				config.nhandles_queue_size
			)?)?;
		}
		let mut wakeup = array!(config.threads, &Wakeup::new()?)?;
		for i in 0..config.threads {
			wakeup[i] = Wakeup::new()?;
		}

		Ok(Self {
			on_read: None,
			on_accept: None,
			on_close: None,
			housekeeper: None,
			on_panic: None,
			config,
			data,
			wakeup,
		})
	}

	fn execute_thread(&mut self, tid: usize, wakeup: &mut Wakeup) -> Result<(), Error> {
		debug!("Executing thread {}", tid)?;
		let mut ctx = EventHandlerContext::new(
			tid,
			self.config.events_per_batch,
			self.config.max_events_in,
			self.config.max_events,
			self.config.max_handles_per_thread,
			self.config.read_slab_count,
		)?;

		// add wakeup
		let handle = wakeup.reader;
		debug!("wakeup handle is {}", handle)?;
		ctx.events_in[ctx.events_in_count] = EventIn {
			handle,
			etype: EventTypeIn::Read,
		};
		ctx.events_in_count += 1;

		debug!("pre_loop")?;
		loop {
			debug!("start loop")?;
			self.process_new_connections(&mut ctx)?;
			self.process_write_queue(&mut ctx)?;
			let count = {
				debug!("calling get_events")?;
				let count = {
					let (requested, _lock) = wakeup.pre_block()?;
					self.get_events(&mut ctx, requested)?
				};
				debug!("get_events returned with {} event", count)?;
				wakeup.post_block()?;
				count
			};
			self.process_events(&mut ctx, count, wakeup)?;
		}
	}

	fn process_write_queue(&mut self, ctx: &mut EventHandlerContext) -> Result<(), Error> {
		let mut data = self.data[ctx.tid].wlock()?;
		loop {
			let guard = data.guard();
			match (**guard).write_queue.dequeue() {
				Some(next) => match ctx.connection_hashtable.get(&next)? {
					Some(ci) => match ci {
						ConnectionInfo::ReadWriteInfo(rwi) => {
							let handle = rwi.handle;
							ctx.events_in[ctx.events_in_count] = EventIn {
								handle,
								etype: EventTypeIn::Write,
							};
							ctx.events_in_count += 1;
						}
						_ => todo!(),
					},
					None => todo!(),
				},
				None => break,
			}
		}
		Ok(())
	}

	fn process_new_connections(&mut self, ctx: &mut EventHandlerContext) -> Result<(), Error> {
		let mut data = self.data[ctx.tid].wlock()?;
		let guard = data.guard();
		debug!(
			"ctx.nhandles.size={},ctx.tid={}",
			(**guard).nhandles.length(),
			ctx.tid
		)?;
		loop {
			let next = (**guard).nhandles.dequeue();
			match next {
				Some(nhandle) => {
					debug!("handle={:?}", nhandle)?;
					match nhandle {
						ConnectionInfo::ListenerInfo(li) => {
							let id = random();
							ctx.events_in[ctx.events_in_count] = EventIn {
								handle: li.handle,
								etype: EventTypeIn::Read,
							};
							ctx.events_in_count += 1;
							ctx.connection_hashtable.insert(&id, nhandle)?;
							ctx.handle_hashtable.insert(&li.handle, &id)?;
						}
						ConnectionInfo::ReadWriteInfo(_rw) => {}
					}
				}
				None => break,
			}
		}
		Ok(())
	}

	fn process_events(
		&mut self,
		ctx: &mut EventHandlerContext,
		count: usize,
		wakeup: &Wakeup,
	) -> Result<(), Error> {
		debug!("process {} events", count)?;
		for i in 0..count {
			debug!("event={:?}", ctx.events[i])?;
			if ctx.events[i].handle == wakeup.reader {
				debug!("WAKEUP, handle={}, tid={}", wakeup.reader, ctx.tid)?;
				read_bytes(ctx.events[i].handle, &mut [0u8; 1]);
				continue;
			}
			match ctx.handle_hashtable.get(&ctx.events[i].handle)? {
				Some(id) => match ctx.connection_hashtable.get(&id)? {
					Some(ci) => match ci {
						ConnectionInfo::ListenerInfo(li) => {
							self.process_accept(li, ctx)?;
						}
						ConnectionInfo::ReadWriteInfo(rw) => match ctx.events[i].etype {
							EventType::Read => self.process_read(rw, ctx)?,
							EventType::Write => self.process_write(rw, ctx)?,
							_ => {}
						},
					},
					None => todo!(),
				},
				None => {
					todo!()
				}
			}
		}
		Ok(())
	}

	fn process_write(
		&mut self,
		mut rw: ReadWriteInfo,
		ctx: &mut EventHandlerContext,
	) -> Result<(), Error> {
		let mut do_close = false;

		{
			let mut write_state = rw.write_state.wlock()?;
			let guard = write_state.guard();
			loop {
				let len = (**guard).write_buffer.len();
				if len == 0 {
					(**guard).unset_flag(WRITE_STATE_FLAG_PENDING);
					break;
				}
				let wlen = write_bytes(rw.handle, &(**guard).write_buffer);
				if wlen < 0 {
					do_close = true;
					break;
				}
				(**guard).write_buffer.drain(0..wlen as usize);
			}
		}

		if do_close {
			self.process_close(ctx, &mut rw)?;
		}
		Ok(())
	}

	fn process_read(
		&mut self,
		mut rw: ReadWriteInfo,
		ctx: &mut EventHandlerContext,
	) -> Result<(), Error> {
		debug!("process read")?;
		let mut do_close = false;
		let mut total_len = 0;
		loop {
			// read as many slabs as we can
			let mut slabs = ctx.read_slabs.borrow_mut();
			let mut slab =
				if rw.last_slab == usize::MAX || rw.slab_offset as usize >= READ_SLAB_PTR_OFFSET {
					let mut slab = slabs.allocate()?;
					let slab_id = slab.id();
					if rw.first_slab == usize::MAX {
						rw.first_slab = slab_id;
					}
					slab.get_mut()[READ_SLAB_PTR_OFFSET..READ_SLAB_SIZE]
						.clone_from_slice(&rw.last_slab.to_be_bytes());
					rw.last_slab = slab_id;
					rw.slab_offset = 0;

					slab
				} else {
					slabs.get_mut(rw.last_slab)?
				};
			let len = if usize!(rw.slab_offset) < READ_SLAB_PTR_OFFSET {
				read_bytes(
					rw.handle,
					&mut slab.get_mut()[usize!(rw.slab_offset)..READ_SLAB_PTR_OFFSET],
				)
			} else {
				rw.slab_offset = 0;
				read_bytes(
					rw.handle,
					&mut slab.get_mut()[usize!(rw.slab_offset)..READ_SLAB_PTR_OFFSET],
				)
			};
			if len == 0 {
				do_close = true;
			}
			if len <= 0 {
				epoll_ctl_impl(
					EPOLLIN | EPOLLONESHOT | EPOLLRDHUP,
					rw.handle,
					&mut ctx.filter_set,
					ctx.selector as *mut c_void,
				)?;

				break;
			}

			total_len += len;
			rw.slab_offset += len as u16;
		}

		if total_len > 0 {
			match &mut self.on_read {
				Some(on_read) => {
					match on_read(
						&mut ConnectionData::new(
							&mut rw,
							ctx.tid,
							ctx.read_slabs.clone(),
							self.wakeup[ctx.tid].clone(),
							self.data[ctx.tid].clone(),
						),
						&mut ctx.callback_context,
					) {
						Ok(_) => {}
						Err(e) => {
							warn!("Callback on_read generated error: {}", e)?;
						}
					}
				}
				None => {}
			}
		}

		if do_close {
			// update rw
			let id = rw.id;
			ctx.connection_hashtable
				.insert(&id, &ConnectionInfo::ReadWriteInfo(rw.clone()))?;
			self.process_close(ctx, &mut rw)?;
		} else {
			// just update hashtable
			let id = rw.id;
			ctx.connection_hashtable
				.insert(&id, &ConnectionInfo::ReadWriteInfo(rw))?;
		}

		Ok(())
	}

	fn process_close(
		&mut self,
		ctx: &mut EventHandlerContext,
		rw: &mut ReadWriteInfo,
	) -> Result<(), Error> {
		debug!("process close for {}/{}", rw.handle, rw.id)?;
		match &mut self.on_close {
			Some(on_close) => {
				match on_close(
					&mut ConnectionData::new(
						rw,
						ctx.tid,
						ctx.read_slabs.clone(),
						self.wakeup[ctx.tid].clone(),
						self.data[ctx.tid].clone(),
					),
					&mut ctx.callback_context,
				) {
					Ok(_) => {}
					Err(e) => {
						warn!("Callback on_read generated error: {}", e)?;
					}
				}
			}
			None => {}
		}

		match ctx.connection_hashtable.remove(&rw.id)? {
			Some(ci) => match ci {
				ConnectionInfo::ReadWriteInfo(mut rwi) => {
					close_impl(ctx, rwi.handle)?;
					ctx.connection_hashtable.remove(&rwi.id)?;
					ctx.handle_hashtable.remove(&rwi.handle)?;
					rwi.clear_through_impl(rwi.last_slab, &ctx.read_slabs)?;
				}
				_ => todo!(),
			},
			None => {
				// already closed
			}
		}

		Ok(())
	}

	fn process_accept(
		&mut self,
		li: ListenerInfo,
		ctx: &mut EventHandlerContext,
	) -> Result<(), Error> {
		info!("process accept");
		set_errno(Errno(0));
		let handle = accept_impl(li.handle)?;
		debug!("accept handle = {},tid={}", handle, ctx.tid)?;
		let id = random();
		let mut rwi = ReadWriteInfo {
			id,
			handle,
			accept_handle: Some(li.handle),
			write_state: lock_box!(WriteState {
				write_buffer: vec![],
				flags: 0
			})?,
			first_slab: usize::MAX,
			last_slab: usize::MAX,
			slab_offset: 0,
		};

		ctx.events_in[ctx.events_in_count] = EventIn {
			handle,
			etype: EventTypeIn::Read,
		};
		ctx.events_in_count += 1;
		match &mut self.on_accept {
			Some(on_accept) => {
				match on_accept(
					&mut ConnectionData::new(
						&mut rwi,
						ctx.tid,
						ctx.read_slabs.clone(),
						self.wakeup[ctx.tid].clone(),
						self.data[ctx.tid].clone(),
					),
					&mut ctx.callback_context,
				) {
					Ok(_) => {}
					Err(e) => {
						warn!("Callback on_read generated error: {}", e)?;
					}
				}
			}
			None => {}
		}

		epoll_ctl_impl(
			EPOLLIN | EPOLLONESHOT | EPOLLRDHUP,
			li.handle,
			&mut ctx.filter_set,
			ctx.selector as *mut c_void,
		)?;

		ctx.connection_hashtable
			.insert(&id, &ConnectionInfo::ReadWriteInfo(rwi))?;
		ctx.handle_hashtable.insert(&handle, &id)?;

		Ok(())
	}

	fn get_events(&self, ctx: &mut EventHandlerContext, requested: bool) -> Result<usize, Error> {
		get_events_impl(&self.config, ctx, requested)
	}
}

impl<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
	EventHandler<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
	for EventHandlerImpl<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
where
	OnRead: FnMut(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	OnAccept: FnMut(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	OnClose: FnMut(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	HouseKeeper:
		FnMut(&mut ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
	OnPanic: FnMut(&mut ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
{
	fn set_on_read(&mut self, on_read: OnRead) -> Result<(), Error> {
		self.on_read = Some(Box::pin(on_read));
		Ok(())
	}
	fn set_on_accept(&mut self, on_accept: OnAccept) -> Result<(), Error> {
		self.on_accept = Some(Box::pin(on_accept));
		Ok(())
	}
	fn set_on_close(&mut self, on_close: OnClose) -> Result<(), Error> {
		self.on_close = Some(Box::pin(on_close));
		Ok(())
	}
	fn set_housekeeper(&mut self, housekeeper: HouseKeeper) -> Result<(), Error> {
		self.housekeeper = Some(Box::pin(housekeeper));
		Ok(())
	}
	fn set_on_panic(&mut self, on_panic: OnPanic) -> Result<(), Error> {
		self.on_panic = Some(Box::pin(on_panic));
		Ok(())
	}
	fn stop(&mut self) -> Result<(), Error> {
		todo!()
	}
	fn start(&mut self) -> Result<(), Error> {
		let tid = lock!(0)?;
		let tp = thread_pool!(
			MaxSize(self.config.threads),
			MinSize(self.config.threads),
			SyncChannelSize(self.config.sync_channel_size)
		)?;

		for _ in 0..self.config.threads {
			let mut tid = tid.clone();
			let mut evh = self.clone();
			let mut wakeup = self.wakeup.clone();
			execute!(tp, {
				let tid = {
					let mut l = tid.wlock()?;
					let guard = l.guard();
					let tid = **guard;
					(**guard) += 1;
					tid
				};
				match Self::execute_thread(&mut evh, tid, &mut wakeup[tid]) {
					Ok(_) => {}
					Err(e) => {
						fatal!("execute_thread generated error: {}", e)?;
					}
				}
				Ok(())
			})?;
		}

		Ok(())
	}

	fn add_client(&mut self, _connection: ClientConnection) -> Result<ConnectionData, Error> {
		todo!()
	}
	fn add_server(&mut self, connection: ServerConnection) -> Result<(), Error> {
		debug!("add server")?;
		if connection.handles.size() != self.data.size() {
			return Err(err!(
				ErrKind::IllegalArgument,
				"connections.handles must equal the number of threads"
			));
		}

		for i in 0..connection.handles.size() {
			let handle = connection.handles[i];
			if handle == 0 {
				// windows/mac do not support reusing address so we pass in 0
				// to indicate no handle
				continue;
			}

			let mut data = self.data[i].wlock()?;
			let wakeup = &mut self.wakeup[i];
			let guard = data.guard();
			(**guard)
				.nhandles
				.enqueue(ConnectionInfo::ListenerInfo(ListenerInfo {
					id: random(),
					handle,
				}))?;
			debug!("add handle: {}", handle)?;
			wakeup.wakeup()?;
		}
		Ok(())
	}
}

// always obtain requested lock before needed, pre_block is only called in one thread so it can
// only be executing once at most so the read lock is sufficient since both other calls get the
// write lock as the first action.
impl Wakeup {
	fn new() -> Result<Self, Error> {
		set_errno(Errno(0));
		let (reader, writer, _tcp_stream, _tcp_listener) = get_reader_writer()?;
		Ok(Self {
			_tcp_stream,
			_tcp_listener,
			reader,
			writer,
			requested: lock_box!(false)?,
			needed: lock_box!(false)?,
		})
	}

	fn wakeup(&mut self) -> Result<(), Error> {
		let mut requested = self.requested.wlock()?;
		let needed = self.needed.rlock()?;
		let need_wakeup = **needed.guard() && !(**requested.guard());
		**requested.guard() = true;
		if need_wakeup {
			debug!("writing to {}", self.writer)?;
			let len = write_bytes(self.writer, &[0u8; 1]);
			debug!("len={},errno={}", len, errno())?;
		}
		Ok(())
	}

	fn pre_block(&mut self) -> Result<(bool, RwLockReadGuardWrapper<bool>), Error> {
		let requested = self.requested.rlock()?;
		{
			let mut needed = self.needed.wlock()?;
			**needed.guard() = true;
		}
		let lock_guard = self.needed.rlock()?;
		let is_requested = **requested.guard();
		Ok((is_requested, lock_guard))
	}

	fn post_block(&mut self) -> Result<(), Error> {
		let mut requested = self.requested.wlock()?;
		let mut needed = self.needed.wlock()?;

		**requested.guard() = false;
		**needed.guard() = false;
		Ok(())
	}
}

fn read_bytes(handle: Handle, buf: &mut [u8]) -> isize {
	set_errno(Errno(0));
	read_bytes_impl(handle, buf)
}

fn write_bytes(handle: Handle, buf: &[u8]) -> isize {
	set_errno(Errno(0));
	let _ = debug!("write bytes to handle = {}", handle);
	write_bytes_impl(handle, buf)
}

#[cfg(test)]
mod test {
	use crate::evh::{create_listeners, errno, read_bytes};
	use crate::types::{EventHandlerImpl, Wakeup};
	use crate::{ConnData, EventHandler, EventHandlerConfig, ServerConnection};
	use bmw_err::*;
	use bmw_log::*;
	use bmw_test::port::pick_free_port;
	use bmw_util::*;
	use std::io::Read;
	use std::io::Write;
	use std::net::TcpStream;
	use std::thread::sleep;
	use std::time::Duration;

	info!();

	#[test]
	fn test_wakeup() -> Result<(), Error> {
		let check = lock!(0)?;
		let mut check_clone = check.clone();
		let mut wakeup = Wakeup::new()?;
		let wakeup_clone = wakeup.clone();

		std::thread::spawn(move || -> Result<(), Error> {
			let mut wakeup = wakeup_clone;
			{
				let wakeup_clone = wakeup.clone();

				loop {
					let len;
					{
						let (_requested, _lock) = wakeup.pre_block()?;
						let mut buffer = [0u8; 1];
						info!("reader = {}", wakeup_clone.reader)?;
						info!("writer = {}", wakeup_clone.writer)?;

						len = read_bytes(wakeup_clone.reader, &mut buffer);
						if len == 1 {
							break;
						}
					}
					sleep(Duration::from_millis(1_000));
					info!("len={},err={}", len, errno())?;
				}
			}
			wakeup.post_block()?;

			let mut check = check_clone.wlock()?;
			**check.guard() = 1;

			Ok(())
		});

		sleep(Duration::from_millis(3_000));
		wakeup.wakeup()?;

		loop {
			std::thread::sleep(std::time::Duration::from_millis(1));
			let check = check.rlock()?;
			if **(check).guard() == 1 {
				break;
			}
		}
		Ok(())
	}

	#[test]
	fn test_eventhandler_basic() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 100_000,
			read_slab_count: 1,
			max_handles_per_thread: 2,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		evh.set_on_read(move |conn_data, _thread_context| {
			info!("on read fs = {}", conn_data.slab_offset())?;
			let first_slab = conn_data.first_slab();
			let slab_offset = conn_data.slab_offset();
			let res = conn_data.borrow_slab_allocator(move |sa| {
				let slab = sa.get(first_slab)?;
				info!("read bytes = {:?}", &slab.get()[0..slab_offset as usize])?;
				let mut ret: Vec<u8> = vec![];
				ret.extend(&slab.get()[0..slab_offset as usize]);
				Ok(ret)
			})?;
			conn_data.clear_through(first_slab)?;
			conn_data.write_handle().write(&res)?;
			info!("res={:?}", res)?;
			Ok(())
		})?;
		evh.set_on_accept(move |conn_data, _thread_context| {
			info!("accept a connection handle = {}", conn_data.get_handle())?;
			Ok(())
		})?;
		evh.set_on_close(move |_conn_data, _thread_context| Ok(()))?;
		evh.set_on_panic(move |_thread_context| Ok(()))?;
		evh.set_housekeeper(move |_thread_context| Ok(()))?;

		evh.start()?;
		let handles = create_listeners(threads, addr, 10)?;
		info!("handles.size={},handles={:?}", handles.size(), handles)?;
		let sc = ServerConnection {
			tls_config: None,
			handles,
		};
		evh.add_server(sc)?;

		let mut connection = TcpStream::connect(addr)?;
		info!("about to write")?;
		connection.write(b"test1")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		info!("about to read")?;
		let len = connection.read(&mut buf)?;
		assert_eq!(&buf[0..len], b"test1");
		info!("read back buf[{}] = {:?}", len, buf)?;
		connection.write(b"test2")?;
		let len = connection.read(&mut buf)?;
		assert_eq!(&buf[0..len], b"test2");

		Ok(())
	}

	#[test]
	fn test_eventhandler_close() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 100_000,
			read_slab_count: 1,
			max_handles_per_thread: 2,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		let mut close_count = lock_box!(0)?;
		let close_count_clone = close_count.clone();

		evh.set_on_read(move |conn_data, _thread_context| {
			info!("on read fs = {}", conn_data.slab_offset())?;
			let first_slab = conn_data.first_slab();
			let slab_offset = conn_data.slab_offset();
			let res = conn_data.borrow_slab_allocator(move |sa| {
				let slab = sa.get(first_slab)?;
				info!("read bytes = {:?}", &slab.get()[0..slab_offset as usize])?;
				let mut ret: Vec<u8> = vec![];
				ret.extend(&slab.get()[0..slab_offset as usize]);
				Ok(ret)
			})?;
			conn_data.clear_through(first_slab)?;
			conn_data.write_handle().write(&res)?;
			info!("res={:?}", res)?;
			Ok(())
		})?;
		evh.set_on_accept(move |conn_data, _thread_context| {
			info!("accept a connection handle = {}", conn_data.get_handle())?;
			Ok(())
		})?;
		evh.set_on_close(move |conn_data, _thread_context| {
			info!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
			let mut close_count = close_count.wlock()?;
			(**close_count.guard()) += 1;
			Ok(())
		})?;
		evh.set_on_panic(move |_thread_context| Ok(()))?;
		evh.set_housekeeper(move |_thread_context| Ok(()))?;

		evh.start()?;
		let handles = create_listeners(threads, addr, 10)?;
		info!("handles.size={},handles={:?}", handles.size(), handles)?;
		let sc = ServerConnection {
			tls_config: None,
			handles,
		};
		evh.add_server(sc)?;

		{
			let mut connection = TcpStream::connect(addr)?;
			connection.write(b"test1")?;
			let mut buf = vec![];
			buf.resize(100, 0u8);
			let len = connection.read(&mut buf)?;
			assert_eq!(&buf[0..len], b"test1");
			connection.write(b"test2")?;
			let len = connection.read(&mut buf)?;
			assert_eq!(&buf[0..len], b"test2");
		}
		info!("here");
		for i in 0..10 {
			info!("i={}", i);
			let mut connection = TcpStream::connect(addr)?;
			connection.write(b"test1")?;
			info!("1");
			let mut buf = vec![];
			buf.resize(100, 0u8);
			let len = connection.read(&mut buf)?;
			info!("2");
			assert_eq!(&buf[0..len], b"test1");
			connection.write(b"test2")?;
			info!("3");
			let len = connection.read(&mut buf)?;
			info!("4");
			assert_eq!(&buf[0..len], b"test2");
		}

		let mut count_count = 0;
		loop {
			sleep(Duration::from_millis(1));
			let count = **((close_count_clone.rlock()?).guard());
			if count != 11 {
				count_count += 1;
				if count_count < 1_000 {
					continue;
				}
			}
			assert_eq!((**((close_count_clone.rlock()?).guard())), 11);
			break;
		}

		Ok(())
	}

	#[test]
	fn test_arc_to_ptr() -> Result<(), Error> {
		use std::sync::Arc;
		let x = Arc::new(10u32);

		let ptr = Arc::into_raw(x);
		let ptr_as_usize = ptr as usize;
		info!("ptr_as_usize = {}", ptr_as_usize)?;
		let ptr_ret = unsafe { Arc::from_raw(ptr_as_usize as *mut u32) };
		info!("ptr_val={:?}", ptr_ret)?;

		Ok(())
	}
}
