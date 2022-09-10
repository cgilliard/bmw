// Copyright (c) 2022, 37 Miners, LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use bmw_deps::num_format::{Locale, ToFormattedString};
use bmw_err::*;
use bmw_evh::*;
use bmw_log::LogConfigOption::*;
use bmw_log::*;
use bmw_util::*;
use clap::{load_yaml, App, ArgMatches};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::TcpStream;
use std::rc::Rc;
use std::sync::mpsc::sync_channel;
use std::time::Instant;

#[cfg(unix)]
use std::os::unix::io::IntoRawFd;
info!();

struct MonAllocator;

static mut MEM_ALLOCATED: usize = 0;
static mut MEM_DEALLOCATED: usize = 0;
static mut LAST_MEMUSED: usize = 0;
static mut ALLOC_COUNT: usize = 0;
static mut DEALLOC_COUNT: usize = 0;

unsafe impl GlobalAlloc for MonAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		MEM_ALLOCATED += layout.size();
		ALLOC_COUNT += 1;
		System.alloc(layout)
	}

	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		MEM_DEALLOCATED += layout.size();
		DEALLOC_COUNT += 1;
		System.dealloc(ptr, layout)
	}
}

#[global_allocator]
static GLOBAL: MonAllocator = MonAllocator;

fn show_mem(start: Instant, msg: &str) -> Result<(), Error> {
	let elapsed = start.elapsed();
	let delta = unsafe { MEM_ALLOCATED as i64 - MEM_DEALLOCATED as i64 };
	let alloc = unsafe { MEM_ALLOCATED };
	let dealloc = unsafe { MEM_DEALLOCATED };
	let alloc_count = unsafe { ALLOC_COUNT };
	let dealloc_count = unsafe { DEALLOC_COUNT };

	info!(
		"{}: alloc: {}, dealloc: {}, alloc_qty: {}, dealloc_qty: {}, delta: {}, elapsed: {:?}",
		msg,
		alloc.to_formatted_string(&Locale::en),
		dealloc.to_formatted_string(&Locale::en),
		alloc_count.to_formatted_string(&Locale::en),
		dealloc_count.to_formatted_string(&Locale::en),
		delta.to_formatted_string(&Locale::en),
		elapsed,
	)?;
	Ok(())
}

fn reset_stats() -> Result<(), Error> {
	unsafe { LAST_MEMUSED = 0 };
	unsafe { MEM_ALLOCATED = 0 };
	unsafe { MEM_DEALLOCATED = 0 };
	unsafe { ALLOC_COUNT = 0 };
	unsafe { DEALLOC_COUNT = 0 };
	Ok(())
}

