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
use crate::types::{
	HashsetIterator, HashtableIterator, Reader, StaticBuilder, StaticHashset, StaticListConfig,
	StaticListIterator, Writer,
};
use crate::{
	Serializable, Slab, SlabAllocator, SlabAllocatorConfig, SlabMut, StaticHashsetConfig,
	StaticHashtable, StaticHashtableConfig, StaticList, GLOBAL_SLAB_ALLOCATOR,
};
use bmw_err::*;
use bmw_log::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::thread;

const SLOT_EMPTY: usize = usize::MAX;
const SLOT_DELETED: usize = usize::MAX - 1;

info!();

pub(crate) struct StaticHashImpl {
	slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	max_value: usize,
	bytes_per_slab: usize,
	slab_size: usize,
	ptr_size: usize,
	entry_array: Vec<usize>,
	size: usize,
	head: usize,
	tail: usize,
	max_load_factor: f64,
}

impl<'a, K, V> Iterator for HashtableIterator<'a, K, V>
where
	K: Serializable,
	V: Serializable,
{
	type Item = (K, V);
	fn next(&mut self) -> Option<<Self as Iterator>::Item> {
		match self.hashtable.get_next(&mut self.cur) {
			Ok(x) => x,
			Err(e) => {
				let _ = error!("get_next generated unexpected error: {}", e);
				None
			}
		}
	}
}

impl<'a, K> Iterator for HashsetIterator<'a, K>
where
	K: Serializable,
{
	type Item = K;
	fn next(&mut self) -> Option<<Self as Iterator>::Item> {
		match self.hashset.get_next_slot(&mut self.cur) {
			Ok(reader) => match reader {
				Some(mut reader) => match K::read(&mut reader) {
					Ok(k) => Some(k),
					Err(e) => {
						let _ = warn!("deserialization generated error: {}", e);
						None
					}
				},
				None => None,
			},

			Err(e) => {
				let _ = error!("get_next generated unexpected error: {}", e);
				None
			}
		}
	}
}

impl<'a, K, V> HashtableIterator<'a, K, V> {
	fn new(hashtable: &'a StaticHashImpl, cur: usize) -> Self {
		Self {
			hashtable,
			cur,
			_phantom_data: PhantomData,
		}
	}
}

impl<'a, K> HashsetIterator<'a, K> {
	fn new(hashset: &'a StaticHashImpl, cur: usize) -> Self {
		Self {
			hashset,
			cur,
			_phantom_data: PhantomData,
		}
	}
}

impl Default for StaticHashtableConfig {
	fn default() -> Self {
		Self {
			max_entries: 1_000_000,
			max_load_factor: 0.8,
		}
	}
}

impl Default for StaticHashsetConfig {
	fn default() -> Self {
		Self {
			max_entries: 1_000_000,
			max_load_factor: 0.8,
		}
	}
}

