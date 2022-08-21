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
use std::collections::HashMap;
use std::time::Instant;

info!();

fn main() -> Result<(), Error> {
	init_slab_allocator!(SlabSize(48), SlabCount(5000))?;
	let mut h = hashtable!(MaxEntries(2000))?;

	let now = Instant::now();
	for i in 0..1000 {
		h.insert(&i, &i)?;
	}
	let elapsed = now.elapsed();
	info!("hashtable elapsed={:?}", elapsed)?;

	let now = Instant::now();
	let mut h2 = HashMap::new();
	let mut v = vec![];
	for i in 0..1000 {
		v.push(i);
	}
	for i in 0..1000 {
		h2.insert(&v[i], &v[i]);
	}
	let elapsed = now.elapsed();
	info!("hashmap elapsed={:?}", elapsed)?;

	let now = Instant::now();
	for i in 0..1000 {
		h.get(&v[i])?;
	}
	let elapsed = now.elapsed();
	info!("hashtable elapsed={:?}", elapsed)?;

	let now = Instant::now();
	for i in 0..1000 {
		h2.get(&v[i]);
	}
	let elapsed = now.elapsed();
	info!("hashmap elapsed={:?}", elapsed)?;

	let now = Instant::now();
	let mut list = list![];
	for i in 0..1000 {
		list.push(i)?;
	}
	let elapsed = now.elapsed();
	info!("list elapsed={:?}", elapsed)?;

	let now = Instant::now();
	let mut vec = vec![];
	for i in 0..1000 {
		vec.push(i);
	}
	let elapsed = now.elapsed();
	info!("vec elapsed={:?}", elapsed)?;

	let now = Instant::now();
	for x in list.iter() {
		if x > 1000 {
			info!("do something")?;
		}
	}
	let elapsed = now.elapsed();
	info!("list iter elapsed={:?}", elapsed)?;

	let now = Instant::now();
	for x in vec {
		if x > 1000 {
			info!("do something")?;
		}
	}
	let elapsed = now.elapsed();
	info!("vec iter elapsed={:?}", elapsed)?;

	Ok(())
}
