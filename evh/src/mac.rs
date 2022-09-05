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

use crate::types::Handle;
#[cfg(unix)]
use bmw_deps::libc::{c_void, fcntl, pipe, read, write, F_SETFL, O_NONBLOCK};
use bmw_err::*;
use std::net::TcpStream;
use std::os::raw::c_int;
use std::sync::Arc;

#[cfg(unix)]
pub(crate) fn get_reader_writer() -> Result<
	(
		Handle,
		Handle,
		Option<Arc<TcpStream>>,
		Option<Arc<TcpStream>>,
	),
	Error,
> {
	let mut retfds = [0i32; 2];
	let fds: *mut c_int = &mut retfds as *mut _ as *mut c_int;
	unsafe { pipe(fds) };
	unsafe { fcntl(retfds[0], F_SETFL, O_NONBLOCK) };
	unsafe { fcntl(retfds[1], F_SETFL, O_NONBLOCK) };
	Ok((retfds[0], retfds[1], None, None))
}

#[cfg(unix)]
pub(crate) fn read_bytes_impl(handle: Handle, buf: &mut [u8]) -> isize {
	let cbuf: *mut c_void = buf as *mut _ as *mut c_void;
	unsafe { read(handle, cbuf, buf.len()) }
}

#[cfg(unix)]
pub(crate) fn write_bytes_impl(handle: Handle, buf: &[u8]) -> isize {
	let cbuf: *const c_void = buf as *const _ as *const c_void;
	unsafe { write(handle, cbuf, buf.len().into()) }
}
