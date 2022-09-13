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
	EventType, EventTypeIn, Handle, LastProcessType, ListenerInfo, ReadWriteInfo, Wakeup,
	WriteState,
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
use std::sync::{Arc, Mutex};

#[cfg(target_os = "linux")]
use crate::linux::*;
#[cfg(target_os = "macos")]
use crate::mac::*;
#[cfg(windows)]
use crate::win::*;

#[cfg(target_os = "windows")]
use bmw_deps::bitvec::vec::BitVec;
#[cfg(target_os = "windows")]
use bmw_deps::wepoll_sys::{epoll_create, EPOLLIN, EPOLLONESHOT, EPOLLOUT, EPOLLRDHUP};
#[cfg(target_os = "windows")]
use std::os::raw::c_void;

#[cfg(target_os = "macos")]
use bmw_deps::kqueue_sys::{kevent, kqueue, EventFilter, EventFlag, FilterFlag};

#[cfg(target_os = "linux")]
use bmw_deps::bitvec::vec::BitVec;
#[cfg(target_os = "linux")]
use bmw_deps::nix::sys::epoll::{epoll_create1, EpollCreateFlags, EpollEvent, EpollFlags};

const READ_SLAB_SIZE: usize = 512;
const READ_SLAB_NEXT_OFFSET: usize = 508;
pub const READ_SLAB_DATA_SIZE: usize = 508;

const HANDLE_SLAB_SIZE: usize = 42;
const CONNECTION_SLAB_SIZE: usize = 90;
#[cfg(target_os = "windows")]
const WRITE_SET_SIZE: usize = 42;

const WRITE_STATE_FLAG_PENDING: u8 = 0x1 << 0;
const WRITE_STATE_FLAG_CLOSE: u8 = 0x1 << 1;
const WRITE_STATE_FLAG_TRIGGER_ON_READ: u8 = 0x1 << 2;

const EAGAIN: i32 = 11;
const ETEMPUNAVAILABLE: i32 = 35;
const WINNONBLOCKING: i32 = 10035;

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
// once per serialization or it will leak and cause other memory related problems.
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
				let first_slab = reader.read_u32()?;
				let last_slab = reader.read_u32()?;
				let slab_offset = reader.read_u16()?;
				let is_accepted = reader.read_u8()? != 0;
				debug!("deser for id = {}", id)?;
				Ok(ConnectionInfo::ReadWriteInfo(ReadWriteInfo {
					id,
					handle,
					accept_handle,
					write_state,
					first_slab,
					last_slab,
					slab_offset,
					is_accepted,
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
				debug!("ser for id={}", ri.id)?;
				writer.write_u8(1)?;
				writer.write_u128(ri.id)?;
				ri.handle.write(writer)?;
				ri.accept_handle.write(writer)?;
				writer.write_usize(ri.write_state.danger_to_usize())?;
				writer.write_u32(ri.first_slab)?;
				writer.write_u32(ri.last_slab)?;
				writer.write_u16(ri.slab_offset)?;
				writer.write_u8(match ri.is_accepted {
					true => 1,
					false => 0,
				})?;
			}
		}

		Ok(())
	}
}

