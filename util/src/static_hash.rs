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
use crate::{
	Serializable, Slab, SlabAllocator, SlabAllocatorConfig, SlabMut, StaticHashtable,
	StaticIterator, GLOBAL_SLAB_ALLOCATOR,
};
use bmw_err::{err, ErrKind, Error};
use bmw_log::*;
use std::collections::hash_map::DefaultHasher;
use std::convert::TryInto;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::io::Cursor;

info!();

const SLAB_OVERHEAD: u64 = 8;
const SLOT_EMPTY: u64 = u64::MAX;
const SLOT_DELETED: u64 = u64::MAX - 1;
const GLOBAL_ALLOCATOR_NOT_INITIALIZED: &str =
	"Thread local slab allocator was not initialized. Initializing with default values.";

#[derive(Debug)]
pub struct StaticHashtableConfig {
	pub max_entries: u64,
	pub max_load_factor: f64,
}

#[derive(Debug)]
pub struct StaticHashsetConfig {
	pub max_entries: u64,
	pub max_load_factor: f64,
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

struct StaticHashImpl {
	config: StaticHashConfig,
	slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	entry_array: Vec<u64>,
	slab_size: usize,
	first_entry: usize,
}

impl StaticHashImpl {
	fn new(
		config: StaticHashConfig,
		slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	) -> Result<Self, Error> {
		let mut entry_array = vec![];
		Self::init(&config, &mut entry_array)?;
		let (slab_size, slabs) = match slabs {
			Some(slabs) => (usize!(slabs.slab_size()?), Some(slabs)),
			None => {
				let slab_size = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
					let slabs = unsafe { f.get().as_mut().unwrap() };
					let slab_size = match slabs.slab_size() {
						Ok(slab_size) => usize!(slab_size),
						Err(_e) => {
							warn!(GLOBAL_ALLOCATOR_NOT_INITIALIZED)?;
							slabs.init(SlabAllocatorConfig::default())?;
							usize!(slabs.slab_size()?)
						}
					};
					Ok(slab_size)
				})?;
				(slab_size, None)
			}
		};
		Ok(StaticHashImpl {
			config,
			slab_size,
			slabs,
			entry_array,
			first_entry: usize::MAX,
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
		debug!("entry array init to size = {}", size)?;
		entry_array.resize(usize!(size), SLOT_EMPTY);
		Ok(())
	}
}

pub struct StaticHashtableIter<'a, K, V> {
	count: u32,
	h: &'a Box<dyn StaticHashtable<K, V>>,
}

impl<'a, K, V> Iterator for StaticHashtableIter<'a, K, V>
where
	K: Serializable + Hash,
	V: Serializable,
{
	type Item = (K, V);
	fn next(self: &mut StaticHashtableIter<'a, K, V>) -> Option<(K, V)> {
		None
	}
}

impl<'a, K, V> IntoIterator for &'a Box<dyn StaticHashtable<K, V>>
where
	K: Serializable + Hash,
	V: Serializable,
{
	type Item = (K, V);
	type IntoIter = StaticHashtableIter<'a, K, V>;
	fn into_iter(self) -> Self::IntoIter {
		Self::IntoIter { count: 0, h: &self }
	}
}

impl<K, V> StaticHashtable<K, V> for StaticHashImpl
where
	K: Serializable + Hash,
	V: Serializable,
{
	fn insert(&mut self, key: &K, value: &V) -> Result<(), Error> {
		self.insert_impl::<K, V>(Some(key), 0, None, Some(value), None)
	}
	fn get(&self, key: &K) -> Result<Option<V>, Error> {
		self.get_impl(Some(key), None, 0)
	}
	fn remove(&mut self, key: &K) -> Result<(), Error> {
		self.remove_impl(key)
	}
	fn iter(&self) -> Result<Box<dyn StaticIterator<'_, (K, V)>>, Error> {
		self.iter_impl()
	}
	fn get_raw<'b>(&'b self, key: &[u8], hash: u64) -> Result<Option<Box<dyn Slab + 'b>>, Error> {
		self.get_raw_impl::<K>(key, hash)
	}
	fn get_raw_mut<'b>(
		&'b mut self,
		key: &[u8],
		hash: u64,
	) -> Result<Option<Box<dyn SlabMut + 'b>>, Error> {
		self.get_raw_mut_impl::<K>(key, hash)
	}
	fn insert_raw(&mut self, key: &[u8], hash: u64, value: &[u8]) -> Result<(), Error> {
		self.insert_impl::<K, V>(None, hash, Some(key), None, Some(value))
	}
}

