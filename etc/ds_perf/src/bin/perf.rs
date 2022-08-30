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
use bmw_log::LogConfigOption::*;
use bmw_log::*;
use bmw_util::*;
use clap::{load_yaml, App};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

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

fn do_arraylist() -> Result<(), Error> {
	info!("testing arraylist")?;
	let mut start;
	{
		reset_stats()?;
		start = Instant::now();
		let mut arraylist = array_list!(10_000)?;
		show_mem(start, "arraylist init")?;
		reset_stats()?;
		start = Instant::now();
		for i in 0..10_000 {
			arraylist.push(i as u32)?;
		}
		show_mem(start, "arraylist insert")?;
		reset_stats()?;
		start = Instant::now();
		let mut count = 0;
		for _x in arraylist.iter() {
			count += 1;
		}
		assert_eq!(count, 10_000);
		show_mem(start, "arraylist iter")?;
		start = Instant::now();
	}
	show_mem(start, "arraylist drop")?;
	Ok(())
}

fn do_array() -> Result<(), Error> {
	info!("Testing array")?;
	let mut start;
	{
		reset_stats()?;
		start = Instant::now();
		let mut array = array!(10_000)?;
		show_mem(start, "array init")?;
		reset_stats()?;
		start = Instant::now();
		for i in 0..10_000 {
			array[i] = i as u32;
		}

		show_mem(start, "array insert")?;
		reset_stats()?;
		start = Instant::now();

		let mut count = 0;
		for _x in array.iter() {
			count += 1;
		}
		assert_eq!(count, 10_000);
		show_mem(start, "array iter")?;
		reset_stats()?;
		start = Instant::now();
	}
	show_mem(start, "array drop")?;
	Ok(())
}

fn do_hashtable(slabs: Rc<RefCell<dyn SlabAllocator>>) -> Result<(), Error> {
	info!("Testing hashtable")?;
	let mut start;
	{
		reset_stats()?;
		start = Instant::now();
		let mut hashtable = hashtable!(MaxEntries(10_000), Slabs(slabs.clone()))?;

		show_mem(start, "hashtable init")?;
		reset_stats()?;
		start = Instant::now();

		for i in 0..10_000 {
			hashtable.insert(&(i as u32), &(i as u32))?;
		}
		show_mem(start, "hashtable insert")?;
		reset_stats()?;
		start = Instant::now();

		for i in 0..10_000 {
			assert!(hashtable.get(&i)?.is_some());
		}
		show_mem(start, "hashtable get")?;
		reset_stats()?;
		start = Instant::now();
	}
	show_mem(start, "hashtable drop")?;

	Ok(())
}

fn do_hashmap() -> Result<(), Error> {
	info!("Testing hashmap")?;
	let mut start;
	{
		reset_stats()?;
		start = Instant::now();
		let mut hashmap = HashMap::new();
		show_mem(start, "hashmap init")?;
		reset_stats()?;
		start = Instant::now();
		let mut v = vec![];
		for i in 0..10_000 {
			v.push(i as u32);
		}

		for i in 0..10_000 {
			hashmap.insert(&v[i], &v[i]);
		}

		show_mem(start, "hashmap insert")?;
		reset_stats()?;
		start = Instant::now();

		for i in 0..10_000 {
			assert!(hashmap.get(&i).is_some());
		}
		show_mem(start, "hashmap get")?;
		reset_stats()?;
		start = Instant::now();
	}
	show_mem(start, "hashmap drop")?;
	Ok(())
}

fn do_vec() -> Result<(), Error> {
	info!("testing vec")?;
	let mut start;
	{
		reset_stats()?;
		start = Instant::now();
		let mut vec = vec![];
		show_mem(start, "vec init")?;
		reset_stats()?;
		start = Instant::now();
		for i in 0..10_000 {
			vec.push(i as u32);
		}
		show_mem(start, "vec insert")?;
		reset_stats()?;
		start = Instant::now();
		let mut count = 0;
		for _x in &vec {
			count += 1;
		}
		show_mem(start, "vec iter")?;
		assert_eq!(count, 10_000);
		reset_stats()?;
		start = Instant::now();
	}
	show_mem(start, "vec drop")?;
	Ok(())
}

fn main() -> Result<(), Error> {
	init_slab_allocator!()?;
	log_init!(LogConfig {
		level: Level(false),
		line_num: LineNum(false),
		..Default::default()
	})?;

	let yml = load_yaml!("ds_perf.yml");
	let args = App::from_yaml(yml).version("1.0").get_matches();

	info!("Starting ds_perf")?;

	let slabs = slab_allocator!(SlabSize(16), SlabCount(10_000))?;
	let hashtable = args.is_present("hashtable");
	let arraylist = args.is_present("arraylist");
	let vec = args.is_present("vec");
	let array = args.is_present("array");
	let hashmap = args.is_present("hashmap");

	if hashtable {
		do_hashtable(slabs.clone())?;
	}
	if hashmap {
		do_hashmap()?;
	}
	if arraylist {
		do_arraylist()?;
	}
	if vec {
		do_vec()?;
	}
	if array {
		do_array()?;
	}

	/*
	info!("ds_perf")?;
	show_mem()?;
	let mut list1 = list![1, 2, 3];
	show_mem()?;
	let list2 = list![1, 2, 3];
	show_mem()?;
	let list3 = Builder::build_list::<u32>(ListConfig {}, None)?;
	show_mem()?;
	list_append!(list1, list2);
	show_mem()?;
	let mut v: Vec<i32> = vec![1, 2, 3];
	show_mem()?;
	v.push(1i32);
	show_mem()?;
	let list_box = list_box![1i32, 2, 3];
	show_mem()?;
	let list_box = list_box![1i32, 2, 3, 4];
	show_mem()?;
	for x in list_box.iter() {}
	show_mem()?;
	for x in &v {}
	show_mem()?;
	{
		let mut hashtable = hashtable!()?;
		show_mem()?;
		hashtable.insert(&2000i32, &3i32)?;
		show_mem()?;
	}
	show_mem()?;
	let mut list_box2 = list_box![];
	show_mem()?;
	list_box2.push(1i16)?;
	show_mem()?;
	let mut hashmap = HashMap::new();
	show_mem()?;
	hashmap.insert(&1000i32, &2i32);
	show_mem()?;
	hashmap.insert(&2000i32, &2i32);
	show_mem()?;
	let mut v2 = vec![];
	for i in 0..1000 {
		v2.push(i);
	}
	for i in 0..1000 {
		hashmap.insert(&v2[i], &10);
	}
	show_mem()?;
	let mut hashtable = hashtable!()?;
	show_mem()?;
	for i in 0..1000 {
		hashtable.insert(&i, &i)?;
	}
	show_mem()?;
	{
		let mut array = array!(100)?;
		array[0] = 0u8;
		show_mem()?;
	}
	show_mem()?;
		*/

	Ok(())
}