impl StaticHashImpl {
	fn new(
		hashtable_config: Option<StaticHashtableConfig>,
		hashset_config: Option<StaticHashsetConfig>,
		list_config: Option<StaticListConfig>,
		slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	) -> Result<Self, Error> {
		let (slab_size, _slab_count) = match slabs.as_ref() {
			Some(slabs) => ((slabs.slab_size()?, slabs.slab_count()?)),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(usize, usize), Error> {
				let slabs = unsafe { f.get().as_mut().unwrap() };
				let slab_size = match slabs.slab_size() {
					Ok(slab_size) => slab_size,
					Err(_e) => {
						let th = thread::current();
						let n = th.name().unwrap_or("unknown");
						let _ = warn!(
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

		let (max_entries, max_load_factor) = match hashtable_config {
			Some(config) => (config.max_entries, config.max_load_factor),
			None => match hashset_config {
				Some(config) => (config.max_entries, config.max_load_factor),
				None => {
					return Err(err!(
						ErrKind::Configuration,
						"must specify either Hashtable or Hashset config"
					));
				}
			},
		};
		let mut entry_array = vec![];
		let size: usize = (max_entries as f64 / max_load_factor).ceil() as usize;
		debug!("entry array init to size = {}", size)?;
		entry_array.resize(size, SLOT_EMPTY);

		let mut x = entry_array.len() + 2; // two more, one for deleted and one for empty
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
			slabs,
			bytes_per_slab,
			max_value,
			slab_size,
			ptr_size,
			entry_array,
			max_load_factor,
			size: 0,
			head: 0,
			tail: 0,
		})
	}

	fn get_next<K, V>(
		&self,
		cur: &mut usize,
	) -> Result<Option<<HashtableIterator<K, V> as Iterator>::Item>, Error>
	where
		K: Serializable,
		V: Serializable,
	{
		match self.get_next_slot(cur)? {
			Some(mut reader) => Ok(Some((K::read(&mut reader)?, V::read(&mut reader)?))),
			None => Ok(None),
		}
	}

	fn get_next_slot(&self, cur: &mut usize) -> Result<Option<SlabReader>, Error> {
		debug!("cur={}", *cur)?;
		if *cur == SLOT_EMPTY || *cur == SLOT_DELETED {
			return Ok(None);
		}
		let slot = self.entry_array[*cur];
		debug!("slot={}", slot)?;
		if slot >= self.max_value {
			debug!("max val")?;
			return Ok(None);
		}

		let mut prev = [0u8; 8];
		let mut next = [0u8; 8];
		let ptr_size = self.ptr_size;
		let mut reader = self.get_reader(slot)?;

		// read both pointers, we only use the second ptr_size bytes

		reader.read_fixed_bytes(&mut next[0..ptr_size])?;
		reader.read_fixed_bytes(&mut prev[0..ptr_size])?;

		*cur = slice_to_usize(&prev[0..ptr_size])?;
		debug!(
			"ncur={}, next was {}",
			*cur,
			slice_to_usize(&next[0..ptr_size])?
		)?;
		Ok(Some(reader))
	}

	fn clear_impl(&mut self) -> Result<(), Error> {
		let mut cur = self.tail;
		loop {
			if cur == SLOT_EMPTY || cur == SLOT_DELETED {
				break;
			}
			debug!("clear impl cur={}", cur)?;
			if self.entry_array[cur] < self.max_value {
				self.free_chain(self.entry_array[cur])?;
			}

			let last_cur = cur;
			if self.get_next_slot(&mut cur)?.is_none() {
				self.entry_array[last_cur] = SLOT_EMPTY;
				break;
			}
			self.entry_array[last_cur] = SLOT_EMPTY;
		}
		debug!("set size to 0")?;
		self.size = 0;
		self.tail = SLOT_EMPTY;
		self.head = SLOT_EMPTY;

		Ok(())
	}

	fn entry_hash<K: Hash>(&self, key: &K) -> usize {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		hash % self.entry_array.len()
	}

	fn get_impl<K>(&self, key: &K) -> Result<Option<(usize, SlabReader)>, Error>
	where
		K: Serializable + Hash + PartialEq,
	{
		let mut entry = self.entry_hash(key);
		let entry_array_len = self.entry_array.len();

		let mut i = 0;
		loop {
			if i >= entry_array_len {
				let msg = "StaticHashImpl: Capacity exceeded";
				return Err(err!(ErrKind::CapacityExceeded, msg));
			}
			if self.entry_array[entry] == SLOT_EMPTY {
				debug!("slot empty at {}", entry)?;
				return Ok(None);
			}

			// does the current key match ours?
			if self.entry_array[entry] != SLOT_DELETED {
				match self.read_key::<K>(self.entry_array[entry])? {
					Some((k, reader)) => {
						if &k == key {
							return Ok(Some((entry, reader)));
						}
					}
					None => {}
				}
			}

			entry = (entry + 1) % entry_array_len;
			i += 1;
		}
	}

	fn insert_impl<K, V>(&mut self, key: &K, value: Option<&V>) -> Result<(), Error>
	where
		K: Serializable + Hash + PartialEq,
		V: Serializable,
	{
		let mut entry = self.entry_hash(key);
		let entry_array_len = self.entry_array.len();
		let max_value = self.max_value;
		let ptr_size = self.ptr_size;
		let tail = self.tail;

		// check the load factor
		if (self.size + 1) as f64 > self.max_load_factor * entry_array_len as f64 {
			let fmt = format!("load factor ({}) exceeded", self.max_load_factor);
			return Err(err!(ErrKind::CapacityExceeded, fmt));
		}

		let mut i = 0;
		loop {
			if i >= entry_array_len {
				let msg = "StaticHashImpl: Capacity exceeded";
				return Err(err!(ErrKind::CapacityExceeded, msg));
			}
			if self.entry_array[entry] == SLOT_EMPTY || self.entry_array[entry] == SLOT_DELETED {
				break;
			}

			// does the current key match ours?
			match self.read_key::<K>(self.entry_array[entry])? {
				Some((k, _reader)) => {
					if &k == key {
						self.size = self.size.saturating_sub(1);
						self.free_chain(self.entry_array[entry])?;
						break;
					}
				}
				None => {}
			}

			entry = (entry + 1) % entry_array_len;
			i += 1;
		}

		let (slab_id, mut writer) = self.get_writer()?;
		let ptr_size = ptr_size;
		let mut prev = [0u8; 8];
		let mut next = [0u8; 8];
		debug!("slab_id={}", slab_id)?;
		// update head/tail pointers
		usize_to_slice(SLOT_EMPTY, &mut next[0..ptr_size])?;
		usize_to_slice(tail, &mut prev[0..ptr_size])?;
		debug!(
			"updating slab id {} with next = {}, prev = {}",
			slab_id, max_value, tail
		)?;
		writer.write_fixed_bytes(&next[0..ptr_size])?;
		writer.write_fixed_bytes(&prev[0..ptr_size])?;

		match key.write(&mut writer) {
			Ok(_) => {}
			Err(e) => {
				warn!("writing key generated error: {}", e)?;
				self.free_chain(slab_id)?;
				return Err(err!(
					ErrKind::CapacityExceeded,
					format!("writing key generated error: {}", e)
				));
			}
		}

		match value {
			Some(value) => match value.write(&mut writer) {
				Ok(_) => {}
				Err(e) => {
					warn!("writing value generated error: {}", e)?;
					self.free_chain(slab_id)?;
					return Err(err!(
						ErrKind::CapacityExceeded,
						format!("writing value generated error: {}", e)
					));
				}
			},
			None => {}
		}

		if self.tail < max_value {
			if self.entry_array[self.tail] < max_value {
				let mut reader = self.get_reader(self.entry_array[self.tail])?;
				reader.read_fixed_bytes(&mut next[0..ptr_size])?;
				reader.read_fixed_bytes(&mut prev[0..ptr_size])?;
				let mut writer = self.get_writer_id(self.entry_array[self.tail])?;
				usize_to_slice(entry, &mut next[0..ptr_size])?;
				writer.write_fixed_bytes(&next[0..ptr_size])?;
				writer.write_fixed_bytes(&prev[0..ptr_size])?;
			}
		}

		self.tail = entry;

		if self.head >= max_value {
			self.head = entry;
		}

		self.entry_array[entry] = slab_id;

		self.size += 1;

		Ok(())
	}

	fn read_key<K>(&self, slab_id: usize) -> Result<Option<(K, SlabReader)>, Error>
	where
		K: Serializable + Hash + PartialEq,
	{
		let ptr_size = self.ptr_size;
		let mut skip = [0u8; 16];
		// get a reader based on requested slab_id
		let mut reader = self.get_reader(slab_id)?;
		// read over prev/next
		reader.read_fixed_bytes(&mut skip[0..ptr_size * 2])?;
		// read our serailized struct
		Ok(Some((K::read(&mut reader)?, reader)))
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

	fn allocate(&mut self) -> Result<SlabMut, Error> {
		match &mut self.slabs {
			Some(slabs) => slabs.allocate(),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<SlabMut, Error> {
				let slabs = unsafe { f.get().as_mut().unwrap() };
				slabs.allocate()
			}),
		}
	}

	fn get_slab(&self, slab_id: usize) -> Result<Slab, Error> {
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

	fn free_chain(&mut self, slab_id: usize) -> Result<(), Error> {
		debug!("free chain on slab = {}", slab_id)?;
		let bytes_per_slab = self.bytes_per_slab;
		let slab_size = self.slab_size;
		let mut next_bytes = slab_id;
		loop {
			let slab = self.get_slab(next_bytes)?;
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

	fn free_iter_list(&mut self, entry: usize) -> Result<(), Error> {
		let slab_id = self.entry_array[entry];
		let mut next = [0u8; 8];
		let mut prev = [0u8; 8];
		let ptr_size = self.ptr_size;

		let mut reader = self.get_reader(slab_id)?;
		reader.read_fixed_bytes(&mut next[0..ptr_size])?;
		reader.read_fixed_bytes(&mut prev[0..ptr_size])?;

		let next_usize_entry = slice_to_usize(&next[0..ptr_size])?;
		let prev_usize_entry = slice_to_usize(&prev[0..ptr_size])?;

		if self.head == entry {
			self.head = next_usize_entry;
		}
		if self.tail == entry {
			self.tail = prev_usize_entry;
		}

		if next_usize_entry < self.max_value {
			let next_usize = self.entry_array[next_usize_entry];
			if next_usize < self.max_value {
				let mut next = [0u8; 8];
				let mut prev = [0u8; 8];
				let mut reader = self.get_reader(next_usize)?;
				reader.read_fixed_bytes(&mut next[0..ptr_size])?;
				reader.read_fixed_bytes(&mut prev[0..ptr_size])?;
				usize_to_slice(prev_usize_entry, &mut prev[0..ptr_size])?;
				let mut writer = self.get_writer_id(next_usize)?;
				writer.write_fixed_bytes(&next[0..ptr_size])?;
				writer.write_fixed_bytes(&prev[0..ptr_size])?;
			}
		}

		if prev_usize_entry < self.max_value {
			let prev_usize = self.entry_array[prev_usize_entry];
			if prev_usize < self.max_value {
				let mut next = [0u8; 8];
				let mut prev = [0u8; 8];
				let mut reader = self.get_reader(prev_usize)?;
				reader.read_fixed_bytes(&mut next[0..ptr_size])?;
				reader.read_fixed_bytes(&mut prev[0..ptr_size])?;
				usize_to_slice(next_usize_entry, &mut next[0..ptr_size])?;
				let mut writer = self.get_writer_id(prev_usize)?;
				writer.write_fixed_bytes(&next[0..ptr_size])?;
				writer.write_fixed_bytes(&prev[0..ptr_size])?;
			}
		}

		Ok(())
	}

	fn remove_impl(&mut self, entry: usize) -> Result<(), Error> {
		debug!("remove impl {}", entry)?;
		self.free_iter_list(entry)?;
		self.free_chain(self.entry_array[entry])?;

		self.entry_array[entry] = SLOT_DELETED;
		self.size = self.size.saturating_sub(1);

		Ok(())
	}

	fn copy_impl(&self) -> Self {
		Self {
			slabs: None,
			bytes_per_slab: self.bytes_per_slab,
			max_value: self.max_value,
			slab_size: self.slab_size,
			ptr_size: self.ptr_size,
			entry_array: self.entry_array.clone(),
			max_load_factor: self.max_load_factor,
			size: self.size,
			head: self.head,
			tail: self.tail,
		}
	}
}

impl Drop for StaticHashImpl {
	fn drop(&mut self) {
		match self.clear_impl() {
			Ok(_) => {}
			Err(e) => {
				let _ = warn!("unexpected error in drop: {}", e);
			}
		}
	}
}

impl<K, V> StaticHashtable<K, V> for StaticHashImpl
where
	K: Serializable + Hash + PartialEq,
	V: Serializable,
{
	fn insert(&mut self, key: &K, value: &V) -> Result<(), Error> {
		self.insert_impl(key, Some(value))
	}
	fn get(&self, key: &K) -> Result<Option<V>, Error> {
		match self.get_impl(key)? {
			Some((_entry, mut reader)) => Ok(Some(V::read(&mut reader)?)),
			None => Ok(None),
		}
	}
	fn remove(&mut self, key: &K) -> Result<Option<V>, Error> {
		match self.get_impl(key)? {
			Some((entry, mut reader)) => {
				let v = V::read(&mut reader)?;
				self.remove_impl(entry)?;
				Ok(Some(v))
			}
			None => Ok(None),
		}
	}
	fn size(&self) -> usize {
		self.size
	}
	fn clear(&mut self) -> Result<(), Error> {
		self.clear_impl()
	}

	fn iter<'a>(&'a self) -> HashtableIterator<'a, K, V> {
		HashtableIterator::new(self, self.tail)
	}

	fn copy(&self) -> Self {
		self.copy_impl()
	}
}

impl<K> StaticHashset<K> for StaticHashImpl
where
	K: Serializable + Hash + PartialEq,
{
	fn insert(&mut self, key: &K) -> Result<(), Error> {
		self.insert_impl::<K, K>(key, None)
	}
	fn contains(&self, key: &K) -> Result<bool, Error> {
		match self.get_impl(key)? {
			Some(_) => Ok(true),
			None => Ok(false),
		}
	}
	fn remove(&mut self, key: &K) -> Result<bool, Error> {
		match self.get_impl(key)? {
			Some((entry, _reader)) => {
				self.remove_impl(entry)?;
				Ok(true)
			}
			None => Ok(false),
		}
	}
	fn size(&self) -> usize {
		self.size
	}
	fn clear(&mut self) -> Result<(), Error> {
		self.clear_impl()
	}

	fn iter<'a>(&'a self) -> HashsetIterator<'a, K> {
		HashsetIterator::new(self, self.tail)
	}

	fn copy(&self) -> Self {
		self.copy_impl()
	}
}

impl<V> StaticList<V> for StaticHashImpl
where
	V: Serializable,
{
	fn push(&mut self, value: &V) -> Result<(), Error> {
		todo!()
	}
	fn iter<'a>(&'a self) -> StaticListIterator<'a, V> {
		todo!()
	}
	fn iter_rev<'a>(&'a self) -> StaticListIterator<'a, V> {
		todo!()
	}
	fn size(&self) -> usize {
		self.size
	}
	fn clear(&mut self) -> Result<(), Error> {
		self.clear_impl()
	}
	fn append(&mut self, list: &impl StaticList<V>) -> Result<(), Error> {
		todo!()
	}
	fn copy(&self) -> Self {
		self.copy_impl()
	}
}