impl<K> StaticHashset<K> for StaticHashImpl
where
	K: Serializable + Hash,
{
	fn insert(&mut self, key: &K) -> Result<(), Error> {
		self.insert_impl::<K, K>(Some(key), 0, None, None, None)
	}
	fn contains(&self, key: &K) -> Result<bool, Error> {
		debug!("contains:self.config={:?},k={:?}", self.config, key)?;
		Ok(self.find_entry(Some(key), None, 0)?.is_some())
	}
	fn remove(&mut self, key: &K) -> Result<(), Error> {
		self.remove_impl(key)
	}
	fn iter(&self) -> Result<Box<dyn StaticIterator<'_, K>>, Error> {
		self.iter_impl()
	}
	fn insert_raw(&mut self, _key: &[u8]) -> Result<(), Error> {
		todo!()
	}
}

impl StaticHashImpl {
	fn get_slab<'a>(&'a self, id: u64) -> Result<Box<dyn Slab + 'a>, Error> {
		match &self.slabs {
			Some(slabs) => Ok(slabs.get(id)?),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<Box<dyn Slab>, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().get(id)? })
			}),
		}
	}

	fn get_mut<'a>(&'a mut self, id: u64) -> Result<Box<dyn SlabMut + 'a>, Error> {
		match &mut self.slabs {
			Some(slabs) => Ok(slabs.get_mut(id)?),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<Box<dyn SlabMut>, Error> {
				Ok(unsafe { f.get().as_mut().unwrap().get_mut(id)? })
			}),
		}
	}

	fn allocate<'a>(&'a mut self) -> Result<Box<dyn SlabMut + 'a>, Error> {
		match &mut self.slabs {
			Some(slabs) => Ok(slabs.allocate()?),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<Box<dyn SlabMut>, Error> {
				Ok(unsafe { f.get().as_mut().unwrap().allocate()? })
			}),
		}
	}

	fn get_free_count(&self) -> Result<usize, Error> {
		match &self.slabs {
			Some(slabs) => Ok(usize!(slabs.free_count()?)),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
				Ok(usize!(unsafe { f.get().as_ref().unwrap().free_count()? }))
			}),
		}
	}

	fn get_raw_mut_impl<'b, K>(
		&'b mut self,
		key_raw: &[u8],
		hash: u64,
	) -> Result<Option<Box<dyn SlabMut + 'b>>, Error>
	where
		K: Serializable + Hash,
	{
		let entry = self.find_entry::<K>(None, Some(key_raw), hash)?;
		debug!("entry at {:?}", entry)?;
		match entry {
			Some(entry) => {
				let id = self.entry_array[entry];
				let ret = self.get_mut(id)?;
				Ok(Some(ret))
			}
			None => Ok(None),
		}
	}

	fn get_raw_impl<'b, K>(
		&'b self,
		key_raw: &[u8],
		hash: u64,
	) -> Result<Option<Box<dyn Slab + 'b>>, Error>
	where
		K: Serializable + Hash,
	{
		let entry = self.find_entry::<K>(None, Some(key_raw), hash)?;
		debug!("entry at {:?}", entry)?;
		match entry {
			Some(entry) => {
				let id = self.entry_array[entry];
				let ret = self.get_slab(id)?;
				Ok(Some(ret))
			}
			None => Ok(None),
		}
	}

	fn get_impl<K, V>(
		&self,
		key_ser: Option<&K>,
		key_raw: Option<&[u8]>,
		hash: u64,
	) -> Result<Option<V>, Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		debug!("get_impl:self.config={:?},k={:?}", self.config, key_ser)?;
		let entry = self.find_entry(key_ser, key_raw, hash)?;
		debug!("entry at {:?}", entry)?;

		match entry {
			Some(entry) => Ok(Some(self.read_ser(entry)?)),
			None => Ok(None),
		}
	}

	fn read_ser<V>(&self, entry: usize) -> Result<V, Error>
	where
		V: Serializable,
	{
		let v = self.read_value(entry)?;
		let mut cursor = Cursor::new(v);
		cursor.set_position(0);
		let mut reader = BinReader::new(&mut cursor);
		let ret = V::read(&mut reader)?;
		Ok(ret)
	}

	fn read_value(&self, entry: usize) -> Result<Vec<u8>, Error> {
		let mut v: Vec<u8> = vec![];
		let slab_id = self.entry_array[entry];

		let mut slab = self.get_slab(slab_id)?;
		let bytes_per_slab = self.slab_size.saturating_sub(usize!(SLAB_OVERHEAD));
		let mut krem = usize!(u64::from_be_bytes(slab.get()[0..8].try_into()?));
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
				value_len = usize!(u64::from_be_bytes(slab_bytes[krem..krem + 8].try_into()?));

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
			slab = self.get_slab(next)?;
			slab_offset = 0;
		}

		debug!("break with vlen={}", v.len())?;
		Ok(v)
	}

	fn find_entry<K>(
		&self,
		key: Option<&K>,
		key_raw: Option<&[u8]>,
		hash: u64,
	) -> Result<Option<usize>, Error>
	where
		K: Serializable + Hash,
	{
		let mut entry = match key {
			Some(key) => self.entry_hash(&key),
			None => usize!(hash) % self.entry_array.len(),
		};

		let max_iter = self.entry_array.len();
		let mut i = 0;
		loop {
			if i >= max_iter {
				debug!("max iter exceeded")?;
				return Ok(None);
			}
			if self.entry_array[entry] == SLOT_EMPTY {
				debug!("found empty slot at {}", entry)?;
				// empty slot means this key is not in the table
				return Ok(None);
			}

			if self.entry_array[entry] != SLOT_DELETED {
				debug!("found possible slot at {}", entry)?;
				// there's a valid entry here. Check if it's ours
				if self.key_match(self.entry_array[entry], key, key_raw)? {
					return Ok(Some(entry));
				}
			}

			entry = (entry + 1) % max_iter;
			i += 1;
		}
	}

	fn key_match<K>(
		&self,
		id: u64,
		key_ser: Option<&K>,
		key_raw: Option<&[u8]>,
	) -> Result<bool, Error>
	where
		K: Serializable,
	{
		// serialize key
		let mut k = vec![];
		match key_ser {
			Some(key_ser) => serialize(&mut k, key_ser)?,
			None => match key_raw {
				Some(key_raw) => {
					k.extend(key_raw);
				}
				None => {
					return Err(err!(
						ErrKind::IllegalArgument,
						"a serializable key or a raw key must be specified"
					))
				}
			},
		}
		let klen = k.len();
		debug!("key_len={}", klen)?;

		// read first slab
		let mut slab = self.get_slab(id)?;
		let len = u64::from_be_bytes(slab.get()[0..8].try_into()?);
		debug!("len={}", len)?;

		if len != u64!(klen) {
			return Ok(false);
		}

		let bytes_per_slab: usize = usize!(self.slab_size.saturating_sub(usize!(SLAB_OVERHEAD)));

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

		let mut offset = end - 8;
		loop {
			let next =
				u64::from_be_bytes(slab.get()[bytes_per_slab..bytes_per_slab + 8].try_into()?);
			slab = self.get_slab(next)?;
			let mut rem = klen.saturating_sub(offset);
			if rem > bytes_per_slab {
				rem = bytes_per_slab;
			}

			if k[offset..offset + rem] != slab.get()[0..rem] {
				return Ok(false);
			}

			offset += bytes_per_slab;
			if offset >= klen {
				break;
			}
		}

		Ok(true)
	}

	fn insert_impl<K, V>(
		&mut self,
		key_ser: Option<&K>,
		hash: u64,
		key_raw: Option<&[u8]>,
		value_ser: Option<&V>,
		value_raw: Option<&[u8]>,
	) -> Result<(), Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		debug!("insert:self.config={:?},k={:?}", self.config, key_ser)?;

		let entry_array_len = self.entry_array.len();
		let mut entry = match key_ser {
			Some(key_ser) => self.entry_hash(key_ser),
			None => hash as usize % entry_array_len,
		};
		let mut i = 0;
		loop {
			if i >= entry_array_len {
				return Err(err!(
					ErrKind::CapacityExceeded,
					"StaticHashImpl: Capacity exceeded"
				));
			}
			if self.entry_array[entry] == SLOT_EMPTY || self.entry_array[entry] == SLOT_DELETED {
				// found an empty or deleted slot, use it
				break;
			}
			entry = (entry + 1) % entry_array_len;
			i += 1;
		}
		debug!("inserting at entry={}", entry)?;

		let bytes_per_slab = self.slab_size.saturating_sub(usize!(SLAB_OVERHEAD));
		let free_count = u64!(self.get_free_count()?);

		let k_len_bytes: &[u8];
		let k_clone;
		let v_clone;
		let x;
		let y;
		let mut krem;
		let mut vrem;
		let v_len_bytes: &[u8];
		let k_bytes: &[u8];
		let v_bytes: &[u8];

		match key_ser {
			Some(key) => {
				debug!("serializing with key")?;
				let mut k = vec![];
				serialize(&mut k, key)?;
				k_clone = k.clone().to_vec();
				k_bytes = &(k_clone);
				krem = k.len();
				x = krem.to_be_bytes();
				k_len_bytes = &x;
			}
			None => match key_raw {
				Some(key_raw) => {
					k_bytes = &key_raw;
					krem = key_raw.len();
					x = krem.to_be_bytes();
					k_len_bytes = &x;
				}
				None => {
					return Err(err!(
						ErrKind::IllegalArgument,
						"both key_raw and key_ser cannot be None"
					));
				}
			},
		}

		match value_ser {
			Some(value) => {
				let mut v = vec![];
				serialize(&mut v, value)?;
				v_clone = v.clone();
				v_bytes = &v_clone;
				vrem = v.len();
				y = vrem.to_be_bytes();
				v_len_bytes = &y;
			}
			None => match value_raw {
				Some(value_raw) => {
					v_bytes = &value_raw;
					vrem = value_raw.len();
					y = vrem.to_be_bytes();
					v_len_bytes = &y;
				}
				None => {
					vrem = 0;
					y = vrem.to_be_bytes();
					v_len_bytes = &y;
					v_bytes = &[];
				}
			},
		}

		let needed_len = v_bytes.len() + k_bytes.len() + 16;
		let slabs_needed = needed_len as u64 / u64!(bytes_per_slab);

		debug!("slabs needed = {}", slabs_needed)?;

		if free_count < slabs_needed {
			return Err(err!(ErrKind::CapacityExceeded, "no more slabs"));
		}

		let mut itt = 0;
		let mut last_id = 0;

		let mut v_len_written = false;
		loop {
			let id;
			{
				let mut slab = self.allocate()?;
				id = slab.id();
				if itt == 0 {
					let slab_mut = slab.get_mut();

					// write klen
					slab_mut[0..8].clone_from_slice(&k_len_bytes);

					// write k
					let mut klen = krem;
					if klen > bytes_per_slab - 8 {
						klen = bytes_per_slab - 8;
					}
					krem = krem.saturating_sub(klen);
					slab_mut[8..8 + klen].clone_from_slice(&k_bytes[0..klen]);

					// write v len if there's room
					if klen + 16 <= bytes_per_slab {
						slab_mut[8 + klen..16 + klen].clone_from_slice(&v_len_bytes);
						v_len_written = true;
					}

					// write v if there's room
					if klen + 16 < bytes_per_slab {
						let mut vlen = bytes_per_slab - (klen + 16);
						if vrem < vlen {
							vlen = vrem;
						}
						slab_mut[klen + 16..klen + 16 + vlen].clone_from_slice(&v_bytes[0..vlen]);
						vrem -= vlen;
					}
				} else {
					let slab_mut = slab.get_mut();
					// write any remaining k
					let mut klen = krem;
					if krem > 0 {
						if klen > bytes_per_slab {
							klen = bytes_per_slab;
						}
						let k_offset = k_bytes.len() - krem;
						slab_mut[0..klen].clone_from_slice(&k_bytes[k_offset..k_offset + klen]);
						krem -= klen;
					}

					let mut value_written_adjustment = 0;
					// write vlen if we should and can
					if krem == 0 && !v_len_written && klen + 8 <= bytes_per_slab {
						slab_mut[klen..8 + klen].clone_from_slice(&v_len_bytes);
						v_len_written = true;
						value_written_adjustment = 8;
					}

					// if we can, write as much of v as possible
					if krem == 0
						&& v_len_written && klen + value_written_adjustment < bytes_per_slab
					{
						let mut vlen = vrem;
						if vlen > bytes_per_slab - (klen + value_written_adjustment) {
							vlen = bytes_per_slab - (klen + value_written_adjustment);
						}
						let v_offset = v_bytes.len() - vrem;
						slab_mut[klen + value_written_adjustment
							..klen + value_written_adjustment + vlen]
							.clone_from_slice(&v_bytes[v_offset..v_offset + vlen]);
						vrem -= vlen;
					}
				}
			}

			if itt == 0 {
				self.entry_array[entry] = id;
				last_id = id;
			} else {
				let mut slab = self.get_mut(last_id)?;
				slab.get_mut()[bytes_per_slab..bytes_per_slab + 8]
					.clone_from_slice(&id.to_be_bytes());
				last_id = id;
			}
			itt += 1;
			if vrem == 0 && krem == 0 && v_len_written {
				break;
			}
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
	pub fn build<K, V>(
		config: StaticHashtableConfig,
		slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	) -> Result<Box<dyn StaticHashtable<K, V>>, Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		Ok(Box::new(StaticHashImpl::new(config.into(), slabs)?))
	}
}

