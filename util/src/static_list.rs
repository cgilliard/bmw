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
use crate::types::{Reader, StaticListConfig};
use crate::{
	Serializable, SlabAllocator, SlabAllocatorConfig, SlabMut, StaticList, Writer,
	GLOBAL_SLAB_ALLOCATOR,
};
use bmw_err::*;
use bmw_log::*;
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
		let ptr_size = self.l.list_ptr_size()?;
		let mut ptr = [0u8; 8];
		set_max(&mut ptr[0..ptr_size]);
		let max = slice_to_usize(&ptr[0..ptr_size])?;
		if self.cur >= max {
			return Ok(None);
		}
		let mut prev = [0u8; 8];
		let mut next = [0u8; 8];

		let mut reader = self.l.get_reader(self.cur)?;
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
	slab_count: usize,
	bytes_per_slab: usize,
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

	fn clear(&mut self) -> Result<(), Error> {
		self.clear_impl()
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
						warn!(
							"Slab allocator was not initialized for thread '{}'. {}",
							n, "Initializing with default values.",
						)?;
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
		let max = slice_to_usize(&ptr[0..ptr_size])?;

		let bytes_per_slab = slab_size.saturating_sub(ptr_size);

		Ok(Self {
			config,
			slabs,
			tail: max,
			head: max,
			size: 0,
			slab_size,
			slab_count,
			bytes_per_slab,
		})
	}

	fn free_chain(&mut self, slab_id: usize) -> Result<(), Error> {
		let bytes_per_slab = self.bytes_per_slab;
		let slab_size = self.slab_size;
		let mut next_bytes = slab_id;
		let mut b = [0u8; 8];
		let ptr_size = self.list_ptr_size()?;
		set_max(&mut b[0..ptr_size]);
		let max = slice_to_usize(&b[0..ptr_size])?;
		loop {
			let slab = self.get(next_bytes)?;
			let id = slab.id();
			next_bytes = slice_to_usize(&slab.get()[bytes_per_slab..slab_size])?;
			debug!("free id = {}, next_bytes={}", id, next_bytes)?;
			self.free(id)?;
			if next_bytes >= max {
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

	fn get_reader(&self, slab_id: usize) -> Result<SlabReader, Error> {
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

		let ptr_size = self.list_ptr_size()?;

		set_max(&mut prev[0..ptr_size]);
		let max = slice_to_usize(&prev[0..ptr_size])?;

		let mut next_slab_id = self.tail;
		loop {
			if next_slab_id >= max {
				break;
			}

			let mut reader = self.get_reader(next_slab_id)?;
			// read over prev/next
			reader.read_fixed_bytes(&mut prev[0..ptr_size])?;
			reader.read_fixed_bytes(&mut next[0..ptr_size])?;
			self.free_chain(next_slab_id)?;
			next_slab_id = slice_to_usize(&next[0..ptr_size])?;
		}
		self.tail = max;
		self.head = max;
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

		let ptr_size = self.list_ptr_size()?;

		set_max(&mut prev[0..ptr_size]);
		let max = slice_to_usize(&prev[0..ptr_size])?;

		if self.tail >= max {
			return Ok(None);
		}

		let target_id = match op {
			OpType::Front => self.head,
			OpType::Back => self.tail,
		};

		// get a reader based on our target_id
		let mut reader = self.get_reader(target_id)?;
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
				if self.head >= max {
					self.tail = max;
				}
				debug!("updating prev/next for target_id={}", prev_head)?;
				if prev_head < max {
					debug!("update prev_head={}", prev_head)?;
					let mut reader = self.get_reader(prev_head)?;
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
				if self.tail >= max {
					debug!("==============setting head to max {}", max)?;
					self.head = max;
				}

				if prev_tail < max {
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

	fn set_arr(&self, arr: &mut [u8], value: usize) -> Result<(), Error> {
		let ptr_size = self.list_ptr_size()?;
		usize_to_slice(value, &mut arr[0..ptr_size])?;
		Ok(())
	}

	fn push_impl<V>(&mut self, value: &V, op: OpType) -> Result<(), Error>
	where
		V: Serializable,
	{
		let mut next = [0u8; 8];
		let mut prev = [0u8; 8];

		let tail = self.tail;
		let head = self.head;
		let ptr_size = self.list_ptr_size()?;
		set_max(&mut prev[0..ptr_size]);
		let max = slice_to_usize(&prev[0..ptr_size])?;

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
		warn!("push_impl to slab_id = {}", slab_id)?;

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
				debug!("got a front,head={}, max={}", head, max)?;
				if head < max {
					// make the previous head's next pointer point to us
					self.set_arr(&mut next, slab_id)?;
					let mut reader = self.get_reader(head)?;
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
				if tail < max {
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
				debug!("================(2)set head to slab_id={}", slab_id)?;
				self.head = slab_id;
				if self.tail >= max {
					self.tail = slab_id;
				}
			}
			OpType::Back => {
				self.tail = slab_id;
				if self.head >= max {
					self.head = slab_id;
					debug!("================set head to slab_id={}", slab_id)?;
				}
			}
		}

		// increment size
		self.size += 1;

		Ok(())
	}

	fn list_ptr_size(&self) -> Result<usize, Error> {
		let mut x = self.slab_count;
		let mut y = 0;
		loop {
			if x == 0 {
				break;
			}
			x >>= 8;
			y += 1;
		}
		Ok(y)
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
	use crate::types::StaticListConfig;
	use crate::GLOBAL_SLAB_ALLOCATOR;
	use crate::{init_slab_allocator, Reader, Serializable, Writer};
	use bmw_err::*;
	use bmw_log::*;

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
}
