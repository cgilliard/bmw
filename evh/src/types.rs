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

use bmw_derive::Serializable;
use bmw_err::*;
use bmw_util::*;
use std::cell::{Ref, RefCell};
use std::net::TcpStream;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::prelude::RawFd;

#[cfg(target_os = "macos")]
use bmw_deps::kqueue_sys::kevent;

#[cfg(target_os = "linux")]
use bmw_deps::bitvec::vec::BitVec;
#[cfg(target_os = "linux")]
use bmw_deps::nix::sys::epoll::EpollEvent;

#[cfg(target_os = "windows")]
use bmw_deps::bitvec::vec::BitVec;

#[derive(Clone, Debug)]
pub struct TlsServerConfig {
	/// The location of the private_key file (privkey.pem).
	pub private_key_file: String,
	/// The location of the certificates file (fullchain.pem).
	pub certificates_file: String,
	/// The sni_host to use with the cert/key pair.
	pub sni_host: String,
}

pub struct TlsClientConfig {
	pub server_name: String,
	pub trusted_cert_full_chain_file: Option<String>,
}

#[cfg(unix)]
pub(crate) type Handle = RawFd;
#[cfg(windows)]
pub(crate) type Handle = u64;

pub struct ThreadContext {}

pub struct ClientConnection {
	pub(crate) handle: Handle,
	pub(crate) tls_config: Option<TlsClientConfig>,
}

pub struct ServerConnection {
	pub(crate) handles: Array<Handle>,
	pub(crate) tls_config: Option<TlsServerConfig>,
}

pub struct ConnectionData<'a> {
	pub(crate) rwi: &'a mut ReadWriteInfo,
	pub(crate) tid: usize,
	pub(crate) slabs: Rc<RefCell<dyn SlabAllocator>>,
	pub(crate) wakeup: Wakeup,
	pub(crate) event_handler_data: Box<dyn LockBox<EventHandlerData>>,
}

#[derive(Clone)]
pub struct WriteHandle {
	pub(crate) write_state: Box<dyn LockBox<WriteState>>,
	pub(crate) id: u128,
	pub(crate) handle: Handle,
	pub(crate) wakeup: Wakeup,
	pub(crate) event_handler_data: Box<dyn LockBox<EventHandlerData>>,
}

pub trait ConnData {
	fn tid(&self) -> usize;
	fn get_connection_id(&self) -> u128;
	fn get_handle(&self) -> Handle;
	fn get_accept_handle(&self) -> Option<Handle>;
	fn write_handle(&self) -> WriteHandle;
	fn borrow_slab_allocator<F, T>(&self, f: F) -> Result<T, Error>
	where
		F: Fn(Ref<dyn SlabAllocator>) -> Result<T, Error>;
	fn slab_offset(&self) -> u16;
	fn first_slab(&self) -> usize;
	fn last_slab(&self) -> usize;
	fn clear_through(&mut self, slab_id: usize) -> Result<(), Error>;
}

pub(crate) struct EventHandlerContext {
	pub(crate) events: Array<Event>,
	pub(crate) events_in: Array<EventIn>,
	pub(crate) events_in_count: usize,
	pub(crate) tid: usize,
	#[cfg(target_os = "macos")]
	pub(crate) kevs: Vec<kevent>,
	#[cfg(target_os = "macos")]
	pub(crate) ret_kevs: Vec<kevent>,
	#[cfg(target_os = "linux")]
	pub(crate) filter_set: BitVec,
	#[cfg(target_os = "windows")]
	pub(crate) filter_set: BitVec,
	#[cfg(target_os = "linux")]
	pub(crate) epoll_events: Vec<EpollEvent>,
	pub(crate) selector: Handle,
	pub(crate) now: u128,
	pub(crate) connection_hashtable: Box<dyn Hashtable<u128, ConnectionInfo>>,
	pub(crate) handle_hashtable: Box<dyn Hashtable<Handle, u128>>,
	pub(crate) read_slabs: Rc<RefCell<dyn SlabAllocator>>,
	pub(crate) _connection_slabs: Rc<RefCell<dyn SlabAllocator>>,
	pub(crate) _handle_slabs: Rc<RefCell<dyn SlabAllocator>>,
	pub(crate) callback_context: ThreadContext,
}

#[derive(Clone)]
pub struct EventHandlerConfig {
	pub threads: usize,
	pub sync_channel_size: usize,
	pub write_queue_size: usize,
	pub nhandles_queue_size: usize,
	pub events_per_batch: usize,
	pub max_events_in: usize,
	pub max_events: usize,
	pub housekeeping_frequency_millis: u128,
	pub read_slab_count: usize,
	pub max_handles_per_thread: usize,
}

