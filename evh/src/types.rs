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

use bmw_deps::rustls::client::ClientConnection as RustlsClientConnection;
use bmw_deps::rustls::server::{ServerConfig, ServerConnection as RustlsServerConnection};
use bmw_derive::Serializable;
use bmw_err::*;
use bmw_util::*;
use std::any::Any;
use std::net::TcpStream;
use std::pin::Pin;
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::prelude::RawFd;

#[cfg(target_os = "linux")]
use bmw_deps::bitvec::vec::BitVec;
#[cfg(target_os = "linux")]
use bmw_deps::nix::sys::epoll::EpollEvent;

#[cfg(target_os = "windows")]
use bmw_deps::bitvec::vec::BitVec;

#[derive(Clone, Debug, PartialEq)]
pub struct TlsServerConfig {
	/// The location of the private_key file (privkey.pem).
	pub private_key_file: String,
	/// The location of the certificates file (fullchain.pem).
	pub certificates_file: String,
	/// The sni_host to use with the cert/key pair.
	pub sni_host: String,
	/// The location of the optional ocsp file.
	pub ocsp_file: Option<String>,
}

#[derive(PartialEq)]
pub struct TlsClientConfig {
	pub sni_host: String,
	pub trusted_cert_full_chain_file: Option<String>,
}

#[cfg(unix)]
pub(crate) type Handle = RawFd;
#[cfg(windows)]
pub(crate) type Handle = usize;

pub struct ThreadContext {
	pub user_data: Box<dyn Any + Send + Sync>,
}

pub struct ClientConnection {
	pub handle: Handle,
	pub tls_config: Option<TlsClientConfig>,
}

pub struct ServerConnection {
	pub handles: Array<Handle>,
	pub tls_config: Vec<TlsServerConfig>,
	pub is_reuse_port: bool,
}

pub struct ConnectionData<'a> {
	pub(crate) rwi: &'a mut ReadWriteInfo,
	pub(crate) tid: usize,
	pub(crate) slabs: &'a mut Box<dyn SlabAllocator + Send + Sync>,
	pub(crate) wakeup: Wakeup,
	pub(crate) event_handler_data: Box<dyn LockBox<EventHandlerData>>,
	pub(crate) debug_write_queue: bool,
	pub(crate) debug_pending: bool,
	pub(crate) debug_write_error: bool,
	pub(crate) debug_suspended: bool,
}

#[derive(Clone)]
pub struct WriteHandle {
	pub(crate) write_state: Box<dyn LockBox<WriteState>>,
	pub(crate) id: u128,
	pub(crate) handle: Handle,
	pub(crate) wakeup: Wakeup,
	pub(crate) event_handler_data: Box<dyn LockBox<EventHandlerData>>,
	pub(crate) debug_write_queue: bool,
	pub(crate) debug_pending: bool,
	pub(crate) debug_write_error: bool,
	pub(crate) debug_suspended: bool,
	pub(crate) tls_server: Option<Box<dyn LockBox<RustlsServerConnection>>>,
	pub(crate) tls_client: Option<Box<dyn LockBox<RustlsClientConnection>>>,
}

pub trait ConnData {
	fn tid(&self) -> usize;
	fn get_connection_id(&self) -> u128;
	fn get_handle(&self) -> Handle;
	fn get_accept_handle(&self) -> Option<Handle>;
	fn write_handle(&self) -> WriteHandle;
	fn borrow_slab_allocator<F, T>(&self, f: F) -> Result<T, Error>
	where
		F: FnMut(&Box<dyn SlabAllocator + Send + Sync>) -> Result<T, Error>;
	fn slab_offset(&self) -> u16;
	fn first_slab(&self) -> u32;
	fn last_slab(&self) -> u32;
	fn clear_through(&mut self, slab_id: u32) -> Result<(), Error>;
}

#[derive(Clone)]
pub struct EventHandlerConfig {
	pub threads: usize,
	pub sync_channel_size: usize,
	pub write_queue_size: usize,
	pub nhandles_queue_size: usize,
	pub max_events_in: usize,
	pub max_events: usize,
	pub housekeeping_frequency_millis: u128,
	pub read_slab_count: usize,
	pub max_handles_per_thread: usize,
}