impl StaticBuilder {
	pub fn build_hashtable<K, V>(
		config: StaticHashtableConfig,
		slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	) -> Result<impl StaticHashtable<K, V>, Error>
	where
		K: Serializable + Hash + PartialEq,
		V: Serializable,
	{
		StaticHashImpl::new(Some(config), None, None, slabs)
	}

	pub fn build_hashset<K>(
		config: StaticHashsetConfig,
		slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	) -> Result<impl StaticHashset<K>, Error>
	where
		K: Serializable + Hash + PartialEq,
	{
		StaticHashImpl::new(None, Some(config), None, slabs)
	}

	pub fn build_list<V>(
		config: StaticListConfig,
		slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	) -> Result<impl StaticList<V>, Error>
	where
		V: Serializable,
	{
		StaticHashImpl::new(None, None, Some(config), slabs)
	}
}

#[cfg(test)]
mod test {
	use crate::static_hash::StaticBuilder;
	use crate::types::StaticHashset;
	use crate::StaticHashsetConfig;
	use crate::GLOBAL_SLAB_ALLOCATOR;
	use crate::{StaticHashtable, StaticHashtableConfig};
	use bmw_deps::rand::random;
	use bmw_err::*;
	use bmw_log::*;
	use std::collections::HashMap;

