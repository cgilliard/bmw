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

use bmw_err::Error;
use bmw_log::*;
use bmw_util::Context;
use bmw_util::*;
use bmw_util::{
	SlabAllocatorBuilder, SlabAllocatorConfig, StaticHashtableBuilder, StaticHashtableConfig,
};
use clap::{load_yaml, App};
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::Instant;

debug!();

// include build information
pub mod built_info {
	include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

const KEY_LEN: usize = 8;
const VALUE_LEN: usize = 8;

struct MonAllocator;

static mut MEM_ALLOCATED: usize = 0;
static mut MEM_DEALLOCATED: usize = 0;

unsafe impl GlobalAlloc for MonAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		MEM_ALLOCATED += layout.size();
		System.alloc(layout)
	}

	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		MEM_DEALLOCATED += layout.size();
		System.dealloc(ptr, layout)
	}
}

#[global_allocator]
static GLOBAL: MonAllocator = MonAllocator;

fn do_test_list() -> Result<(), Error> {
	init_slab_allocator!(48, 131072);

	{
		let mut list = list![];
		let mut vec = vec![];
		for _ in 0..131072 {
			let r = rand::random::<i32>();
			list.push(&r)?;
			vec.push(r);
		}
		let now = Instant::now();
		list.sort()?;
		let elapsed = now.elapsed();
		info!("list sort took {:?}", elapsed)?;

		let now = Instant::now();
		vec.sort();
		let elapsed = now.elapsed();
		info!("vec sort took {:?}", elapsed)?;

		let mut i = 0;
		for x in &list {
			assert_eq!(x, vec[i]);
			i += 1;
		}
		assert_eq!(i, vec.len());
		for x in list.iter_rev() {
			i -= 1;
			assert_eq!(x, vec[i]);
		}
		assert_eq!(i, 0);
	}

	let mut list = list![];
	list.push(&1)?;
	let vec: Vec<i32> = vec![0i32];

	let mut nlist = list![];
	let now = Instant::now();
	for _ in 0..10_000 {
		//let mut nlist = list![];
		nlist.append(&list)?;
		nlist.clear()?;
	}
	let elapsed = now.elapsed();
	info!(
		"List append={}ms",
		elapsed.as_nanos() as f64 / 1_000_000 as f64
	)?;

	let now = Instant::now();
	for _ in 0..10_000 {
		let mut nvec: Vec<i32> = vec![];
		nvec.extend(&vec);
	}
	let elapsed = now.elapsed();
	info!(
		"Vec append={}ms",
		elapsed.as_nanos() as f64 / 1_000_000 as f64
	)?;

	Ok(())
}