impl ReadWriteInfo {
	fn clear_through_impl(
		&mut self,
		slab_id: u32,
		slabs: &mut Box<dyn SlabAllocator + Send + Sync>,
	) -> Result<(), Error> {
		let mut next = self.first_slab;

		debug!("clear through impl with slab_id = {}", slab_id)?;
		loop {
			if next == u32::MAX {
				break;
			}

			let next_slab: u32 = u32::from_be_bytes(try_into!(
				&slabs.get(next.try_into()?)?.get()[READ_SLAB_NEXT_OFFSET..READ_SLAB_SIZE]
			)?);
			slabs.free(next.try_into()?)?;
			debug!("free {}", next)?;

			if next == slab_id {
				if slab_id == self.last_slab {
					self.first_slab = u32::MAX;
					self.last_slab = u32::MAX;
					self.slab_offset = 0;
				} else {
					self.first_slab = next_slab;
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
		Self {
			user_data: Box::new(0),
		}
	}
}

impl EventHandlerContext {
	fn new(
		tid: usize,
		max_events_in: usize,
		max_events: usize,
		max_handles_per_thread: usize,
		read_slab_count: usize,
	) -> Result<Self, Error> {
		let events = array!(max_events * 2, &Event::default())?;
		let mut events_in = vec![];
		events_in.resize(max_events_in, EventIn::default());

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

		let handle_hashtable = hashtable_sync_box!(
			SlabSize(HANDLE_SLAB_SIZE),
			SlabCount(max_handles_per_thread)
		)?;
		let connection_hashtable = hashtable_sync_box!(
			SlabSize(CONNECTION_SLAB_SIZE),
			SlabCount(max_handles_per_thread)
		)?;

		#[cfg(target_os = "windows")]
		let write_set = hashset_sync_box!(SlabSize(WRITE_SET_SIZE), SlabCount(2 * MAX_EVENTS))?;

		#[cfg(target_os = "linux")]
		let epoll_events = {
			let mut epoll_events = vec![];
			epoll_events.resize(max_events * 2, EpollEvent::new(EpollFlags::empty(), 0));
			epoll_events
		};

		let mut read_slabs = Builder::build_sync_slabs();
		read_slabs.init(SlabAllocatorConfig {
			slab_size: READ_SLAB_SIZE,
			slab_count: read_slab_count,
		})?;

		Ok(EventHandlerContext {
			connection_hashtable,
			handle_hashtable,
			read_slabs,
			events,
			events_in,
			tid,
			now: 0,
			counter: 0,
			count: 0,
			last_process_type: LastProcessType::OnRead,
			last_rw: None,
			#[cfg(target_os = "linux")]
			filter_set,
			#[cfg(target_os = "windows")]
			filter_set,
			#[cfg(target_os = "windows")]
			write_set,
			#[cfg(target_os = "macos")]
			selector: unsafe { kqueue() },
			#[cfg(target_os = "linux")]
			selector: epoll_create1(EpollCreateFlags::empty())?,
			#[cfg(target_os = "linux")]
			epoll_events,
			#[cfg(windows)]
			selector: unsafe { epoll_create(1) } as usize,
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
		debug_write_queue: bool,
	) -> Self {
		Self {
			handle,
			id,
			wakeup,
			write_state,
			event_handler_data,
			debug_write_queue,
		}
	}
	pub fn close(&mut self) -> Result<(), Error> {
		{
			debug!("wlock for {}", self.id)?;
			let mut write_state = self.write_state.wlock()?;
			let guard = write_state.guard();
			(**guard).set_flag(WRITE_STATE_FLAG_CLOSE);
			debug!("unlockwlock for {}", self.id)?;
		}
		{
			let mut event_handler_data = self.event_handler_data.wlock()?;
			let guard = event_handler_data.guard();
			(**guard).write_queue.enqueue(self.id)?;
		}
		self.wakeup.wakeup()?;
		Ok(())
	}
	pub fn write(&mut self, data: &[u8]) -> Result<(), Error> {
		let data_len = data.len();
		let len = {
			let write_state = self.write_state.rlock()?;
			if (**write_state.guard()).is_set(WRITE_STATE_FLAG_CLOSE) {
				return Err(err!(ErrKind::IO, "write handle closed"));
			}
			if (**write_state.guard()).is_set(WRITE_STATE_FLAG_PENDING) {
				0
			} else {
				if self.debug_write_queue {
					if data_len > 4 {
						if data[0] == 'a' as u8 {
							write_bytes(self.handle, &data[0..4])
						} else if data[0] == 'b' as u8 {
							write_bytes(self.handle, &data[0..3])
						} else if data[0] == 'c' as u8 {
							write_bytes(self.handle, &data[0..2])
						} else if data[0] == 'd' as u8 {
							write_bytes(self.handle, &data[0..1])
						} else {
							write_bytes(self.handle, data)
						}
					} else {
						write_bytes(self.handle, data)
					}
				} else {
					write_bytes(self.handle, data)
				}
			}
		};
		if errno().0 != 0 || len < 0 {
			// check for would block
			if errno().0 != EAGAIN && errno().0 != ETEMPUNAVAILABLE && errno().0 != WINNONBLOCKING {
				return Err(err!(
					ErrKind::IO,
					format!("writing generated error: {}", errno())
				));
			}
			self.queue_data(data)?;
		} else {
			let len: usize = len.try_into()?;
			if len < data_len {
				self.queue_data(&data[len..])?;
			}
		}
		Ok(())
	}
	pub fn trigger_on_read(&mut self) -> Result<(), Error> {
		{
			debug!("wlock for {}", self.id)?;
			let mut write_state = self.write_state.wlock()?;
			let guard = write_state.guard();
			(**guard).set_flag(WRITE_STATE_FLAG_TRIGGER_ON_READ);
			debug!("unlockwlock for {}", self.id)?;
		}
		{
			let mut event_handler_data = self.event_handler_data.wlock()?;
			let guard = event_handler_data.guard();
			(**guard).write_queue.enqueue(self.id)?;
		}
		self.wakeup.wakeup()?;
		Ok(())
	}

	pub fn handle(&self) -> Handle {
		self.handle
	}

	fn queue_data(&mut self, data: &[u8]) -> Result<(), Error> {
		let was_pending = {
			debug!("wlock for {}", self.id)?;
			let mut write_state = self.write_state.wlock()?;
			let guard = write_state.guard();
			let ret = (**guard).is_set(WRITE_STATE_FLAG_PENDING);
			(**guard).set_flag(WRITE_STATE_FLAG_PENDING);
			(**guard).write_buffer.extend(data);
			debug!("unlock wlock for {}", self.id)?;
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
		slabs: &'a mut Box<dyn SlabAllocator + Send + Sync>,
		wakeup: Wakeup,
		event_handler_data: Box<dyn LockBox<EventHandlerData>>,
		debug_write_queue: bool,
	) -> Self {
		Self {
			rwi,
			tid,
			slabs,
			wakeup,
			event_handler_data,
			debug_write_queue,
		}
	}

	fn clear_through_impl(&mut self, slab_id: u32) -> Result<(), Error> {
		self.rwi.clear_through_impl(slab_id, &mut self.slabs)
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
			self.debug_write_queue,
		)
	}
	fn borrow_slab_allocator<F, T>(&self, mut f: F) -> Result<T, Error>
	where
		F: FnMut(&Box<dyn SlabAllocator + Send + Sync>) -> Result<T, Error>,
	{
		f(&self.slabs)
	}
	fn slab_offset(&self) -> u16 {
		self.rwi.slab_offset
	}
	fn first_slab(&self) -> u32 {
		self.rwi.first_slab
	}
	fn last_slab(&self) -> u32 {
		self.rwi.last_slab
	}
	fn clear_through(&mut self, slab_id: u32) -> Result<(), Error> {
		self.clear_through_impl(slab_id)
	}
}

impl EventHandlerData {
	fn new(write_queue_size: usize, nhandles_queue_size: usize) -> Result<Self, Error> {
		let connection_info = ConnectionInfo::ListenerInfo(ListenerInfo {
			handle: 0,
			id: 0,
			is_reuse_port: false,
		});

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
		Self::check_config(&config)?;
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
			thread_pool_stopper: None,
			debug_write_queue: false,
		})
	}

	#[cfg(test)]
	fn set_debug_write_queue(&mut self, value: bool) {
		self.debug_write_queue = value;
	}

	fn check_config(config: &EventHandlerConfig) -> Result<(), Error> {
		if config.read_slab_count >= u32::MAX.try_into()? {
			return Err(err!(
				ErrKind::Configuration,
				"read_slab_count must be smaller than u32::MAX"
			));
		}
		Ok(())
	}

	fn execute_thread(
		&mut self,
		wakeup: &mut Wakeup,
		ctx: &mut EventHandlerContext,
		callback_context: &mut ThreadContext,
		is_restart: bool,
	) -> Result<(), Error> {
		debug!("Executing thread {}", ctx.tid)?;

		// add wakeup if this is the first start
		if !is_restart {
			let handle = wakeup.reader;
			debug!("wakeup handle is {}", handle)?;
			ctx.events_in.push(EventIn {
				handle,
				etype: EventTypeIn::Read,
			});
		} else {
			// we have to do different cleanup for each type.
			match ctx.last_process_type {
				LastProcessType::OnRead => match ctx.last_rw.clone() {
					Some(mut rw) => {
						ctx.connection_hashtable
							.insert(&rw.id, &ConnectionInfo::ReadWriteInfo(rw.clone()))?;
						self.process_close(ctx, &mut rw, callback_context)?;
					}
					None => {}
				},
				LastProcessType::OnAccept => {
					close_impl(ctx, ctx.events[ctx.counter].handle)?;
				}
				LastProcessType::OnClose => {
					// remove from hashtables
					match ctx
						.handle_hashtable
						.remove(&ctx.events[ctx.counter].handle)?
					{
						Some(id) => {
							match ctx.connection_hashtable.remove(&id)? {
								Some(ci) => match ci {
									ConnectionInfo::ReadWriteInfo(mut rwi) => {
										ctx.handle_hashtable.remove(&rwi.handle)?;
										rwi.clear_through_impl(rwi.last_slab, &mut ctx.read_slabs)?;
										close_impl(ctx, rwi.handle)?;
									}
									ConnectionInfo::ListenerInfo(li) => warn!(
								"Unexpected error: listener info found in process close: {:?}",
								li
							)?,
								},
								None => {
									// already closed
								}
							}
						}
						None => {
							// already closed
						}
					}
				}
			}

			// skip over the panicked request and continue processing remaining events
			ctx.counter += 1;
			self.process_events(ctx, wakeup, callback_context)?;
		}

		#[cfg(target_os = "macos")]
		let mut ret_kevs = vec![];
		#[cfg(target_os = "macos")]
		for _ in 0..self.config.max_events {
			ret_kevs.push(kevent::new(
				0,
				EventFilter::EVFILT_SYSCOUNT,
				EventFlag::empty(),
				FilterFlag::empty(),
			));
		}
		#[cfg(target_os = "macos")]
		let mut kevs = vec![];

		loop {
			debug!("start loop")?;
			let stop = self.process_new_connections(ctx)?;
			if stop {
				break;
			}
			self.process_write_queue(ctx)?;
			ctx.count = {
				debug!("calling get_events")?;
				let count = {
					let (requested, _lock) = wakeup.pre_block()?;
					#[cfg(target_os = "macos")]
					let count = self.get_events(ctx, requested, &mut kevs, &mut ret_kevs)?;
					#[cfg(not(target_os = "macos"))]
					let count = self.get_events(ctx, requested)?;
					count
				};
				debug!("get_events returned with {} event", count)?;
				wakeup.post_block()?;
				#[cfg(target_os = "windows")]
				ctx.write_set.clear()?;
				count
			};
			ctx.counter = 0;
			self.process_events(ctx, wakeup, callback_context)?;
		}
		debug!("thread {} stop ", ctx.tid)?;
		self.close_handles(ctx)?;
		Ok(())
	}

	fn close_handles(&self, ctx: &mut EventHandlerContext) -> Result<(), Error> {
		for (_, conn) in ctx.connection_hashtable.iter() {
			match conn {
				ConnectionInfo::ListenerInfo(li) => {
					debug!("shut down li = {:?}", li)?;
					close_handle_impl(li.handle)?;
				}
				ConnectionInfo::ReadWriteInfo(rw) => {
					debug!("shut down rw = {:?}", rw)?;
					close_handle_impl(rw.handle)?;
				}
			}
		}
		Ok(())
	}

	fn process_write_queue(&mut self, ctx: &mut EventHandlerContext) -> Result<(), Error> {
		let mut data = self.data[ctx.tid].wlock()?;
		loop {
			let guard = data.guard();
			match (**guard).write_queue.dequeue() {
				Some(next) => match ctx.connection_hashtable.get(&next)? {
					Some(ci) => match ci {
						ConnectionInfo::ReadWriteInfo(ref rwi) => {
							let handle = rwi.handle;
							ctx.events_in.push(EventIn {
								handle,
								etype: EventTypeIn::Write,
							});

							// we must update the hashtable to keep
							// things consistent in terms of our
							// deserializable/serializable
							ctx.connection_hashtable.insert(&next, &ci)?;
						}
						ConnectionInfo::ListenerInfo(li) => {
							warn!("Attempt to write to a listener: {:?}", li.handle)?;
						}
					},
					None => warn!("Couldn't look up conn info for {}", next)?,
				},
				None => break,
			}
		}
		Ok(())
	}

	fn process_new_connections(&mut self, ctx: &mut EventHandlerContext) -> Result<bool, Error> {
		let mut data = self.data[ctx.tid].wlock()?;
		let guard = data.guard();
		if (**guard).stop {
			return Ok(true);
		}
		debug!(
			"ctx.nhandles.size={},ctx.tid={}",
			(**guard).nhandles.length(),
			ctx.tid
		)?;
		loop {
			let mut next = (**guard).nhandles.dequeue();
			match next {
				Some(ref mut nhandle) => {
					debug!("handle={:?} on tid={}", nhandle, ctx.tid)?;
					match nhandle {
						ConnectionInfo::ListenerInfo(li) => {
							let id = random();
							match Self::insert_hashtables(ctx, id, li.handle, nhandle) {
								Ok(_) => {
									ctx.events_in.push(EventIn {
										handle: li.handle,
										etype: EventTypeIn::Read,
									});
								}
								Err(e) => {
									warn!("insert_hashtables generated error: {}. Closing.", e)?;
									close_impl(ctx, li.handle)?;
								}
							}
						}
						ConnectionInfo::ReadWriteInfo(rw) => {
							match Self::insert_hashtables(ctx, rw.id, rw.handle, nhandle) {
								Ok(_) => {
									ctx.events_in.push(EventIn {
										handle: rw.handle,
										etype: EventTypeIn::Read,
									});
								}
								Err(e) => {
									warn!("insert_hashtables generated error: {}. Closing.", e)?;
									close_impl(ctx, rw.handle)?;
								}
							}
						}
					}
				}
				None => break,
			}
		}
		Ok(false)
	}

	fn insert_hashtables(
		ctx: &mut EventHandlerContext,
		id: u128,
		handle: Handle,
		conn_info: &ConnectionInfo,
	) -> Result<(), Error> {
		ctx.connection_hashtable.insert(&id, conn_info)?;
		ctx.handle_hashtable.insert(&handle, &id)?;
		Ok(())
	}

	fn process_events(
		&mut self,
		ctx: &mut EventHandlerContext,
		wakeup: &Wakeup,
		callback_context: &mut ThreadContext,
	) -> Result<(), Error> {
		loop {
			if ctx.counter == ctx.count {
				break;
			}
			if ctx.events[ctx.counter].handle == wakeup.reader {
				debug!("WAKEUP, handle={}, tid={}", wakeup.reader, ctx.tid)?;
				read_bytes(ctx.events[ctx.counter].handle, &mut [0u8; 1]);
				#[cfg(target_os = "windows")]
				epoll_ctl_impl(
					EPOLLIN | EPOLLONESHOT | EPOLLRDHUP,
					ctx.events[ctx.counter].handle,
					&mut ctx.filter_set,
					ctx.selector as *mut c_void,
					ctx.tid,
				)?;

				ctx.counter += 1;
				continue;
			}
			match ctx.handle_hashtable.get(&ctx.events[ctx.counter].handle)? {
				Some(id) => match ctx.connection_hashtable.get(&id)? {
					Some(ci) => match ci {
						ConnectionInfo::ListenerInfo(li) => loop {
							let handle = self.process_accept(&li, ctx, callback_context)?;
							#[cfg(unix)]
							if handle <= 0 {
								break;
							}
							#[cfg(windows)]
							if handle == usize::MAX {
								break;
							}
						},
						ConnectionInfo::ReadWriteInfo(rw) => match ctx.events[ctx.counter].etype {
							EventType::Read => self.process_read(rw, ctx, callback_context)?,
							EventType::Write => self.process_write(rw, ctx, callback_context)?,
							EventType::Error => self.process_error(rw, ctx, callback_context)?,
							_ => {}
						},
					},
					None => warn!("Couldn't look up conn info for {}", id)?,
				},
				None => warn!(
					"Couldn't look up id for handle {}, tid={}",
					ctx.events[ctx.counter].handle, ctx.tid
				)?,
			}
			ctx.counter += 1;
		}
		Ok(())
	}

	fn process_error(
		&mut self,
		mut rw: ReadWriteInfo,
		ctx: &mut EventHandlerContext,
		callback_context: &mut ThreadContext,
	) -> Result<(), Error> {
		// write back to keep our hashtable consistent
		let id = rw.id;
		ctx.connection_hashtable
			.insert(&id, &ConnectionInfo::ReadWriteInfo(rw.clone()))?;
		self.process_close(ctx, &mut rw, callback_context)?;

		Ok(())
	}

	fn process_write(
		&mut self,
		mut rw: ReadWriteInfo,
		ctx: &mut EventHandlerContext,
		callback_context: &mut ThreadContext,
	) -> Result<(), Error> {
		let mut do_close = false;
		let mut trigger_on_read = false;

		{
			debug!("wlock for {}", rw.id)?;
			let mut write_state = rw.write_state.wlock()?;
			let guard = write_state.guard();
			loop {
				let len = (**guard).write_buffer.len();
				if len == 0 {
					(**guard).unset_flag(WRITE_STATE_FLAG_PENDING);
					if (**guard).is_set(WRITE_STATE_FLAG_CLOSE) {
						do_close = true;
					}
					if (**guard).is_set(WRITE_STATE_FLAG_TRIGGER_ON_READ) {
						(**guard).unset_flag(WRITE_STATE_FLAG_TRIGGER_ON_READ);
						trigger_on_read = true;
					}
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

		if trigger_on_read {
			match &mut self.on_read {
				Some(on_read) => {
					ctx.last_process_type = LastProcessType::OnRead;
					ctx.last_rw = Some(rw.clone());
					match on_read(
						&mut ConnectionData::new(
							&mut rw,
							ctx.tid,
							&mut ctx.read_slabs,
							self.wakeup[ctx.tid].clone(),
							self.data[ctx.tid].clone(),
							self.debug_write_queue,
						),
						callback_context,
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
			let id = rw.id;
			ctx.connection_hashtable
				.insert(&id, &ConnectionInfo::ReadWriteInfo(rw.clone()))?;
			self.process_close(ctx, &mut rw, callback_context)?;
		} else {
			let id = rw.id;
			let _handle = rw.handle;
			ctx.connection_hashtable
				.insert(&id, &ConnectionInfo::ReadWriteInfo(rw))?;
			#[cfg(target_os = "windows")]
			{
				epoll_ctl_impl(
					EPOLLIN | EPOLLOUT | EPOLLONESHOT | EPOLLRDHUP,
					_handle,
					&mut ctx.filter_set,
					ctx.selector as *mut c_void,
					ctx.tid,
				)?;
				ctx.write_set.insert(&_handle)?;
			}
		}
		Ok(())
	}

	fn process_read(
		&mut self,
		mut rw: ReadWriteInfo,
		ctx: &mut EventHandlerContext,
		callback_context: &mut ThreadContext,
	) -> Result<(), Error> {
		let mut do_close = false;
		let mut total_len = 0;
		loop {
			// read as many slabs as we can
			let slabs = &mut ctx.read_slabs;
			let mut slab = if rw.last_slab == u32::MAX {
				debug!("pre allocate")?;
				let mut slab = match slabs.allocate() {
					Ok(slab) => slab,
					Err(e) => {
						// we could not allocate slabs. Drop connection.
						warn!("slabs.allocate generated error: {}", e)?;
						total_len = 0;
						do_close = true;
						break;
					}
				};
				debug!("allocate: {}/tid={}", slab.id(), ctx.tid)?;
				let slab_id: u32 = slab.id().try_into()?;
				rw.last_slab = slab_id;
				rw.first_slab = slab_id;

				rw.slab_offset = 0;
				slab.get_mut()[READ_SLAB_NEXT_OFFSET..READ_SLAB_SIZE]
					.clone_from_slice(&u32::MAX.to_be_bytes());
				slab
			} else if rw.slab_offset as usize == READ_SLAB_NEXT_OFFSET {
				let slab_id: u32;
				{
					debug!("pre_allocate")?;
					let mut slab = match slabs.allocate() {
						Ok(slab) => slab,
						Err(e) => {
							// we could not allocate slabs. Drop connection.
							warn!("slabs.allocate generated error: {}", e)?;
							total_len = 0;
							do_close = true;
							break;
						}
					};
					slab_id = slab.id().try_into()?;
					debug!("allocatesecond: {}/tid={}", slab_id, ctx.tid)?;
					slab.get_mut()[READ_SLAB_NEXT_OFFSET..READ_SLAB_SIZE]
						.clone_from_slice(&u32::MAX.to_be_bytes());
				}

				slabs.get_mut(rw.last_slab.try_into()?)?.get_mut()
					[READ_SLAB_NEXT_OFFSET..READ_SLAB_SIZE]
					.clone_from_slice(&(slab_id as u32).to_be_bytes());
				rw.last_slab = slab_id;
				rw.slab_offset = 0;
				debug!("rw.last_slab={}", rw.last_slab)?;

				slabs.get_mut(slab_id.try_into()?)?
			} else {
				slabs.get_mut(rw.last_slab.try_into()?)?
			};
			let len = read_bytes(
				rw.handle,
				&mut slab.get_mut()[usize!(rw.slab_offset)..READ_SLAB_NEXT_OFFSET],
			);

			if len == 0 {
				do_close = true;
			}
			if len < 0 {
				// EAGAIN is would block. -2 is would block for windows
				if errno().0 != EAGAIN
					&& errno().0 != ETEMPUNAVAILABLE
					&& errno().0 != WINNONBLOCKING
					&& len != -2
				{
					do_close = true;
				}

				if len == -2 {
					#[cfg(target_os = "windows")]
					{
						if !ctx.write_set.contains(&rw.handle)? {
							epoll_ctl_impl(
								EPOLLIN | EPOLLONESHOT | EPOLLRDHUP,
								rw.handle,
								&mut ctx.filter_set,
								ctx.selector as *mut c_void,
								ctx.tid,
							)?;
						}
					}
				}
			}

			let target_len = READ_SLAB_NEXT_OFFSET
				.saturating_sub(rw.slab_offset.into())
				.try_into()?;
			if len < target_len {
				if len > 0 {
					total_len += len;
					rw.slab_offset += len as u16;

					#[cfg(target_os = "windows")]
					{
						if !ctx.write_set.contains(&rw.handle)? {
							epoll_ctl_impl(
								EPOLLIN | EPOLLONESHOT | EPOLLRDHUP,
								rw.handle,
								&mut ctx.filter_set,
								ctx.selector as *mut c_void,
								ctx.tid,
							)?;
						}
					}
				}

				break;
			}

			total_len += len;
			rw.slab_offset += len as u16;
		}
		debug!("read {} on tid = {}", total_len, ctx.tid)?;
		if total_len > 0 {
			match &mut self.on_read {
				Some(on_read) => {
					ctx.last_process_type = LastProcessType::OnRead;
					ctx.last_rw = Some(rw.clone());
					match on_read(
						&mut ConnectionData::new(
							&mut rw,
							ctx.tid,
							&mut ctx.read_slabs,
							self.wakeup[ctx.tid].clone(),
							self.data[ctx.tid].clone(),
							self.debug_write_queue,
						),
						callback_context,
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
			self.process_close(ctx, &mut rw, callback_context)?;
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
		callback_context: &mut ThreadContext,
	) -> Result<(), Error> {
		match &mut self.on_close {
			Some(on_close) => {
				ctx.last_process_type = LastProcessType::OnClose;
				match on_close(
					&mut ConnectionData::new(
						rw,
						ctx.tid,
						&mut ctx.read_slabs,
						self.wakeup[ctx.tid].clone(),
						self.data[ctx.tid].clone(),
						self.debug_write_queue,
					),
					callback_context,
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
					// set the close flag to true so if another thread tries to
					// write there will be an error
					{
						let mut state = rwi.write_state.wlock()?;
						let guard = state.guard();
						(**guard).set_flag(WRITE_STATE_FLAG_CLOSE);
					}
					ctx.handle_hashtable.remove(&rwi.handle)?;
					rwi.clear_through_impl(rwi.last_slab, &mut ctx.read_slabs)?;
					close_impl(ctx, rwi.handle)?;
				}
				ConnectionInfo::ListenerInfo(li) => warn!(
					"Unexpected error: listener info found in process close: {:?}",
					li
				)?,
			},
			None => {
				// already closed
			}
		}

		Ok(())
	}

	fn process_accept(
		&mut self,
		li: &ListenerInfo,
		ctx: &mut EventHandlerContext,
		callback_context: &mut ThreadContext,
	) -> Result<Handle, Error> {
		set_errno(Errno(0));
		let handle = accept_impl(li.handle)?;
		// this is a would block and means no more accepts to process
		#[cfg(unix)]
		if handle < 0 {
			return Ok(handle);
		}
		#[cfg(windows)]
		if handle == usize::MAX {
			return Ok(handle);
		}
		debug!(
			"accept handle = {},tid={},reuse_port={}",
			handle, ctx.tid, li.is_reuse_port
		)?;
		let id = random();
		let mut rwi = ReadWriteInfo {
			id,
			handle,
			accept_handle: Some(li.handle),
			write_state: lock_box!(WriteState {
				write_buffer: vec![],
				flags: 0
			})?,
			first_slab: u32::MAX,
			last_slab: u32::MAX,
			slab_offset: 0,
			is_accepted: true,
		};

		#[cfg(target_os = "windows")]
		epoll_ctl_impl(
			EPOLLIN | EPOLLONESHOT | EPOLLRDHUP,
			li.handle,
			&mut ctx.filter_set,
			ctx.selector as *mut c_void,
			ctx.tid,
		)?;

		if li.is_reuse_port {
			self.process_accepted_connection(ctx, handle, rwi, id, callback_context)?;
			debug!("process acc: {},tid={}", handle, ctx.tid)?;
		} else {
			let tid = random::<usize>() % self.config.threads;
			debug!("tid={},threads={}", tid, self.config.threads)?;

			ctx.last_process_type = LastProcessType::OnAccept;
			match &mut self.on_accept {
				Some(on_accept) => {
					match on_accept(
						&mut ConnectionData::new(
							&mut rwi,
							tid,
							&mut ctx.read_slabs,
							self.wakeup[tid].clone(),
							self.data[tid].clone(),
							self.debug_write_queue,
						),
						callback_context,
					) {
						Ok(_) => {}
						Err(e) => {
							warn!("Callback on_read generated error: {}", e)?;
						}
					}
				}
				None => {}
			}

			{
				let mut data = self.data[tid].wlock()?;
				let guard = data.guard();
				(**guard)
					.nhandles
					.enqueue(ConnectionInfo::ReadWriteInfo(rwi))?;
			}

			debug!("wakeup called on tid = {}", tid)?;
			self.wakeup[tid].wakeup()?;
		}
		Ok(handle)
	}

	fn process_accepted_connection(
		&mut self,
		ctx: &mut EventHandlerContext,
		handle: Handle,
		mut rwi: ReadWriteInfo,
		id: u128,
		callback_context: &mut ThreadContext,
	) -> Result<(), Error> {
		ctx.last_process_type = LastProcessType::OnAccept;
		match &mut self.on_accept {
			Some(on_accept) => {
				match on_accept(
					&mut ConnectionData::new(
						&mut rwi,
						ctx.tid,
						&mut ctx.read_slabs,
						self.wakeup[ctx.tid].clone(),
						self.data[ctx.tid].clone(),
						self.debug_write_queue,
					),
					callback_context,
				) {
					Ok(_) => {}
					Err(e) => {
						warn!("Callback on_read generated error: {}", e)?;
					}
				}
			}
			None => {}
		}

		match Self::insert_hashtables(ctx, id, handle, &ConnectionInfo::ReadWriteInfo(rwi)) {
			Ok(_) => {
				ctx.events_in.push(EventIn {
					handle,
					etype: EventTypeIn::Read,
				});
			}
			Err(e) => {
				warn!("insert_hashtables generated error: {}. Closing.", e)?;
				close_impl(ctx, handle)?;
			}
		}

		Ok(())
	}

	#[cfg(not(target_os = "macos"))]
	fn get_events(&self, ctx: &mut EventHandlerContext, requested: bool) -> Result<usize, Error> {
		get_events_impl(&self.config, ctx, requested)
	}

	#[cfg(target_os = "macos")]
	fn get_events(
		&self,
		ctx: &mut EventHandlerContext,
		requested: bool,
		kevs: &mut Vec<kevent>,
		ret_kevs: &mut Vec<kevent>,
	) -> Result<usize, Error> {
		get_events_impl(&self.config, ctx, requested, kevs, ret_kevs)
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
		match self.thread_pool_stopper.as_mut() {
			Some(ref mut stopper) => stopper.stop()?,
			None => {}
		}
		for i in 0..self.wakeup.size() {
			let mut data = self.data[i].wlock()?;
			let guard = data.guard();
			(**guard).stop = true;
			self.wakeup[i].wakeup()?;
		}
		Ok(())
	}
	fn start(&mut self) -> Result<(), Error> {
		let config = ThreadPoolConfig {
			max_size: self.config.threads,
			min_size: self.config.threads,
			sync_channel_size: self.config.sync_channel_size,
			..Default::default()
		};
		let mut tp = Builder::build_thread_pool(config)?;

		let mut v = vec![];
		let mut v_panic = vec![];
		for i in 0..self.config.threads {
			let evh = self.clone();
			let wakeup = self.wakeup.clone();
			let ctx = EventHandlerContext::new(
				i,
				self.config.max_events_in,
				self.config.max_events,
				self.config.max_handles_per_thread,
				self.config.read_slab_count,
			)?;
			let ctx = lock_box!(ctx)?;
			let thread_context = ThreadContext::new();
			let thread_context = Arc::new(Mutex::new(thread_context));
			v.push((
				evh.clone(),
				wakeup.clone(),
				ctx.clone(),
				thread_context.clone(),
			));
			v_panic.push((evh, wakeup, ctx, thread_context));
		}

		let mut executor = lock_box!(tp.executor()?)?;
		let mut executor_clone = executor.clone();

		tp.set_on_panic(move |id| -> Result<(), Error> {
			let id: usize = id.try_into()?;
			let mut evh = v_panic[id].0.clone();
			let mut wakeup = v_panic[id].1.clone();
			let mut ctx = v_panic[id].2.clone();
			let thread_context = v_panic[id].3.clone();
			let mut executor = executor.wlock()?;
			let executor = executor.guard();
			(**executor).execute(
				async move {
					let mut ctx = ctx.wlock_ignore_poison()?;
					let ctx = ctx.guard();

					let mut thread_context = {
						match thread_context.lock() {
							Ok(tc) => tc,
							Err(e) => e.into_inner(),
						}
					};
					match Self::execute_thread(
						&mut evh,
						&mut wakeup[id],
						&mut *ctx,
						&mut *thread_context,
						true,
					) {
						Ok(_) => {}
						Err(e) => {
							fatal!("execute_thread generated error: {}", e)?;
						}
					}

					Ok(())
				},
				id.try_into()?,
			)?;
			Ok(())
		})?;

		tp.start()?;

		{
			let mut executor = executor_clone.wlock()?;
			let guard = executor.guard();
			(**guard) = tp.executor()?;
		}

		for i in 0..self.config.threads {
			let mut evh = v[i].0.clone();
			let mut wakeup = v[i].1.clone();
			let mut ctx = v[i].2.clone();
			let thread_context = v[i].3.clone();

			execute!(tp, i.try_into()?, {
				let tid = i;

				let mut ctx = ctx.wlock_ignore_poison()?;
				let ctx = ctx.guard();

				let mut thread_context = match thread_context.lock() {
					Ok(tc) => tc,
					Err(e) => e.into_inner(),
				};

				match Self::execute_thread(
					&mut evh,
					&mut wakeup[tid],
					&mut *ctx,
					&mut *thread_context,
					false,
				) {
					Ok(_) => {}
					Err(e) => {
						fatal!("execute_thread generated error: {}", e)?;
					}
				}
				Ok(())
			})?;
		}

		self.thread_pool_stopper = Some(tp.stopper()?);

		Ok(())
	}

	fn add_client(&mut self, connection: ClientConnection) -> Result<WriteHandle, Error> {
		let tid: usize = random::<usize>() % self.data.size();
		let id: u128 = random::<u128>();
		let handle = connection.handle;
		let write_state = lock_box!(WriteState {
			write_buffer: vec![],
			flags: 0
		})?;
		let wh = WriteHandle::new(
			handle,
			id,
			self.wakeup[tid].clone(),
			write_state.clone(),
			self.data[tid].clone(),
			self.debug_write_queue,
		);

		let rwi = ReadWriteInfo {
			id,
			handle,
			accept_handle: None,
			write_state,
			first_slab: u32::MAX,
			last_slab: u32::MAX,
			slab_offset: 0,
			is_accepted: false,
		};

		{
			let mut data = self.data[tid].wlock()?;
			let guard = data.guard();
			(**guard)
				.nhandles
				.enqueue(ConnectionInfo::ReadWriteInfo(rwi))?;
		}

		self.wakeup[tid].wakeup()?;
		Ok(wh)
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
					is_reuse_port: connection.is_reuse_port,
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
			debug!("wakeup writing to {}", self.writer)?;
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
	use crate::evh::{READ_SLAB_NEXT_OFFSET, READ_SLAB_SIZE};
	use crate::types::{EventHandlerImpl, Wakeup};
	use crate::{
		ClientConnection, ConnData, EventHandler, EventHandlerConfig, ServerConnection,
		READ_SLAB_DATA_SIZE,
	};
	use bmw_deps::rand::random;
	use bmw_err::*;
	use bmw_log::*;
	use bmw_test::port::pick_free_port;
	use bmw_util::*;
	use std::io::Read;
	use std::io::Write;
	use std::net::TcpStream;
	#[cfg(unix)]
	use std::os::unix::io::IntoRawFd;
	#[cfg(windows)]
	use std::os::windows::io::IntoRawSocket;
	use std::sync::mpsc::sync_channel;
	use std::thread::{sleep, spawn};
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
					sleep(Duration::from_millis(100));
					info!("len={},err={}", len, errno())?;
				}
			}
			wakeup.post_block()?;

			let mut check = check_clone.wlock()?;
			**check.guard() = 1;

			Ok(())
		});

		sleep(Duration::from_millis(300));
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
		info!("basic Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 100_000,
			read_slab_count: 2,
			max_handles_per_thread: 2,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		evh.set_on_read(move |conn_data, _thread_context| {
			debug!("on read fs = {}", conn_data.slab_offset())?;
			let first_slab = conn_data.first_slab();
			let last_slab = conn_data.last_slab();
			let slab_offset = conn_data.slab_offset();
			debug!("first_slab={}", first_slab)?;
			let res = conn_data.borrow_slab_allocator(move |sa| {
				let slab = sa.get(first_slab.try_into()?)?;
				assert_eq!(first_slab, last_slab);
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
			is_reuse_port: false,
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
		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_close() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("close Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 30,
			max_handles_per_thread: 3,
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
				let slab = sa.get(first_slab.try_into()?)?;
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
			is_reuse_port: false,
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

		let total = 1000;
		for _ in 0..total {
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

		let mut count_count = 0;
		loop {
			sleep(Duration::from_millis(1));
			let count = **((close_count_clone.rlock()?).guard());
			if count != total + 1 {
				count_count += 1;
				if count_count < 1_000 {
					continue;
				}
			}
			assert_eq!((**((close_count_clone.rlock()?).guard())), total + 1);
			break;
		}
		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_server_close() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("server_close Using port: {}", port)?;
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
				let slab = sa.get(first_slab.try_into()?)?;
				info!("read bytes = {:?}", &slab.get()[0..slab_offset as usize])?;
				if slab.get()[0] == 'x' as u8 {
					Ok(vec![])
				} else {
					let mut ret: Vec<u8> = vec![];
					ret.extend(&slab.get()[0..slab_offset as usize]);
					Ok(ret)
				}
			})?;
			conn_data.clear_through(first_slab)?;
			if res.len() > 0 {
				conn_data.write_handle().write(&res)?;
			} else {
				conn_data.write_handle().close()?;
			}
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
			is_reuse_port: false,
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
			connection.write(b"xabc")?;
			let len = connection.read(&mut buf)?;
			assert_eq!(len, 0);
			assert_eq!(**((close_count_clone.rlock()?).guard()), 1);
		}

		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_multi_slab_message() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("multi_slab_message Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 100_000,
			read_slab_count: 3,
			max_handles_per_thread: 2,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		evh.set_on_read(move |conn_data, _thread_context| {
			info!("on read fs = {}", conn_data.slab_offset())?;
			let first_slab = conn_data.first_slab();
			let last_slab = conn_data.last_slab();
			let slab_offset = conn_data.slab_offset();
			let res = conn_data.borrow_slab_allocator(move |sa| {
				assert_ne!(first_slab, last_slab);

				let mut ret: Vec<u8> = vec![];
				let mut slab_id = first_slab;
				loop {
					if slab_id == last_slab {
						let slab = sa.get(slab_id.try_into()?)?;
						ret.extend(&slab.get()[0..slab_offset as usize]);
						break;
					} else {
						let slab = sa.get(slab_id.try_into()?)?;
						let slab_bytes = slab.get();
						ret.extend(&slab_bytes[0..READ_SLAB_NEXT_OFFSET]);
						slab_id = u32::from_be_bytes(try_into!(
							&slab_bytes[READ_SLAB_NEXT_OFFSET..READ_SLAB_SIZE]
						)?);
					}
				}
				Ok(ret)
			})?;
			conn_data.clear_through(first_slab)?;
			conn_data.write_handle().write(&res)?;
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
		let sc = ServerConnection {
			tls_config: None,
			handles,
			is_reuse_port: false,
		};
		evh.add_server(sc)?;

		let mut connection = TcpStream::connect(addr)?;
		let mut message = ['a' as u8; 1024];
		for i in 0..1024 {
			message[i] = 'a' as u8 + (i % 26) as u8;
		}
		connection.write(&message)?;
		let mut buf = vec![];
		buf.resize(2000, 0u8);
		let len = connection.read(&mut buf)?;
		assert_eq!(len, 1024);
		for i in 0..len {
			assert_eq!(buf[i], 'a' as u8 + (i % 26) as u8);
		}

		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_client() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("eventhandler client Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 100_000,
			read_slab_count: 1,
			max_handles_per_thread: 3,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		let mut client_handle = lock_box!(0)?;
		let client_handle_clone = client_handle.clone();

		let mut client_received_test1 = lock_box!(false)?;
		let mut server_received_test1 = lock_box!(false)?;
		let mut server_received_abc = lock_box!(false)?;
		let client_received_test1_clone = client_received_test1.clone();
		let server_received_test1_clone = server_received_test1.clone();
		let server_received_abc_clone = server_received_abc.clone();

		evh.set_on_read(move |conn_data, _thread_context| {
			info!("on read handle={}", conn_data.get_handle())?;
			let first_slab = conn_data.first_slab();
			let last_slab = conn_data.last_slab();
			let slab_offset = conn_data.slab_offset();
			debug!("first_slab={}", first_slab)?;
			let res = conn_data.borrow_slab_allocator(move |sa| {
				let slab = sa.get(first_slab.try_into()?)?;
				assert_eq!(first_slab, last_slab);
				info!("read bytes = {:?}", &slab.get()[0..slab_offset as usize])?;
				let mut ret: Vec<u8> = vec![];
				ret.extend(&slab.get()[0..slab_offset as usize]);
				Ok(ret)
			})?;
			conn_data.clear_through(first_slab)?;
			let client_handle = client_handle_clone.rlock()?;
			let guard = client_handle.guard();
			if conn_data.get_handle() != **guard {
				if res[0] == 't' as u8 {
					conn_data.write_handle().write(&res)?;
					if res == b"test1".to_vec() {
						let mut server_received_test1 = server_received_test1.wlock()?;
						(**server_received_test1.guard()) = true;
					}
				}
				if res == b"abc".to_vec() {
					let mut server_received_abc = server_received_abc.wlock()?;
					(**server_received_abc.guard()) = true;
				}
			} else {
				let mut x = vec![];
				x.extend(b"abc");
				conn_data.write_handle().write(&x)?;
				if res == b"test1".to_vec() {
					let mut client_received_test1 = client_received_test1.wlock()?;
					(**client_received_test1.guard()) = true;
				}
			}
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
			is_reuse_port: false,
		};
		evh.add_server(sc)?;

		let connection = TcpStream::connect(addr)?;
		#[cfg(unix)]
		let connection_handle = connection.into_raw_fd();
		#[cfg(windows)]
		let connection_handle = connection.into_raw_socket().try_into()?;
		{
			let mut client_handle = client_handle.wlock()?;
			(**client_handle.guard()) = connection_handle;
		}

		let client = ClientConnection {
			handle: connection_handle,
			tls_config: None,
		};
		let mut wh = evh.add_client(client)?;

		wh.write(b"test1")?;
		let mut count = 0;
		loop {
			sleep(Duration::from_millis(1));
			if !(**(client_received_test1_clone.rlock()?.guard())
				&& **(server_received_test1_clone.rlock()?.guard())
				&& **(server_received_abc_clone.rlock()?.guard()))
			{
				count += 1;
				if count < 2_000 {
					continue;
				}
			}
			assert!(**(client_received_test1_clone.rlock()?.guard()));
			assert!(**(server_received_test1_clone.rlock()?.guard()));
			assert!(**(server_received_abc_clone.rlock()?.guard()));
			break;
		}

		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_is_reuse_port() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("reuse port Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 2,
			max_handles_per_thread: 3,
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
				let slab = sa.get(first_slab.try_into()?)?;
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

		let mut tid0count = lock_box!(0)?;
		let mut tid1count = lock_box!(0)?;
		let tid0count_clone = tid0count.clone();
		let tid1count_clone = tid1count.clone();
		evh.set_on_accept(move |conn_data, _thread_context| {
			if conn_data.tid() == 0 {
				let mut tid0count = tid0count.wlock()?;
				let guard = tid0count.guard();
				(**guard) += 1;
			} else if conn_data.tid() == 1 {
				let mut tid1count = tid1count.wlock()?;
				let guard = tid1count.guard();
				(**guard) += 1;
			}
			info!(
				"accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
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
			is_reuse_port: true,
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

		let total = 100;
		for _ in 0..total {
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

		let mut count_count = 0;
		loop {
			sleep(Duration::from_millis(1));
			let count = **((close_count_clone.rlock()?).guard());
			if count != total + 1 {
				count_count += 1;
				if count_count < 1_000 {
					continue;
				}
			}
			assert_eq!((**((close_count_clone.rlock()?).guard())), total + 1);
			break;
		}

		let tid0count = **(tid0count_clone.rlock()?.guard());
		let tid1count = **(tid1count_clone.rlock()?.guard());
		info!("tid0count={},tid1count={}", tid0count, tid1count)?;
		#[cfg(target_os = "linux")]
		{
			assert_ne!(tid0count, 0);
			assert_ne!(tid1count, 0);
		}

		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_stop() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("stop Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 2,
			max_handles_per_thread: 3,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		evh.set_on_read(move |conn_data, _thread_context| {
			info!("on read fs = {}", conn_data.slab_offset())?;
			let first_slab = conn_data.first_slab();
			let slab_offset = conn_data.slab_offset();
			let res = conn_data.borrow_slab_allocator(move |sa| {
				let slab = sa.get(first_slab.try_into()?)?;
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
			info!(
				"stop accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
			Ok(())
		})?;

		evh.set_on_close(move |conn_data, _thread_context| {
			info!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
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
			is_reuse_port: true,
		};
		evh.add_server(sc)?;

		let mut connection = TcpStream::connect(addr)?;
		sleep(Duration::from_millis(1000));
		evh.stop()?;
		sleep(Duration::from_millis(1000));

		let mut buf = vec![];
		buf.resize(100, 0u8);
		let res = connection.read(&mut buf);

		assert!(res.is_err() || res.unwrap() == 0);

		Ok(())
	}

	#[test]
	fn test_eventhandler_partial_clear() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("partial clear Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 4,
			max_handles_per_thread: 2,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		evh.set_on_read(move |conn_data, _thread_context| {
			debug!("on read slab_offset = {}", conn_data.slab_offset())?;
			let first_slab = conn_data.first_slab();
			let last_slab = conn_data.last_slab();
			let mut second_slab = usize::MAX;
			let slab_offset = conn_data.slab_offset();
			let (res, second_slab) = conn_data.borrow_slab_allocator(move |sa| {
				let mut slab_id = first_slab;
				let mut ret: Vec<u8> = vec![];
				info!("on_read ")?;
				loop {
					info!("loop with id={}", slab_id)?;
					let slab = sa.get(slab_id.try_into()?)?;
					let slab_bytes = slab.get();
					debug!("read bytes = {:?}", &slab.get()[0..slab_offset as usize])?;
					if slab_id != last_slab {
						ret.extend(&slab_bytes[0..READ_SLAB_DATA_SIZE as usize]);
					} else {
						ret.extend(&slab_bytes[0..slab_offset as usize]);
						break;
					}
					slab_id = u32::from_be_bytes(try_into!(
						slab_bytes[READ_SLAB_DATA_SIZE..READ_SLAB_DATA_SIZE + 4]
					)?);
					if second_slab == usize::MAX {
						info!("set secondslab to {} ", slab_id)?;
						second_slab = slab_id.try_into()?;
					}
					info!("end loop")?;
				}
				Ok((ret, second_slab))
			})?;
			info!("second_slab={}", second_slab)?;
			if second_slab != usize::MAX {
				conn_data.clear_through(second_slab.try_into()?)?;
			} else {
				conn_data.clear_through(last_slab)?;
			}
			conn_data.write_handle().write(&res)?;
			info!("res={:?}", res)?;
			Ok(())
		})?;

		evh.set_on_accept(move |conn_data, _thread_context| {
			info!(
				"accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
			Ok(())
		})?;

		evh.set_on_close(move |conn_data, _thread_context| {
			info!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
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
			is_reuse_port: true,
		};

		sleep(Duration::from_millis(1000));
		evh.add_server(sc)?;
		sleep(Duration::from_millis(1000));

		let mut connection = TcpStream::connect(addr)?;
		let mut message = ['a' as u8; 1024];
		for i in 0..1024 {
			message[i] = 'a' as u8 + (i % 26) as u8;
		}
		connection.write(&message)?;
		let mut buf = vec![];
		buf.resize(2000, 0u8);
		let len = connection.read(&mut buf)?;
		assert_eq!(len, 1024);
		for i in 0..len {
			assert_eq!(buf[i], 'a' as u8 + (i % 26) as u8);
		}

		connection.write(&message)?;
		let mut buf = vec![];
		buf.resize(5000, 0u8);
		let len = connection.read(&mut buf)?;
		// there are some remaining bytes left in the last of the three slabs.
		// only 8 bytes so we have 8 + 1024 = 1032.
		assert_eq!(len, 1032);

		assert_eq!(buf[0], 99);
		assert_eq!(buf[1], 100);
		assert_eq!(buf[2], 101);
		assert_eq!(buf[3], 102);
		assert_eq!(buf[4], 103);
		assert_eq!(buf[5], 104);
		assert_eq!(buf[6], 105);
		assert_eq!(buf[7], 106);
		for i in 8..1032 {
			assert_eq!(buf[i], 'a' as u8 + ((i - 8) % 26) as u8);
		}

		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_different_lengths1() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("different len1 Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 21,
			max_handles_per_thread: 2,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		evh.set_on_read(move |conn_data, _thread_context| {
			debug!("on read slab_offset = {}", conn_data.slab_offset())?;
			let first_slab = conn_data.first_slab();
			let last_slab = conn_data.last_slab();
			let slab_offset = conn_data.slab_offset();
			debug!("firstslab={},last_slab={}", first_slab, last_slab)?;
			let res = conn_data.borrow_slab_allocator(move |sa| {
				let mut slab_id = first_slab;
				let mut ret: Vec<u8> = vec![];
				loop {
					let slab = sa.get(slab_id.try_into()?)?;
					let slab_bytes = slab.get();
					let offset = if slab_id == last_slab {
						slab_offset as usize
					} else {
						READ_SLAB_DATA_SIZE
					};
					debug!("read bytes = {:?}", &slab.get()[0..offset as usize])?;
					ret.extend(&slab_bytes[0..offset]);

					if slab_id == last_slab {
						break;
					}
					slab_id = u32::from_be_bytes(try_into!(
						slab_bytes[READ_SLAB_DATA_SIZE..READ_SLAB_DATA_SIZE + 4]
					)?);
				}
				Ok(ret)
			})?;
			conn_data.clear_through(last_slab)?;
			debug!("res.len={}", res.len())?;
			conn_data.write_handle().write(&res)?;
			Ok(())
		})?;

		evh.set_on_accept(move |conn_data, _thread_context| {
			debug!(
				"accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
			Ok(())
		})?;
		evh.set_on_close(move |conn_data, _thread_context| {
			debug!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
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
			is_reuse_port: true,
		};
		evh.add_server(sc)?;

		let mut stream = TcpStream::connect(addr)?;

		let mut bytes = [0u8; 10240];
		for i in 0..10240 {
			bytes[i] = 'a' as u8 + i as u8 % 26;
		}

		for i in 1..2000 {
			info!("i={}", i)?;
			stream.write(&bytes[0..i])?;
			let mut buf = vec![];
			buf.resize(i, 0u8);
			let len = stream.read(&mut buf[0..i])?;
			assert_eq!(len, i);
			assert_eq!(&buf[0..len], &bytes[0..len]);
		}

		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_different_lengths_client() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("lengths client Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 21,
			max_handles_per_thread: 2,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;
		evh.set_on_read(move |conn_data, _thread_context| {
			debug!("on read slab_offset = {}", conn_data.slab_offset())?;
			let first_slab = conn_data.first_slab();
			let last_slab = conn_data.last_slab();
			let slab_offset = conn_data.slab_offset();
			let res = conn_data.borrow_slab_allocator(move |sa| {
				let mut slab_id = first_slab;
				let mut ret: Vec<u8> = vec![];
				loop {
					let slab = sa.get(slab_id.try_into()?)?;
					let slab_bytes = slab.get();
					debug!("read bytes = {:?}", &slab.get()[0..slab_offset as usize])?;
					if slab_id != last_slab {
						ret.extend(&slab_bytes[0..READ_SLAB_DATA_SIZE as usize]);
					} else {
						ret.extend(&slab_bytes[0..slab_offset as usize]);
						break;
					}
					slab_id = u32::from_be_bytes(try_into!(
						slab_bytes[READ_SLAB_DATA_SIZE..READ_SLAB_DATA_SIZE + 4]
					)?);
				}
				Ok(ret)
			})?;
			conn_data.clear_through(last_slab)?;
			conn_data.write_handle().write(&res)?;
			debug!("res={:?}", res)?;
			Ok(())
		})?;

		evh.set_on_accept(move |conn_data, _thread_context| {
			debug!(
				"accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
			Ok(())
		})?;
		evh.set_on_close(move |conn_data, _thread_context| {
			debug!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
			Ok(())
		})?;
		evh.set_on_panic(move |_thread_context| Ok(()))?;
		evh.set_housekeeper(move |_thread_context| Ok(()))?;

		evh.start()?;
		let handles = create_listeners(threads, addr, 10)?;
		debug!("handles.size={},handles={:?}", handles.size(), handles)?;
		let sc = ServerConnection {
			tls_config: None,
			handles,
			is_reuse_port: true,
		};
		evh.add_server(sc)?;

		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 21,
			max_handles_per_thread: 2,
			..Default::default()
		};
		let mut evh2 = crate::Builder::build_evh(config.clone())?;

		let expected = lock_box!("".to_string())?;
		let mut expected_clone = expected.clone();
		let (tx, rx) = sync_channel(1);

		evh2.set_on_read(move |conn_data, _thread_context| {
			debug!("on read offset = {}", conn_data.slab_offset())?;
			let first_slab = conn_data.first_slab();
			let slab_offset = conn_data.slab_offset();
			let last_slab = conn_data.last_slab();
			let value = conn_data.borrow_slab_allocator(move |sa| {
				let mut slab_id = first_slab;
				let mut full = vec![];
				loop {
					let slab = sa.get(slab_id.try_into()?)?;
					let slab_bytes = slab.get();
					let offset = if slab_id == last_slab {
						slab_offset as usize
					} else {
						READ_SLAB_DATA_SIZE
					};

					full.extend(&slab_bytes[0..offset]);
					if slab_id == last_slab {
						break;
					} else {
						slab_id = u32::from_be_bytes(try_into!(
							slab_bytes[READ_SLAB_DATA_SIZE..READ_SLAB_DATA_SIZE + 4]
						)?);
					}
				}
				Ok(full)
			})?;

			let expected = expected.rlock()?;
			let guard = expected.guard();

			if value.len() == (**guard).len() {
				assert_eq!(std::str::from_utf8(&value)?, (**guard));
				tx.send(())?;
				conn_data.clear_through(last_slab)?;
			}
			Ok(())
		})?;

		evh2.set_on_accept(move |conn_data, _thread_context| {
			debug!(
				"accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
			Ok(())
		})?;
		evh2.set_on_close(move |conn_data, _thread_context| {
			debug!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
			Ok(())
		})?;
		evh2.set_on_panic(move |_thread_context| Ok(()))?;
		evh2.set_housekeeper(move |_thread_context| Ok(()))?;
		evh2.start()?;

		let connection = TcpStream::connect(addr.clone())?;
		connection.set_nonblocking(true)?;
		#[cfg(unix)]
		let connection_handle = connection.into_raw_fd();
		#[cfg(windows)]
		let connection_handle = connection.into_raw_socket().try_into()?;

		let client = ClientConnection {
			handle: connection_handle,
			tls_config: None,
		};
		let mut wh = evh2.add_client(client)?;

		let mut bytes = [0u8; 2000];
		for i in 0..2000 {
			bytes[i] = 'a' as u8 + i as u8 % 26;
		}

		for i in 1..1024 {
			let rand: usize = random();
			let rand = rand % 26;
			info!("i={},rand[0]={}", i, bytes[rand])?;
			let s = std::str::from_utf8(&bytes[rand..i + rand])?.to_string();
			{
				let mut expected = expected_clone.wlock()?;
				let guard = expected.guard();
				**guard = s;
				for j in 0..i {
					let mut b = [0u8; 1];
					b[0] = bytes[j + rand];
					wh.write(&b[0..1])?;
				}
			}

			rx.recv()?;
		}

		evh.stop()?;
		evh2.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_out_of_slabs() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("out of slabs Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 1,
			max_handles_per_thread: 2,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		evh.set_on_read(move |conn_data, _thread_context| {
			debug!("on read slab_offset = {}", conn_data.slab_offset())?;
			let first_slab = conn_data.first_slab();
			let last_slab = conn_data.last_slab();
			let slab_offset = conn_data.slab_offset();
			debug!("firstslab={},last_slab={}", first_slab, last_slab)?;
			let res = conn_data.borrow_slab_allocator(move |sa| {
				let mut slab_id = first_slab;
				let mut ret: Vec<u8> = vec![];
				loop {
					let slab = sa.get(slab_id.try_into()?)?;
					let slab_bytes = slab.get();
					let offset = if slab_id == last_slab {
						slab_offset as usize
					} else {
						READ_SLAB_DATA_SIZE
					};
					debug!("read bytes = {:?}", &slab.get()[0..offset as usize])?;
					ret.extend(&slab_bytes[0..offset]);

					if slab_id == last_slab {
						break;
					}
					slab_id = u32::from_be_bytes(try_into!(
						slab_bytes[READ_SLAB_DATA_SIZE..READ_SLAB_DATA_SIZE + 4]
					)?);
				}
				Ok(ret)
			})?;
			debug!("res.len={}", res.len())?;
			conn_data.write_handle().write(&res)?;
			Ok(())
		})?;

		evh.set_on_accept(move |conn_data, _thread_context| {
			debug!(
				"accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
			Ok(())
		})?;

		evh.set_on_close(move |conn_data, _thread_context| {
			debug!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
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
			is_reuse_port: true,
		};
		evh.add_server(sc)?;

		let mut stream = TcpStream::connect(addr)?;

		// do a normal request
		stream.write(b"test")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 4);
		assert_eq!(&buf[0..len], b"test");

		sleep(Duration::from_millis(100));
		// do a request that uses 2 slabs (with capacity of only one)
		let mut buf = [10u8; 600];
		stream.write(&buf)?;
		assert!(stream.read(&mut buf).is_err());

		sleep(Duration::from_millis(100));
		// now we should be able to continue with small requests

		let mut stream = TcpStream::connect(addr)?;
		stream.write(b"posterror")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 9);
		assert_eq!(&buf[0..len], b"posterror");

		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_user_data() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("user data Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 1,
			max_handles_per_thread: 2,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		evh.set_on_read(move |conn_data, thread_context| {
			assert_eq!(
				thread_context.user_data.downcast_ref::<String>().unwrap(),
				&"something".to_string()
			);
			debug!("on read slab_offset = {}", conn_data.slab_offset())?;
			let first_slab = conn_data.first_slab();
			let last_slab = conn_data.last_slab();
			let slab_offset = conn_data.slab_offset();
			debug!("firstslab={},last_slab={}", first_slab, last_slab)?;
			let res = conn_data.borrow_slab_allocator(move |sa| {
				let mut slab_id = first_slab;
				let mut ret: Vec<u8> = vec![];
				loop {
					let slab = sa.get(slab_id.try_into()?)?;
					let slab_bytes = slab.get();
					let offset = if slab_id == last_slab {
						slab_offset as usize
					} else {
						READ_SLAB_DATA_SIZE
					};
					debug!("read bytes = {:?}", &slab.get()[0..offset as usize])?;
					ret.extend(&slab_bytes[0..offset]);

					if slab_id == last_slab {
						break;
					}
					slab_id = u32::from_be_bytes(try_into!(
						slab_bytes[READ_SLAB_DATA_SIZE..READ_SLAB_DATA_SIZE + 4]
					)?);
				}
				Ok(ret)
			})?;
			debug!("res.len={}", res.len())?;
			conn_data.write_handle().write(&res)?;
			Ok(())
		})?;

		evh.set_on_accept(move |conn_data, thread_context| {
			debug!(
				"accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
			thread_context.user_data = Box::new("something".to_string());
			Ok(())
		})?;

		evh.set_on_close(move |conn_data, _thread_context| {
			debug!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
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
			is_reuse_port: true,
		};
		evh.add_server(sc)?;

		let mut stream = TcpStream::connect(addr)?;

		// do a normal request
		stream.write(b"test")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 4);
		assert_eq!(&buf[0..len], b"test");

		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_trigger_on_read() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("trigger on_read Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 2;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 1,
			max_handles_per_thread: 2,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		evh.set_on_read(move |conn_data, _thread_context| {
			conn_data.write_handle().write(b"1234")?;
			let mut wh = conn_data.write_handle();

			spawn(move || -> Result<(), Error> {
				info!("new thread")?;
				sleep(Duration::from_millis(1000));
				wh.write(b"5678")?;
				sleep(Duration::from_millis(1000));
				wh.trigger_on_read()?;
				Ok(())
			});
			Ok(())
		})?;

		evh.set_on_accept(move |conn_data, _thread_context| {
			debug!(
				"accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
			Ok(())
		})?;

		evh.set_on_close(move |conn_data, _thread_context| {
			debug!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
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
			is_reuse_port: true,
		};
		evh.add_server(sc)?;

		let mut stream = TcpStream::connect(addr)?;

		// do a normal request
		stream.write(b"test")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 4);
		assert_eq!(&buf[0..len], b"1234");

		let len = stream.read(&mut buf)?;
		assert_eq!(len, 4);
		assert_eq!(&buf[0..len], b"5678");

		let len = stream.read(&mut buf)?;
		assert_eq!(len, 4);
		assert_eq!(&buf[0..len], b"1234");

		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_thread_panic() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("thread_panic on_read Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 1;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 10,
			max_handles_per_thread: 10,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		evh.set_on_read(move |conn_data, _thread_context| {
			let mut wh = conn_data.write_handle();

			let first_slab = conn_data.first_slab();
			let slab_offset = conn_data.slab_offset();

			let res = conn_data.borrow_slab_allocator(move |sa| {
				let slab = sa.get(first_slab.try_into()?)?;
				let slab_bytes = slab.get();
				let mut ret = vec![];
				ret.extend(&slab_bytes[0..slab_offset as usize]);
				Ok(ret)
			})?;

			if res[0] == 'a' as u8 {
				let x: Option<u32> = None;
				let _y = x.unwrap();
			} else {
				wh.write(&res)?;
			}
			conn_data.clear_through(first_slab)?;

			Ok(())
		})?;

		evh.set_on_accept(move |conn_data, _thread_context| {
			debug!(
				"accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
			Ok(())
		})?;

		evh.set_on_close(move |conn_data, _thread_context| {
			debug!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
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
			is_reuse_port: true,
		};
		evh.add_server(sc)?;

		let mut stream = TcpStream::connect(addr)?;

		// do a normal request
		stream.write(b"test")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 4);
		assert_eq!(&buf[0..len], b"test");

		// create a thread panic
		stream.write(b"aaa")?;
		sleep(Duration::from_millis(5000));

		// read and we should get 0 for close
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 0);

		// connect and send another request
		let mut stream = TcpStream::connect(addr)?;
		stream.write(b"test")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 4);
		assert_eq!(&buf[0..len], b"test");

		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_thread_panic_multi() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("thread_panic on_read Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 1;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 10,
			max_handles_per_thread: 10,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		let x = lock_box!(0)?;
		let mut x_clone = x.clone();

		evh.set_on_read(move |conn_data, _thread_context| {
			let mut wh = conn_data.write_handle();

			let first_slab = conn_data.first_slab();
			let slab_offset = conn_data.slab_offset();

			let res = conn_data.borrow_slab_allocator(move |sa| {
				let slab = sa.get(first_slab.try_into()?)?;
				let slab_bytes = slab.get();
				let mut ret = vec![];
				ret.extend(&slab_bytes[0..slab_offset as usize]);
				Ok(ret)
			})?;

			if res[0] == 'a' as u8 {
				let x: Option<u32> = None;
				let _y = x.unwrap();
			} else if res[0] == 'b' as u8 {
				loop {
					sleep(Duration::from_millis(10));
					if **(x.rlock()?.guard()) != 0 {
						break;
					}
				}
				wh.write(&res)?;
			} else {
				wh.write(&res)?;
			}
			conn_data.clear_through(first_slab)?;

			Ok(())
		})?;

		evh.set_on_accept(move |conn_data, _thread_context| {
			debug!(
				"accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
			Ok(())
		})?;

		evh.set_on_close(move |conn_data, _thread_context| {
			debug!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
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
			is_reuse_port: true,
		};
		evh.add_server(sc)?;

		// make 4 connections
		let mut stream1 = TcpStream::connect(addr)?;
		let mut stream2 = TcpStream::connect(addr)?;
		let mut stream3 = TcpStream::connect(addr)?;
		let mut stream4 = TcpStream::connect(addr)?;

		// do a normal request on 1
		stream1.write(b"test")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream1.read(&mut buf)?;
		assert_eq!(len, 4);
		assert_eq!(&buf[0..len], b"test");

		// do a normal request on 2
		stream2.write(b"test")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream2.read(&mut buf)?;
		assert_eq!(len, 4);
		assert_eq!(&buf[0..len], b"test");

		// do a normal request on 3
		stream3.write(b"test")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream3.read(&mut buf)?;
		assert_eq!(len, 4);
		assert_eq!(&buf[0..len], b"test");

		// pause request
		stream4.write(b"bbbb")?;
		sleep(Duration::from_millis(100));

		// normal request
		stream1.write(b"1")?;
		sleep(Duration::from_millis(100));

		// create panic
		stream2.write(b"a")?;
		sleep(Duration::from_millis(100));

		// normal request
		stream3.write(b"c")?;
		sleep(Duration::from_millis(100));

		// unblock with guard
		**(x_clone.wlock()?.guard()) = 1;

		// read responses

		// normal echo after lock lifted
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream4.read(&mut buf)?;
		assert_eq!(len, 4);
		assert_eq!(&buf[0..len], b"bbbb");

		//normal echo
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream1.read(&mut buf)?;
		assert_eq!(len, 1);
		assert_eq!(&buf[0..len], b"1");

		// panic so close len == 0
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream2.read(&mut buf)?;
		assert_eq!(len, 0);

		// normal echo
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream3.read(&mut buf)?;
		assert_eq!(len, 1);
		assert_eq!(&buf[0..len], b"c");

		evh.stop()?;

		Ok(())
	}

	#[test]
	fn test_eventhandler_too_many_connections() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("thread_panic on_read Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 1;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 10,
			max_handles_per_thread: 2,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		evh.set_on_read(move |conn_data, _thread_context| {
			let mut wh = conn_data.write_handle();

			let first_slab = conn_data.first_slab();
			let slab_offset = conn_data.slab_offset();

			let res = conn_data.borrow_slab_allocator(move |sa| {
				let slab = sa.get(first_slab.try_into()?)?;
				let slab_bytes = slab.get();
				let mut ret = vec![];
				ret.extend(&slab_bytes[0..slab_offset as usize]);
				Ok(ret)
			})?;

			wh.write(&res)?;
			conn_data.clear_through(first_slab)?;

			Ok(())
		})?;

		evh.set_on_accept(move |conn_data, _thread_context| {
			debug!(
				"accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
			Ok(())
		})?;

		evh.set_on_close(move |conn_data, _thread_context| {
			debug!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
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
			is_reuse_port: true,
		};
		evh.add_server(sc)?;

		{
			let mut stream1 = TcpStream::connect(addr)?;

			// do a normal request on 1
			stream1.write(b"test")?;
			let mut buf = vec![];
			buf.resize(100, 0u8);
			let len = stream1.read(&mut buf)?;
			assert_eq!(len, 4);
			assert_eq!(&buf[0..len], b"test");

			// there are already two connections on the single thread (listener/stream).
			// so another connection will fail

			let mut stream2 = TcpStream::connect(addr)?;
			assert_eq!(stream2.read(&mut buf)?, 0);
		}
		sleep(Duration::from_millis(100));
		// now that stream1 is dropped we should be able to reconnect

		let mut stream1 = TcpStream::connect(addr)?;

		// do a normal request on 1
		stream1.write(b"12345")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream1.read(&mut buf)?;
		assert_eq!(len, 5);
		assert_eq!(&buf[0..len], b"12345");

		Ok(())
	}

	#[test]
	fn test_evh_write_queue() -> Result<(), Error> {
		let port = pick_free_port()?;
		info!("stop Using port: {}", port)?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let threads = 1;
		let config = EventHandlerConfig {
			threads,
			housekeeping_frequency_millis: 10_000,
			read_slab_count: 20,
			max_handles_per_thread: 30,
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;
		evh.set_debug_write_queue(true);

		evh.set_on_read(move |conn_data, _thread_context| {
			info!("on read fs = {}", conn_data.slab_offset())?;
			let first_slab = conn_data.first_slab();
			let slab_offset = conn_data.slab_offset();
			let res = conn_data.borrow_slab_allocator(move |sa| {
				let slab = sa.get(first_slab.try_into()?)?;
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
			info!(
				"stop accept a connection handle = {}, tid={}",
				conn_data.get_handle(),
				conn_data.tid()
			)?;
			Ok(())
		})?;

		evh.set_on_close(move |conn_data, _thread_context| {
			info!(
				"on close: {}/{}",
				conn_data.get_handle(),
				conn_data.get_connection_id()
			)?;
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
			is_reuse_port: true,
		};
		evh.add_server(sc)?;

		let mut stream = TcpStream::connect(addr)?;

		// do a normal request
		stream.write(b"12345")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 5);
		assert_eq!(&buf[0..len], b"12345");

		// this request uses the write queue
		stream.write(b"a12345")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		sleep(Duration::from_millis(1000));
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 6);
		assert_eq!(&buf[0..len], b"a12345");

		// this request uses the write queue
		stream.write(b"b12345")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		sleep(Duration::from_millis(1000));
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 6);
		assert_eq!(&buf[0..len], b"b12345");

		// this request uses the write queue
		stream.write(b"c12345")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		sleep(Duration::from_millis(1000));
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 6);
		assert_eq!(&buf[0..len], b"c12345");

		// this request uses the write queue
		stream.write(b"d12345")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		sleep(Duration::from_millis(1000));
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 6);
		assert_eq!(&buf[0..len], b"d12345");

		// this request uses the write queue
		stream.write(b"e12345")?;
		let mut buf = vec![];
		buf.resize(100, 0u8);
		let len = stream.read(&mut buf)?;
		assert_eq!(len, 6);
		assert_eq!(&buf[0..len], b"e12345");
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