	info!();

	#[test]
	fn test_static_hashtable() -> Result<(), Error> {
		let mut hashtable = StaticBuilder::build_hashtable(StaticHashtableConfig::default(), None)?;
		hashtable.insert(&1, &2)?;
		let v = hashtable.get(&1)?;
		assert_eq!(v.unwrap(), 2);
		assert_eq!(hashtable.size(), 1);
		Ok(())
	}

	#[test]
	fn test_remove_static_hashtable() -> Result<(), Error> {
		let mut hashtable = StaticBuilder::build_hashtable(StaticHashtableConfig::default(), None)?;
		hashtable.insert(&1, &2)?;
		let v = hashtable.get(&1)?;
		assert_eq!(v.unwrap(), 2);
		assert_eq!(hashtable.size(), 1);
		assert_eq!(hashtable.remove(&2)?, None);
		assert_eq!(hashtable.remove(&1)?, Some(2));
		assert_eq!(hashtable.remove(&1)?, None);
		assert_eq!(hashtable.size(), 0);

		Ok(())
	}

	#[test]
	fn test_compare() -> Result<(), Error> {
		let mut keys = vec![];
		let mut values = vec![];
		for _ in 0..1_000 {
			keys.push(random::<u32>());
			values.push(random::<u32>());
		}
		let mut hashtable = StaticBuilder::build_hashtable(StaticHashtableConfig::default(), None)?;
		let mut hashmap = HashMap::new();
		for i in 0..1_000 {
			hashtable.insert(&keys[i], &values[i])?;
			hashmap.insert(&keys[i], &values[i]);
		}

		for _ in 0..100 {
			let index: usize = random::<usize>() % 1_000;
			hashtable.remove(&keys[index])?;
			hashmap.remove(&keys[index]);
		}

		let mut i = 0;
		for (k, vm) in &hashmap {
			let vt = hashtable.get(&k)?;
			assert_eq!(&vt.unwrap(), *vm);
			i += 1;
		}

		assert_eq!(i, hashtable.size());
		assert_eq!(i, hashmap.len());

		Ok(())
	}