pub struct StaticHashsetBuilder {}

impl StaticHashsetBuilder {
	pub fn build<K>(
		config: StaticHashsetConfig,
		slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	) -> Result<Box<dyn StaticHashset<K>>, Error>
	where
		K: Serializable + Hash,
	{
		Ok(Box::new(StaticHashImpl::new(config.into(), slabs)?))
	}
}

#[cfg(test)]
mod test {
	use crate::types::SlabAllocatorConfig;
	use crate::types::{Reader, Writer};
	use crate::{
		Serializable, SlabAllocatorBuilder, StaticHashsetBuilder, StaticHashsetConfig,
		StaticHashtableBuilder, StaticHashtableConfig,
	};
	use bmw_err::Error;
	use bmw_log::*;
	use std::collections::hash_map::DefaultHasher;
	use std::hash::{Hash, Hasher};

	debug!();

	pub fn initialize() -> Result<(), Error> {
		let _ = info!("init");
		crate::GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(), Error> {
			let sa = unsafe { f.get().as_mut().unwrap() };
			sa.init(SlabAllocatorConfig {
				slab_count: 30_000,
				..Default::default()
			})?;
			Ok(())
		})
	}

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
	fn test_small_static_hashtable() -> Result<(), Error> {
		let mut slabs = SlabAllocatorBuilder::build();
		slabs.init(SlabAllocatorConfig::default())?;
		let mut sh = StaticHashtableBuilder::build(StaticHashtableConfig::default(), Some(slabs))?;
		sh.insert(&1, &2)?;
		assert_eq!(sh.get(&1)?, Some(2));
		let mut sh2 = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
		for i in 0..20 {
			sh2.insert(&BigThing::new(i, i), &BigThing::new(i, i))?;
			info!("i={}", i)?;
			assert_eq!(sh2.get(&BigThing::new(i, i))?, Some(BigThing::new(i, i)));
		}

		let mut sh3 = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
		sh3.insert(&10, &20)?;
		assert_eq!(sh3.get(&10)?, Some(20));

		for (x, y) in &sh {
			info!("x={},y={}", x, y)?;
		}
		sh.insert(&2, &4)?;

		Ok(())
	}

	#[test]
	fn test_static_hashtable() -> Result<(), Error> {
		initialize()?;
		let mut slabs1 = SlabAllocatorBuilder::build();
		slabs1.init(SlabAllocatorConfig {
			slab_count: 30_000,
			..Default::default()
		})?;
		let mut slabs2 = SlabAllocatorBuilder::build();
		slabs2.init(SlabAllocatorConfig::default())?;
		let mut sh = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
		sh.insert(&1, &2)?;
		assert_eq!(sh.get(&1)?, Some(2));

		let mut sh2 = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
		for i in 0..4000 {
			sh2.insert(&BigThing::new(i, i), &BigThing::new(i, i))?;
			assert_eq!(sh2.get(&BigThing::new(i, i))?, Some(BigThing::new(i, i)));
		}

		let mut sh3 = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
		sh3.insert(&10, &20)?;
		assert_eq!(sh3.get(&10)?, Some(20));

		Ok(())
	}

	#[test]
	fn test_static_hashset() -> Result<(), Error> {
		initialize()?;
		let mut slabs1 = SlabAllocatorBuilder::build();
		slabs1.init(SlabAllocatorConfig::default())?;
		let mut sh = StaticHashsetBuilder::build(StaticHashsetConfig::default(), None)?;
		sh.insert(&1u32)?;
		sh.insert(&9)?;
		sh.insert(&18)?;

		for i in 0..20 {
			if i == 1 || i == 9 || i == 18 {
				assert!(sh.contains(&i)?);
			} else {
				assert!(!sh.contains(&i)?);
			}
		}
		Ok(())
	}

	#[test]
	fn test_hashtable_raw() -> Result<(), Error> {
		initialize()?;
		let mut slabs1 = SlabAllocatorBuilder::build();
		slabs1.init(SlabAllocatorConfig::default())?;
		let mut sh =
			StaticHashtableBuilder::build::<(), ()>(StaticHashtableConfig::default(), None)?;

		let mut hasher = DefaultHasher::new();
		(b"hi").hash(&mut hasher);
		let hash = hasher.finish();
		sh.insert_raw(b"hi", hash, b"ok")?;
		let slab = sh.get_raw(b"hi", hash)?.unwrap();
		// key = 104/105 (hi), value = 111/107 (ok)
		assert_eq!(
			slab.get()[0..20],
			[0, 0, 0, 0, 0, 0, 0, 2, 104, 105, 0, 0, 0, 0, 0, 0, 0, 2, 111, 107]
		);
		Ok(())
	}
}
