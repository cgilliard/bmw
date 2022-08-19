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

use crate::misc::{set_max, slice_to_usize, usize_to_slice};
use crate::ser::{SlabReader, SlabWriter};
use crate::slabs::Slab;
use crate::types::{Reader, SortableList, StaticListConfig};
use crate::{
	Serializable, SlabAllocator, SlabAllocatorConfig, SlabMut, StaticList, Writer,
	GLOBAL_SLAB_ALLOCATOR,
};
use bmw_err::*;
use bmw_log::*;
use std::cmp::Ordering;
use std::fmt::{Debug, Display, Formatter};
use std::marker::PhantomData;
use std::thread;

info!();

enum OpType {
	Front,
	Back,
}

pub enum IterType {
	Forward,
	Backward,
}

pub struct StaticListIterator<'a, V> {
	l: &'a StaticListImpl,
	cur: usize,
	direction: IterType,
	_phantom_data: PhantomData<V>,
}

impl<'a, V> Iterator for StaticListIterator<'a, V>
where
	V: Serializable,
{
	type Item = V;
	fn next(&mut self) -> Option<V> {
		match self.do_next() {
			Ok(next) => next,
			Err(e) => {
				let _ = error!("StaticListIterator.do_next generated error: {}", e);
				None
			}
		}
	}
}

impl<'a, V> StaticListIterator<'a, V>
where
	V: Serializable,
{
	fn new(direction: IterType, l: &'a StaticListImpl, cur: usize) -> Self {
		Self {
			l,
			_phantom_data: PhantomData,
			cur,
			direction,
		}
	}

	fn do_next(&mut self) -> Result<Option<V>, Error> {
		let ptr_size = self.l.ptr_size;
		if self.cur >= self.l.max_value {
			return Ok(None);
		}
		let mut prev = [0u8; 8];
		let mut next = [0u8; 8];

		let mut reader = self.l.get_reader_impl(self.cur)?;
		// read over prev/next
		reader.read_fixed_bytes(&mut prev[0..ptr_size])?;
		reader.read_fixed_bytes(&mut next[0..ptr_size])?;

		// read our serialized struct
		let ret = Some(V::read(&mut reader)?);
		match self.direction {
			IterType::Forward => {
				self.cur = slice_to_usize(&prev[0..ptr_size])?;
			}
			IterType::Backward => {
				self.cur = slice_to_usize(&next[0..ptr_size])?;
			}
		}
		Ok(ret)
	}
}

struct StaticListImpl {
	tail: usize,
	head: usize,
	size: usize,
	config: StaticListConfig,
	slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	slab_size: usize,
	bytes_per_slab: usize,
	ptr_size: usize,
	max_value: usize,
}

impl<V> PartialEq for dyn StaticList<V>
where
	V: Serializable + PartialEq,
{
	fn eq(&self, rhs: &Self) -> bool {
		if rhs.size() != self.size() {
			return false;
		}

		let mut itt1 = rhs.iter();
		let mut itt2 = self.iter();

		loop {
			let next1 = itt1.next();
			let next2 = itt2.next();

			if next1 != next2 {
				return false;
			}
			if next1.is_none() {
				break;
			}
		}

		true
	}
}

impl<V> Clone for Box<dyn StaticList<V>>
where
	V: Serializable + Clone,
{
	fn clone(&self) -> Self {
		let mut list = StaticListBuilder::build::<V>(self.config(), None).unwrap();
		for x in self.iter() {
			list.push(&x).unwrap();
		}
		list
	}
}

impl<V> Debug for dyn StaticList<V>
where
	V: Serializable + Debug,
{
	fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
		write!(f, "[")?;
		let mut i = 0;
		let size = self.size();
		for x in self.iter() {
			i += 1;
			if i != size {
				write!(f, "{:?}, ", x)?;
			} else {
				write!(f, "{:?}", x)?;
			}
		}
		write!(f, "]")
	}
}

impl<V> Display for dyn StaticList<V>
where
	V: Serializable + Display,
{
	fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
		write!(f, "[")?;
		let mut i = 0;
		let size = self.size();
		for x in self.iter() {
			i += 1;
			if i != size {
				write!(f, "{}, ", x)?;
			} else {
				write!(f, "{}", x)?;
			}
		}
		write!(f, "]")
	}
}

impl Drop for StaticListImpl {
	fn drop(self: &mut StaticListImpl) {
		match StaticListImpl::clear_impl(self) {
			Ok(_) => {}
			Err(e) => {
				let _ = error!("clear_impl generated error: {}", e);
			}
		}
	}
}

impl<V> StaticList<V> for StaticListImpl
where
	V: Serializable,
{
	fn config(&self) -> StaticListConfig {
		self.config.clone()
	}
	fn push(&mut self, value: &V) -> Result<(), Error> {
		let ret = self.push_impl(value, OpType::Back);
		ret
	}
	fn pop(&mut self) -> Result<Option<V>, Error> {
		let ret = self.pop_impl(OpType::Back);
		ret
	}
	fn push_front(&mut self, value: &V) -> Result<(), Error> {
		self.push_impl(value, OpType::Front)
	}
	fn pop_front(&mut self) -> Result<Option<V>, Error> {
		self.pop_impl(OpType::Front)
	}

	fn iter<'a>(&'a self) -> StaticListIterator<'a, V>
	where
		V: Serializable,
	{
		self.iter_impl(IterType::Forward, self.head)
	}

	fn iter_rev<'a>(&'a self) -> StaticListIterator<'a, V>
	where
		V: Serializable,
	{
		self.iter_impl(IterType::Backward, self.tail)
	}

	fn size(&self) -> usize {
		self.size
	}

	fn ptr_size(&self) -> usize {
		self.ptr_size
	}

	fn clear(&mut self) -> Result<(), Error> {
		self.clear_impl()
	}

	fn head(&self) -> usize {
		self.head
	}
	fn tail(&self) -> usize {
		self.tail
	}
	fn slab(&self, slab_id: usize) -> Result<Slab, Error> {
		self.get(slab_id)
	}
	fn swap(&mut self, slab_id1: usize, slab_id2: usize) -> Result<(), Error> {
		self.swap_impl(slab_id1, slab_id2)
	}
	fn merge(&mut self, ptr1: usize, ptr2: usize) -> Result<(), Error> {
		self.merge_impl(ptr1, ptr2)
	}

	fn get_reader(&mut self, slab_id: usize) -> Result<SlabReader, Error> {
		self.get_reader_impl(slab_id)
	}

	fn max_value(&self) -> usize {
		self.max_value
	}
}