pub trait EventHandler<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
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
	OnPanic: FnMut(&mut ThreadContext, Box<dyn Any + Send>) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
{
	fn set_on_read(&mut self, on_read: OnRead) -> Result<(), Error>;
	fn set_on_accept(&mut self, on_accept: OnAccept) -> Result<(), Error>;
	fn set_on_close(&mut self, on_close: OnClose) -> Result<(), Error>;
	fn set_housekeeper(&mut self, housekeeper: HouseKeeper) -> Result<(), Error>;
	fn set_on_panic(&mut self, on_panic: OnPanic) -> Result<(), Error>;
	fn stop(&mut self) -> Result<(), Error>;
	fn start(&mut self) -> Result<(), Error>;
	fn add_client(&mut self, connection: ClientConnection) -> Result<WriteHandle, Error>;
	fn add_server(&mut self, connection: ServerConnection) -> Result<(), Error>;
}

pub struct Builder {}

// pub(crate) types

#[derive(Clone)]
pub(crate) enum LastProcessType {
	OnRead,
	OnClose,
	OnAccept,
	Housekeeper,
}

#[derive(Clone)]
pub(crate) struct EventHandlerContext {
	pub(crate) events: Array<Event>,
	pub(crate) events_in: Vec<EventIn>,
	pub(crate) tid: usize,
	#[cfg(target_os = "linux")]
	pub(crate) filter_set: BitVec,
	#[cfg(target_os = "windows")]
	pub(crate) filter_set: BitVec,
	#[cfg(target_os = "linux")]
	pub(crate) epoll_events: Vec<EpollEvent>,
	pub(crate) selector: Handle,
	pub(crate) now: u128,
	pub(crate) last_housekeeper: u128,
	pub(crate) connection_hashtable: Box<dyn Hashtable<u128, ConnectionInfo> + Send + Sync>,
	pub(crate) handle_hashtable: Box<dyn Hashtable<Handle, u128> + Send + Sync>,
	pub(crate) read_slabs: Box<dyn SlabAllocator + Send + Sync>,
	#[cfg(target_os = "windows")]
	pub(crate) write_set: Box<dyn Hashset<Handle> + Send + Sync>,
	pub(crate) counter: usize,
	pub(crate) count: usize,
	pub(crate) last_process_type: LastProcessType,
	pub(crate) last_rw: Option<ReadWriteInfo>,
	pub(crate) buffer: Vec<u8>,
	pub(crate) do_write_back: bool,
}

#[derive(Clone)]
pub(crate) struct EventHandlerImpl<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
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
	OnPanic: FnMut(&mut ThreadContext, Box<dyn Any + Send>) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
{
	pub(crate) on_read: Option<Pin<Box<OnRead>>>,
	pub(crate) on_accept: Option<Pin<Box<OnAccept>>>,
	pub(crate) on_close: Option<Pin<Box<OnClose>>>,
	pub(crate) on_panic: Option<Pin<Box<OnPanic>>>,
	pub(crate) housekeeper: Option<Pin<Box<HouseKeeper>>>,
	pub(crate) config: EventHandlerConfig,
	pub(crate) data: Array<Box<dyn LockBox<EventHandlerData>>>,
	pub(crate) wakeup: Array<Wakeup>,
	pub(crate) thread_pool_stopper: Option<ThreadPoolStopper>,
	pub(crate) debug_write_queue: bool,
	pub(crate) debug_pending: bool,
	pub(crate) debug_write_error: bool,
	pub(crate) debug_suspended: bool,
	pub(crate) debug_fatal_error: bool,
	pub(crate) debug_tls_server_error: bool,
	pub(crate) debug_read_error: bool,
	pub(crate) debug_tls_read: bool,
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

#[derive(Clone)]
pub(crate) struct ListenerInfo {
	pub(crate) id: u128,
	pub(crate) handle: Handle,
	pub(crate) is_reuse_port: bool,
	pub(crate) tls_config: Option<Arc<ServerConfig>>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReadWriteInfo {
	pub(crate) id: u128,
	pub(crate) handle: Handle,
	pub(crate) accept_handle: Option<Handle>,
	pub(crate) write_state: Box<dyn LockBox<WriteState>>,
	pub(crate) first_slab: u32,
	pub(crate) last_slab: u32,
	pub(crate) slab_offset: u16,
	pub(crate) is_accepted: bool,
	pub(crate) tls_server: Option<Box<dyn LockBox<RustlsServerConnection>>>,
	pub(crate) tls_client: Option<Box<dyn LockBox<RustlsClientConnection>>>,
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
	pub(crate) stopped: bool,
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
	Suspend,
	Resume,
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
