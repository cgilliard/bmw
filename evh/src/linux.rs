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
	Event, EventHandlerConfig, EventHandlerContext, EventType, EventTypeIn, Handle,
};
use bmw_deps::errno::{errno, set_errno, Errno};
use bmw_deps::libc::{
	self, accept, c_void, close, fcntl, pipe, read, sockaddr, write, F_SETFL, O_NONBLOCK,
};
use bmw_deps::nix::sys::epoll::{epoll_ctl, epoll_wait, EpollEvent, EpollFlags, EpollOp};
use bmw_deps::nix::sys::socket::{
	bind, listen, socket, AddressFamily, InetAddr, SockAddr, SockFlag, SockType,
};
use bmw_err::*;
use bmw_log::*;
use bmw_util::*;
use std::mem::{self, size_of, zeroed};
use std::net::SocketAddr;
use std::net::TcpStream;
use std::os::raw::c_int;
use std::os::unix::prelude::RawFd;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

info!();

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

pub(crate) fn close_impl(ctx: &mut EventHandlerContext, handle: Handle) -> Result<(), Error> {
	let handle_as_usize = handle.try_into()?;
	debug!("filter set remove {}, tid={}", handle_as_usize, ctx.tid)?;
	ctx.filter_set.replace(handle_as_usize, false);
	unsafe {
		close(handle);
	}
	Ok(())
}

pub(crate) fn close_handle_impl(handle: Handle) -> Result<(), Error> {
	unsafe {
		close(handle);
	}
	Ok(())
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

	debug!("accept handle = {}", handle)?;

	if handle < 0 {
		if errno().0 == libc::EAGAIN {
			// would block, return the negative number
			return Ok(handle);
		}
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
		unsafe {
			fcntl(fd, F_SETFL, O_NONBLOCK);
		}
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
	debug!("in get_events_impl in_count={}", ctx.events_in.len())?;
	for evt in &ctx.events_in {
		let mut interest = EpollFlags::empty();
		if evt.etype == EventTypeIn::Read
			|| evt.etype == EventTypeIn::Accept
			|| evt.etype == EventTypeIn::Resume
		{
			let fd = evt.handle;
			debug!("add in read fd = {},tid={}", fd, ctx.tid)?;
			if fd >= ctx.filter_set.len().try_into()? {
				ctx.filter_set.resize((fd + 100).try_into()?, false);
			}

			interest |= EpollFlags::EPOLLIN;
			interest |= EpollFlags::EPOLLET;
			interest |= EpollFlags::EPOLLRDHUP;

			let handle_as_usize: usize = fd.try_into()?;
			let op = match ctx.filter_set.get(handle_as_usize) {
				Some(bitref) => {
					if *bitref {
						debug!("found {}. using mod", handle_as_usize)?;
						EpollOp::EpollCtlMod
					} else {
						debug!("not found {}. using add", handle_as_usize)?;
						EpollOp::EpollCtlAdd
					}
				}
				None => {
					debug!("not found {} (none). using add", handle_as_usize)?;
					EpollOp::EpollCtlAdd
				}
			};
			ctx.filter_set.replace(handle_as_usize, true);
			let mut event = EpollEvent::new(interest, evt.handle.try_into()?);
			let res = epoll_ctl(ctx.selector, op, evt.handle, &mut event);
			match res {
				Ok(_) => {}
				Err(e) => error!(
					"Error epoll_ctl2: {}, fd={}, op={:?},tid={}",
					e, fd, op, ctx.tid
				)?,
			}
		} else if evt.etype == EventTypeIn::Write {
			let fd = evt.handle;
			debug!("add in write fd = {},tid={}", fd, ctx.tid)?;
			if fd > ctx.filter_set.len().try_into()? {
				ctx.filter_set.resize((fd + 100).try_into()?, true);
			}
			interest |= EpollFlags::EPOLLOUT;
			interest |= EpollFlags::EPOLLIN;
			interest |= EpollFlags::EPOLLRDHUP;
			interest |= EpollFlags::EPOLLET;

			let handle_as_usize: usize = fd.try_into()?;
			let op = match ctx.filter_set.get(handle_as_usize) {
				Some(bitref) => {
					if *bitref {
						EpollOp::EpollCtlMod
					} else {
						EpollOp::EpollCtlAdd
					}
				}
				None => EpollOp::EpollCtlAdd,
			};
			ctx.filter_set.set(handle_as_usize, true);

			let mut event = EpollEvent::new(interest, evt.handle.try_into()?);
			let res = epoll_ctl(ctx.selector, op, evt.handle, &mut event);
			match res {
				Ok(_) => {}
				Err(e) => error!(
					"Error epoll_ctl1: {}, fd={}, op={:?},tid={}",
					e, fd, op, ctx.tid
				)?,
			}
		} else if evt.etype == EventTypeIn::Suspend {
			let fd = evt.handle;
			debug!("add in write fd = {},tid={}", fd, ctx.tid)?;
			if fd > ctx.filter_set.len().try_into()? {
				ctx.filter_set.resize((fd + 100).try_into()?, true);
			}

			let handle_as_usize: usize = fd.try_into()?;
			ctx.filter_set.set(handle_as_usize, false);
			let op = EpollOp::EpollCtlDel;

			let mut event = EpollEvent::new(interest, evt.handle.try_into()?);
			let res = epoll_ctl(ctx.selector, op, evt.handle, &mut event);
			match res {
				Ok(_) => {}
				Err(e) => error!(
					"Error epoll_ctl1: {}, fd={}, op={:?},tid={}",
					e, fd, op, ctx.tid
				)?,
			}
		}
	}
	ctx.events_in.clear();
	ctx.events_in.shrink_to(config.max_events_in);

	let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
	let diff = now - ctx.now;
	let sleep = if wakeup_requested {
		0
	} else {
		config.housekeeping_frequency_millis.saturating_sub(diff)
	}
	.try_into()?;

	debug!("wakeup req = {}", wakeup_requested)?;
	let results = epoll_wait(ctx.selector, &mut ctx.epoll_events, sleep);

	ctx.now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();

	let mut res_count = 0;
	match results {
		Ok(results) => {
			if results > 0 {
				for i in 0..results {
					if !(ctx.epoll_events[i].events() & EpollFlags::EPOLLOUT).is_empty() {
						ctx.events[res_count] = Event {
							handle: ctx.epoll_events[i].data() as Handle,
							etype: EventType::Write,
						};
						res_count += 1;
					}
					if !(ctx.epoll_events[i].events() & EpollFlags::EPOLLIN).is_empty() {
						ctx.events[res_count] = Event {
							handle: ctx.epoll_events[i].data() as Handle,
							etype: EventType::Read,
						};
						res_count += 1;
					}
				}
			}
		}
		Err(e) => {
			error!("epoll wait generated error: {}", e.to_string())?;
		}
	}

	Ok(res_count)
}
