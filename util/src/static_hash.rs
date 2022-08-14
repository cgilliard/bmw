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
use crate::{
	RawHashsetIterator, RawHashtableIterator, Serializable, Slab, SlabAllocator,
	SlabAllocatorConfig, SlabMut, StaticHashset, StaticHashsetConfig, StaticHashtable,
	StaticHashtableConfig, GLOBAL_SLAB_ALLOCATOR,
};
use bmw_err::{err, try_into, ErrKind, Error};
use bmw_log::*;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::thread;

info!();

const SLAB_OVERHEAD: usize = 8;
const SLOT_EMPTY: usize = usize::MAX;
const SLOT_DELETED: usize = usize::MAX - 1;

#[derive(Debug, Clone, Copy)]
struct StaticHashConfig {
	max_entries: usize,
	max_load_factor: f64,
	debug_clear_error: bool,
	debug_do_next_error: bool,
	debug_get_slab_error: bool,
	debug_max_iter: bool,
}

impl From<StaticHashtableConfig> for StaticHashConfig {
	fn from(config: StaticHashtableConfig) -> Self {
		let _ = debug!("converting {:?}", config);
		Self {
			max_entries: config.max_entries,
			max_load_factor: config.max_load_factor,
			debug_clear_error: false,
			debug_do_next_error: false,
			debug_get_slab_error: config.debug_get_slab_error,
			debug_max_iter: false,
		}
	}
}

impl From<StaticHashsetConfig> for StaticHashConfig {
	fn from(config: StaticHashsetConfig) -> Self {
		let _ = debug!("converting {:?}", config);
		Self {
			max_entries: config.max_entries,
			max_load_factor: config.max_load_factor,
			debug_clear_error: false,
			debug_do_next_error: false,
			debug_get_slab_error: config.debug_get_slab_error,
			debug_max_iter: false,
		}
	}
}

impl From<StaticHashConfig> for StaticHashsetConfig {
	fn from(config: StaticHashConfig) -> Self {
		Self {
			max_entries: config.max_entries,
			max_load_factor: config.max_load_factor,
			debug_get_slab_error: config.debug_get_slab_error,
		}
	}
}

impl From<StaticHashConfig> for StaticHashtableConfig {
	fn from(config: StaticHashConfig) -> Self {
		Self {
			max_entries: config.max_entries,
			max_load_factor: config.max_load_factor,
			debug_get_slab_error: config.debug_get_slab_error,
		}
	}
}

impl Default for StaticHashtableConfig {
	fn default() -> Self {
		Self {
			max_entries: 1_000_000,
			max_load_factor: 0.75,
			debug_get_slab_error: false,
		}
	}
}

impl Default for StaticHashsetConfig {
	fn default() -> Self {
		Self {
			max_entries: 1_000_000,
			max_load_factor: 0.75,
			debug_get_slab_error: false,
		}
	}
}

struct StaticHashImpl {
	config: StaticHashConfig,
	slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	entry_array: Vec<usize>,
	slab_size: usize,
	first_entry: usize,
	size: usize,
}

impl Drop for StaticHashImpl {
	fn drop(self: &mut StaticHashImpl) {
		match Self::do_drop(self) {
			Ok(_) => {}
			Err(e) => {
				let _ = error!(
					"Drop of StaticHashImpl resulted in an unexpected error: {}",
					e
				);
			}
		}
	}
}

struct RawHashsetIteratorImpl<'a> {
	h: &'a StaticHashImpl,
	cur: usize,
}

impl<'a> RawHashsetIterator for RawHashsetIteratorImpl<'a> {}

impl<'a> Iterator for RawHashsetIteratorImpl<'a> {
	type Item = Vec<u8>;
	fn next(&mut self) -> Option<<Self as Iterator>::Item> {
		match Self::do_next(self) {
			Ok(x) => x,
			Err(e) => {
				let _ = error!("StaticHashsetRawIter generated unexpected error: {}", e);
				None
			}
		}
	}
}

impl<'a> RawHashsetIteratorImpl<'a> {
	fn do_next(self: &mut RawHashsetIteratorImpl<'a>) -> Result<Option<Vec<u8>>, Error> {
		if self.h.config.debug_do_next_error {
			return Err(err!(ErrKind::Test, "do_next err"));
		}
		Ok(if self.cur == usize::MAX {
			None
		} else {
			match self.h.get_slab(self.h.entry_array[self.cur]) {
				Ok(slab) => {
					let (k, _v) = self.h.read_value(slab.id())?;

					self.cur = usize::from_be_bytes(try_into!(slab.get()[0..8])?);
					Some(k)
				}
				Err(e) => {
					error!("get_slab generated error: {}", e)?;
					None
				}
			}
		})
	}
}

struct RawHashtableIteratorImpl<'a> {
	h: &'a StaticHashImpl,
	cur: usize,
}

impl<'a> RawHashtableIterator for RawHashtableIteratorImpl<'a> {}

impl<'a> Iterator for RawHashtableIteratorImpl<'a> {
	type Item = (Vec<u8>, Vec<u8>);
	fn next(&mut self) -> Option<<Self as Iterator>::Item> {
		match Self::do_next(self) {
			Ok(x) => x,
			Err(e) => {
				let _ = error!("StaticHashtableRawIter generated unexpected error: {}", e);
				None
			}
		}
	}
}

impl<'a> RawHashtableIteratorImpl<'a> {
	fn do_next(
		self: &mut RawHashtableIteratorImpl<'a>,
	) -> Result<Option<(Vec<u8>, Vec<u8>)>, Error> {
		if self.h.config.debug_do_next_error {
			return Err(err!(ErrKind::Test, "do_next err"));
		}

		Ok(if self.cur == usize::MAX {
			None
		} else {
			match self.h.get_slab(self.h.entry_array[self.cur]) {
				Ok(slab) => {
					let (k, v) = self.h.read_value(slab.id())?;

					self.cur = usize::from_be_bytes(try_into!(slab.get()[0..8])?);
					Some((k, v))
				}
				Err(e) => {
					error!("get slab generated error: {}", e)?;
					None
				}
			}
		})
	}
}

pub struct StaticHashsetIter<'a, K> {
	cur: usize,
	h: &'a Box<dyn StaticHashset<K>>,
	debug_do_next_error: bool,
}

impl<'a, K> Iterator for StaticHashsetIter<'a, K>
where
	K: Serializable + Hash,
{
	type Item = K;
	fn next(self: &mut StaticHashsetIter<'a, K>) -> Option<K> {
		match Self::do_next(self) {
			Ok(x) => x,
			Err(e) => {
				let _ = error!("StaticHashsetIter generated unexpected error: {}", e);
				None
			}
		}
	}
}

