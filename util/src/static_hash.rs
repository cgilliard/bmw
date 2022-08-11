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

use crate::ser::{serialize, BinReader};
use crate::types::StaticHashset;
use crate::{Serializable, SlabAllocator, StaticHashtable, StaticIterator};
use bmw_err::{err, ErrKind, Error};
use bmw_log::*;
use std::collections::hash_map::DefaultHasher;
use std::convert::TryInto;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::io::Cursor;

debug!();

const SLAB_OVERHEAD: u64 = 8;
const SLOT_EMPTY: u64 = u64::MAX;
const SLOT_DELETED: u64 = u64::MAX - 1;

#[derive(Debug)]
pub struct StaticHashtableConfig {
	max_entries: u64,
	max_load_factor: f64,
}

#[derive(Debug)]
pub struct StaticHashsetConfig {
	max_entries: u64,
	max_load_factor: f64,
}

#[derive(Debug)]
struct StaticHashConfig {
	max_entries: u64,
	max_load_factor: f64,
}

impl From<StaticHashtableConfig> for StaticHashConfig {
	fn from(config: StaticHashtableConfig) -> Self {
		let _ = debug!("converting {:?}", config);
		Self {
			max_entries: config.max_entries,
			max_load_factor: config.max_load_factor,
		}
	}
}

impl From<StaticHashsetConfig> for StaticHashConfig {
	fn from(config: StaticHashsetConfig) -> Self {
		let _ = debug!("converting {:?}", config);
		Self {
			max_entries: config.max_entries,
			max_load_factor: config.max_load_factor,
		}
	}
}

impl Default for StaticHashtableConfig {
	fn default() -> Self {
		Self {
			max_entries: 1_000_000,
			max_load_factor: 0.75,
		}
	}
}

impl Default for StaticHashsetConfig {
	fn default() -> Self {
		Self {
			max_entries: 1_000_000,
			max_load_factor: 0.75,
		}
	}
}

struct StaticHashImpl<'a> {
	config: StaticHashConfig,
	slabs: &'a mut dyn SlabAllocator,
	entry_array: Vec<u64>,
}

impl<'a> StaticHashImpl<'a> {
	fn new(config: StaticHashConfig, slabs: &'a mut dyn SlabAllocator) -> Result<Self, Error> {
		if config.max_load_factor <= 0.0 || config.max_load_factor > 1.0 {
			return Err(err!(
				ErrKind::IllegalArgument,
				"StaticHash: max_load_factor must be greater than 0 and equal to or less than 1."
			));
		}
		// calculate size of entry_array. Must be possible to have max_entries, with the
		// max_load_factor
		let size: i32 = (config.max_entries as f64 / config.max_load_factor).floor() as i32;
		let size: usize = size.try_into()?;
		let mut entry_array = vec![];
		debug!("entry array init to size = {}", size)?;
		entry_array.resize(size, SLOT_EMPTY);

		Ok(StaticHashImpl {
			config,
			slabs,
			entry_array,
		})
	}
}

impl<'a, K, V> StaticHashtable<K, V> for StaticHashImpl<'a>
where
	K: Serializable + Hash,
	V: Serializable,
{
	fn insert(&mut self, key: &K, value: &V) -> Result<(), Error> {
		self.insert_impl::<K, V>(key, Some(value))
	}
	fn get(&self, key: &K) -> Result<Option<V>, Error> {
		self.get_impl(key)
	}
	fn remove(&mut self, key: &K) -> Result<(), Error> {
		self.remove_impl(key)
	}
	fn iter(&self) -> Result<Box<dyn StaticIterator<'_, (K, V)>>, Error> {
		self.iter_impl()
	}
}

impl<'a, K> StaticHashset<K> for StaticHashImpl<'a>
where
	K: Serializable + Hash,
{
	fn insert(&mut self, key: &K) -> Result<(), Error> {
		self.insert_impl::<K, K>(key, None)
	}
	fn contains(&self, key: &K) -> Result<bool, Error> {
		debug!("contains:self.config={:?},k={:?}", self.config, key)?;
		todo!()
	}
	fn remove(&mut self, key: &K) -> Result<(), Error> {
		self.remove_impl(key)
	}
	fn iter(&self) -> Result<Box<dyn StaticIterator<'_, K>>, Error> {
		self.iter_impl()
	}
}

