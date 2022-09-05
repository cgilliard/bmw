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

use bmw_deps::interprocess::unnamed_pipe::{UnnamedPipeReader, UnnamedPipeWriter};
use bmw_err::*;
use bmw_util::*;
use std::net::TcpStream;
use std::pin::Pin;
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::prelude::RawFd;

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
	pub(crate) handles: Box<dyn List<Handle>>,
	pub(crate) tls_config: Option<TlsServerConfig>,
}

pub struct ConnectionData {}

pub trait ConnectionDataTrait {
	fn tid(&self) -> usize;
	fn get_connection_id(&self) -> u128;
	fn get_handle(&self) -> Handle;
	fn get_accept_handle(&self) -> Option<Handle>;
	fn close(&self) -> Result<(), Error>;
	fn write(&self, data: &[u8]) -> Result<(), Error>;
	fn trigger_on_read(&self) -> Result<(), Error>;
	fn slab_iter<'a>(&'a self) -> Box<dyn Iterator<Item = Slab> + 'a>;
	fn delete_head(&self) -> Result<(), Error>;
}

#[derive(Clone)]
pub struct EventHandlerConfig {
	pub threads: usize,
	pub sync_channel_size: usize,
}

pub trait EventHandler<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
where
	OnRead: Fn(ConnectionData, ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	OnAccept: Fn(ConnectionData, ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	OnClose: Fn(ConnectionData, ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	HouseKeeper: Fn(ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
	OnPanic: Fn(ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
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

#[derive(Clone)]
pub(crate) struct EventHandlerImpl<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
where
	OnRead: Fn(ConnectionData, ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	OnAccept: Fn(ConnectionData, ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	OnClose: Fn(ConnectionData, ThreadContext) -> Result<(), Error>
		+ Send
		+ 'static
		+ Clone
		+ Sync
		+ Unpin,
	HouseKeeper: Fn(ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
	OnPanic: Fn(ThreadContext) -> Result<(), Error> + Send + 'static + Clone + Sync + Unpin,
{
	pub(crate) on_read: Option<Pin<Box<OnRead>>>,
	pub(crate) on_accept: Option<Pin<Box<OnAccept>>>,
	pub(crate) on_close: Option<Pin<Box<OnClose>>>,
	pub(crate) on_panic: Option<Pin<Box<OnPanic>>>,
	pub(crate) housekeeper: Option<Pin<Box<HouseKeeper>>>,
	pub(crate) config: EventHandlerConfig,
}

#[derive(Clone)]
pub(crate) struct WakeupState {
	pub(crate) needed: bool,
	pub(crate) requested: bool,
}

#[derive(Clone)]
pub(crate) struct Wakeup {
	pub(crate) _tcp_stream: Option<Arc<TcpStream>>,
	pub(crate) _tcp_listener: Option<Arc<TcpStream>>,
	pub(crate) _reader_unp: Arc<UnnamedPipeReader>,
	pub(crate) _writer_unp: Arc<UnnamedPipeWriter>,
	pub(crate) reader: Handle,
	pub(crate) writer: Handle,
	pub(crate) state: Box<dyn LockBox<WakeupState>>,
}
