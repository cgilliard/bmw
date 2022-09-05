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

use crate::types::{EventHandlerImpl, Handle, Wakeup, WakeupState};
use crate::{
	ClientConnection, ConnectionData, EventHandler, EventHandlerConfig, ServerConnection,
	ThreadContext,
};
use bmw_deps::errno::{errno, set_errno, Errno};
use bmw_deps::interprocess::unnamed_pipe::pipe;
use bmw_err::*;
use bmw_log::*;
use bmw_util::*;
use std::sync::Arc;

#[cfg(windows)]
use bmw_deps::winapi;
#[cfg(windows)]
use bmw_deps::ws2_32::{ioctlsocket, recv, send, setsockopt};
#[cfg(windows)]
use std::net::{TcpListener, TcpStream};
#[cfg(windows)]
use std::os::raw::c_int;
#[cfg(windows)]
use std::os::windows::io::AsRawSocket;

#[cfg(unix)]
use bmw_deps::libc::{c_void, fcntl, read, write, F_SETFL, O_NONBLOCK};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;

#[cfg(windows)]
const WINSOCK_BUF_SIZE: winapi::c_int = 100_000_000;

info!();

impl Default for EventHandlerConfig {
	fn default() -> Self {
		Self {
			threads: 6,
			sync_channel_size: 10,
		}
	}
}

impl<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
	EventHandlerImpl<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
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
	pub(crate) fn new(config: EventHandlerConfig) -> Result<Self, Error> {
		Ok(Self {
			on_read: None,
			on_accept: None,
			on_close: None,
			housekeeper: None,
			on_panic: None,
			config,
		})
	}

	fn execute_thread(&self, tid: usize) -> Result<(), Error> {
		debug!("Executing thread {}", tid)?;

		Ok(())
	}
}

impl<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
	EventHandler<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
	for EventHandlerImpl<OnRead, OnAccept, OnClose, HouseKeeper, OnPanic>
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
			let evh = self.clone();
			execute!(tp, {
				let tid = {
					let mut l = tid.wlock()?;
					let guard = l.guard();
					let tid = **guard;
					(**guard) += 1;
					tid
				};
				Self::execute_thread(&evh, tid)?;
				Ok(())
			})?;
		}

		Ok(())
	}

	fn add_client(&mut self, _connection: ClientConnection) -> Result<ConnectionData, Error> {
		todo!()
	}
	fn add_server(&mut self, _connection: ServerConnection) -> Result<(), Error> {
		todo!()
	}
}

impl Wakeup {
	fn new() -> Result<Self, Error> {
		let (writer_unp, reader_unp) = pipe()?;
		let mut _tcp_stream = None;
		let mut _tcp_listener = None;
		#[cfg(windows)]
		let (reader, writer) = {
			let mut rethandles = [0u64; 2];
			let handles: *mut c_int = &mut rethandles as *mut _ as *mut c_int;
			let (listener, stream) = socket_pipe(handles)?;
			let listener_socket = listener.as_raw_socket();
			let stream_socket = stream.as_raw_socket();
			_tcp_stream = Some(Arc::new(stream));
			_tcp_listener = Some(Arc::new(listener));
			(listener_socket, stream_socket)
		};
		#[cfg(unix)]
		let (reader, writer) = {
			let reader = reader_unp.as_raw_fd();
			let writer = writer_unp.as_raw_fd();
			unsafe {
				fcntl(reader, F_SETFL, O_NONBLOCK);
				fcntl(writer, F_SETFL, O_NONBLOCK);
			}
			(reader, writer)
		};
		let _writer_unp = Arc::new(writer_unp);
		let _reader_unp = Arc::new(reader_unp);
		Ok(Self {
			_reader_unp,
			_writer_unp,
			_tcp_stream,
			_tcp_listener,
			reader,
			writer,
			state: lock_box!(WakeupState {
				needed: true,
				requested: false
			})?,
		})
	}

	fn wakeup(&mut self) -> Result<(), Error> {
		let need_wakeup = {
			let mut state = self.state.wlock()?;
			let guard = state.guard();

			(**guard).requested = true;
			(**guard).needed
		};
		if need_wakeup {
			debug!("writing to {}", self.writer)?;
			let len = write_bytes(self.writer, &[0u8; 1])?;
			debug!("len={},errno={}", len, errno())?;
		}
		Ok(())
	}

	fn state(&mut self) -> &mut Box<dyn LockBox<WakeupState>> {
		&mut self.state
	}

	fn post_block(&mut self) -> Result<(), Error> {
		let mut state = self.state.wlock()?;
		let guard = state.guard();
		(**guard).needed = false;
		(**guard).requested = false;

		Ok(())
	}
}