impl<'a, K> StaticHashsetIter<'a, K>
where
	K: Serializable,
{
	fn do_next(self: &mut StaticHashsetIter<'a, K>) -> Result<Option<K>, Error> {
		if self.debug_do_next_error {
			return Err(err!(ErrKind::Test, "do_next err"));
		}
		Ok(if self.cur == usize::MAX {
			debug!("NONE")?;
			None
		} else {
			debug!("cur={}", self.cur)?;
			let entry_array = self.h.get_array();
			match self.h.slab(entry_array[self.cur]) {
				Ok(slab) => {
					let k = self.h.read_k(slab.id())?;
					let slab = slab.get();
					debug!("slab={:?}", slab)?;

					self.cur = usize::from_be_bytes(try_into!(slab[0..8])?);
					let prev = usize::from_be_bytes(try_into!(slab[8..16])?);
					debug!("self.cur is now {}, prev={}", self.cur, prev)?;
					Some(k)
				}
				Err(e) => {
					error!("slabhash iter error: {}", e)?;
					None
				}
			}
		})
	}
}

pub struct StaticHashtableIter<'a, K, V> {
	cur: usize,
	h: &'a Box<dyn StaticHashtable<K, V>>,
	debug_do_next_error: bool,
}

impl<'a, K, V> Iterator for StaticHashtableIter<'a, K, V>
where
	K: Serializable + Hash,
	V: Serializable,
{
	type Item = (K, V);
	fn next(self: &mut StaticHashtableIter<'a, K, V>) -> Option<(K, V)> {
		match Self::do_next(self) {
			Ok(x) => x,
			Err(e) => {
				let _ = error!("StaticHashtableIter generated unexpected error: {}", e);
				None
			}
		}
	}
}

impl<'a, K, V> StaticHashtableIter<'a, K, V>
where
	K: Serializable,
	V: Serializable,
{
	fn do_next(self: &mut StaticHashtableIter<'a, K, V>) -> Result<Option<(K, V)>, Error> {
		if self.debug_do_next_error {
			return Err(err!(ErrKind::Test, "do_next err"));
		}
		Ok(if self.cur == usize::MAX {
			debug!("NONE")?;
			None
		} else {
			debug!("cur={}", self.cur)?;
			let entry_array = self.h.get_array();
			match self.h.slab(entry_array[self.cur]) {
				Ok(slab) => {
					let (k, v) = self.h.read_kv(slab.id())?;
					let slab = slab.get();
					debug!("slab={:?}", slab)?;

					self.cur = usize::from_be_bytes(try_into!(slab[0..8])?);
					let prev = usize::from_be_bytes(try_into!(slab[8..16])?);
					debug!("self.cur is now {}, prev={}", self.cur, prev)?;
					Some((k, v))
				}
				Err(e) => {
					error!("get_slab generated error: {}", e)?;
					None
				}
			}
		})
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
		Self::IntoIter {
			cur: self.first_entry(),
			h: &self,
			debug_do_next_error: false,
		}
	}
}

impl<'a, K> IntoIterator for &'a Box<dyn StaticHashset<K>>
where
	K: Serializable + Hash,
{
	type Item = K;
	type IntoIter = StaticHashsetIter<'a, K>;

	fn into_iter(self) -> Self::IntoIter {
		Self::IntoIter {
			cur: self.first_entry(),
			h: &self,
			debug_do_next_error: false,
		}
	}
}

impl<K, V> StaticHashtable<K, V> for StaticHashImpl
where
	K: Serializable + Hash,
	V: Serializable,
{
	fn config(&self) -> StaticHashtableConfig {
		self.config.into()
	}
	fn insert(&mut self, key: &K, value: &V) -> Result<(), Error> {
		self.insert_impl::<K, V>(Some(key), 0, None, Some(value), None)
	}
	fn get(&self, key: &K) -> Result<Option<V>, Error> {
		self.get_impl(Some(key), None, 0)
	}
	fn remove(&mut self, key: &K) -> Result<bool, Error> {
		self.remove_impl(Some(key), None, 0)
	}
	fn get_raw<'b>(&'b self, key: &[u8], hash: usize) -> Result<Option<Box<dyn Slab + 'b>>, Error> {
		self.get_raw_impl::<K>(key, hash)
	}
	fn get_raw_mut<'b>(
		&'b mut self,
		key: &[u8],
		hash: usize,
	) -> Result<Option<Box<dyn SlabMut + 'b>>, Error> {
		self.get_raw_mut_impl::<K>(key, hash)
	}
	fn insert_raw(&mut self, key: &[u8], hash: usize, value: &[u8]) -> Result<(), Error> {
		self.insert_impl::<K, V>(None, hash, Some(key), None, Some(value))
	}
	fn remove_raw(&mut self, key: &[u8], hash: usize) -> Result<bool, Error> {
		self.remove_impl::<K>(None, Some(key), hash)
	}
	fn iter_raw<'b>(&'b self) -> Box<dyn RawHashtableIterator<Item = (Vec<u8>, Vec<u8>)> + 'b> {
		let cur = self.first_entry;
		let h = self;
		let r = RawHashtableIteratorImpl { cur, h };
		Box::new(r)
	}
	fn size(&self) -> usize {
		self.size
	}
	fn clear(&mut self) -> Result<(), Error> {
		self.clear_impl()
	}
	fn first_entry(&self) -> usize {
		self.first_entry
	}

	fn slab<'b>(&'b self, id: usize) -> Result<Box<dyn Slab + 'b>, Error> {
		self.get_slab(id)
	}

	fn read_kv(&self, slab_id: usize) -> Result<(K, V), Error> {
		self.read_kv_ser(slab_id)
	}
	fn get_array(&self) -> &Vec<usize> {
		&self.entry_array
	}
}

impl<K> StaticHashset<K> for StaticHashImpl
where
	K: Serializable + Hash,
{
	fn config(&self) -> StaticHashsetConfig {
		self.config.into()
	}
	fn insert(&mut self, key: &K) -> Result<(), Error> {
		self.insert_impl::<K, K>(Some(key), 0, None, None, None)
	}
	fn contains(&self, key: &K) -> Result<bool, Error> {
		debug!("contains:self.config={:?}", self.config)?;
		Ok(self.find_entry(Some(key), None, 0)?.is_some())
	}
	fn contains_raw(&self, key: &[u8], hash: usize) -> Result<bool, Error> {
		debug!("contains_raw:self.config={:?}", self.config)?;
		Ok(self.find_entry::<K>(None, Some(key), hash)?.is_some())
	}

	fn remove(&mut self, key: &K) -> Result<bool, Error> {
		self.remove_impl(Some(key), None, 0)
	}
	fn insert_raw(&mut self, key: &[u8], hash: usize) -> Result<(), Error> {
		self.insert_impl::<K, K>(None, hash, Some(key), None, None)
	}
	fn remove_raw(&mut self, key: &[u8], hash: usize) -> Result<bool, Error> {
		self.remove_impl::<K>(None, Some(key), hash)
	}

	fn iter_raw<'b>(&'b self) -> Box<dyn RawHashsetIterator<Item = Vec<u8>> + 'b> {
		let cur = self.first_entry;
		let h = self;
		let r = RawHashsetIteratorImpl { cur, h };
		Box::new(r)
	}

	fn size(&self) -> usize {
		self.size
	}
	fn clear(&mut self) -> Result<(), Error> {
		self.clear_impl()
	}
	fn first_entry(&self) -> usize {
		self.first_entry
	}
	fn slab<'b>(&'b self, id: usize) -> Result<Box<dyn Slab + 'b>, Error> {
		self.get_slab(id)
	}
	fn read_k(&self, slab_id: usize) -> Result<K, Error> {
		let k = self.read_k_ser::<K>(slab_id)?;
		Ok(k)
	}
	fn get_array(&self) -> &Vec<usize> {
		&self.entry_array
	}
}