impl<'a, V> IntoIterator for &'a Box<dyn StaticList<V>>
where
	V: Serializable,
{
	type Item = V;
	type IntoIter = StaticListIterator<'a, V>;

	fn into_iter(self) -> Self::IntoIter {
		self.iter()
	}
}

impl<V> SortableList<V> for dyn StaticList<V>
where
	V: Serializable + Ord + Debug,
{
	fn sort(&mut self) -> Result<(), Error> {
		sort_impl(self)
	}
}

fn sort_impl<V>(list: &mut dyn StaticList<V>) -> Result<(), Error>
where
	V: Serializable + Ord + Debug,
{
	let mut group_size = 2;
	let list_size = list.size();
	let mut ptr = list.head();
	let mut read_value_sum: usize = 0;
	loop {
		let mut i = 0;
		loop {
			if i >= list_size {
				break;
			}
			if group_size == 2 && i + 1 < list_size {
				let mut next = backward(list, ptr)?;

				let now = std::time::Instant::now();
				let first_value = read_value(list, ptr)?;
				let second_value = read_value(list, next)?;
				read_value_sum += now.elapsed().as_nanos() as usize;

				let cmp = first_value.cmp(&second_value);

				if cmp == Ordering::Greater {
					list.swap(ptr, next)?;
					next = backward(list, next)?;
				}

				ptr = backward(list, next)?;
			} else if group_size > 2 {
				let current_group_size = if i + group_size > list.size() {
					list.size() - i
				} else {
					group_size
				};
				debug!("curgrp={}", current_group_size)?;

				if current_group_size > 2 {
					let mut ptr2 = ptr;
					for _ in 0..(group_size / 2) {
						if ptr2 >= list.max_value() {
							// this is a list at the end,
							// we only need to process the ones that are at
							// least 50% of the expected full length of the
							// group
							break;
						}
						ptr2 = backward(list, ptr2)?;
					}

					debug!(
						"sort group with ptr1 = {}, ptr2= {}, group_size={}",
						ptr, ptr2, current_group_size
					)?;

					let ptr1 = ptr;
					ptr = merge_list(list, ptr1, ptr2, group_size, &mut read_value_sum)?;

					for _ in 0..current_group_size {
						ptr = backward(list, ptr)?;
					}
				} else if current_group_size > 0 {
					for _ in 0..current_group_size {
						ptr = backward(list, ptr)?;
					}
				}
			}
			i += group_size;
		}
		if group_size >= list_size {
			break;
		}
		group_size *= 2;
		ptr = list.head();
	}
	Ok(())
}

fn merge_list<V>(
	list: &mut dyn StaticList<V>,
	mut ptr1: usize,
	mut ptr2: usize,
	group_size: usize,
	read_value_sum: &mut usize,
) -> Result<usize, Error>
where
	V: Serializable + Ord + Debug,
{
	let hops = group_size / 2;
	let mut ptr1_hops = 0;
	let mut ptr2_hops = 0;
	let mut ret = ptr1;
	let mut first = true;
	loop {
		if ptr1_hops == hops || ptr2_hops == hops {
			debug!("break because hops {} {} {}", ptr1_hops, ptr2_hops, hops)?;
			break;
		}

		if ptr1 == ptr2 {
			debug!("break because ptr1 == ptr2 == {}", ptr1)?;
			break;
		}

		if ptr2 == list.max_value() {
			debug!("break because ptr2 = {}", ptr2)?;
			break;
		}

		let now = std::time::Instant::now();
		let v1 = read_value(list, ptr1)?;
		let v2 = read_value(list, ptr2)?;
		*read_value_sum += now.elapsed().as_nanos() as usize;

		if v1.cmp(&v2) == Ordering::Greater {
			debug!("merge swap at {} {} {:?} {:?}", ptr1, ptr2, v1, v2)?;
			let ptr2next = backward(list, ptr2)?;
			list.merge(ptr1, ptr2)?;
			if first {
				first = false;
				ret = ptr2;
			}
			ptr2 = ptr2next;

			ptr2_hops += 1;
		} else {
			debug!("merge no-op at {} {} {:?} {:?}", ptr1, ptr2, v1, v2)?;
			first = false;
			ptr1 = backward(list, ptr1)?;
			ptr1_hops += 1;
		}

		if ptr2 == list.max_value() {
			debug!("break because ptr2 == list.max_value")?;
			break;
		}
	}

	Ok(ret)
}

fn read_value<V>(list: &mut dyn StaticList<V>, slab_id: usize) -> Result<V, Error>
where
	V: Serializable + Ord,
{
	let ptr_size = list.ptr_size();
	let mut skip = [0u8; 8];
	// get a reader based on our target_id
	let mut reader = list.get_reader(slab_id)?;
	// read over prev/next
	reader.read_fixed_bytes(&mut skip[0..ptr_size])?;
	reader.read_fixed_bytes(&mut skip[0..ptr_size])?;
	// read our serailized struct
	V::read(&mut reader)
}

#[cfg(test)]
fn forward<V>(list: &mut dyn StaticList<V>, slab_id: usize) -> Result<usize, Error>
where
	V: Serializable + Ord,
{
	let ptr_size = list.ptr_size();
	let slab = list.slab(slab_id)?;
	slice_to_usize(&slab.get()[ptr_size..ptr_size * 2])
}

fn backward<V>(list: &mut dyn StaticList<V>, slab_id: usize) -> Result<usize, Error>
where
	V: Serializable + Ord,
{
	let ptr_size = list.ptr_size();
	let slab = list.slab(slab_id)?;
	slice_to_usize(&slab.get()[0..ptr_size])
}

impl StaticListImpl {
	fn new(
		config: StaticListConfig,
		slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	) -> Result<Self, Error> {
		let (slab_size, slab_count) = match slabs.as_ref() {
			Some(slabs) => ((slabs.slab_size()?, slabs.slab_count()?)),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(usize, usize), Error> {
				let slabs = unsafe { f.get().as_mut().unwrap() };
				let slab_size = match slabs.slab_size() {
					Ok(slab_size) => slab_size,
					Err(_e) => {
						let th = thread::current();
						let n = th.name().unwrap_or("unknown");
						println!(
							"WARN: Slab allocator was not initialized for thread '{}'. {}",
							n, "Initializing with default values.",
						);
						slabs.init(SlabAllocatorConfig::default())?;
						slabs.slab_size()?
					}
				};
				let slab_count = slabs.slab_count()?;
				Ok((slab_size, slab_count))
			})?,
		};