#[cfg(target_os = "windows")]
fn socket_pipe(fds: *mut i32) -> Result<(TcpStream, TcpStream), Error> {
	let port = bmw_deps::portpicker::pick_unused_port().unwrap_or(9999);
	let listener = TcpListener::bind(format!("127.0.0.1:{}", port))?;
	let stream = TcpStream::connect(format!("127.0.0.1:{}", port))?;
	debug!("port={}", port)?;
	let listener = listener.accept()?;
	let fds: &mut [i32] = unsafe { std::slice::from_raw_parts_mut(fds, 2) };
	fds[0] = listener.0.as_raw_socket().try_into().unwrap();
	fds[1] = stream.as_raw_socket().try_into().unwrap();
	set_windows_socket_options(fds[0].try_into()?)?;
	set_windows_socket_options(fds[1].try_into()?)?;

	Ok((listener.0, stream))
}

fn read_bytes(handle: Handle, buf: &mut [u8]) -> Result<isize, Error> {
	set_errno(Errno(0));
	#[cfg(unix)]
	{
		let cbuf: *mut c_void = buf as *mut _ as *mut c_void;
		Ok(unsafe { read(handle, cbuf, buf.len()) })
	}
	#[cfg(target_os = "windows")]
	{
		debug!("handle={}", handle)?;
		let cbuf: *mut i8 = buf as *mut _ as *mut i8;
		let mut len = unsafe { recv(handle, cbuf, buf.len().try_into()?, 0) };
		if errno().0 == 10035 {
			// would block
			len = -2;
		}
		Ok(len.try_into().unwrap_or(-1))
	}
}

fn write_bytes(handle: Handle, buf: &[u8]) -> Result<isize, Error> {
	set_errno(Errno(0));
	#[cfg(unix)]
	{
		let cbuf: *const c_void = buf as *const _ as *const c_void;
		let ret = unsafe { write(handle, cbuf, buf.len().into()) };
		Ok(ret)
	}
	#[cfg(target_os = "windows")]
	{
		let cbuf: *mut i8 = buf as *const _ as *mut i8;
		debug!("send to handle = {}", handle)?;
		Ok(unsafe {
			send(
				handle.try_into().unwrap_or(0),
				cbuf,
				(buf.len()).try_into().unwrap_or(0),
				0,
			)
			.try_into()?
		})
	}
}

#[cfg(windows)]
fn set_windows_socket_options(handle: Handle) -> Result<(), Error> {
	let fionbio = 0x8004667eu32;
	let ioctl_res = unsafe { ioctlsocket(handle, fionbio as c_int, &mut 1) };

	if ioctl_res != 0 {
		return Err(err!(
			ErrKind::IO,
			format!("complete fion with error: {}", errno().to_string())
		));
	}
	let sockoptres = unsafe {
		setsockopt(
			handle,
			winapi::SOL_SOCKET,
			winapi::SO_SNDBUF,
			&WINSOCK_BUF_SIZE as *const _ as *const i8,
			std::mem::size_of_val(&WINSOCK_BUF_SIZE) as winapi::c_int,
		)
	};

	if sockoptres != 0 {
		return Err(err!(
			ErrKind::IO,
			format!("setsocketopt resulted in error: {}", errno().to_string())
		));
	}

	Ok(())
}

#[cfg(test)]
mod test {
	use crate::evh::{errno, read_bytes};
	use crate::types::{EventHandlerImpl, Wakeup};
	use crate::{EventHandler, EventHandlerConfig};
	use bmw_err::*;
	use bmw_log::*;
	use bmw_util::*;
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
						let mut state = wakeup.state().wlock()?;
						state.guard().needed = true;
						let _requested = state.guard().requested;
						let mut buffer = [0u8; 1];
						info!("reader = {}", wakeup_clone.reader)?;
						info!("writer = {}", wakeup_clone.writer)?;

						len = read_bytes(wakeup_clone.reader, &mut buffer)?;
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
		let config = EventHandlerConfig {
			..Default::default()
		};
		let mut evh = EventHandlerImpl::new(config)?;

		evh.set_on_read(move |_conn_data, _thread_context| Ok(()))?;
		evh.set_on_accept(move |_conn_data, _thread_context| Ok(()))?;
		evh.set_on_close(move |_conn_data, _thread_context| Ok(()))?;
		evh.set_on_panic(move |_thread_context| Ok(()))?;
		evh.set_housekeeper(move |_thread_context| Ok(()))?;

		evh.start()?;
		println!("start evh complete");
		sleep(Duration::from_millis(1_000));

		Ok(())
	}
}