// include build information
pub mod built_info {
	include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

fn run_eventhandler(args: ArgMatches) -> Result<(), Error> {
	let threads: usize = match args.is_present("threads") {
		true => args.value_of("threads").unwrap().parse()?,
		false => 1,
	};
	let port = 8081;
	info!("Using port: {}", port)?;
	let addr = &format!("127.0.0.1:{}", port)[..];
	let config = EventHandlerConfig {
		threads,
		housekeeping_frequency_millis: 10_000,
		read_slab_count: 20,
		max_handles_per_thread: 30,
		..Default::default()
	};
	let mut evh = bmw_evh::Builder::build_evh(config)?;

	let mut close_count = lock_box!(0)?;
	let close_count_clone = close_count.clone();

	evh.set_on_read(move |conn_data, _thread_context| {
		debug!("on read fs = {}", conn_data.slab_offset())?;
		let first_slab = conn_data.first_slab();
		let slab_offset = conn_data.slab_offset();
		let res = conn_data.borrow_slab_allocator(move |sa| {
			let slab = sa.get(first_slab.try_into()?)?;
			debug!("read bytes = {:?}", &slab.get()[0..slab_offset as usize])?;
			let mut ret: Vec<u8> = vec![];
			ret.extend(&slab.get()[0..slab_offset as usize]);
			Ok(ret)
		})?;
		conn_data.clear_through(first_slab)?;
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
		let mut close_count = close_count.wlock()?;
		(**close_count.guard()) += 1;
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

	std::thread::park();

	Ok(())
}

fn run_client(args: ArgMatches) -> Result<(), Error> {
	let port = 8081;
	let itt: usize = match args.is_present("itt") {
		true => args.value_of("itt").unwrap().parse()?,
		false => 1_000,
	};
	let count: usize = match args.is_present("count") {
		true => args.value_of("count").unwrap().parse()?,
		false => 10,
	};
	let clients: usize = match args.is_present("clients") {
		true => args.value_of("clients").unwrap().parse()?,
		false => 1,
	};
	let total = count * itt * clients;

	info!("itt={},count={},clients={}", itt, count, clients)?;

	info!("Using port: {}", port)?;
	let addr = &format!("127.0.0.1:{}", port)[..];
	let threads = clients;
	let config = EventHandlerConfig {
		threads,
		housekeeping_frequency_millis: 10_000,
		read_slab_count: 2,
		max_handles_per_thread: 3,
		..Default::default()
	};
	let mut evh = bmw_evh::Builder::build_evh(config)?;

	let mut recv_count = lock_box!(0usize)?;
	let mut recv_count_clone = recv_count.clone();
	let (tx, rx) = sync_channel(1);
	let mut sender = lock_box!(tx)?;

	evh.set_on_read(move |conn_data, _thread_context| {
		debug!("on read fs = {}", conn_data.slab_offset())?;
		let first_slab = conn_data.first_slab();
		let slab_offset = conn_data.slab_offset();
		let last_slab = conn_data.last_slab();
		let mut sender = sender.clone();
		let mut recv_count = recv_count.clone();
		conn_data.borrow_slab_allocator(move |sa| {
			let slab = sa.get(first_slab.try_into()?)?;
			debug!(
				"read bytes[{}] = {:?}",
				slab_offset,
				&slab.get()[0..slab_offset as usize]
			)?;
			let mut recv_count = recv_count.wlock()?;
			let guard = recv_count.guard();
			(**guard) += slab_offset as usize;
			if (**guard) >= count * 4 * threads {
				let mut tx = sender.wlock()?;
				(**tx.guard()).send(1)?;
			}
			Ok(())
		})?;
		conn_data.clear_through(last_slab)?;
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

	let mut whs = vec![];
	for i in 0..clients {
		let connection = TcpStream::connect(addr)?;
		#[cfg(unix)]
		let connection_handle = connection.into_raw_fd();
		#[cfg(windows)]
		let connection_handle = connection.into_raw_socket();

		let client = ClientConnection {
			handle: connection_handle,
			tls_config: None,
		};
		let mut wh = evh.add_client(client)?;
		whs.push(wh);
	}

	let now = Instant::now();
	for i in 0..itt {
		for _ in 0..count {
			for wh in &mut whs {
				wh.write(b"test")?;
			}
		}

		let ret = rx.recv()?;
		{
			let mut recv_count = recv_count_clone.wlock()?;
			let guard = recv_count.guard();
			(**guard) = 0;
		}
	}
	let elapsed = now.elapsed();
	let elapsed_nanos = elapsed.as_nanos() as f64;
	let qps = ((itt * count * clients) as f64 / elapsed_nanos) * 1_000_000_000.0;
	info!(
		"received {} messages in {:?}, QPS = {}",
		total, elapsed, qps,
	)?;

	Ok(())
}

fn main() -> Result<(), Error> {
	global_slab_allocator!()?;
	log_init!(LogConfig {
		show_bt: ShowBt(false),
		..Default::default()
	})?;

	let yml = load_yaml!("perf.yml");
	let args = App::from_yaml(yml)
		.version(built_info::PKG_VERSION)
		.get_matches();

	let client = args.is_present("client");
	let eventhandler = args.is_present("eventhandler");

	if client {
		info!("Starting perf client")?;
		run_client(args)?;
	} else if eventhandler {
		info!("Starting eventhandler")?;
		run_eventhandler(args)?;
	}

	Ok(())
}