impl StaticHashImpl {
	fn new(
		config: StaticHashConfig,
		slabs: Option<Box<dyn SlabAllocator + Send + Sync>>,
	) -> Result<Self, Error> {
		let mut entry_array = vec![];
		Self::init(&config, &mut entry_array)?;
		let (slab_size, slabs) = match slabs {
			Some(slabs) => (slabs.slab_size()?, Some(slabs)),
			None => {
				let slab_size = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
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
			size: 0,
		})
	}

	fn init(config: &StaticHashConfig, entry_array: &mut Vec<usize>) -> Result<(), Error> {
		if config.max_load_factor <= 0.0 || config.max_load_factor > 1.0 {
			return Err(err!(
				ErrKind::IllegalArgument,
				"StaticHash: max_load_factor must be greater than 0 and equal to or less than 1."
			));
		}
		if config.max_entries == 0 {
			return Err(err!(
				ErrKind::IllegalArgument,
				"StaticHash: max_entries must be greater than 0"
			));
		}

		// calculate size of entry_array. Must be possible to have max_entries, with the
		// max_load_factor
		let size: usize = (config.max_entries as f64 / config.max_load_factor).floor() as usize;
		debug!("entry array init to size = {}", size)?;
		entry_array.resize(size, SLOT_EMPTY);
		Ok(())
	}