impl<'a> StaticHashImpl<'a> {
	fn get_impl<K, V>(&self, key: &K) -> Result<Option<V>, Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		debug!("get_impl:self.config={:?},k={:?}", self.config, key)?;
		let entry = self.find_entry(key)?;
		debug!("entry at {:?}", entry)?;

		match entry {
			Some(entry) => self.read_value(entry),
			None => Ok(None),
		}
	}

	fn read_value<V>(&self, entry: usize) -> Result<Option<V>, Error>
	where
		V: Serializable,
	{
		let slab_id = self.entry_array[entry];
		let slab = self.slabs.get(slab_id)?;
		let slab_bytes = slab.get();

		// TODO: multislab
		let klen = u64::from_be_bytes(slab_bytes[0..8].try_into()?);
		let mut offset: usize = klen.try_into()?;
		offset += 8;
		let valuelen = u64::from_be_bytes(slab_bytes[offset..offset + 8].try_into()?);
		let valuelen: usize = valuelen.try_into()?;
		let mut v: Vec<u8> = vec![];
		v.extend(&slab_bytes[offset + 8..offset + 8 + valuelen]);
		let mut cursor = Cursor::new(v);
		cursor.set_position(0);
		let mut reader = BinReader::new(&mut cursor);
		let ret = Some(V::read(&mut reader)?);
		Ok(ret)
	}

	fn find_entry<K>(&self, key: &K) -> Result<Option<usize>, Error>
	where
		K: Serializable + Hash,
	{
		let mut entry = self.entry_hash(key);

		let max_iter = self.entry_array.len();
		let mut i = 0;
		loop {
			if i >= max_iter {
				return Ok(None);
			}
			if self.entry_array[entry] == SLOT_EMPTY {
				// empty slot means this key is not in the table
				return Ok(None);
			}

			if self.entry_array[entry] != SLOT_DELETED {
				// there's a valid entry here. Check if it's ours
				if self.key_match(self.entry_array[entry], key)? {
					return Ok(Some(entry));
				}
			}

			entry = (entry + 1) % max_iter;
			i += 1;
		}
	}

	fn key_match<K>(&self, id: u64, key: &K) -> Result<bool, Error>
	where
		K: Serializable,
	{
		// serialize key
		let mut k = vec![];
		serialize(&mut k, key)?;

		let klen = k.len();
		debug!("key_len={}", klen)?;

		// read first slab
		let slab = self.slabs.get(id)?;
		let len = u64::from_be_bytes(slab.get()[0..8].try_into()?);
		debug!("len={}", len)?;

		if len != klen.try_into()? {
			return Ok(false);
		}

		let mut offset = 0;
		let bytes_per_slab: usize = self
			.slabs
			.slab_size()?
			.saturating_sub(SLAB_OVERHEAD)
			.try_into()?;

		// compare the first slab
		let mut end = 8 + klen;
		if end > bytes_per_slab {
			end = bytes_per_slab;
		}
		if slab.get()[8..end] != k[0..end - 8] {
			return Ok(false);
		}
		if end < bytes_per_slab {
			return Ok(true);
		}

		loop {
			offset += bytes_per_slab;
			if offset >= klen {
				break;
			}
		}

		Ok(true)
	}

	fn insert_impl<K, V>(&mut self, key: &K, value: Option<&V>) -> Result<(), Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		debug!("insert:self.config={:?},k={:?}", self.config, key)?;

		let max_iter = self.entry_array.len();
		let mut entry = self.entry_hash(key);
		let mut i = 0;
		loop {
			if i >= max_iter {
				return Err(err!(
					ErrKind::CapacityExceeded,
					"StaticHashImpl: Capacity exceeded"
				));
			}
			if self.entry_array[entry] == SLOT_EMPTY || self.entry_array[entry] == SLOT_DELETED {
				// found an empty or deleted slot, use it
				break;
			}
			entry = (entry + 1) % max_iter;
			i += 1;
		}
		debug!("inserting at entry={}", entry)?;

		let mut k = vec![];
		serialize(&mut k, key)?;
		let key_len: u64 = k.len().try_into()?;
		let mut v = vec![];
		match value {
			Some(value) => {
				serialize(&mut v, value)?;
			}
			None => {}
		}
		k.extend(v.len().to_be_bytes());
		k.extend(v);
		let mut v = vec![];
		v.extend(key_len.to_be_bytes());
		v.extend(k);
		let needed_len = v.len();
		let slabs_needed =
			needed_len as u64 / self.slabs.slab_size()?.saturating_sub(SLAB_OVERHEAD);

		if self.slabs.free_count()? < slabs_needed {
			return Err(err!(ErrKind::CapacityExceeded, "no more slabs"));
		}

		let mut v_offset: usize = 0;
		let bytes_per_slab: usize = self
			.slabs
			.slab_size()?
			.saturating_sub(SLAB_OVERHEAD)
			.try_into()?;
		let mut slab_list = vec![];

		debug!("vlen={}", v.len())?;
		loop {
			let mut slab = self.slabs.allocate()?;
			slab_list.push(slab.id());
			let mut len = v.len() - v_offset;
			if len > bytes_per_slab {
				len = bytes_per_slab;
			}
			slab.get_mut()[0..len].clone_from_slice(&v[v_offset..v_offset + len]);
			v_offset += len;
			if v_offset >= v.len().try_into()? {
				break;
			}
		}

		debug!("slab_list={:?}", slab_list)?;
		self.entry_array[entry] = slab_list[0];
		let slab_list_len = slab_list.len();
		for i in 0..slab_list_len.saturating_sub(1) {
			trace!("processing slab i={}", i)?;
			let mut slab = self.slabs.get_mut(slab_list[i])?;
			slab.get_mut()[bytes_per_slab..bytes_per_slab + 8]
				.clone_from_slice(&slab_list[i + 1].to_be_bytes());
		}

		Ok(())
	}

	fn remove_impl<K>(&mut self, key: &K) -> Result<(), Error>
	where
		K: Serializable,
	{
		debug!("remove:self.config={:?},k={:?}", self.config, key)?;
		todo!();
	}

	fn iter_impl<K>(&self) -> Result<Box<dyn StaticIterator<'_, K>>, Error>
	where
		K: Serializable,
	{
		debug!("iter:self.config={:?}", self.config)?;
		todo!()
	}

	fn entry_hash<K: Hash>(&self, key: &K) -> usize {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;

		let max_iter = self.entry_array.len();

		hash % max_iter
	}
}