	#[test]
	fn test_iterator() -> Result<(), Error> {
		let mut hashtable = StaticBuilder::build_hashtable(StaticHashtableConfig::default(), None)?;
		hashtable.insert(&1, &10)?;
		hashtable.insert(&2, &20)?;
		hashtable.insert(&3, &30)?;
		hashtable.insert(&4, &40)?;
		let size = hashtable.size();
		let mut i = 0;
		for (k, v) in hashtable.iter() {
			info!("k={},v={}", k, v)?;
			assert_eq!(hashtable.get(&k)?, Some(v));
			i += 1;
		}

		assert_eq!(i, 4);
		assert_eq!(size, i);

		hashtable.remove(&3)?;
		let size = hashtable.size();
		let mut i = 0;
		for (k, v) in hashtable.iter() {
			info!("k={},v={}", k, v)?;
			assert_eq!(hashtable.get(&k)?, Some(v));
			i += 1;
		}
		assert_eq!(i, 3);
		assert_eq!(size, i);

		hashtable.remove(&4)?;
		let size = hashtable.size();
		let mut i = 0;
		for (k, v) in hashtable.iter() {
			info!("k={},v={}", k, v)?;
			assert_eq!(hashtable.get(&k)?, Some(v));
			i += 1;
		}
		assert_eq!(i, 2);
		assert_eq!(size, i);

		Ok(())
	}