		let mut x = slab_count;
		let mut ptr_size = 0;
		loop {
			if x == 0 {
				break;
			}
			x >>= 8;
			ptr_size += 1;
		}
		let mut ptr = [0u8; 8];
		set_max(&mut ptr[0..ptr_size]);
		let max_value = slice_to_usize(&ptr[0..ptr_size])?;

		let bytes_per_slab = slab_size.saturating_sub(ptr_size);

		Ok(Self {
			config,
			slabs,
			tail: max_value,
			head: max_value,
			size: 0,
			slab_size,
			bytes_per_slab,
			ptr_size,
			max_value,
		})
	}

	fn free_chain(&mut self, slab_id: usize) -> Result<(), Error> {
		let bytes_per_slab = self.bytes_per_slab;
		let slab_size = self.slab_size;
		let mut next_bytes = slab_id;
		loop {
			let slab = self.get(next_bytes)?;
			let id = slab.id();
			next_bytes = slice_to_usize(&slab.get()[bytes_per_slab..slab_size])?;
			debug!("free id = {}, next_bytes={}", id, next_bytes)?;
			self.free(id)?;
			if next_bytes >= self.max_value {
				break;
			}
		}
		Ok(())
	}

	fn allocate(&mut self) -> Result<SlabMut, Error> {
		match &mut self.slabs {
			Some(slabs) => slabs.allocate(),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<SlabMut, Error> {
				let slabs = unsafe { f.get().as_mut().unwrap() };
				slabs.allocate()
			}),
		}
	}

	fn get(&self, slab_id: usize) -> Result<Slab, Error> {
		match &self.slabs {
			Some(slabs) => slabs.get(slab_id),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<Slab, Error> {
				let slabs = unsafe { f.get().as_mut().unwrap() };
				slabs.get(slab_id)
			}),
		}
	}

	fn free(&mut self, slab_id: usize) -> Result<(), Error> {
		match &mut self.slabs {
			Some(slabs) => slabs.free(slab_id),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(), Error> {
				let slabs = unsafe { f.get().as_mut().unwrap() };
				slabs.free(slab_id)
			}),
		}
	}

	#[cfg(test)]
	fn get_slab_ids(&self) -> Result<Vec<usize>, Error> {
		let mut prev = [0u8; 8];
		let mut ret = vec![];
		let ptr_size = self.ptr_size;
		let mut cur = self.head;

		loop {
			if cur >= self.max_value {
				break;
			}
			ret.push(cur);

			let mut reader = self.get_reader_impl(cur)?;
			// read over prev/next
			reader.read_fixed_bytes(&mut prev[0..ptr_size])?;
			cur = slice_to_usize(&prev[0..ptr_size])?;
		}

		Ok(ret)
	}

	fn merge_impl(&mut self, ptr1: usize, ptr2: usize) -> Result<(), Error> {
		if ptr1 == ptr2 {
			return Ok(());
		}

		let ptr_size = self.ptr_size;
		let mut next1 = [0u8; 8];
		let mut prev1 = [0u8; 8];
		let mut next2 = [0u8; 8];
		let mut prev2 = [0u8; 8];

		let mut next1next = [0u8; 8];
		let mut next1prev = [0u8; 8];

		let mut next2next = [0u8; 8];
		let mut next2prev = [0u8; 8];
		let mut prev2next = [0u8; 8];
		let mut prev2prev = [0u8; 8];

		let mut ptr1_bytes = [0u8; 8];
		let mut ptr2_bytes = [0u8; 8];

		usize_to_slice(ptr1, &mut ptr1_bytes[0..ptr_size])?;
		usize_to_slice(ptr2, &mut ptr2_bytes[0..ptr_size])?;

		let mut reader = self.get_reader_impl(ptr1)?;
		reader.read_fixed_bytes(&mut prev1[0..ptr_size])?;
		reader.read_fixed_bytes(&mut next1[0..ptr_size])?;

		let mut reader = self.get_reader_impl(ptr2)?;
		reader.read_fixed_bytes(&mut prev2[0..ptr_size])?;
		reader.read_fixed_bytes(&mut next2[0..ptr_size])?;

		let next1slab = slice_to_usize(&next1[0..ptr_size])?;

		let next2slab = slice_to_usize(&next2[0..ptr_size])?;
		let prev2slab = slice_to_usize(&prev2[0..ptr_size])?;

		if next2slab == ptr1 {
			self.swap_impl(ptr1, ptr2)?;
			return Ok(());
		}
		if next1slab == ptr2 {
			return Err(err!(
				ErrKind::IllegalArgument,
				"ptr2 cannot be adjacent and on the left of ptr1"
			));
		}

		if prev2slab < self.max_value {
			let mut reader = self.get_reader_impl(prev2slab)?;
			reader.read_fixed_bytes(&mut prev2prev[0..ptr_size])?;
			reader.read_fixed_bytes(&mut prev2next[0..ptr_size])?;
		}

		let mut reader = self.get_reader_impl(next2slab)?;
		reader.read_fixed_bytes(&mut next2prev[0..ptr_size])?;
		reader.read_fixed_bytes(&mut next2next[0..ptr_size])?;

		if next1slab < self.max_value {
			let mut reader = self.get_reader_impl(next1slab)?;
			reader.read_fixed_bytes(&mut next1prev[0..ptr_size])?;
			reader.read_fixed_bytes(&mut next1next[0..ptr_size])?;
		}

		let mut writer = self.get_writer_id(ptr2)?;
		writer.write_fixed_bytes(&ptr1_bytes[0..ptr_size])?;
		writer.write_fixed_bytes(&next1[0..ptr_size])?;

		let mut writer = self.get_writer_id(ptr1)?;
		writer.write_fixed_bytes(&prev1[0..ptr_size])?;
		writer.write_fixed_bytes(&ptr2_bytes[0..ptr_size])?;

		if prev2slab < self.max_value {
			let mut writer = self.get_writer_id(prev2slab)?;
			writer.write_fixed_bytes(&prev2prev[0..ptr_size])?;
			writer.write_fixed_bytes(&next2[0..ptr_size])?;
		}

		let mut writer = self.get_writer_id(next2slab)?;
		writer.write_fixed_bytes(&prev2[0..ptr_size])?;
		writer.write_fixed_bytes(&next2next[0..ptr_size])?;

		if next1slab < self.max_value {
			let mut writer = self.get_writer_id(next1slab)?;
			writer.write_fixed_bytes(&ptr2_bytes[0..ptr_size])?;
			writer.write_fixed_bytes(&next1next[0..ptr_size])?;
		}

		if ptr1 == self.head {
			self.head = ptr2;
		}
		if ptr2 == self.tail {
			self.tail = slice_to_usize(&next2[0..ptr_size])?;
		}

		Ok(())
	}

	fn swap_impl(&mut self, slab_id1: usize, slab_id2: usize) -> Result<(), Error> {
		if slab_id1 == slab_id2 {
			return Ok(());
		}
		let ptr_size = self.ptr_size;
		let mut next1 = [0u8; 8];
		let mut prev1 = [0u8; 8];
		let mut next2 = [0u8; 8];
		let mut prev2 = [0u8; 8];

		let mut next1next = [0u8; 8];
		let mut next1prev = [0u8; 8];
		let mut prev1next = [0u8; 8];
		let mut prev1prev = [0u8; 8];

		let mut next2next = [0u8; 8];
		let mut next2prev = [0u8; 8];
		let mut prev2next = [0u8; 8];
		let mut prev2prev = [0u8; 8];

		let mut slab1_bytes = [0u8; 8];
		let mut slab2_bytes = [0u8; 8];

		usize_to_slice(slab_id1, &mut slab1_bytes[0..ptr_size])?;
		usize_to_slice(slab_id2, &mut slab2_bytes[0..ptr_size])?;

		// read next/prev for slab_id1
		let mut reader = self.get_reader_impl(slab_id1)?;
		reader.read_fixed_bytes(&mut prev1[0..ptr_size])?;
		reader.read_fixed_bytes(&mut next1[0..ptr_size])?;

		// read next/prev for slab_id2
		let mut reader = self.get_reader_impl(slab_id2)?;
		reader.read_fixed_bytes(&mut prev2[0..ptr_size])?;
		reader.read_fixed_bytes(&mut next2[0..ptr_size])?;

		let next1slab = slice_to_usize(&next1[0..ptr_size])?;
		let prev1slab = slice_to_usize(&prev1[0..ptr_size])?;

		let next2slab = slice_to_usize(&next2[0..ptr_size])?;
		let prev2slab = slice_to_usize(&prev2[0..ptr_size])?;

		// write the swapped next/prev to slab_id1
		let mut writer = self.get_writer_id(slab_id1)?;
		writer.write_fixed_bytes(&prev2[0..ptr_size])?;
		writer.write_fixed_bytes(&next2[0..ptr_size])?;

		// write the swapped next/prev to slab_id2
		let mut writer = self.get_writer_id(slab_id2)?;
		writer.write_fixed_bytes(&prev1[0..ptr_size])?;
		writer.write_fixed_bytes(&next1[0..ptr_size])?;

		if next1slab < self.max_value {
			if next1slab != slab_id2 {
				let mut reader = self.get_reader_impl(next1slab)?;
				reader.read_fixed_bytes(&mut next1prev[0..ptr_size])?;
				reader.read_fixed_bytes(&mut next1next[0..ptr_size])?;

				let mut writer = self.get_writer_id(next1slab)?;
				writer.write_fixed_bytes(&slab2_bytes[0..ptr_size])?;
				writer.write_fixed_bytes(&next1next[0..ptr_size])?;
			} else {
				debug!("x1")?;
				let mut reader = self.get_reader_impl(next1slab)?;
				reader.read_fixed_bytes(&mut next1prev[0..ptr_size])?;
				reader.read_fixed_bytes(&mut next1next[0..ptr_size])?;

				let mut writer = self.get_writer_id(next1slab)?;
				writer.write_fixed_bytes(&prev1[0..ptr_size])?;
				writer.write_fixed_bytes(&slab1_bytes[0..ptr_size])?;
			}
		}

		if prev1slab < self.max_value {
			if prev1slab != slab_id2 {
				let mut reader = self.get_reader_impl(prev1slab)?;
				reader.read_fixed_bytes(&mut prev1prev[0..ptr_size])?;
				reader.read_fixed_bytes(&mut prev1next[0..ptr_size])?;

				let mut writer = self.get_writer_id(prev1slab)?;
				writer.write_fixed_bytes(&prev1prev[0..ptr_size])?;
				writer.write_fixed_bytes(&slab2_bytes[0..ptr_size])?;
			} else {
				debug!("x2")?;
				let mut reader = self.get_reader_impl(prev1slab)?;
				reader.read_fixed_bytes(&mut prev1prev[0..ptr_size])?;
				reader.read_fixed_bytes(&mut prev1next[0..ptr_size])?;

				let mut writer = self.get_writer_id(prev1slab)?;
				writer.write_fixed_bytes(&slab1_bytes[0..ptr_size])?;
				writer.write_fixed_bytes(&next1[0..ptr_size])?;
			}
		}

		if next2slab < self.max_value {
			if next2slab != slab_id1 {
				let mut reader = self.get_reader_impl(next2slab)?;
				reader.read_fixed_bytes(&mut next2prev[0..ptr_size])?;
				reader.read_fixed_bytes(&mut next2next[0..ptr_size])?;

				let mut writer = self.get_writer_id(next2slab)?;
				writer.write_fixed_bytes(&slab1_bytes[0..ptr_size])?;
				writer.write_fixed_bytes(&next2next[0..ptr_size])?;
			} else {
				debug!("x3")?;
				let mut reader = self.get_reader_impl(next2slab)?;
				reader.read_fixed_bytes(&mut next2prev[0..ptr_size])?;
				reader.read_fixed_bytes(&mut next2next[0..ptr_size])?;

				let mut writer = self.get_writer_id(next2slab)?;
				writer.write_fixed_bytes(&prev2[0..ptr_size])?;
				writer.write_fixed_bytes(&slab2_bytes[0..ptr_size])?;
			}
		}

		if prev2slab < self.max_value {
			if prev2slab != slab_id1 {
				let mut reader = self.get_reader_impl(prev2slab)?;
				reader.read_fixed_bytes(&mut prev2prev[0..ptr_size])?;
				reader.read_fixed_bytes(&mut prev2next[0..ptr_size])?;

				let mut writer = self.get_writer_id(prev2slab)?;
				writer.write_fixed_bytes(&prev2prev[0..ptr_size])?;
				writer.write_fixed_bytes(&slab1_bytes[0..ptr_size])?;
			} else {
				debug!("x4")?;
				let mut reader = self.get_reader_impl(prev2slab)?;
				reader.read_fixed_bytes(&mut prev2prev[0..ptr_size])?;
				reader.read_fixed_bytes(&mut prev2next[0..ptr_size])?;

				let mut writer = self.get_writer_id(prev2slab)?;
				writer.write_fixed_bytes(&slab2_bytes[0..ptr_size])?;
				writer.write_fixed_bytes(&next2[0..ptr_size])?;
			}
		}

		// update head/tail if needed
		if self.head == slab_id1 {
			self.head = slab_id2;
			debug!("update head = {}", self.head)?;
		} else if self.head == slab_id2 {
			self.head = slab_id1;
			debug!("update head = {}", self.head)?;
		}

		if self.tail == slab_id1 {
			self.tail = slab_id2;
			debug!("update tail = {}", self.tail)?;
		} else if self.tail == slab_id2 {
			self.tail = slab_id1;
			debug!("update tail = {}", self.tail)?;
		}

		Ok(())
	}

	fn set_arr(&self, arr: &mut [u8], value: usize) -> Result<(), Error> {
		let ptr_size = self.ptr_size;
		usize_to_slice(value, &mut arr[0..ptr_size])?;
		Ok(())
	}

	fn get_reader_impl(&self, slab_id: usize) -> Result<SlabReader, Error> {
		Ok(match &self.slabs {
			Some(slabs) => SlabReader::new(Some(slabs), slab_id)?,
			None => SlabReader::new(None, slab_id)?,
		})
	}

	fn get_writer(&mut self) -> Result<(usize, SlabWriter), Error> {
		let bytes_per_slab = self.bytes_per_slab;
		let slab_size = self.slab_size;
		let mut slab = self.allocate()?;
		// set next to 0xFF
		let slab_mut = slab.get_mut();
		for i in bytes_per_slab..slab_size {
			slab_mut[i] = 0xFF;
		}

		let slab_id = slab.id();
		let slab_writer = match &mut self.slabs {
			Some(slabs) => SlabWriter::new(Some(slabs), slab_id)?,
			None => SlabWriter::new(None, slab_id)?,
		};

		Ok((slab_id, slab_writer))
	}

	fn get_writer_id(&mut self, slab_id: usize) -> Result<SlabWriter, Error> {
		debug!("get_writer_id,id={}", slab_id)?;
		Ok(match &mut self.slabs {
			Some(slabs) => SlabWriter::new(Some(slabs), slab_id)?,
			None => SlabWriter::new(None, slab_id)?,
		})
	}

	fn clear_impl(&mut self) -> Result<(), Error> {
		let mut prev = [0u8; 8];
		let mut next = [0u8; 8];
		let mut next_slab_id = self.tail;

		loop {
			if next_slab_id >= self.max_value {
				break;
			}

			let mut reader = self.get_reader_impl(next_slab_id)?;
			// read over prev/next
			reader.read_fixed_bytes(&mut prev[0..self.ptr_size])?;
			reader.read_fixed_bytes(&mut next[0..self.ptr_size])?;
			self.free_chain(next_slab_id)?;
			next_slab_id = slice_to_usize(&next[0..self.ptr_size])?;
		}
		self.tail = self.max_value;
		self.head = self.max_value;
		self.size = 0;

		Ok(())
	}

	fn iter_impl<'a, V>(&'a self, iter_type: IterType, cur: usize) -> StaticListIterator<'a, V>
	where
		V: Serializable,
	{
		StaticListIterator::new(iter_type, self, cur)
	}

	fn pop_impl<V>(&mut self, op: OpType) -> Result<Option<V>, Error>
	where
		V: Serializable,
	{
		let mut prev = [0u8; 8];
		let mut next = [0u8; 8];
		let ptr_size = self.ptr_size;

		if self.tail >= self.max_value {
			return Ok(None);
		}

		let target_id = match op {
			OpType::Front => self.head,
			OpType::Back => self.tail,
		};

		// get a reader based on our target_id
		let mut reader = self.get_reader_impl(target_id)?;
		// read over prev/next
		reader.read_fixed_bytes(&mut prev[0..ptr_size])?;
		reader.read_fixed_bytes(&mut next[0..ptr_size])?;
		// read our serailized struct
		let ret = Some(V::read(&mut reader)?);

		// we have the structure, now update the list
		match op {
			OpType::Front => {
				self.head = slice_to_usize(&prev[0..ptr_size])?;
				let prev_head = self.head;
				debug!("head reset to {}", self.head)?;
				if self.head >= self.max_value {
					self.tail = self.max_value;
				}
				debug!("updating prev/next for target_id={}", prev_head)?;
				if prev_head < self.max_value {
					debug!("update prev_head={}", prev_head)?;
					let mut reader = self.get_reader_impl(prev_head)?;
					reader.read_fixed_bytes(&mut prev[0..ptr_size])?;
					let mut writer = self.get_writer_id(prev_head)?;
					set_max(&mut next); // set this next pointer to none
					writer.write_fixed_bytes(&prev[0..ptr_size])?;
					writer.write_fixed_bytes(&next[0..ptr_size])?;
				}
			}
			OpType::Back => {
				debug!("updating in a pop back for id = {}", target_id)?;
				let prev_tail = self.tail;
				self.tail = slice_to_usize(&next[0..ptr_size])?;
				if self.tail >= self.max_value {
					debug!("setting head to max {}", self.max_value)?;
					self.head = self.max_value;
				}

				if prev_tail < self.max_value {
					let mut writer = self.get_writer_id(prev_tail)?;
					set_max(&mut prev); // set this prev pointer to none
					writer.write_fixed_bytes(&prev[0..ptr_size])?;
				}
			}
		}

		// update size
		self.size = self.size.saturating_sub(1);

		// finally, free the slabs
		self.free_chain(target_id)?;

		Ok(ret)
	}

	fn push_impl<V>(&mut self, value: &V, op: OpType) -> Result<(), Error>
	where
		V: Serializable,
	{
		let mut next = [0u8; 8];
		let mut prev = [0u8; 8];

		let tail = self.tail;
		let head = self.head;
		let ptr_size = self.ptr_size;

		match op {
			OpType::Front => {
				debug!("front items head(prev) = {}", head)?;
				self.set_arr(&mut prev, head)?;
				set_max(&mut next);
			}
			OpType::Back => {
				self.set_arr(&mut next, tail)?;
				set_max(&mut prev);
			}
		}

		let (slab_id, mut writer) = self.get_writer()?;
		debug!("push_impl to slab_id = {}", slab_id)?;

		// write prev/next
		writer.write_fixed_bytes(&prev[0..ptr_size])?;
		writer.write_fixed_bytes(&next[0..ptr_size])?;
		// write our value
		match value.write(&mut writer) {
			Ok(_) => {}
			Err(e) => {
				warn!("writing value generated error: {}", e)?;

				debug!("free slab = {}", slab_id)?;
				// clean up this chain
				self.free_chain(slab_id)?;

				return Err(err!(
					ErrKind::CapacityExceeded,
					format!("writing value generated error: {}", e)
				));
			}
		}

		match op {
			OpType::Front => {
				debug!("got a front,head={}, max={}", head, self.max_value)?;
				if head < self.max_value {
					// make the previous head's next pointer point to us
					self.set_arr(&mut next, slab_id)?;
					let mut reader = self.get_reader_impl(head)?;
					debug!(
						"reading head = {}, set prev={:?}, next={:?}",
						head, prev, next
					)?;
					reader.read_fixed_bytes(&mut prev[0..ptr_size])?;
					let mut writer = self.get_writer_id(head)?;
					writer.write_fixed_bytes(&prev[0..ptr_size])?;
					writer.write_fixed_bytes(&next[0..ptr_size])?;
				}
			}
			OpType::Back => {
				if tail < self.max_value {
					// make the previous tail's prev pointer point to us
					self.set_arr(&mut prev, slab_id)?;
					let mut writer = self.get_writer_id(tail)?;
					writer.write_fixed_bytes(&prev[0..ptr_size])?;
				}
			}
		}

		// update tail/head
		match op {
			OpType::Front => {
				debug!("set head to slab_id={}", slab_id)?;
				self.head = slab_id;
				if self.tail >= self.max_value {
					self.tail = slab_id;
				}
			}
			OpType::Back => {
				self.tail = slab_id;
				if self.head >= self.max_value {
					self.head = slab_id;
					debug!("set head to slab_id={}", slab_id)?;
				}
			}
		}

		// increment size
		self.size += 1;

		Ok(())
	}
}