pub struct StaticHashtableBuilder {}

impl StaticHashtableBuilder {
	pub fn build<'a, K, V>(
		config: StaticHashtableConfig,
		slabs: &'a mut dyn SlabAllocator,
	) -> Result<Box<dyn StaticHashtable<K, V> + 'a>, Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		Ok(Box::new(StaticHashImpl::new(config.into(), slabs)?))
	}
}

pub struct StaticHashsetBuilder {}

impl StaticHashsetBuilder {
	pub fn build<'a, K>(
		config: StaticHashsetConfig,
		slabs: &'a mut dyn SlabAllocator,
	) -> Result<Box<dyn StaticHashset<K> + 'a>, Error>
	where
		K: Serializable + Hash,
	{
		Ok(Box::new(StaticHashImpl::new(config.into(), slabs)?))
	}
}

#[cfg(test)]
mod test {
	use crate::types::{Reader, Writer};
	use crate::SlabAllocatorConfig;
	use crate::{
		Serializable, SlabAllocatorBuilder, StaticHashtableBuilder, StaticHashtableConfig,
	};
	use bmw_err::Error;

	#[derive(Debug, Hash, PartialEq)]
	struct BigThing {
		val: u32,
	}

	impl BigThing {
		fn new(val: u32) -> Self {
			Self { val }
		}
	}

	impl Serializable for BigThing {
		fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
			let val = reader.read_u32()?;
			Ok(Self { val })
		}
		fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
			writer.write_u32(self.val)?;
			Ok(())
		}
	}

	#[test]
	fn test_static_hashtable() -> Result<(), Error> {
		let mut slabs1 = SlabAllocatorBuilder::build(SlabAllocatorConfig::default())?;
		let mut slabs2 = SlabAllocatorBuilder::build(SlabAllocatorConfig::default())?;
		let mut sh = StaticHashtableBuilder::build(StaticHashtableConfig::default(), &mut *slabs1)?;
		sh.insert(&1, &2)?;
		assert_eq!(sh.get(&1)?, Some(2));

		let mut sh2 =
			StaticHashtableBuilder::build(StaticHashtableConfig::default(), &mut *slabs2)?;
		sh2.insert(&BigThing::new(1), &BigThing::new(10))?;
		assert_eq!(sh2.get(&BigThing::new(1))?, Some(BigThing::new(10)));
		Ok(())
	}
}
