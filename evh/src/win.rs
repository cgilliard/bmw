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

#[cfg(windows)]
use crate::types::Handle;
#[cfg(windows)]
use bmw_deps::errno::{errno, set_errno, Errno};
#[cfg(windows)]
use bmw_deps::winapi;
#[cfg(windows)]
use bmw_deps::ws2_32::{ioctlsocket, recv, send, setsockopt};
use bmw_err::*;
use bmw_log::*;
#[cfg(windows)]
use std::net::{TcpListener, TcpStream};
#[cfg(windows)]
use std::os::raw::c_int;
#[cfg(windows)]
use std::os::windows::io::AsRawSocket;
#[cfg(windows)]
use std::sync::Arc;

#[cfg(windows)]
const WINSOCK_BUF_SIZE: winapi::c_int = 100_000_000;

info!();

#[cfg(target_os = "windows")]
pub(crate) fn socket_pipe(fds: *mut i32) -> Result<(TcpStream, TcpStream), Error> {
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

#[cfg(windows)]
pub(crate) fn set_windows_socket_options(handle: Handle) -> Result<(), Error> {
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

#[cfg(windows)]
pub(crate) fn read_bytes_impl(handle: Handle, buf: &mut [u8]) -> isize {
	let cbuf: *mut i8 = buf as *mut _ as *mut i8;
	match buf.len().try_into() {
		Ok(len) => {
			let mut len = unsafe { recv(handle, cbuf, len, 0) };
			if errno().0 == 10035 {
				// would block
				len = -2;
			}
			len
		}
		Err(e) => {
			let _ = error!("couldn't convert length");
			-1
		}
	}
	.try_into()
	.unwrap_or(-1)
}

#[cfg(windows)]
pub(crate) fn write_bytes_impl(handle: Handle, buf: &[u8]) -> isize {
	let cbuf: *mut i8 = buf as *const _ as *mut i8;
	match buf.len().try_into() {
		Ok(len) => unsafe { send(handle, cbuf, len, 0).try_into().unwrap_or(-1) },
		Err(_) => -1,
	}
}

#[cfg(windows)]
pub(crate) fn get_reader_writer() -> Result<
	(
		Handle,
		Handle,
		Option<Arc<TcpStream>>,
		Option<Arc<TcpStream>>,
	),
	Error,
> {
	let (_tcp_stream, _tcp_listener);
	let mut rethandles = [0u64; 2];
	let handles: *mut c_int = &mut rethandles as *mut _ as *mut c_int;
	let (listener, stream) = socket_pipe(handles)?;
	let listener_socket = listener.as_raw_socket();
	let stream_socket = stream.as_raw_socket();
	_tcp_stream = Some(Arc::new(stream));
	_tcp_listener = Some(Arc::new(listener));
	Ok((listener_socket, stream_socket, _tcp_listener, _tcp_stream))
}