fn main() -> Result<(), Error> {
	log_init!(LogConfig {
		show_bt: bmw_log::LogConfigOption::ShowBt(false),
		line_num: bmw_log::LogConfigOption::LineNum(false),
		..Default::default()
	})?;

	let yml = load_yaml!("hash_perf.yml");
	let args = App::from_yaml(yml)
		.version(built_info::PKG_VERSION)
		.get_matches();

	let test_list = args.is_present("test_list");

	if test_list {
		do_test_list()?;
		return Ok(());
	}
	let count = match args.is_present("count") {
		true => args.value_of("count").unwrap().parse()?,
		false => 1_000,
	};
	let static_hash_size = match args.is_present("size") {
		true => args.value_of("size").unwrap().parse()?,
		false => 2_000,
	};
	let no_gets = args.is_present("no_gets");
	let do_static = args.is_present("do_static");
	let do_hash = args.is_present("do_hash");
	let itt = match args.is_present("itt") {
		true => args.value_of("itt").unwrap().parse()?,
		false => 1,
	};
	let slab_size = match args.is_present("slab_size") {
		true => args.value_of("slab_size").unwrap().parse()?,
		false => 48,
	};
	let slab_count = match args.is_present("slab_count") {
		true => args.value_of("slab_count").unwrap().parse()?,
		false => 10_000,
	};
	let park = args.is_present("park");
	let get_count = match args.is_present("get_count") {
		true => args.value_of("get_count").unwrap().parse()?,
		false => 100,
	};

	let _iterator = args.is_present("with_iterator");

	if do_static && do_hash {
		error!("You can only do either --do_static or --do_hash, not both")?;
		return Ok(());
	}

	info!("Starting tests")?;
	if do_static {
		for _ in 0..itt {
			let now = Instant::now();
			{
				let sconf = SlabAllocatorConfig {
					slab_size,
					slab_count,
					..Default::default()
				};
				let shconfig = StaticHashtableConfig {
					max_entries: static_hash_size,
					max_load_factor: 1.0,
					..Default::default()
				};
				let mut slabs = SlabAllocatorBuilder::build();
				slabs.init(sconf)?;
				let mut sh = StaticHashtableBuilder::build(shconfig, Some(slabs))?;
				//let mut sh = StaticHashtableBuilder::build::<(), ()>(shconfig, None)?;

				let mut keys = vec![];
				let mut values = vec![];

				for _ in 0..count {
					let key: [u8; KEY_LEN] = rand::random();
					let value: [u8; VALUE_LEN] = rand::random();
					keys.push(key);
					values.push(value);
				}
				let mut context = Context::new();
				for i in 0..count {
					sh.insert(&keys[i], &values[i])?;
					if !no_gets {
						for _ in 0..get_count {
							assert_eq!(sh.get(&keys[i])?, Some(values[i]));
						}
					}
				}

				info!("Memory used (pre_drop) = {}mb", mem_used())?;

				info!("Memory Allocated (pre_drop) = {}mb", mem_alloc())?;

				info!("Memory De-allocated (pre_drop) = {}mb", mem_dealloc())?;
			}

			info!("Memory used (post_drop) = {}mb", mem_used())?;

			info!("Memory Allocated (post_drop) = {}mb", mem_alloc())?;

			info!("Memory De-allocated (post_drop) = {}mb", mem_dealloc())?;

			info!(
				"(StaticHash) Elapsed time = {:.2}ms",
				now.elapsed().as_nanos() as f64 / 1_000_000 as f64
			)?;
		}
	}

	if do_hash {
		for _ in 0..itt {
			let now = Instant::now();
			{
				let mut hash_map = HashMap::new();
				let mut keys = vec![];
				let mut values = vec![];

				for _ in 0..count {
					let key: [u8; KEY_LEN] = rand::random();
					let value: [u8; VALUE_LEN] = rand::random();
					keys.push(key);
					values.push(value);
				}
				for i in 0..count {
					hash_map.insert(&keys[i], &values[i]);
					if !no_gets {
						for _ in 0..get_count {
							hash_map.get(&keys[i]);
						}
					}
				}

				info!("Memory used (pre_drop) = {}mb", mem_used())?;

				info!("Memory Allocated (pre_drop) = {}mb", mem_alloc())?;

				info!("Memory De-allocated (pre_drop) = {}mb", mem_dealloc())?;
			}

			info!("Memory used (post drop) = {}mb", mem_used())?;

			info!("Memory Allocated (post_drop) = {}mb", mem_alloc())?;

			info!("Memory De-allocated (post_drop) = {}mb", mem_dealloc())?;

			info!(
				"(HashMap) Elapsed time = {:.2}ms",
				now.elapsed().as_nanos() as f64 / 1_000_000 as f64
			)?;
		}
	}

	if park {
		std::thread::park();
	}

	Ok(())
}

fn mem_used() -> f64 {
	unsafe { MEM_ALLOCATED }.saturating_sub(unsafe { MEM_DEALLOCATED }) as f64 / 1_000_000 as f64
}

fn mem_alloc() -> f64 {
	(unsafe { MEM_ALLOCATED }) as f64 / 1_000_000 as f64
}

fn mem_dealloc() -> f64 {
	(unsafe { MEM_DEALLOCATED }) as f64 / 1_000_000 as f64
}