pub struct StaticListBuilder {}

impl StaticListBuilder {
	pub fn build<V>(
		config: StaticListConfig,
		slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	) -> Result<Box<dyn StaticList<V>>, Error>
	where
		V: Serializable,
	{
		Ok(Box::new(StaticListImpl::new(config, slabs)?))
	}
}

impl Default for StaticListConfig {
	fn default() -> Self {
		Self {}
	}
}

#[cfg(test)]
mod test {
	use crate as bmw_util;
	use crate::static_list::StaticListBuilder;
	use crate::static_list::StaticListImpl;
	use crate::static_list::StaticListIterator;
	use crate::static_list::{backward, forward};
	use crate::types::SortableList;
	use crate::types::StaticList;
	use crate::types::StaticListConfig;
	use crate::GLOBAL_SLAB_ALLOCATOR;
	use crate::{init_slab_allocator, list, Reader, Serializable, Writer};
	use bmw_deps::rand;
	use bmw_err::*;
	use bmw_log::*;
	use std::time::Instant;

	info!();

	#[test]
	fn test_static_list() -> Result<(), Error> {
		let mut list = StaticListBuilder::build(StaticListConfig::default(), None)?;

		let free_count1 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count1)?;

		list.push(&1)?;
		list.push(&2)?;
		list.push(&3)?;
		list.push(&4)?;