	fn do_drop(self: &mut StaticHashImpl) -> Result<(), Error> {
		debug!("Dropping StaticHashImpl with config: {:?}", self.config)?;
		Self::clear_impl(self)?;
		Ok(())
	}
	fn get_slab<'a>(&'a self, id: usize) -> Result<Box<dyn Slab + 'a>, Error> {
		if self.config.debug_get_slab_error {
			return Err(err!(ErrKind::Test, "simulate get_slab error"));
		}
		match &self.slabs {
			Some(slabs) => Ok(slabs.get(id)?),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<Box<dyn Slab>, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().get(id)? })
			}),
		}
	}

	fn get_mut<'a>(&'a mut self, id: usize) -> Result<Box<dyn SlabMut + 'a>, Error> {
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

	fn free(&mut self, id: usize) -> Result<(), Error> {
		match &mut self.slabs {
			Some(slabs) => Ok(slabs.free(id)?),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(), Error> {
				unsafe { f.get().as_mut().unwrap().free(id)? };
				Ok(())
			}),
		}
	}

	fn get_free_count(&self) -> Result<usize, Error> {
		match &self.slabs {
			Some(slabs) => Ok(slabs.free_count()?),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
			}),
		}
	}

	fn clear_impl(&mut self) -> Result<(), Error> {
		if self.config.debug_clear_error {
			return Err(err!(ErrKind::Test, "simulate drop error"));
		}

		self.size = 0;
		let mut entry = self.first_entry;
		loop {
			if entry == SLOT_EMPTY {
				break;
			}
			let cur = self.entry_array[entry];
			self.entry_array[entry] = SLOT_DELETED;

			{
				debug!("cur={}", cur)?;
				match self.get_slab(cur) {
					Ok(slab) => {
						let slab = slab.get();
						entry = usize::from_be_bytes(try_into!(slab[0..8])?);
						debug!("entry is now {}", entry)?;
					}
					Err(e) => {
						error!(
							"Error iterating through slab hash. It is in an invalid state: {}",
							e
						)?;
					}
				}
			}

			self.free_tail(cur)?;
		}
		Ok(())
	}

	fn get_raw_mut_impl<'b, K>(
		&'b mut self,
		key_raw: &[u8],
		hash: usize,
	) -> Result<Option<Box<dyn SlabMut + 'b>>, Error>
	where
		K: Serializable + Hash,
	{
		let slab_id = {
			let entry = self.find_entry::<K>(None, Some(key_raw), hash)?;
			if entry.is_none() {
				return Ok(None);
			}
			let entry = entry.unwrap();
			let slab = entry.1;
			slab.id()
		};
		Ok(Some(self.get_mut(slab_id)?))
	}

	fn get_raw_impl<'b, K>(
		&'b self,
		key_raw: &[u8],
		hash: usize,
	) -> Result<Option<Box<dyn Slab + 'b>>, Error>
	where
		K: Serializable + Hash,
	{
		match self.find_entry::<K>(None, Some(key_raw), hash)? {
			Some((_entry, slab)) => Ok(Some(slab)),
			None => Ok(None),
		}
	}

	fn get_impl<K, V>(
		&self,
		key_ser: Option<&K>,
		key_raw: Option<&[u8]>,
		hash: usize,
	) -> Result<Option<V>, Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		debug!("get_impl:self.config={:?}", self.config)?;
		let slab = self.find_entry(key_ser, key_raw, hash)?;

		match slab {
			Some((_entry, slab)) => Ok(Some(self.read_kv_ser::<K, V>(slab.id())?.1)),
			None => Ok(None),
		}
	}

	fn read_k_ser<K>(&self, slab_id: usize) -> Result<K, Error>
	where
		K: Serializable,
	{
		let (k, _v) = self.read_value(slab_id)?;
		let mut cursor = Cursor::new(k);
		cursor.set_position(0);
		let mut reader1 = BinReader::new(&mut cursor);
		let k = K::read(&mut reader1)?;
		Ok(k)
	}

	fn read_kv_ser<K, V>(&self, slab_id: usize) -> Result<(K, V), Error>
	where
		K: Serializable,
		V: Serializable,
	{
		let (k, v) = self.read_value(slab_id)?;
		let mut cursor = Cursor::new(k);
		cursor.set_position(0);
		let mut reader1 = BinReader::new(&mut cursor);
		let mut cursor = Cursor::new(v);
		cursor.set_position(0);
		let mut reader2 = BinReader::new(&mut cursor);
		let k = K::read(&mut reader1)?;
		let v = V::read(&mut reader2)?;
		Ok((k, v))
	}

	fn read_value(&self, slab_id: usize) -> Result<(Vec<u8>, Vec<u8>), Error> {
		let mut k: Vec<u8> = vec![];
		let mut v: Vec<u8> = vec![];

		let mut slab = self.get_slab(slab_id)?;
		let bytes_per_slab = self.slab_size.saturating_sub(SLAB_OVERHEAD);
		let mut krem = usize::from_be_bytes(try_into!(slab.get()[16..24])?);
		debug!("krem read = {},slab={:?}", krem, slab.get())?;
		let klen = krem;
		let mut slab_offset = 24;
		let mut value_len: usize = 0;
		let mut voffset = usize::MAX;
		let mut vrem = usize::MAX;
		loop {
			let slab_bytes = slab.get();
			let slab_id = slab.id();
			debug!(
				"slab_id={}, krem={},voffset={},slab_offset={},bytes_per={}",
				slab_id, krem, voffset, slab_offset, bytes_per_slab
			)?;
			if krem <= bytes_per_slab.saturating_sub(slab_offset + 8)
				&& voffset == usize::MAX
				&& (krem + slab_offset) <= bytes_per_slab
			{
				debug!("read {}-{}", krem + slab_offset, krem + slab_offset + 8)?;
				debug!("slab={:?}", slab_bytes,)?;
				value_len = usize::from_be_bytes(try_into!(
					slab_bytes[krem + slab_offset..krem + slab_offset + 8]
				)?);

				voffset = krem + 8 + slab_offset;
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
				debug!("extend = {}-{}", voffset, end)?;
				v.extend(&slab_bytes[voffset..end]);
				debug!("vlen={}, vrem={}, end={}", v.len(), vrem, end)?;
				voffset = 0;
				vrem = vrem.saturating_sub(end - voffset);
			}

			if krem > 0 {
				let mut klen = krem;
				if krem > bytes_per_slab.saturating_sub(slab_offset) {
					klen = bytes_per_slab.saturating_sub(slab_offset);
				}
				k.extend(&slab_bytes[slab_offset..slab_offset + klen]);
			}

			if v.len() >= value_len && voffset != usize::MAX {
				break;
			}

			let diff = bytes_per_slab.saturating_sub(slab_offset);
			debug!("kremin={},bytes_per_slab + slab_offset={}", krem, diff)?;

			krem = krem.saturating_sub(diff);
			debug!("kremout={}", krem)?;
			let next =
				usize::from_be_bytes(try_into!(slab_bytes[bytes_per_slab..bytes_per_slab + 8])?);
			slab = self.get_slab(next)?;
			slab_offset = 0;
		}

		debug!("break with klen={},vlen={}", k.len(), v.len())?;
		Ok((k, v))
	}

	fn find_entry<'a, K>(
		&'a self,
		key: Option<&K>,
		key_raw: Option<&[u8]>,
		hash: usize,
	) -> Result<Option<(usize, Box<dyn Slab + 'a>)>, Error>
	where
		K: Serializable + Hash,
	{
		let mut entry = match key {
			Some(key) => self.entry_hash(&key),
			None => hash % self.entry_array.len(),
		};
		debug!("entry hashed to {}", entry)?;
		let max_iter = self.entry_array.len();
		let mut i = 0;
		loop {
			if i >= max_iter || self.config.debug_max_iter {
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
				let slab = self.key_match(self.entry_array[entry], key, key_raw)?;
				if slab.is_some() {
					let slab = slab.unwrap();
					return Ok(Some((entry, slab)));
				}
				debug!("no match")?;
			}

			entry = (entry + 1) % max_iter;
			i += 1;
		}
	}

	// This function has full coverage, but tarpaulin is flagging a few lines
	// Setting to ignore for now.
	#[cfg(not(tarpaulin_include))]
	fn key_match<'a, K>(
		&'a self,
		id: usize,
		key_ser: Option<&K>,
		key_raw: Option<&[u8]>,
	) -> Result<Option<Box<dyn Slab + 'a>>, Error>
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
					let fmt = "a serializable key or a raw key must be specified";
					let e = err!(ErrKind::IllegalArgument, fmt);
					return Err(e);
				}
			},
		}
		let klen = k.len();
		debug!("key_len={}", klen)?;

		// read first slab
		let mut slab = self.get_slab(id)?;
		let len = usize::from_be_bytes(try_into!(slab.get()[16..24])?);
		debug!("len={}", len)?;
		if len != klen {
			return Ok(None);
		}

		let bytes_per_slab: usize = self.slab_size.saturating_sub(SLAB_OVERHEAD);

		// compare the first slab
		let mut end = 24 + klen;
		if end > bytes_per_slab {
			end = bytes_per_slab;
		}
		if slab.get()[24..end] != k[0..end - 24] {
			return Ok(None);
		}
		if end < bytes_per_slab {
			return Ok(Some(slab));
		}

		let mut offset = end - 24;
		loop {
			let cvt = try_into!(slab.get()[bytes_per_slab..bytes_per_slab + 8])?;
			let next = usize::from_be_bytes(cvt);
			slab = self.get_slab(next)?;
			let mut rem = klen.saturating_sub(offset);
			if rem > bytes_per_slab {
				rem = bytes_per_slab;
			}

			if k[offset..offset + rem] != slab.get()[0..rem] {
				return Ok(None);
			}

			offset += bytes_per_slab;
			if offset >= klen {
				break;
			}
		}

		Ok(Some(self.get_slab(id)?))
	}

	fn free_tail(&mut self, mut slab_id: usize) -> Result<(), Error> {
		let bytes_per_slab = self.slab_size.saturating_sub(SLAB_OVERHEAD);
		debug!("free tail id = {}", slab_id)?;
		let mut prev = usize::MAX;
		let mut next = usize::MAX;

		{
			let mut first = true;
			let mut to_delete;
			loop {
				{
					let slab = self.get_slab(slab_id)?;
					to_delete = slab_id;
					let slab_bytes = slab.get();
					if first {
						next = usize::from_be_bytes(try_into!(slab_bytes[0..8])?);
						prev = usize::from_be_bytes(try_into!(slab_bytes[8..16])?);
						debug!("prev={},next={}", prev, next)?;
					}
					slab_id = usize::from_be_bytes(try_into!(
						slab_bytes[bytes_per_slab..bytes_per_slab + 8]
					)?);
					debug!("next={}", slab_id)?;

					first = false;
				}
				if to_delete != usize::MAX {
					self.free(to_delete)?;
				}

				if slab_id == usize::MAX {
					break;
				}
			}
		}

		if prev != usize::MAX
			&& self.entry_array[prev] != SLOT_EMPTY
			&& self.entry_array[prev] != SLOT_DELETED
		{
			let mut slab = self.get_mut(self.entry_array[prev])?;
			slab.get_mut()[0..8].clone_from_slice(&next.to_be_bytes());
		} else {
			self.first_entry = next;
		}

		if next != usize::MAX
			&& self.entry_array[next] != SLOT_EMPTY
			&& self.entry_array[next] != SLOT_DELETED
		{
			let mut slab = self.get_mut(self.entry_array[next])?;
			slab.get_mut()[8..16].clone_from_slice(&prev.to_be_bytes());
		}

		Ok(())
	}

	// This function has full coverage, but tarpaulin is flagging a few lines.
	// Setting to ignore for now.
	#[cfg(not(tarpaulin_include))]
	fn insert_impl<K, V>(
		&mut self,
		key_ser: Option<&K>,
		hash: usize,
		key_raw: Option<&[u8]>,
		value_ser: Option<&V>,
		value_raw: Option<&[u8]>,
	) -> Result<(), Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		debug!("insert:self.config={:?}", self.config)?;

		let bytes_per_slab = self.slab_size.saturating_sub(SLAB_OVERHEAD);
		let free_count = self.get_free_count()?;

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
					let fmt = "both key_raw and key_ser cannot be None";
					let e = err!(ErrKind::IllegalArgument, fmt);
					return Err(e);
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

		let needed_len = v_bytes.len() + k_bytes.len() + 32;
		let slabs_needed = needed_len as usize / bytes_per_slab;

		if free_count < slabs_needed + 1 {
			return Err(err!(ErrKind::CapacityExceeded, "no more slabs"));
		}

		let entry_array_len = self.entry_array.len();
		let mut entry = match key_ser {
			Some(key_ser) => self.entry_hash(key_ser),
			None => hash as usize % entry_array_len,
		};
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
			if self
				.key_match(self.entry_array[entry], key_ser, key_raw)?
				.is_some()
			{
				// subtract because we add it back later
				self.size = self.size.saturating_sub(1);
				self.free_tail(self.entry_array[entry])?;
				break;
			}

			entry = (entry + 1) % entry_array_len;
			i += 1;
		}

		if (self.size + 1) as f64 / self.entry_array.len() as f64 > self.config.max_load_factor {
			let fmt = format!("load factor ({}) exceeded", self.config.max_load_factor);
			return Err(err!(ErrKind::CapacityExceeded, fmt));
		}
		debug!("inserting at entry={}", entry)?;

		let mut itt = 0;
		let mut last_id = 0;
		let mut first_id = 0;

		let mut v_len_written = false;
		let first_entry = self.first_entry;

		loop {
			let id;
			{
				let mut slab = self.allocate()?;
				id = slab.id();
				let slab_mut = slab.get_mut();
				// mark the last bytes as pointing to usize::MAX. This will be
				// overwritten later if there's additional slabs
				slab_mut[bytes_per_slab..bytes_per_slab + 8]
					.clone_from_slice(&usize::MAX.to_be_bytes());
				if itt == 0 {
					first_id = id;

					// update iter list
					let first_entry_bytes = first_entry.to_be_bytes();
					debug!("febytes={:?}", first_entry_bytes)?;
					slab_mut[0..8].clone_from_slice(&first_entry_bytes);
					slab_mut[8..16].clone_from_slice(&usize::MAX.to_be_bytes());

					// write klen
					slab_mut[16..24].clone_from_slice(&k_len_bytes);

					// write k
					let mut klen = krem;
					if klen > bytes_per_slab - 24 {
						klen = bytes_per_slab - 24;
					}
					krem = krem.saturating_sub(klen);
					slab_mut[24..24 + klen].clone_from_slice(&k_bytes[0..klen]);

					// write v len if there's room
					if klen + 32 <= bytes_per_slab {
						slab_mut[24 + klen..32 + klen].clone_from_slice(&v_len_bytes);
						v_len_written = true;
					}

					// write v if there's room
					if klen + 32 < bytes_per_slab {
						let mut vlen = bytes_per_slab - (klen + 32);
						if vrem < vlen {
							vlen = vrem;
						}
						slab_mut[klen + 32..klen + 32 + vlen].clone_from_slice(&v_bytes[0..vlen]);
						vrem -= vlen;
					}
				} else {
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
		debug!("fe={},setto={}", self.first_entry, first_id)?;

		// update reverse list
		if self.first_entry != usize::MAX {
			let mut slab = self.get_mut(self.entry_array[first_entry])?;
			slab.get_mut()[8..16].clone_from_slice(&entry.to_be_bytes());
		}
		self.first_entry = entry;
		self.size += 1;
		Ok(())
	}

	fn remove_impl<K>(
		&mut self,
		key_ser: Option<&K>,
		key_raw: Option<&[u8]>,
		hash: usize,
	) -> Result<bool, Error>
	where
		K: Serializable + Hash,
	{
		let (entry, slab_id) = match self.find_entry(key_ser, key_raw, hash)? {
			Some((entry, slab)) => (entry, slab.id()),
			None => return Ok(false),
		};

		self.free_tail(slab_id)?;
		self.entry_array[entry] = SLOT_DELETED;
		self.size = self.size.saturating_sub(1);
		Ok(true)
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
	use crate::static_hash::StaticHashConfig;
	use crate::static_hash::StaticHashImpl;
	use crate::static_hash::{RawHashsetIteratorImpl, RawHashtableIteratorImpl};
	use crate::types::SlabAllocatorConfig;
	use crate::types::{Reader, Writer};
	use crate::GLOBAL_SLAB_ALLOCATOR;
	use crate::{
		Serializable, SlabAllocatorBuilder, StaticHashsetBuilder, StaticHashsetConfig,
		StaticHashtableBuilder, StaticHashtableConfig,
	};
	use bmw_deps::rand;
	use bmw_err::Error;
	use bmw_log::*;
	use std::collections::hash_map::DefaultHasher;
	use std::collections::HashMap;
	use std::hash::{Hash, Hasher};
	use std::str::from_utf8;

	debug!();

	pub fn initialize() -> Result<(), Error> {
		let _ = debug!("init");
		let _ = crate::GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(), Error> {
			let sa = unsafe { f.get().as_mut().unwrap() };
			sa.init(SlabAllocatorConfig {
				slab_count: 30_000,
				..Default::default()
			})?;
			Ok(())
		});
		Ok(())
	}

	#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
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
		assert_eq!(sh3.size(), 1);

		let mut count = 0;
		let mut ks = vec![];
		let mut vs = vec![];
		for (x, y) in &sh2 {
			info!("x={:?},y={:?}", x, y)?;
			ks.push(x);
			vs.push(y);
			count += 1;
		}
		assert_eq!(count, 20);
		ks.sort();
		vs.sort();
		for i in 0..20 {
			assert_eq!(ks[i].val1, i as u32);
			assert_eq!(vs[i].val1, i as u32);
			assert_eq!(ks[i].val2, i as u32);
			assert_eq!(vs[i].val2, i as u32);
		}
		sh2.insert(&BigThing::new(8, 3), &BigThing::new(1, 3))?;
		Ok(())
	}

	#[test]
	fn test_hashtable_replace() -> Result<(), Error> {
		let mut sh = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
		sh.insert(&1, &2)?;
		assert_eq!(sh.get(&1)?, Some(2));
		sh.insert(&1, &3)?;
		assert_eq!(sh.get(&1)?, Some(3));
		let mut count = 0;
		for (k, v) in &sh {
			info!("k={:?},v={:?}", k, v)?;
			assert_eq!(k, 1);
			assert_eq!(v, 3);
			count += 1;
		}
		assert_eq!(count, 1);
		Ok(())
	}

	#[test]
	fn test_static_hashtable() -> Result<(), Error> {
		{
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
				info!("i={}", i)?;
				sh2.insert(&BigThing::new(i, i), &BigThing::new(i, i))?;
				assert_eq!(sh2.get(&BigThing::new(i, i))?, Some(BigThing::new(i, i)));
				//info!("bigthing={:?}", BigThing::new(i, i));
			}

			let mut sh3 = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
			sh3.insert(&10, &20)?;
			assert_eq!(sh3.get(&10)?, Some(20));
		}

		let free_count = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		assert_eq!(free_count, 30_000);

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
		sh.insert_raw(b"hi", usize!(hash), b"ok")?;
		{
			let mut slab = sh.get_raw_mut(b"hi", usize!(hash))?.unwrap();
			slab.get_mut()[35] = 106;
		}
		{
			assert!(sh.get_raw_mut(b"hi2", 0).unwrap().is_none());
		}
		{
			let slab = sh.get_raw(b"hi", usize!(hash))?.unwrap();
			// key = 104/105 (hi), value = 111/106 (oj) (updated the k -> j with slab mut
			assert_eq!(
				slab.get()[0..36],
				[
					255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
					0, 0, 0, 0, 0, 0, 0, 2, 104, 105, 0, 0, 0, 0, 0, 0, 0, 2, 111, 106
				]
			);
		}

		let mut count = 0;
		for _ in &sh {
			count += 1;
		}

		assert_eq!(count, 1);

		sh.remove_raw(b"hi", usize!(hash))?;
		assert!(sh.get_raw(b"hi", usize!(hash))?.is_none());

		assert_eq!(sh.size(), 0);

		Ok(())
	}

	#[test]
	fn test_hashtable_remove() -> Result<(), Error> {
		initialize()?;
		let mut sh = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
		assert_eq!(sh.get(&1)?, None);
		sh.insert(&1, &100)?;
		assert_eq!(sh.get(&1)?, Some(100));
		sh.remove(&1)?;
		assert_eq!(sh.get(&1)?, None);

		Ok(())
	}

	#[test]
	fn test_insert_raw_hashset() -> Result<(), Error> {
		initialize()?;
		let mut sh = StaticHashsetBuilder::build::<()>(StaticHashsetConfig::default(), None)?;
		sh.insert_raw(&[1], 1)?;
		sh.insert_raw(&[2], 2)?;

		assert!(sh.contains_raw(&[1], 1)?);
		assert!(sh.contains_raw(&[2], 2)?);
		assert!(!sh.contains_raw(&[3], 3)?);

		sh.remove_raw(&[2], 2)?;

		assert!(sh.contains_raw(&[1], 1)?);
		assert!(!sh.contains_raw(&[2], 2)?);
		assert!(!sh.contains_raw(&[3], 3)?);

		let mut count = 0;

		for _ in &sh {
			count += 1;
		}

		assert_eq!(count, 1);

		Ok(())
	}

	#[test]
	fn test_hashset_iter() -> Result<(), Error> {
		initialize()?;

		let free_count1 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;

		info!("free_count={}", free_count1)?;
		{
			let mut sh = StaticHashsetBuilder::build(StaticHashsetConfig::default(), None)?;

			sh.insert(&4)?;
			sh.insert(&1)?;
			sh.insert(&2)?;
			sh.insert(&3)?;
			sh.insert(&3)?;
			sh.insert(&3)?;
			sh.insert(&3)?;
			sh.insert(&4)?;
			sh.insert(&5)?;

			let mut count = 0;
			for x in &sh {
				info!("x={}", x)?;
				count += 1;
			}

			assert_eq!(count, 5);
			assert_eq!(sh.size(), 5);
			assert!(sh.contains(&1)?);
			assert!(sh.contains(&2)?);
			assert!(sh.contains(&3)?);
			assert!(sh.contains(&4)?);
			assert!(sh.contains(&5)?);

			let free_count2 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
			})?;
			info!("free_count={}", free_count2)?;
			assert_eq!(free_count2, free_count1 - 5);

			sh.remove(&3)?;
			assert!(sh.contains(&1)?);
			assert!(sh.contains(&2)?);
			assert!(!sh.contains(&3)?);
			assert!(sh.contains(&4)?);
			assert!(sh.contains(&5)?);
			let free_count3 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
			})?;
			assert_eq!(free_count3, free_count1 - 4);
			assert_eq!(sh.size(), 4);
		}

		let free_count4 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count4)?;
		assert_eq!(free_count4, free_count1);

		let mut sh = StaticHashsetBuilder::build(StaticHashsetConfig::default(), None)?;
		sh.insert(&1)?;
		sh.insert(&2)?;
		sh.insert(&3)?;
		sh.insert(&4)?;
		sh.insert(&5)?;
		assert_eq!(sh.size(), 5);
		sh.clear()?;
		assert_eq!(sh.size(), 0);

		Ok(())
	}

	fn rand_string(min_len: usize, max_len: usize) -> String {
		let r: usize = rand::random();
		let r = min_len + (r % (max_len - min_len));
		let chars = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/+";
		let mut ret = vec![];
		for _ in 0..r {
			let v: usize = rand::random();
			ret.push(chars[(v % chars.len())] as u8);
		}
		from_utf8(&ret[..]).unwrap().to_string()
	}

	#[test]
	fn test_rand() -> Result<(), Error> {
		let _ = crate::GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(), Error> {
			let sa = unsafe { f.get().as_mut().unwrap() };
			sa.init(SlabAllocatorConfig {
				slab_count: 300_000,
				slab_size: 128,
				..Default::default()
			})?;
			Ok(())
		});

		assert_eq!(
			GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
			})?,
			300_000
		);

		{
			let mut sh = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
			let str = rand_string(10, 100);
			let mut check_table = HashMap::new();
			info!("str={}", str)?;

			let mut keys = vec![];
			let mut values = vec![];
			for _ in 0..1000 {
				keys.push(rand_string(100, 2000));
				values.push(rand_string(200, 3000));
			}

			let mut del_count = 0;
			for i in 0..keys.len() {
				check_table.insert(&keys[i], &values[i]);
				sh.insert(&keys[i], &values[i])?;

				if i > 0 {
					let r: usize = rand::random();
					let r = r % (i * 10);
					if i > r {
						// do a delete around 10%
						sh.remove(&keys[del_count])?;
						check_table.remove(&keys[del_count]);
						del_count += 1;
						info!("del here {}", del_count)?;
					}
				}
			}

			assert_eq!(sh.size(), check_table.len());
			info!("size={}", sh.size())?;
			for (k, v) in check_table {
				let v_sh = sh.get(k)?;
				assert_eq!(&v_sh.unwrap(), v);
			}
			assert!(sh.get(&str).unwrap().is_none());
		}

		assert_eq!(
			GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
			})?,
			300_000
		);

		Ok(())
	}

	#[test]
	fn test_clear() -> Result<(), Error> {
		let mut slabs = SlabAllocatorBuilder::build();
		slabs.init(SlabAllocatorConfig {
			slab_count: 10,
			..Default::default()
		})?;
		let mut sh = StaticHashtableBuilder::build(StaticHashtableConfig::default(), Some(slabs))?;

		for i in 0..10 {
			sh.insert(&i, &i)?;
		}
		assert_eq!(sh.size(), 10);
		sh.clear()?;
		assert_eq!(sh.size(), 0);
		for i in 0..10 {
			sh.insert(&i, &i)?;
		}
		assert_eq!(sh.size(), 10);
		assert!(sh.insert(&100, &100).is_err());
		sh.clear()?;
		assert!(sh.insert(&100, &100).is_ok());
		assert_eq!(sh.get(&100).unwrap().unwrap(), 100);

		Ok(())
	}

	#[test]
	fn test_raw_hashset_iter() -> Result<(), Error> {
		initialize()?;
		let mut sh = StaticHashsetBuilder::build::<()>(StaticHashsetConfig::default(), None)?;
		let mut hasher = DefaultHasher::new();
		(b"hi").hash(&mut hasher);
		let hash = hasher.finish();
		sh.insert_raw(b"hi", usize!(hash))?;
		assert!(sh.contains_raw(b"hi", usize!(hash))?);

		let mut count = 0;
		for x in sh.iter_raw() {
			info!("x={:?}", x)?;
			// key = 104/105 (hi), value = 111/107 (ok)
			assert_eq!(x, vec![104, 105]);
			count += 1;
		}

		assert_eq!(count, 1);

		assert!(sh.contains_raw(b"hi", usize!(hash))?);
		sh.remove_raw(b"hi", usize!(hash))?;
		assert!(!sh.contains_raw(b"hi", usize!(hash))?);
		assert_eq!(sh.size(), 0);

		Ok(())
	}

	#[test]
	fn test_raw_iter() -> Result<(), Error> {
		initialize()?;
		let mut slabs1 = SlabAllocatorBuilder::build();
		slabs1.init(SlabAllocatorConfig::default())?;
		let mut sh =
			StaticHashtableBuilder::build::<(), ()>(StaticHashtableConfig::default(), None)?;

		let mut hasher = DefaultHasher::new();
		(b"hi").hash(&mut hasher);
		let hash = hasher.finish();
		sh.insert_raw(b"hi", usize!(hash), b"ok")?;
		{
			let slab = sh.get_raw(b"hi", usize!(hash))?.unwrap();
			// key = 104/105 (hi), value = 111/107 (ok)
			assert_eq!(
				slab.get()[0..36],
				[
					255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
					0, 0, 0, 0, 0, 0, 0, 2, 104, 105, 0, 0, 0, 0, 0, 0, 0, 2, 111, 107
				]
			);
		}

		let mut count = 0;
		for x in sh.iter_raw() {
			info!("x={:?}", x)?;
			// key = 104/105 (hi), value = 111/107 (ok)
			assert_eq!(x, (vec![104, 105], vec![111, 107]));
			count += 1;
		}

		assert_eq!(count, 1);

		assert!(sh.remove_raw(b"hi", usize!(hash))?);
		assert!(sh.get_raw(b"hi", usize!(hash))?.is_none());

		assert_eq!(sh.size(), 0);
		assert!(!sh.remove_raw(b"hi", usize!(hash))?);

		Ok(())
	}

	#[test]
	fn test_other_hash_configs() -> Result<(), Error> {
		let mut sh = StaticHashtableBuilder::build(
			StaticHashtableConfig {
				max_entries: 1,
				max_load_factor: 1.0,
				..StaticHashtableConfig::default()
			},
			None,
		)?;
		assert!(sh.insert(&1, &1).is_ok());
		assert!(sh.insert(&1, &2).is_ok());
		assert!(sh.insert(&2, &1).is_err());

		assert!(StaticHashtableBuilder::build::<(), ()>(
			StaticHashtableConfig {
				max_entries: 0,
				..StaticHashtableConfig::default()
			},
			None,
		)
		.is_err());

		assert!(StaticHashtableBuilder::build::<(), ()>(
			StaticHashtableConfig {
				max_load_factor: 0.0,
				..StaticHashtableConfig::default()
			},
			None,
		)
		.is_err());

		assert!(StaticHashtableBuilder::build::<(), ()>(
			StaticHashtableConfig {
				max_load_factor: -0.1,
				..StaticHashtableConfig::default()
			},
			None,
		)
		.is_err());

		assert!(StaticHashtableBuilder::build::<(), ()>(
			StaticHashtableConfig {
				max_load_factor: 1.1,
				..StaticHashtableConfig::default()
			},
			None,
		)
		.is_err());

		Ok(())
	}

	#[test]
	fn test_simulate_errors() -> Result<(), Error> {
		initialize()?;
		{
			let config = StaticHashtableConfig::default();
			let mut sh = StaticHashImpl::new(config.into(), None)?;
			sh.insert_impl(Some(&1), 0, None, Some(&1), None)?;
			sh.config.debug_clear_error = true;
			sh.config.debug_do_next_error = true;

			let mut rhii = RawHashsetIteratorImpl {
				h: &sh,
				cur: sh.first_entry,
			};

			assert!(rhii.next().iter().next().is_none());

			let mut rhii = RawHashtableIteratorImpl {
				h: &sh,
				cur: sh.first_entry,
			};
			assert!(rhii.next().iter().next().is_none());
		}
		{
			let mut allocator = SlabAllocatorBuilder::build();
			allocator.init(SlabAllocatorConfig {
				slab_size: 128,
				..Default::default()
			})?;
			let config = StaticHashsetConfig {
				..StaticHashsetConfig::default()
			};
			let mut sh = StaticHashImpl::new(config.into(), Some(allocator))?;
			let mut key1 = vec![];
			for _ in 0..200 {
				key1.push('a' as u8);
			}
			//let key1 = from_utf8(&key1[..])?.to_string();
			sh.insert_impl::<(), ()>(None, 0, Some(&key1[..]), None, None)?;
			assert!(sh
				.find_entry::<()>(None, Some(&key1[..]), 0)
				.unwrap()
				.is_some());
			// test key match
			let mut key1 = vec![];
			for i in 0..200 {
				if i == 190 {
					key1.push('b' as u8);
				} else {
					key1.push('a' as u8);
				}
			}
			//let key1 = from_utf8(&key1[..])?.to_string();
			assert!(sh
				.find_entry::<()>(None, Some(&key1[..]), 0)
				.unwrap()
				.is_none());
			assert!(sh.find_entry::<()>(None, None, 0).is_err());
		}

		{
			let config = StaticHashtableConfig::default();
			let mut sh = StaticHashImpl::new(config.into(), None)?;

			// invalid insert
			let r = sh.insert_impl::<i32, i32>(None, 0, None, None, None);
			assert!(r.is_err());

			sh.insert_impl(Some(&1), 0, None, Some(&1), None)?;
			sh.config.debug_get_slab_error = true;
			let mut rhii = RawHashsetIteratorImpl {
				h: &sh,
				cur: sh.first_entry,
			};
			assert!(rhii.next().iter().next().is_none());

			let mut rhii = RawHashtableIteratorImpl {
				h: &sh,
				cur: sh.first_entry,
			};
			assert!(rhii.next().iter().next().is_none());
		}

		{
			let config = StaticHashsetConfig::default();
			let mut sh = StaticHashsetBuilder::build(config, None)?;
			sh.insert(&1)?;
			let mut iter = sh.into_iter();
			iter.debug_do_next_error = true;
			assert!(iter.next().is_none());
		}

		{
			let mut config = StaticHashsetConfig::default();
			config.debug_get_slab_error = true;
			let mut sh = StaticHashsetBuilder::build(config, None)?;
			sh.insert(&1)?;
			let mut iter = sh.into_iter();
			assert!(iter.next().is_none());
		}

		{
			let config = StaticHashtableConfig::default();
			let mut sh = StaticHashtableBuilder::build(config, None)?;
			sh.insert(&1, &2)?;
			let mut iter = sh.into_iter();
			iter.debug_do_next_error = true;
			assert!(iter.next().is_none());
		}

		{
			let mut config = StaticHashtableConfig::default();
			config.debug_get_slab_error = true;
			let mut sh = StaticHashtableBuilder::build(config, None)?;
			sh.insert(&1, &2)?;
			let mut iter = sh.into_iter();
			assert!(iter.next().is_none());
		}

		{
			let config = StaticHashtableConfig {
				max_entries: 1,
				max_load_factor: 1.0,
				..StaticHashtableConfig::default()
			};
			let mut slabs = SlabAllocatorBuilder::build();
			slabs.init(SlabAllocatorConfig::default())?;
			let mut sh = StaticHashtableBuilder::build(config, Some(slabs))?;
			sh.insert(&1, &2)?;
			assert!(sh.insert(&2, &3).is_err());
		}

		{
			let config = StaticHashtableConfig {
				max_entries: 100,

				..StaticHashtableConfig::default()
			};
			let mut slabs = SlabAllocatorBuilder::build();
			slabs.init(SlabAllocatorConfig {
				slab_count: 1,
				..SlabAllocatorConfig::default()
			})?;
			let mut sh = StaticHashtableBuilder::build(config, Some(slabs))?;
			sh.insert(&1, &2)?;
			assert!(sh.insert(&2, &3).is_err());
		}

		Ok(())
	}

	#[derive(Debug, PartialEq)]
	struct VarSer {
		len: usize,
	}

	impl Serializable for VarSer {
		fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
			let len = reader.read_usize()?;
			for _ in 0..len {
				reader.read_u8()?;
			}
			Ok(Self { len })
		}

		fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
			writer.write_usize(self.len)?;
			for _ in 0..self.len {
				writer.write_u8(0)?;
			}
			Ok(())
		}
	}

	#[test]
	fn test_resources() -> Result<(), Error> {
		let config = StaticHashtableConfig {
			max_entries: 100,

			..StaticHashtableConfig::default()
		};
		let mut slabs = SlabAllocatorBuilder::build();
		slabs.init(SlabAllocatorConfig {
			slab_count: 1,
			slab_size: 128,
			..SlabAllocatorConfig::default()
		})?;
		let mut sh = StaticHashtableBuilder::build(config, Some(slabs))?;
		assert!(sh.insert(&80u32, &VarSer { len: 76 }).is_err());
		assert!(sh.insert(&90u32, &VarSer { len: 75 }).is_ok());
		sh.remove(&90u32)?;
		assert!(sh.insert(&80u32, &VarSer { len: 75 }).is_ok());
		assert_eq!(sh.get(&80u32)?, Some(VarSer { len: 75 }));

		Ok(())
	}

	#[test]
	fn test_load_factor() -> Result<(), Error> {
		let config = StaticHashtableConfig {
			max_entries: 100,
			max_load_factor: 0.5,
			..StaticHashtableConfig::default()
		};
		let mut slabs = SlabAllocatorBuilder::build();
		slabs.init(SlabAllocatorConfig {
			slab_count: 1000,
			slab_size: 128,
			..SlabAllocatorConfig::default()
		})?;
		let mut sh = StaticHashtableBuilder::build(config, Some(slabs))?;

		for i in 0..100 {
			sh.insert(&i, &0)?;
		}
		assert!(sh.insert(&100, &0).is_err());

		Ok(())
	}

	#[test]
	fn test_deleted_slot() -> Result<(), Error> {
		let config = StaticHashtableConfig {
			max_entries: 100,
			max_load_factor: 0.5,
			..StaticHashtableConfig::default()
		};
		let mut slabs = SlabAllocatorBuilder::build();
		slabs.init(SlabAllocatorConfig {
			slab_count: 1000,
			slab_size: 128,
			..SlabAllocatorConfig::default()
		})?;
		let mut sh = StaticHashtableBuilder::build(config, Some(slabs))?;

		sh.insert(&1, &0)?;
		sh.remove(&1)?;
		assert!(sh.get(&1).unwrap().is_none());
		sh.insert(&1, &2)?;
		assert!(sh.get(&1).unwrap().is_some());

		Ok(())
	}

	#[test]
	fn test_max_iter() -> Result<(), Error> {
		initialize()?;
		let mut config: StaticHashConfig = StaticHashsetConfig::default().into();
		config.debug_max_iter = true;
		let mut sh = StaticHashImpl::new(config, None)?;

		sh.insert_impl::<i32, ()>(Some(&1), 0, None, None, None)?;

		assert!(sh.find_entry(Some(&1), None, 0)?.is_none());
		sh.config.debug_max_iter = false;
		assert!(sh.find_entry(Some(&1), None, 0)?.is_some());

		Ok(())
	}
}
