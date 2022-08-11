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
use std::cell::UnsafeCell;
use std::collections::hash_map::DefaultHasher;
use std::convert::TryInto;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::io::Cursor;

info!();

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
	slabs_unsafe: Option<&'a UnsafeCell<Box<dyn SlabAllocator>>>,
	slabs: Option<&'a mut Box<dyn SlabAllocator>>,
	entry_array: Vec<u64>,
}

impl<'a> StaticHashImpl<'a> {
	fn new(config: StaticHashConfig, slabs: &'a mut Box<dyn SlabAllocator>) -> Result<Self, Error> {
		let mut entry_array = vec![];
		Self::init(&config, &mut entry_array)?;
		Ok(StaticHashImpl {
			config,
			slabs_unsafe: None,
			slabs: Some(slabs),
			entry_array,
		})
	}

	fn new_unsafe(
		config: StaticHashConfig,
		slabs: &'a UnsafeCell<Box<dyn SlabAllocator>>,
	) -> Result<Self, Error> {
		let mut entry_array = vec![];
		Self::init(&config, &mut entry_array)?;

		Ok(StaticHashImpl {
			config,
			slabs_unsafe: Some(slabs),
			slabs: None,
			entry_array,
		})
	}

	fn init(config: &StaticHashConfig, entry_array: &mut Vec<u64>) -> Result<(), Error> {
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
		debug!("entry array init to size = {}", size)?;
		entry_array.resize(size, SLOT_EMPTY);
		Ok(())
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
	fn slabs_as_ref(&self) -> Result<&dyn SlabAllocator, Error> {
		match self.slabs_unsafe {
			Some(slabs) => match unsafe { slabs.get().as_ref() } {
				Some(slabs) => Ok(&**slabs),
				None => Err(err!(ErrKind::IllegalState, "slab allocator not accessible")),
			},
			None => Ok(&***self.slabs.as_ref().unwrap()),
		}
	}

	fn slabs_as_mut(&mut self) -> Result<&mut dyn SlabAllocator, Error> {
		match self.slabs_unsafe {
			Some(slabs) => match unsafe { slabs.get().as_mut() } {
				Some(slabs) => Ok(&mut **slabs),
				None => Err(err!(ErrKind::IllegalState, "slab allocator not accessible")),
			},
			None => Ok(&mut ***self.slabs.as_mut().unwrap()),
		}
	}

	fn get_impl<K, V>(&self, key: &K) -> Result<Option<V>, Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		debug!("get_impl:self.config={:?},k={:?}", self.config, key)?;
		let entry = self.find_entry(key)?;
		debug!("entry at {:?}", entry)?;

		match entry {
			Some(entry) => Ok(Some(self.read_value(entry)?)),
			None => Ok(None),
		}
	}

	fn read_value<V>(&self, entry: usize) -> Result<V, Error>
	where
		V: Serializable,
	{
		let mut v: Vec<u8> = vec![];
		let slab_id = self.entry_array[entry];
		let mut slab = self.slabs_as_ref()?.get(slab_id)?;
		let bytes_per_slab: usize = self
			.slabs_as_ref()?
			.slab_size()?
			.saturating_sub(SLAB_OVERHEAD)
			.try_into()?;
		let mut krem: usize = u64::from_be_bytes(slab.get()[0..8].try_into()?).try_into()?;
		let klen = krem;
		krem += 8;
		let mut slab_offset = 8;
		let mut value_len: usize = 0;
		let mut voffset = usize::MAX;
		let mut vrem = usize::MAX;
		loop {
			let slab_bytes = slab.get();
			debug!("slab_id={}, krem+8={}", slab.id(), krem + 8)?;
			if krem < slab_offset + bytes_per_slab
				&& voffset == usize::MAX
				&& krem + 8 <= bytes_per_slab
			{
				value_len =
					u64::from_be_bytes(slab_bytes[krem..krem + 8].try_into()?).try_into()?;

				if value_len > 1_000_000 {
					debug!("krem={},value_len={}", krem, value_len)?;
					for i in krem..krem + 32 {
						debug!("=====[{}]={}", i, slab_bytes[i])?;
					}
					break;
				}

				voffset = krem + 8;
				vrem = value_len;
				debug!(
					"krem={},value_len={},voffset={},klen={}",
					krem, value_len, voffset, klen
				)?;
			}

			if voffset != usize::MAX {
				let mut end = (value_len - v.len()) + voffset;
				if end > bytes_per_slab {
					end = bytes_per_slab;
				}
				v.extend(&slab_bytes[voffset..end]);
				debug!("vlen={}, vrem={}, end={}", v.len(), vrem, end)?;
				voffset = 0;
				vrem = vrem.saturating_sub(end - voffset);
			}

			if v.len() >= value_len && voffset != usize::MAX {
				break;
			}

			krem = krem.saturating_sub(bytes_per_slab);
			let next =
				u64::from_be_bytes(slab_bytes[bytes_per_slab..bytes_per_slab + 8].try_into()?);
			slab = self.slabs_as_ref()?.get(next)?;
			slab_offset = 0;
		}

		debug!("break with vlen={}", v.len())?;

		let mut cursor = Cursor::new(v);
		cursor.set_position(0);
		let mut reader = BinReader::new(&mut cursor);
		let ret = V::read(&mut reader)?;
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
		let slabs = self.slabs_as_ref()?;
		// serialize key
		let mut k = vec![];
		serialize(&mut k, key)?;

		let klen = k.len();
		debug!("key_len={}", klen)?;

		// read first slab
		let slab = slabs.get(id)?;
		let len = u64::from_be_bytes(slab.get()[0..8].try_into()?);
		debug!("len={}", len)?;

		if len != klen.try_into()? {
			return Ok(false);
		}

		let mut offset = 0;
		let bytes_per_slab: usize = slabs
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

		let slab_size = self.slabs_as_ref()?.slab_size()?;
		let bytes_per_slab = slab_size.saturating_sub(SLAB_OVERHEAD);

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

		debug!(
			"klen={},k.len={}, bytes_per_slab={},varA={},varB={}",
			key_len,
			k.len(),
			bytes_per_slab,
			(key_len + 15) % bytes_per_slab,
			bytes_per_slab.saturating_sub((key_len + 16) % bytes_per_slab)
		)?;

		if (key_len + 15) % bytes_per_slab < 8 {
			for _ in (key_len + 15) % bytes_per_slab..7 {
				debug!("push0==========")?;
				k.push(0);
			}
		}

		//if (k.len() + 16) % bytes_per_slab

		k.extend(v.len().to_be_bytes());
		k.extend(v);
		let mut v = vec![];
		v.extend(key_len.to_be_bytes());
		v.extend(k);

		let needed_len = v.len();
		let slabs_needed = needed_len as u64 / bytes_per_slab;

		debug!("slabs needed = {}", slabs_needed)?;

		if self.slabs_as_ref()?.free_count()? < slabs_needed {
			return Err(err!(ErrKind::CapacityExceeded, "no more slabs"));
		}

		let mut v_offset: usize = 0;
		let bytes_per_slab: usize = self
			.slabs_as_ref()?
			.slab_size()?
			.saturating_sub(SLAB_OVERHEAD)
			.try_into()?;
		let mut slab_list = vec![];

		debug!("vlen={}", v.len())?;
		loop {
			let mut slab = self.slabs_as_mut()?.allocate()?;
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
			let mut slab = self.slabs_as_mut()?.get_mut(slab_list[i])?;
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
	pub fn build_unsafe<'a, K, V>(
		config: StaticHashtableConfig,
		slabs: &'a UnsafeCell<Box<dyn SlabAllocator>>,
	) -> Result<Box<dyn StaticHashtable<K, V> + 'a>, Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		Ok(Box::new(StaticHashImpl::new_unsafe(config.into(), slabs)?))
	}