		assert_eq!(list.pop()?, Some(4));
		assert_eq!(list.pop()?, Some(3));
		assert_eq!(list.pop()?, Some(2));
		assert_eq!(list.pop()?, Some(1));
		assert_eq!(list.pop()?, None);
		assert_eq!(list.pop()?, None);

		list.push(&10)?;
		list.push(&20)?;
		assert_eq!(list.pop()?, Some(20));
		list.push(&30)?;
		list.push(&40)?;

		assert_eq!(list.pop()?, Some(40));
		assert_eq!(list.pop()?, Some(30));
		assert_eq!(list.pop()?, Some(10));
		assert_eq!(list.pop()?, None);

		let free_count2 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count2)?;
		assert_eq!(free_count1, free_count2);

		Ok(())
	}

	#[test]
	fn test_push_front() -> Result<(), Error> {
		let mut list = StaticListBuilder::build(StaticListConfig::default(), None)?;

		let free_count1 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count1)?;

		list.push(&1)?;
		list.push(&2)?;
		list.push(&3)?;
		list.push(&4)?;
		list.push_front(&5)?;

		assert_eq!(list.pop()?, Some(4));
		assert_eq!(list.pop()?, Some(3));
		assert_eq!(list.pop()?, Some(2));
		assert_eq!(list.pop()?, Some(1));
		assert_eq!(list.pop()?, Some(5));
		assert_eq!(list.pop()?, None);
		assert_eq!(list.pop()?, None);

		list.push(&10)?;
		list.push(&20)?;
		assert_eq!(list.pop()?, Some(20));
		list.push(&30)?;
		list.push(&40)?;

		assert_eq!(list.pop()?, Some(40));
		assert_eq!(list.pop()?, Some(30));
		assert_eq!(list.pop()?, Some(10));
		assert_eq!(list.pop()?, None);

		let free_count2 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count2)?;
		assert_eq!(free_count1, free_count2);

		Ok(())
	}

	#[test]
	fn test_front() -> Result<(), Error> {
		let mut list = StaticListBuilder::build(StaticListConfig::default(), None)?;

		let free_count1 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count1)?;

		list.push_front(&1)?;
		list.push_front(&2)?;
		list.push_front(&3)?;
		list.push_front(&4)?;

		assert_eq!(list.pop_front()?, Some(4));
		assert_eq!(list.pop_front()?, Some(3));
		assert_eq!(list.pop_front()?, Some(2));
		assert_eq!(list.pop_front()?, Some(1));
		assert_eq!(list.pop_front()?, None);

		let free_count2 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count2)?;
		assert_eq!(free_count1, free_count2);

		Ok(())
	}

	#[test]
	fn test_pop_front() -> Result<(), Error> {
		let mut list = StaticListBuilder::build(StaticListConfig::default(), None)?;

		let free_count1 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count1)?;

		list.push(&1)?;
		list.push(&2)?;
		list.push(&3)?;
		list.push(&4)?;
		list.push_front(&5)?;

		assert_eq!(list.pop()?, Some(4));
		assert_eq!(list.pop()?, Some(3));
		assert_eq!(list.pop_front()?, Some(5));
		assert_eq!(list.pop()?, Some(2));
		assert_eq!(list.pop()?, Some(1));
		assert_eq!(list.pop()?, None);
		assert_eq!(list.pop()?, None);
		assert_eq!(list.pop_front()?, None);

		let free_count2 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count2)?;
		assert_eq!(free_count1, free_count2);

		Ok(())
	}

	#[test]
	fn test_iter() -> Result<(), Error> {
		let mut list = StaticListBuilder::build(StaticListConfig::default(), None)?;

		list.push(&1)?;
		list.push(&2)?;
		list.push(&3)?;
		list.push(&4)?;
		list.push_front(&5)?;

		let exp = vec![5, 1, 2, 3, 4];

		let mut i = 0;
		for x in list.iter() {
			assert_eq!(x, exp[i]);
			i += 1;
		}

		i -= 1;
		for x in list.iter_rev() {
			assert_eq!(x, exp[i]);
			i = i.saturating_sub(1);
		}

		for x in &list {
			info!("xt={}", x)?;
		}

		Ok(())
	}

	#[test]
	fn test_clear() -> Result<(), Error> {
		let mut list = StaticListBuilder::build(StaticListConfig::default(), None)?;
		list.push(&1)?;
		list.push(&2)?;
		list.push(&3)?;
		list.push(&4)?;
		list.push_front(&5)?;

		assert_eq!(list.size(), 5);
		list.clear()?;
		assert_eq!(list.size(), 0);

		Ok(())
	}

	#[test]
	fn test_drop() -> Result<(), Error> {
		let free_count1;
		{
			let mut list = StaticListBuilder::build(StaticListConfig::default(), None)?;
			free_count1 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
			})?;
			info!("free_count={}", free_count1)?;
			list.push(&1)?;
			list.push(&2)?;
			list.push(&3)?;
			list.push(&4)?;
			list.push_front(&5)?;

			assert_eq!(list.size(), 5);
		}

		let free_count2 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count2)?;
		assert_eq!(free_count1, free_count2);

		Ok(())
	}

	struct VarSize {
		size: usize,
	}
	impl Serializable for VarSize {
		fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
			let size = reader.read_usize()?;
			for _ in 0..size {
				reader.read_u8()?;
			}
			Ok(Self { size })
		}
		fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
			writer.write_usize(self.size)?;
			for _ in 0..self.size {
				writer.write_u8(0)?;
			}
			Ok(())
		}
	}

	#[test]
	fn test_out_of_slabs() -> Result<(), Error> {
		init_slab_allocator!(128, 11);
		let mut list = StaticListBuilder::build(StaticListConfig::default(), None)?;
		let free_count1 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count1)?;
		for _ in 0..5 {
			list.push(&VarSize { size: 130 })?;
		}

		assert!(list.push(&VarSize { size: 130 }).is_err());
		debug!("calling clear")?;
		list.clear()?;

		let free_count2 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		debug!("free_count={}", free_count2)?;
		assert_eq!(free_count1, free_count2);
		Ok(())
	}

	#[test]
	fn test_equal() -> Result<(), Error> {
		let x = list![1, 2, 3];
		let mut y = x.clone();
		assert_eq!(&x, &y);
		y.push(&6)?;
		assert_ne!(&x, &y);
		//assert_eq!(list![1, 3, 2], list![1, 3, 2]);
		//assert_ne!(list![1, 3, 2], list![1, 3, 3]);
		//assert!(list![1, 2, 3] != list![3, 2]);
		//assert!(list![1, 2, 3] != list![3, 2, 1]);

		info!("list={:?}", list![2, 3, 4])?;
		info!("list={}", list![1, 2, 3])?;

		Ok(())
	}

	#[test]
	fn test_swap() -> Result<(), Error> {
		let mut list = StaticListImpl::new(StaticListConfig::default(), None)?;
		list.push(&0)?;
		list.push(&1)?;
		list.push(&2)?;
		let iter: StaticListIterator<i32> = list.iter();
		for x in iter {
			info!("x={}", x)?;
		}

		// rely on ordering of slabs being sequential
		list.swap_impl(0, 2)?;
		let iter: StaticListIterator<i32> = list.iter();

		let actual = vec![2, 1, 0];
		let mut i = 0;

		for x in iter {
			assert_eq!(x, actual[i]);
			info!("x={}", x)?;
			i += 1;
		}

		assert_eq!(i, 3);

		let iter: StaticListIterator<i32> = list.iter_rev();

		for x in iter {
			info!("x={}", x)?;
			i -= 1;
			assert_eq!(x, actual[i]);
		}

		assert_eq!(i, 0);

		Ok(())
	}

	fn do_rand(i: usize) -> Result<(), Error> {
		let s = std::time::Instant::now();
		let mut list = StaticListImpl::new(StaticListConfig::default(), None)?;
		debug!("elapsed={:?}", s.elapsed())?;
		let mut vec = vec![];
		for _ in 0..i {
			let rand: u128 = rand::random();
			list.push(&rand)?;
			vec.push(rand);
		}

		let ids = list.get_slab_ids()?;
		let vindex1 = rand::random::<usize>() % ids.len();
		let vindex2 = rand::random::<usize>() % ids.len();
		let index1 = ids[vindex1];
		let index2 = ids[vindex2];

		debug!("ids={:?}", ids)?;

		let iter: StaticListIterator<u128> = list.iter();
		let mut count = 0;
		for x in iter {
			assert_eq!(x, vec[count]);
			count += 1;
		}
		assert_eq!(count, i);
		let iter: StaticListIterator<u128> = list.iter_rev();
		for x in iter {
			count -= 1;
			assert_eq!(x, vec[count]);
		}

		debug!("swap index = {} and index = {}", index1, index2)?;
		list.swap_impl(index1, index2)?;
		let tmp = vec[vindex1];
		vec[vindex1] = vec[vindex2];
		vec[vindex2] = tmp;

		let iter: StaticListIterator<u128> = list.iter();
		let mut count = 0;
		for x in iter {
			if x != vec[count] {
				info!("mismatch at {}, list=, vec={:?}", count, vec)?;
				info!("list=")?;
				let iter2: StaticListIterator<u128> = list.iter();
				let mut i2 = 0;
				for x2 in iter2 {
					info!("x2[{}]={}", i2, x2)?;
					i2 += 1;
					if i2 > vec.len() {
						break;
					}
				}
			}
			assert_eq!(x, vec[count]);
			count += 1;
		}
		assert_eq!(count, i);
		let iter: StaticListIterator<u128> = list.iter_rev();
		for x in iter {
			count -= 1;
			assert_eq!(x, vec[count]);
		}

		list.clear_impl()?;

		Ok(())
	}

	#[test]
	fn test_merge() -> Result<(), Error> {
		let mut list = list![10, 20, 3, 40, 100, 200, 30, 40];
		info!("list={}", list)?;
		let mut ptr = list.head();
		for _ in 0..2 {
			ptr = backward(&mut *list, ptr)?;
		}
		list.merge(list.head(), ptr)?;
		let expected = vec![3, 10, 20, 40, 100, 200, 30, 40];
		info!("list={}", list)?;
		let mut i = 0;
		for x in &list {
			assert_eq!(x, expected[i]);
			i += 1;
		}
		assert_eq!(i, list.size());
		assert_eq!(i, expected.len());

		for x in list.iter_rev() {
			i -= 1;
			assert_eq!(x, expected[i]);
		}

		assert_eq!(i, 0);

		let mut ptr = list.head();
		for _ in 0..1 {
			ptr = backward(&mut *list, ptr)?;
		}
		let mut ptr2 = list.tail();
		ptr2 = forward(&mut *list, ptr2)?;
		list.merge(ptr, ptr2)?;
		let expected = vec![3, 30, 10, 20, 40, 100, 200, 40];
		let mut i = 0;
		for x in &list {
			assert_eq!(x, expected[i]);
			i += 1;
		}
		assert_eq!(i, list.size());
		assert_eq!(i, expected.len());

		for x in list.iter_rev() {
			i -= 1;
			assert_eq!(x, expected[i]);
		}

		assert_eq!(i, 0);
		info!("list={}", list)?;
		list.merge(list.head(), list.tail())?;
		info!("list={}", list)?;

		let expected = vec![40, 3, 30, 10, 20, 40, 100, 200];
		let mut i = 0;
		for x in &list {
			assert_eq!(x, expected[i]);
			i += 1;
		}
		assert_eq!(i, list.size());
		assert_eq!(i, expected.len());

		for x in list.iter_rev() {
			i -= 1;
			assert_eq!(x, expected[i]);
		}

		assert_eq!(i, 0);

		let ptr1 = list.head();
		let mut ptr2 = list.head();
		ptr2 = backward(&mut *list, ptr2)?;

		list.merge(ptr1, ptr2)?;
		info!("list={}", list)?;

		let expected = vec![3, 40, 30, 10, 20, 40, 100, 200];
		let mut i = 0;
		for x in &list {
			assert_eq!(x, expected[i]);
			i += 1;
		}
		assert_eq!(i, list.size());
		assert_eq!(i, expected.len());

		for x in list.iter_rev() {
			info!("x={}", x)?;
			i -= 1;
			assert_eq!(x, expected[i]);
		}

		assert_eq!(i, 0);

		Ok(())
	}

	#[test]
	fn test_rand_swap() -> Result<(), Error> {
		for i in 130..140 {
			for _ in 0..30 {
				do_rand(i)?;
			}
		}
		Ok(())
	}

	#[test]
	fn test_sort() -> Result<(), Error> {
		let mut list = list![1, 3, 9, 7, 10, 20, 21, 11];
		info!("list={}", list)?;
		list.sort()?;
		assert_eq!(&list, &list![1, 3, 7, 9, 10, 11, 20, 21]);
		info!("list={}", list)?;
		Ok(())
	}

	#[test]
	fn test_sort_cases() -> Result<(), Error> {
		let mut list = list![1, 2, 3, 4];
		list.sort()?;
		assert_eq!(&list, &list![1, 2, 3, 4]);

		let mut list = list![2, 1, 3, 4];
		list.sort()?;
		assert_eq!(&list, &list![1, 2, 3, 4]);

		let mut list = list![2, 1, 4, 3];
		list.sort()?;
		assert_eq!(&list, &list![1, 2, 3, 4]);

		let mut list = list![1, 3, 2, 4];
		list.sort()?;
		assert_eq!(&list, &list![1, 2, 3, 4]);

		let mut list = list![1, 3, 4, 2];
		list.sort()?;
		assert_eq!(&list, &list![1, 2, 3, 4]);

		let mut list = list![4, 3, 2, 1];
		list.sort()?;
		assert_eq!(&list, &list![1, 2, 3, 4]);

		Ok(())
	}

	#[test]
	fn test_rand_sort() -> Result<(), Error> {
		for i in 400..450 {
			for _ in 0..10 {
				info!("testing with i = {}", i)?;
				match do_test_rand_sort(i) {
					Ok(_) => {}
					Err(e) => {
						error!("do_test_rand {} err: {}", i, e)?;
						assert!(false);
					}
				}
			}
		}
		Ok(())
	}

	fn do_test_rand_sort(size: usize) -> Result<(), Error> {
		let mut list = list![];
		let mut vec = vec![];
		for _ in 0..size {
			let r = rand::random::<i32>();
			list.push(&r)?;
			vec.push(r);
		}
		info!("list_rand={}", list)?;
		let now = Instant::now();
		list.sort()?;
		let elapsed = now.elapsed();
		info!("list sort took {:?}", elapsed)?;

		let now = Instant::now();
		vec.sort();
		let elapsed = now.elapsed();
		info!("vec sort took {:?}", elapsed)?;

		info!("vec={:?}", vec)?;
		info!("list={}", list)?;

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
		Ok(())
	}
}