pub trait EventHandler<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
where
	OnRead: Fn(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	OnAccept: Fn(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	OnClose: Fn(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	HouseKeeper:
		Fn(&mut ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
	OnPanic: Fn(&mut ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
{
	fn set_on_read(&mut self, on_read: OnRead) -> Result<(), Error>;
	fn set_on_accept(&mut self, on_accept: OnAccept) -> Result<(), Error>;
	fn set_on_close(&mut self, on_close: OnClose) -> Result<(), Error>;
	fn set_housekeeper(&mut self, housekeeper: HouseKeeper) -> Result<(), Error>;
	fn set_on_panic(&mut self, on_panic: OnPanic) -> Result<(), Error>;
	fn stop(&mut self) -> Result<(), Error>;
	fn start(&mut self) -> Result<(), Error>;
	fn add_client(&mut self, connection: ClientConnection) -> Result<ConnectionData, Error>;
	fn add_server(&mut self, connection: ServerConnection) -> Result<(), Error>;
}

pub struct Builder {}

#[derive(Clone)]
pub(crate) struct EventHandlerImpl<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
where
	OnRead: Fn(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	OnAccept: Fn(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	OnClose: Fn(&mut ConnectionData, &mut ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	HouseKeeper:
		Fn(&mut ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
	OnPanic: Fn(&mut ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
{
	pub(crate) on_read: Option<Pin<Box<OnRead>>>,
	pub(crate) on_accept: Option<Pin<Box<OnAccept>>>,
	pub(crate) on_close: Option<Pin<Box<OnClose>>>,
	pub(crate) on_panic: Option<Pin<Box<OnPanic>>>,
	pub(crate) housekeeper: Option<Pin<Box<HouseKeeper>>>,
	pub(crate) config: EventHandlerConfig,
	pub(crate) data: Array<Box<dyn LockBox<EventHandlerData>>>,
	pub(crate) wakeup: Array<Wakeup>,
}

#[derive(Clone)]
pub(crate) struct Wakeup {
	pub(crate) _tcp_stream: Option<Arc<TcpStream>>,
	pub(crate) _tcp_listener: Option<Arc<TcpStream>>,
	pub(crate) reader: Handle,
	pub(crate) writer: Handle,
	pub(crate) requested: Box<dyn LockBox<bool>>,
	pub(crate) needed: Box<dyn LockBox<bool>>,
}

#[derive(Clone, Debug)]
pub(crate) enum ConnectionInfo {
	ListenerInfo(ListenerInfo),
	ReadWriteInfo(ReadWriteInfo),
}

unsafe impl Send for ConnectionInfo {}
unsafe impl Sync for ConnectionInfo {}

#[derive(Clone, Debug, Serializable)]
pub(crate) struct ListenerInfo {
	pub(crate) id: u128,
	pub(crate) handle: Handle,
}

#[derive(Clone, Debug)]
pub(crate) struct ReadWriteInfo {
	pub(crate) id: u128,
	pub(crate) handle: Handle,
	pub(crate) accept_handle: Option<Handle>,
	pub(crate) write_state: Box<dyn LockBox<WriteState>>,
	pub(crate) first_slab: usize,
	pub(crate) last_slab: usize,
	pub(crate) slab_offset: u16,
}

#[derive(Clone, Debug)]
pub(crate) struct WriteState {
	pub(crate) write_buffer: Vec<u8>,
	pub(crate) flags: u8,
}

#[derive(Clone)]
pub(crate) struct EventHandlerData {
	pub(crate) write_queue: Box<dyn Queue<u128> + Send + Sync>,
	pub(crate) nhandles: Box<dyn Queue<ConnectionInfo> + Send + Sync>,
	pub(crate) stop: bool,
}

#[derive(Debug, Clone, Serializable, PartialEq)]
pub(crate) enum EventType {
	Accept,
	Read,
	Write,
	Error,
}

#[derive(Debug, Clone, Serializable, PartialEq)]
pub(crate) enum EventTypeIn {
	Accept,
	Read,
	Write,
}

#[derive(Debug, Clone, Serializable, PartialEq)]
pub(crate) struct Event {
	pub(crate) handle: Handle,
	pub(crate) etype: EventType,
}

#[derive(Debug, Clone, Serializable, PartialEq)]
pub(crate) struct EventIn {
	pub(crate) handle: Handle,
	pub(crate) etype: EventTypeIn,
}