	pub fn build<'a, K, V>(
		config: StaticHashtableConfig,
		slabs: &'a mut Box<dyn SlabAllocator>,
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
	pub fn build_unsafe<'a, K>(
		config: StaticHashsetConfig,
		slabs: &'a UnsafeCell<Box<dyn SlabAllocator>>,
	) -> Result<Box<dyn StaticHashset<K> + 'a>, Error>
	where
		K: Serializable + Hash,
	{
		Ok(Box::new(StaticHashImpl::new_unsafe(config.into(), slabs)?))
	}

	pub fn build<'a, K, V>(
		config: StaticHashsetConfig,
		slabs: &'a mut Box<dyn SlabAllocator>,
	) -> Result<Box<dyn StaticHashtable<K, V> + 'a>, Error>
	where
		K: Serializable + Hash,
		V: Serializable,
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
		val1: u32,
		val2: u32,
		middle: Vec<u8>,
	}

	impl BigThing {
		fn new(val1: u32, val2: u32) -> Self {
			let mut middle = vec![];
			for i in 0..val1 {
				middle.push((i % 256) as u8);
			}
			Self { val1, val2, middle }
		}
	}

	impl Serializable for BigThing {
		fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
			let val1 = reader.read_u32()?;
			let mut middle = vec![];
			for _ in 0..val1 {
				middle.push(reader.read_u8()?);
			}
			let val2 = reader.read_u32()?;
			Ok(Self { val1, val2, middle })
		}
		fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
			writer.write_u32(self.val1)?;
			for i in 0..self.val1 {
				writer.write_u8(self.middle[i as usize])?;
			}
			writer.write_u32(self.val2)?;
			Ok(())
		}
	}

	#[test]
	fn test_static_hashtable() -> Result<(), Error> {
		let slabs1 = SlabAllocatorBuilder::build_unsafe(SlabAllocatorConfig::default())?;
		let mut slabs2 = SlabAllocatorBuilder::build(SlabAllocatorConfig::default())?;
		let mut sh =
			StaticHashtableBuilder::build_unsafe(StaticHashtableConfig::default(), &slabs1)?;
		sh.insert(&1, &2)?;
		assert_eq!(sh.get(&1)?, Some(2));

		/*let mut sh2 =
		StaticHashtableBuilder::build_unsafe(StaticHashtableConfig::default(), &slabs1)?; */

		/*
		for i in 0..993 {
			sh2.insert(&BigThing::new(i + 1, 5), &BigThing::new(1, i))?;
			assert_eq!(
				sh2.get(&BigThing::new(i + 1, 5))?,
				Some(BigThing::new(1, i))
			);
		}
				*/

		let mut sh2 =
			StaticHashtableBuilder::build_unsafe(StaticHashtableConfig::default(), &slabs1)?;
		for i in 0..2000 {
			sh2.insert(&BigThing::new(i, i), &BigThing::new(i, i))?;
			assert_eq!(sh2.get(&BigThing::new(i, i))?, Some(BigThing::new(i, i)));
		}

		let mut sh3 = StaticHashtableBuilder::build(StaticHashtableConfig::default(), &mut slabs2)?;
		sh3.insert(&10, &20)?;
		assert_eq!(sh3.get(&10)?, Some(20));

		Ok(())
	}
}
