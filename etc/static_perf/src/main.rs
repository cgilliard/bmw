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

use crate::ConfigOption::*;
use bmw_err::*;
use bmw_log::*;
use bmw_util::*;
use clap::{load_yaml, App};
use std::cell::RefMut;
use std::collections::HashMap;
use std::time::Instant;

info!();

fn main() -> Result<(), Error> {
	let yml = load_yaml!("static_perf.yml");
	let args = App::from_yaml(yml).version("1.0").get_matches();

	let global = args.is_present("global");
	let itt = match args.is_present("itt") {
		true => args.value_of("itt").unwrap().parse()?,
		false => 10_000,
	};

	init_slab_allocator!(SlabSize(48), SlabCount(itt))?;
	let slabs = SlabAllocatorBuilder::build_ref();
	let config = SlabAllocatorConfig {
		slab_size: 48,
		slab_count: itt,
		..Default::default()
	};
	{
		let mut slabs: RefMut<_> = slabs.borrow_mut();
		slabs.init(config)?;
	}

	/*
	let slabs2 = SlabAllocatorBuilder::build_ref();
	let config = SlabAllocatorConfig {
		slab_size: 48,
		slab_count: 10_000,
		..Default::default()
	};

	{
		let mut slabs2: RefMut<_> = slabs2.borrow_mut();
		slabs2.init(config)?;
	}
		*/

	{
		let mut h = if global {
			hashtable!(MaxEntries(itt))?
		} else {
			let config = StaticHashtableConfig {
				max_entries: itt,
				..Default::default()
			};
			StaticBuilder::build_hashtable(config, Some(slabs.clone()))?
		};

		let now = Instant::now();
		for i in 0..itt {
			h.insert(&i, &i)?;
		}
		let elapsed = now.elapsed();
		let x_micros = elapsed.as_micros();
		info!("hashtable insert elapsed={:?}", elapsed)?;

		let now = Instant::now();
		let mut h2 = HashMap::new();
		let mut v = vec![];
		for i in 0..itt {
			v.push(i);
		}
		for i in 0..itt {
			h2.insert(&v[i], &v[i]);
		}
		let elapsed = now.elapsed();
		info!(
			"hashmap insert elapsed={:?},delta={}",
			elapsed,
			x_micros as f64 / elapsed.as_micros() as f64
		)?;

		let now = Instant::now();
		for i in 0..itt {
			h.get(&v[i])?;
		}
		let elapsed = now.elapsed();
		let x_micros = elapsed.as_micros();
		info!("hashtable get elapsed={:?}", elapsed)?;

		let now = Instant::now();
		for i in 0..itt {
			h2.get(&v[i]);
		}
		let elapsed = now.elapsed();
		info!(
			"hashmap get elapsed={:?},delta={}",
			elapsed,
			x_micros as f64 / elapsed.as_micros() as f64
		)?;
	}

	let mut list = if !global {
		StaticBuilder::build_list(StaticListConfig::default(), Some(slabs))?
	} else {
		list![]
	};
	let now = Instant::now();
	for i in 0..itt {
		list.push(i)?;
	}
	let elapsed = now.elapsed();
	let x_micros = elapsed.as_micros();
	info!("list insert = {:?}", elapsed)?;
	let now = Instant::now();
	let mut vec = vec![];
	for i in 0..itt {
		vec.push(i);
	}
	let elapsed = now.elapsed();
	info!(
		"vec insert elapsed={:?},delta={}",
		elapsed,
		x_micros as f64 / elapsed.as_micros() as f64
	)?;

	let now = Instant::now();
	for x in list.iter() {
		if x > itt {
			info!("do something")?;
		}
	}
	let elapsed = now.elapsed();
	let x_micros = elapsed.as_nanos();
	info!("list iter elapsed={:?}", elapsed)?;

	let now = Instant::now();
	for x in vec {
		if x > itt {
			info!("do something")?;
		}
	}
	let elapsed = now.elapsed();
	info!(
		"vec iter elapsed={:?},delta={}",
		elapsed,
		x_micros as f64 / elapsed.as_nanos() as f64
	)?;
	Ok(())
}