	#[test]
	fn test_clear() -> Result<(), Error> {
		let mut hashtable = StaticBuilder::build_hashtable(StaticHashtableConfig::default(), None)?;
		let free_count1 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count1)?;

		hashtable.insert(&1, &10)?;
		hashtable.insert(&2, &20)?;
		hashtable.insert(&3, &30)?;
		hashtable.insert(&4, &40)?;
		let size = hashtable.size();
		let mut i = 0;
		for (k, v) in hashtable.iter() {
			info!("k={},v={}", k, v)?;
			assert_eq!(hashtable.get(&k)?, Some(v));
			i += 1;
		}

		assert_eq!(i, 4);
		assert_eq!(size, i);

		hashtable.clear()?;
		assert_eq!(hashtable.size(), 0);

		let free_count2 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count2)?;
		assert_eq!(free_count1, free_count2);

		Ok(())
	}

	#[test]
	fn test_hashtable_drop() -> Result<(), Error> {
		let free_count1;
		{
			let mut hashtable =
				StaticBuilder::build_hashtable(StaticHashtableConfig::default(), None)?;
			free_count1 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
			})?;
			info!("free_count={}", free_count1)?;

			hashtable.insert(&1, &10)?;
			hashtable.insert(&2, &20)?;
			hashtable.insert(&3, &30)?;
			hashtable.insert(&4, &40)?;
			let size = hashtable.size();
			let mut i = 0;
			for (k, v) in hashtable.iter() {
				info!("k={},v={}", k, v)?;
				assert_eq!(hashtable.get(&k)?, Some(v));
				i += 1;
			}

			assert_eq!(i, 4);
			assert_eq!(size, i);
		}

		let free_count2 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count2)?;
		assert_eq!(free_count1, free_count2);

		Ok(())
	}

	#[test]
	fn test_hashset() -> Result<(), Error> {
		let mut hashset = StaticBuilder::build_hashset(StaticHashsetConfig::default(), None)?;
		hashset.insert(&1)?;
		hashset.insert(&2)?;
		hashset.insert(&3)?;
		hashset.insert(&4)?;
		let size = hashset.size();
		let mut i = 0;
		for k in hashset.iter() {
			info!("k={}", k)?;
			assert_eq!(hashset.contains(&k)?, true);
			i += 1;
		}

		assert_eq!(i, 4);
		assert_eq!(size, i);

		hashset.remove(&3)?;
		let size = hashset.size();
		let mut i = 0;
		for k in hashset.iter() {
			info!("k={}", k)?;
			assert_eq!(hashset.contains(&k)?, true);
			i += 1;
		}
		assert_eq!(i, 3);
		assert_eq!(size, i);

		hashset.remove(&4)?;
		let size = hashset.size();
		let mut i = 0;
		for k in hashset.iter() {
			info!("k={}", k)?;
			assert_eq!(hashset.contains(&k)?, true);
			i += 1;
		}
		assert_eq!(i, 2);
		assert_eq!(size, i);
		Ok(())
	}
}
