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

use crate::types::{EventHandlerConfig, EventHandlerContext, Handle};
use bmw_deps::errno::{errno, set_errno, Errno};
use bmw_deps::libc::{
	self, accept, c_void, fcntl, pipe, read, sockaddr, write, F_SETFL, O_NONBLOCK,
};
use bmw_deps::nix::sys::socket::{
	bind, listen, socket, AddressFamily, InetAddr, SockAddr, SockFlag, SockType,
};
use bmw_err::*;
use bmw_util::*;
use std::mem::{self, size_of, zeroed};
use std::net::SocketAddr;
use std::net::TcpStream;
use std::os::raw::c_int;
use std::os::unix::prelude::RawFd;
use std::str::FromStr;
use std::sync::Arc;

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

pub(crate) fn read_bytes_impl(handle: Handle, buf: &mut [u8]) -> isize {
	let cbuf: *mut c_void = buf as *mut _ as *mut c_void;
	unsafe { read(handle, cbuf, buf.len()) }
}

pub(crate) fn write_bytes_impl(handle: Handle, buf: &[u8]) -> isize {
	let cbuf: *const c_void = buf as *const _ as *const c_void;
	unsafe { write(handle, cbuf, buf.len().into()) }
}

pub(crate) fn accept_impl(fd: RawFd) -> Result<RawFd, Error> {
	set_errno(Errno(0));
	let handle = unsafe {
		accept(
			fd,
			&mut sockaddr { ..zeroed() },
			&mut (size_of::<sockaddr>() as u32).try_into()?,
		)
	};

	if handle < 0 {
		let fmt = format!("accept failed: {}", errno());
		return Err(err!(ErrKind::IO, fmt));
	}

	unsafe {
		fcntl(handle, F_SETFL, O_NONBLOCK);
	}

	Ok(handle)
}

pub(crate) fn create_listeners_impl(
	size: usize,
	addr: &str,
	listen_size: usize,
) -> Result<Array<Handle>, Error> {
	let std_sa = SocketAddr::from_str(&addr).unwrap();
	let inet_addr = InetAddr::from_std(&std_sa);
	let sock_addr = SockAddr::new_inet(inet_addr);
	let mut ret = array!(size, &0)?;
	for i in 0..size {
		let fd = get_socket()?;
		bind(fd, &sock_addr)?;
		listen(fd, listen_size)?;
		ret[i] = fd;
	}

	Ok(ret)
}

fn get_socket() -> Result<RawFd, Error> {
	let raw_fd = socket(
		AddressFamily::Inet,
		SockType::Stream,
		SockFlag::empty(),
		None,
	)?;

	let optval: libc::c_int = 1;
	unsafe {
		libc::setsockopt(
			raw_fd,
			libc::SOL_SOCKET,
			libc::SO_REUSEPORT,
			&optval as *const _ as *const libc::c_void,
			mem::size_of_val(&optval) as libc::socklen_t,
		)
	};

	unsafe {
		libc::setsockopt(
			raw_fd,
			libc::SOL_SOCKET,
			libc::SO_REUSEADDR,
			&optval as *const _ as *const libc::c_void,
			mem::size_of_val(&optval) as libc::socklen_t,
		)
	};

	Ok(raw_fd)
}

pub(crate) fn get_events_impl(
	config: &EventHandlerConfig,
	ctx: &mut EventHandlerContext,
	wakeup_requested: bool,
) -> Result<usize, Error> {
	todo!()
}
