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
use crate::slabs::Slab;
use crate::slabs::SlabMut;
use crate::{
	Context, Reader, Serializable, SlabAllocator, SlabAllocatorConfig, StaticHashset,
	StaticHashsetConfig, StaticHashtable, StaticHashtableConfig, Writer, GLOBAL_SLAB_ALLOCATOR,
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

pub struct StaticHashImpl {
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

pub struct RawHashsetIterator<'a> {
	h: &'a StaticHashImpl,
	cur: usize,
}

impl<'a> Iterator for RawHashsetIterator<'a> {
	type Item = Slab<'a>;
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

impl<'a> RawHashsetIterator<'a> {
	fn do_next(self: &mut RawHashsetIterator<'a>) -> Result<Option<Slab<'a>>, Error> {
		if self.h.config.debug_do_next_error {
			return Err(err!(ErrKind::Test, "do_next err"));
		}

		Ok(if self.cur == usize::MAX {
			None
		} else {
			match self.h.get_slab(self.h.entry_array[self.cur]) {
				Ok(slab) => {
					self.cur = usize::from_be_bytes(try_into!(slab.get()[0..8])?);
					Some(slab)
				}
				Err(e) => {
					error!("get slab generated error: {}", e)?;
					None
				}
			}
		})
	}
}

pub struct RawHashtableIterator<'a> {
	pub h: &'a StaticHashImpl,
	pub cur: usize,
}

impl<'a> Iterator for RawHashtableIterator<'a> {
	type Item = Slab<'a>;
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

impl<'a> RawHashtableIterator<'a> {
	fn do_next(self: &mut RawHashtableIterator<'a>) -> Result<Option<Slab<'a>>, Error> {
		if self.h.config.debug_do_next_error {
			return Err(err!(ErrKind::Test, "do_next err"));
		}

		Ok(if self.cur == usize::MAX {
			None
		} else {
			match self.h.get_slab(self.h.entry_array[self.cur]) {
				Ok(slab) => {
					self.cur = usize::from_be_bytes(try_into!(slab.get()[0..8])?);
					Some(slab)
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
	context: Context,
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
	K: Serializable + Hash,
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
			let entry_array = self.h.get_array(&mut self.context);
			match self.h.slab(&mut self.context, entry_array[self.cur]) {
				Ok(slab) => {
					let k = self.h.read_k(&mut self.context, slab.id())?;
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
	context: Context,
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
	K: Serializable + Hash,
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
			let entry_array = self.h.get_array(&mut self.context);
			match self.h.slab(&mut self.context, entry_array[self.cur]) {
				Ok(slab) => {
					self.context.buf1.clear();
					let (k, v) = self.h.read_kv(&mut self.context, slab.id())?;
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

impl<'a, K, V> Serializable for Box<dyn StaticHashtable<K, V>>
where
	K: Serializable + Hash + 'a,
	V: Serializable + 'a,
{
	fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
		let mut context = Context::new();
		let config = StaticHashtableConfig::read(reader)?;
		let size = reader.read_usize()?;
		let mut hashtable = StaticHashtableBuilder::build(config, None)?;
		for _ in 0..size {
			let k = K::read(reader)?;
			let v = V::read(reader)?;
			hashtable.insert(&mut context, &k, &v)?;
		}

		Ok(hashtable)
	}
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
		let mut context = Context::new();
		StaticHashtableConfig::write(&self.config(), writer)?;
		writer.write_usize(self.size(&mut context))?;
		for slab in self.iter_raw(&mut context) {
			let (k, v) = self.read_kv(&mut context, slab.id())?;
			K::write(&k, writer)?;
			V::write(&v, writer)?;
		}
		Ok(())
	}
}

impl<'a, K> Serializable for Box<dyn StaticHashset<K>>
where
	K: Serializable + Hash + 'a,
{
	fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
		let mut context = Context::new();
		let config = StaticHashsetConfig::read(reader)?;
		let size = reader.read_usize()?;
		let mut hashset = StaticHashsetBuilder::build(config, None)?;
		for _ in 0..size {
			let k = K::read(reader)?;
			hashset.insert(&mut context, &k)?;
		}

		Ok(hashset)
	}
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
		StaticHashsetConfig::write(&self.config(), writer)?;
		let mut context = Context::new();
		writer.write_usize(self.size(&mut context))?;
		for slab in self.iter_raw(&mut context) {
			let k = self.read_k(&mut context, slab.id())?;
			K::write(&k, writer)?;
		}
		Ok(())
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
		let mut context = Context::new();
		let cur = self.first_entry(&mut context);
		Self::IntoIter {
			cur,
			h: &self,
			debug_do_next_error: false,
			context,
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
		let mut context = Context::new();
		let cur = self.first_entry(&mut context);
		Self::IntoIter {
			cur,
			h: &self,
			debug_do_next_error: false,
			context,
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
	fn insert(&mut self, context: &mut Context, key: &K, value: &V) -> Result<(), Error> {
		let ret = self.insert_impl::<K, V>(Some(key), 0, None, Some(value), None, context);
		context.shrink();
		ret
	}
	fn get(&self, context: &mut Context, key: &K) -> Result<Option<V>, Error> {
		let ret = self.get_impl(Some(key), None, 0, context);
		context.shrink();
		ret
	}
	fn remove(&mut self, context: &mut Context, key: &K) -> Result<Option<V>, Error> {
		let ret = self.remove_impl(Some(key), None, 0, context);
		context.shrink();
		ret
	}
	fn get_raw<'b>(
		&'b self,
		context: &mut Context,
		key: &[u8],
		hash: usize,
	) -> Result<Option<Slab<'b>>, Error> {
		let ret = self.get_raw_impl::<K>(key, hash, &mut context.buf1);
		context.shrink();
		ret
	}

	fn insert_raw(
		&mut self,
		context: &mut Context,
		key: &[u8],
		hash: usize,
		value: &[u8],
	) -> Result<(), Error> {
		let ret = self.insert_impl::<K, V>(None, hash, Some(key), None, Some(value), context);
		context.shrink();
		ret
	}
	fn remove_raw(
		&mut self,
		context: &mut Context,
		key: &[u8],
		hash: usize,
	) -> Result<bool, Error> {
		let ret = self.remove_impl::<K, V>(None, Some(key), hash, context)?;
		context.shrink();
		Ok(ret.is_some())
	}
	fn iter_raw<'b>(&'b self, _context: &mut Context) -> RawHashtableIterator<'b> {
		let cur = self.first_entry;
		let h = self;
		let r = RawHashtableIterator { cur, h };
		r
	}
	fn size(&self, _context: &mut Context) -> usize {
		self.size
	}
	fn clear(&mut self, _context: &mut Context) -> Result<(), Error> {
		self.clear_impl()
	}
	fn first_entry(&self, _context: &mut Context) -> usize {
		self.first_entry
	}

	fn slab<'b>(&'b self, _context: &mut Context, id: usize) -> Result<Slab<'b>, Error> {
		self.get_slab(id)
	}

	fn read_kv(&self, context: &mut Context, slab_id: usize) -> Result<(K, V), Error> {
		let ret = self.read_kv_ser(slab_id, context);
		context.shrink();
		ret
	}
	fn get_array(&self, _context: &mut Context) -> &Vec<usize> {
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
	fn insert(&mut self, context: &mut Context, key: &K) -> Result<(), Error> {
		let ret = self.insert_impl::<K, K>(Some(key), 0, None, None, None, context);
		context.shrink();
		ret
	}
	fn contains(&self, context: &mut Context, key: &K) -> Result<bool, Error> {
		debug!("contains:self.config={:?}", self.config)?;
		let ret = self
			.find_entry(Some(key), None, 0, &mut context.buf1)?
			.is_some();
		context.shrink();
		Ok(ret)
	}
	fn contains_raw(&self, context: &mut Context, key: &[u8], hash: usize) -> Result<bool, Error> {
		debug!("contains_raw:self.config={:?}", self.config)?;
		let ret = self
			.find_entry::<K>(None, Some(key), hash, &mut context.buf1)?
			.is_some();
		context.shrink();
		Ok(ret)
	}

	fn remove(&mut self, context: &mut Context, key: &K) -> Result<bool, Error> {
		let (entry, slab) = match self.find_entry(Some(key), None, 0, &mut context.buf1)? {
			Some((entry, slab)) => (entry, slab),
			None => {
				context.shrink();
				return Ok(false);
			}
		};

		let slab_id = slab.id();
		self.free_tail(slab_id)?;
		self.entry_array[entry] = SLOT_DELETED;
		self.size = self.size.saturating_sub(1);
		context.shrink();

		Ok(true)
	}
	fn insert_raw(&mut self, context: &mut Context, key: &[u8], hash: usize) -> Result<(), Error> {
		let ret = self.insert_impl::<K, K>(None, hash, Some(key), None, None, context);
		context.shrink();
		ret
	}
	fn remove_raw(
		&mut self,
		context: &mut Context,
		key: &[u8],
		hash: usize,
	) -> Result<bool, Error> {
		let (entry, slab_id) =
			match self.find_entry::<K>(None, Some(key), hash, &mut context.buf1)? {
				Some((entry, slab)) => {
					context.shrink();
					(entry, slab.id())
				}
				None => {
					context.shrink();
					return Ok(false);
				}
			};

		self.free_tail(slab_id)?;
		self.entry_array[entry] = SLOT_DELETED;
		self.size = self.size.saturating_sub(1);

		Ok(true)
	}

	fn iter_raw<'b>(&'b self, _context: &mut Context) -> RawHashsetIterator<'b> {
		let cur = self.first_entry;
		let h = self;
		let r = RawHashsetIterator { cur, h };
		r
	}

	fn size(&self, _context: &mut Context) -> usize {
		self.size
	}
	fn clear(&mut self, _context: &mut Context) -> Result<(), Error> {
		self.clear_impl()
	}
	fn first_entry(&self, _context: &mut Context) -> usize {
		self.first_entry
	}
	fn slab<'b>(&'b self, _context: &mut Context, id: usize) -> Result<Slab<'b>, Error> {
		self.get_slab(id)
	}
	fn read_k(&self, context: &mut Context, slab_id: usize) -> Result<K, Error> {
		let k = self.read_k_ser::<K>(slab_id, context)?;
		context.shrink();
		Ok(k)
	}
	fn get_array(&self, _context: &mut Context) -> &Vec<usize> {
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
		let size: usize = (config.max_entries as f64 / config.max_load_factor).ceil() as usize;
		debug!("entry array init to size = {}", size)?;
		entry_array.resize(size, SLOT_EMPTY);
		Ok(())
	}

	fn do_drop(self: &mut StaticHashImpl) -> Result<(), Error> {
		debug!("Dropping StaticHashImpl with config: {:?}", self.config)?;
		Self::clear_impl(self)?;
		Ok(())
	}
	fn get_slab<'a>(&'a self, id: usize) -> Result<Slab<'a>, Error> {
		if self.config.debug_get_slab_error {
			return Err(err!(ErrKind::Test, "simulate get_slab error"));
		}
		match &self.slabs {
			Some(slabs) => Ok(slabs.get(id)?),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<Slab<'a>, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().get(id)? })
			}),
		}
	}

	fn get_mut<'a>(&'a mut self, id: usize) -> Result<SlabMut<'a>, Error> {
		match &mut self.slabs {
			Some(slabs) => Ok(slabs.get_mut(id)?),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<SlabMut<'a>, Error> {
				Ok(unsafe { f.get().as_mut().unwrap().get_mut(id)? })
			}),
		}
	}

	fn allocate<'a>(&'a mut self) -> Result<SlabMut<'a>, Error> {
		match &mut self.slabs {
			Some(slabs) => Ok(slabs.allocate()?),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<SlabMut<'a>, Error> {
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

	fn get_raw_impl<'b, K>(
		&'b self,
		key_raw: &[u8],
		hash: usize,
		tmp: &mut Vec<u8>,
	) -> Result<Option<Slab<'b>>, Error>
	where
		K: Serializable + Hash,
	{
		match self.find_entry::<K>(None, Some(key_raw), hash, tmp)? {
			Some((_entry, slab)) => Ok(Some(slab)),
			None => Ok(None),
		}
	}

	fn get_impl<K, V>(
		&self,
		key_ser: Option<&K>,
		key_raw: Option<&[u8]>,
		hash: usize,
		context: &mut Context,
	) -> Result<Option<V>, Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		debug!("get_impl:self.config={:?}", self.config)?;
		let slab = self.find_entry(key_ser, key_raw, hash, &mut context.buf1)?;
		match slab {
			Some((_entry, slab)) => Ok(Some(self.read_kv_ser::<K, V>(slab.id(), context)?.1)),
			None => Ok(None),
		}
	}

	fn read_k_ser<K>(&self, slab_id: usize, context: &mut Context) -> Result<K, Error>
	where
		K: Serializable + Hash,
	{
		let k = &mut context.buf2;
		k.clear();
		let mut _v = &mut context.buf3;
		self.read_value(slab_id, k, _v)?;
		let mut cursor = Cursor::new(k);
		cursor.set_position(0);
		let mut reader1 = BinReader::new(&mut cursor);
		let k = K::read(&mut reader1)?;
		Ok(k)
	}

	fn read_kv_ser<K, V>(&self, slab_id: usize, context: &mut Context) -> Result<(K, V), Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		let k = &mut context.buf2;
		let v = &mut context.buf3;
		k.clear();
		v.clear();
		self.read_value(slab_id, k, v)?;
		let mut cursor = Cursor::new(k);
		cursor.set_position(0);
		context.buf1.clear();
		let mut reader1 = BinReader::new(&mut cursor);
		let k = K::read(&mut reader1)?;

		let mut cursor = Cursor::new(v);
		cursor.set_position(0);
		context.buf1.clear();
		let mut reader2 = BinReader::new(&mut cursor);
		let v = V::read(&mut reader2)?;
		Ok((k, v))
	}

	fn read_value(&self, slab_id: usize, k: &mut Vec<u8>, v: &mut Vec<u8>) -> Result<(), Error> {
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
		Ok(())
	}

	fn find_entry<'a, K>(
		&'a self,
		key: Option<&K>,
		key_raw: Option<&[u8]>,
		hash: usize,
		tmp: &mut Vec<u8>,
	) -> Result<Option<(usize, Slab<'a>)>, Error>
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
				let slab = self.key_match(self.entry_array[entry], key, key_raw, tmp)?;
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
		tmp: &mut Vec<u8>,
	) -> Result<Option<Slab<'a>>, Error>
	where
		K: Serializable + Hash,
	{
		// serialize key
		match key_ser {
			Some(key_ser) => {
				tmp.clear();
				serialize(tmp, key_ser)?
			}
			None => match key_raw {
				Some(key_raw) => {
					tmp.clear();
					tmp.extend(key_raw);
				}
				None => {
					let fmt = "a serializable key or a raw key must be specified";
					let e = err!(ErrKind::IllegalArgument, fmt);
					return Err(e);
				}
			},
		}
		let klen = tmp.len();
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
		if slab.get()[24..end] != tmp[0..end - 24] {
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

			if tmp[offset..offset + rem] != slab.get()[0..rem] {
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
		context: &mut Context,
	) -> Result<(), Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		debug!("insert:self.config={:?}", self.config)?;

		let bytes_per_slab = self.slab_size.saturating_sub(SLAB_OVERHEAD);
		let free_count = self.get_free_count()?;

		let k_len_bytes: &[u8];
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
				context.buf1.clear();
				serialize(&mut context.buf1, key)?;
				k_bytes = &(context.buf1);
				krem = context.buf1.len();
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

		if k_bytes.len() == 0 {
			let fmt = "key must not be 0 length";
			let e = err!(ErrKind::IllegalArgument, fmt);
			return Err(e);
		}

		match value_ser {
			Some(value) => {
				context.buf2.clear();
				serialize(&mut context.buf2, value)?;
				v_bytes = &(context.buf2);
				vrem = context.buf2.len();
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
		let slabs_needed = needed_len.saturating_sub(1) as usize / bytes_per_slab;

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
				.key_match(self.entry_array[entry], key_ser, key_raw, &mut context.buf3)?
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

		if (self.size + 1) as f64 > self.config.max_load_factor * self.entry_array.len() as f64 {
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

	fn remove_impl<K, V>(
		&mut self,
		key_ser: Option<&K>,
		key_raw: Option<&[u8]>,
		hash: usize,
		context: &mut Context,
	) -> Result<Option<V>, Error>
	where
		K: Serializable + Hash,
		V: Serializable,
	{
		let (entry, slab_id) = match self.find_entry(key_ser, key_raw, hash, &mut context.buf1)? {
			Some((entry, slab)) => (entry, slab.id()),
			None => return Ok(None),
		};

		self.free_tail(slab_id)?;
		self.entry_array[entry] = SLOT_DELETED;
		self.size = self.size.saturating_sub(1);

		Ok(Some(self.read_kv_ser::<K, V>(slab_id, context)?.1))
	}

	fn entry_hash<K: Hash>(&self, key: &K) -> usize {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;

		let max_iter = self.entry_array.len();

		hash % max_iter
	}
}

/// A builder struct used to build a [`crate::StaticHashtable`]. This macro
/// is called by [`crate::hashtable`]. [`crate::StaticHashtable`]s are generally built
/// through that macro.
pub struct StaticHashtableBuilder {}

impl StaticHashtableBuilder {
	/// Build a [`crate::StaticHashtable`] based on the specified
	/// [`crate::StaticHashtableConfig`] and optional [`crate::SlabAllocator`].
	/// The function returns a boxed [`crate::StaticHashtable`] on success
	/// and returns a [`bmw_err::Error`] on failure.
	///
	/// # Errors
	///
	/// * [`bmw_err::ErrorKind::IllegalArgument`] if the max_load_factor is not greater
	/// than 0 and less than or equal to 1.0 or if the max_entries is equal to 0.
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

/// A builder struct used to build a [`crate::StaticHashset`]. This macro
/// is called by [`crate::hashset`]. [`crate::StaticHashset`]s are generally built
/// through that macro.
pub struct StaticHashsetBuilder {}

impl StaticHashsetBuilder {
	/// Build a [`crate::StaticHashset`] based on the specified
	/// [`crate::StaticHashsetConfig`] and optional [`crate::SlabAllocator`].
	/// The function returns a boxed [`crate::StaticHashset`] on success
	/// and returns a [`bmw_err::Error`] on failure.
	///
	/// # Errors
	///
	/// * [`bmw_err::ErrorKind::IllegalArgument`] if the max_load_factor is not greater
	/// than 0 and less than or equal to 1.0 or if the max_entries is equal to 0.
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
	use crate as bmw_util;
	use crate::static_hash::StaticHashConfig;
	use crate::static_hash::StaticHashImpl;
	use crate::static_hash::{RawHashsetIterator, RawHashtableIterator};
	use crate::types::SlabAllocatorConfig;
	use crate::types::{Reader, Writer};
	use crate::GLOBAL_SLAB_ALLOCATOR;
	use crate::{
		ctx, hashtable, slab_allocator, Serializable, SlabAllocatorBuilder, StaticHashsetBuilder,
		StaticHashsetConfig, StaticHashtableBuilder, StaticHashtableConfig,
	};
	use bmw_deps::rand;
	use bmw_err::*;
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
		let ctx = ctx!();
		let mut slabs = SlabAllocatorBuilder::build();
		slabs.init(SlabAllocatorConfig::default())?;
		let mut sh = StaticHashtableBuilder::build(StaticHashtableConfig::default(), Some(slabs))?;
		sh.insert(ctx, &1, &2)?;
		assert_eq!(sh.get(ctx, &1)?, Some(2));
		let mut sh2 = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
		for i in 0..20 {
			sh2.insert(ctx, &BigThing::new(i, i), &BigThing::new(i, i))?;
			info!("i={}", i)?;
			assert_eq!(
				sh2.get(ctx, &BigThing::new(i, i))?,
				Some(BigThing::new(i, i))
			);
		}

		let mut sh3 = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
		sh3.insert(ctx, &10, &20)?;
		assert_eq!(sh3.get(ctx, &10)?, Some(20));
		assert_eq!(sh3.size(ctx), 1);

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
		sh2.insert(ctx, &BigThing::new(8, 3), &BigThing::new(1, 3))?;
		Ok(())
	}

	#[test]
	fn test_hashtable_replace() -> Result<(), Error> {
		let ctx = ctx!();
		let mut sh = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
		sh.insert(ctx, &1, &2)?;
		assert_eq!(sh.get(ctx, &1)?, Some(2));
		sh.insert(ctx, &1, &3)?;
		assert_eq!(sh.get(ctx, &1)?, Some(3));
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
		let ctx = ctx!();
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
			sh.insert(ctx, &1, &2)?;
			assert_eq!(sh.get(ctx, &1)?, Some(2));

			let mut sh2 = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;

			for i in 0..4000 {
				info!("i={}", i)?;
				sh2.insert(ctx, &BigThing::new(i, i), &BigThing::new(i, i))?;
				assert_eq!(
					sh2.get(ctx, &BigThing::new(i, i))?,
					Some(BigThing::new(i, i))
				);
			}

			let mut sh3 = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
			sh3.insert(ctx, &10, &20)?;
			assert_eq!(sh3.get(ctx, &10)?, Some(20));
		}

		let free_count = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		assert_eq!(free_count, 30_000);

		Ok(())
	}

	#[test]
	fn test_static_hashset() -> Result<(), Error> {
		let ctx = ctx!();
		initialize()?;
		let mut slabs1 = SlabAllocatorBuilder::build();
		slabs1.init(SlabAllocatorConfig::default())?;
		let mut sh = StaticHashsetBuilder::build(StaticHashsetConfig::default(), None)?;
		sh.insert(ctx, &1u32)?;
		sh.insert(ctx, &9)?;
		sh.insert(ctx, &18)?;

		for i in 0..20 {
			if i == 1 || i == 9 || i == 18 {
				assert!(sh.contains(ctx, &i)?);
			} else {
				assert!(!sh.contains(ctx, &i)?);
			}
		}
		Ok(())
	}

	#[test]
	fn test_hashtable_raw() -> Result<(), Error> {
		let ctx = ctx!();
		initialize()?;
		let mut slabs1 = SlabAllocatorBuilder::build();
		slabs1.init(SlabAllocatorConfig::default())?;
		let mut sh =
			StaticHashtableBuilder::build::<(), ()>(StaticHashtableConfig::default(), None)?;

		let mut hasher = DefaultHasher::new();
		(b"hi").hash(&mut hasher);
		let hash = hasher.finish();
		sh.insert_raw(ctx, b"hi", usize!(hash), b"ok")?;
		assert!(sh.get_raw(ctx, b"hi2", 0).unwrap().is_none());
		{
			let slab = sh.get_raw(ctx, b"hi", usize!(hash))?.unwrap();
			// key = 104/105 (hi), value = 111/107 (oj)
			assert_eq!(
				slab.get()[0..36],
				[
					255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
					0, 0, 0, 0, 0, 0, 0, 2, 104, 105, 0, 0, 0, 0, 0, 0, 0, 2, 111, 107
				]
			);
		}

		let mut count = 0;
		for _ in &sh {
			count += 1;
		}

		assert_eq!(count, 1);

		sh.remove_raw(ctx, b"hi", usize!(hash))?;
		assert!(sh.get_raw(ctx, b"hi", usize!(hash))?.is_none());

		assert_eq!(sh.size(ctx), 0);

		Ok(())
	}

	#[test]
	fn test_hashtable_remove() -> Result<(), Error> {
		let ctx = ctx!();
		initialize()?;
		let mut sh = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
		assert_eq!(sh.get(ctx, &1)?, None);
		sh.insert(ctx, &1, &100)?;
		assert_eq!(sh.get(ctx, &1)?, Some(100));
		sh.remove(ctx, &1)?;
		assert_eq!(sh.get(ctx, &1)?, None);

		Ok(())
	}

	#[test]
	fn test_insert_raw_hashset() -> Result<(), Error> {
		let ctx = ctx!();
		initialize()?;
		let mut sh = StaticHashsetBuilder::build::<()>(StaticHashsetConfig::default(), None)?;
		sh.insert_raw(ctx, &[1], 1)?;
		sh.insert_raw(ctx, &[2], 2)?;

		assert!(sh.contains_raw(ctx, &[1], 1)?);
		assert!(sh.contains_raw(ctx, &[2], 2)?);
		assert!(!sh.contains_raw(ctx, &[3], 3)?);

		sh.remove_raw(ctx, &[2], 2)?;

		assert!(sh.contains_raw(ctx, &[1], 1)?);
		assert!(!sh.contains_raw(ctx, &[2], 2)?);
		assert!(!sh.contains_raw(ctx, &[3], 3)?);

		let mut count = 0;

		for _ in &sh {
			count += 1;
		}

		assert_eq!(count, 1);

		Ok(())
	}

	#[test]
	fn test_hashset_iter() -> Result<(), Error> {
		let ctx = ctx!();
		initialize()?;

		let free_count1 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;

		info!("free_count={}", free_count1)?;
		{
			let mut sh = StaticHashsetBuilder::build(StaticHashsetConfig::default(), None)?;

			sh.insert(ctx, &4)?;
			sh.insert(ctx, &1)?;
			sh.insert(ctx, &2)?;
			sh.insert(ctx, &3)?;
			sh.insert(ctx, &3)?;
			sh.insert(ctx, &3)?;
			sh.insert(ctx, &3)?;
			sh.insert(ctx, &4)?;
			sh.insert(ctx, &5)?;

			let mut count = 0;
			for x in &sh {
				info!("x={}", x)?;
				count += 1;
			}

			assert_eq!(count, 5);
			assert_eq!(sh.size(ctx), 5);
			assert!(sh.contains(ctx, &1)?);
			assert!(sh.contains(ctx, &2)?);
			assert!(sh.contains(ctx, &3)?);
			assert!(sh.contains(ctx, &4)?);
			assert!(sh.contains(ctx, &5)?);

			let free_count2 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
			})?;
			info!("free_count={}", free_count2)?;
			assert_eq!(free_count2, free_count1 - 5);

			sh.remove(ctx, &3)?;
			assert!(sh.contains(ctx, &1)?);
			assert!(sh.contains(ctx, &2)?);
			assert!(!sh.contains(ctx, &3)?);
			assert!(sh.contains(ctx, &4)?);
			assert!(sh.contains(ctx, &5)?);
			let free_count3 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
			})?;
			assert_eq!(free_count3, free_count1 - 4);
			assert_eq!(sh.size(ctx), 4);
		}

		let free_count4 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;
		info!("free_count={}", free_count4)?;
		assert_eq!(free_count4, free_count1);

		let mut sh = StaticHashsetBuilder::build(StaticHashsetConfig::default(), None)?;
		sh.insert(ctx, &1)?;
		sh.insert(ctx, &2)?;
		sh.insert(ctx, &3)?;
		sh.insert(ctx, &4)?;
		sh.insert(ctx, &5)?;
		assert_eq!(sh.size(ctx), 5);
		sh.clear(ctx)?;
		assert_eq!(sh.size(ctx), 0);

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
		let ctx = ctx!();
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
				sh.insert(ctx, &keys[i], &values[i])?;

				if i > 0 {
					let r: usize = rand::random();
					let r = r % (i * 10);
					if i > r {
						// do a delete around 10%
						sh.remove(ctx, &keys[del_count])?;
						check_table.remove(&keys[del_count]);
						del_count += 1;
						info!("del here {}", del_count)?;
					}
				}
			}

			assert_eq!(sh.size(ctx), check_table.len());
			info!("size={}", sh.size(ctx))?;
			for (k, v) in check_table {
				let v_sh = sh.get(ctx, k)?;
				assert_eq!(&v_sh.unwrap(), v);
			}
			assert!(sh.get(ctx, &str).unwrap().is_none());
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
		let ctx = ctx!();
		let mut slabs = SlabAllocatorBuilder::build();
		slabs.init(SlabAllocatorConfig {
			slab_count: 10,
			..Default::default()
		})?;
		let mut sh = StaticHashtableBuilder::build(StaticHashtableConfig::default(), Some(slabs))?;

		for i in 0..10 {
			sh.insert(ctx, &i, &i)?;
		}
		assert_eq!(sh.size(ctx), 10);
		sh.clear(ctx)?;
		assert_eq!(sh.size(ctx), 0);
		for i in 0..10 {
			sh.insert(ctx, &i, &i)?;
		}
		assert_eq!(sh.size(ctx), 10);
		assert!(sh.insert(ctx, &100, &100).is_err());
		sh.clear(ctx)?;
		assert!(sh.insert(ctx, &100, &100).is_ok());
		assert_eq!(sh.get(ctx, &100).unwrap().unwrap(), 100);

		Ok(())
	}

	#[test]
	fn test_raw_hashset_iter() -> Result<(), Error> {
		let ctx = ctx!();
		initialize()?;
		let mut sh = StaticHashsetBuilder::build::<()>(StaticHashsetConfig::default(), None)?;
		let mut hasher = DefaultHasher::new();
		(b"hi").hash(&mut hasher);
		let hash = hasher.finish();
		sh.insert_raw(ctx, b"hi", usize!(hash))?;
		assert!(sh.contains_raw(ctx, b"hi", usize!(hash))?);

		let mut count = 0;
		for x in sh.iter_raw(ctx) {
			info!("x={:?}", x.get())?;
			// key = 104/105 (hi), value = 111/107 (ok)
			assert_eq!(&x.get()[23..26], &[2, 104, 105]);
			count += 1;
		}

		assert_eq!(count, 1);

		assert!(sh.contains_raw(ctx, b"hi", usize!(hash))?);
		sh.remove_raw(ctx, b"hi", usize!(hash))?;
		assert!(!sh.contains_raw(ctx, b"hi", usize!(hash))?);
		assert_eq!(sh.size(ctx), 0);

		Ok(())
	}

	#[test]
	fn test_raw_iter() -> Result<(), Error> {
		let ctx = ctx!();
		initialize()?;
		let mut slabs1 = SlabAllocatorBuilder::build();
		slabs1.init(SlabAllocatorConfig::default())?;
		let mut sh =
			StaticHashtableBuilder::build::<(), ()>(StaticHashtableConfig::default(), None)?;

		let mut hasher = DefaultHasher::new();
		(b"hi").hash(&mut hasher);
		let hash = hasher.finish();
		sh.insert_raw(ctx, b"hi", usize!(hash), b"ok")?;
		{
			let slab = sh.get_raw(ctx, b"hi", usize!(hash))?.unwrap();
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
		for x in sh.iter_raw(ctx) {
			info!("x={:?}", x.get())?;
			// key = 104/105 (hi), value = 111/107 (ok)
			assert_eq!(&x.get()[23..26], &[2, 104, 105]);
			count += 1;
		}

		assert_eq!(count, 1);

		assert!(sh.remove_raw(ctx, b"hi", usize!(hash))?);
		assert!(sh.get_raw(ctx, b"hi", usize!(hash))?.is_none());

		assert_eq!(sh.size(ctx), 0);
		assert!(!sh.remove_raw(ctx, b"hi", usize!(hash))?);

		Ok(())
	}

	#[test]
	fn test_other_hash_configs() -> Result<(), Error> {
		let ctx = ctx!();
		let mut sh = StaticHashtableBuilder::build(
			StaticHashtableConfig {
				max_entries: 1,
				max_load_factor: 1.0,
				..StaticHashtableConfig::default()
			},
			None,
		)?;
		assert!(sh.insert(ctx, &1, &1).is_ok());
		assert!(sh.insert(ctx, &1, &2).is_ok());
		assert!(sh.insert(ctx, &2, &1).is_err());

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
		let ctx = ctx!();
		let mut tmp = vec![];
		initialize()?;
		{
			let config = StaticHashtableConfig::default();
			let mut sh = StaticHashImpl::new(config.into(), None)?;
			sh.insert_impl(Some(&1), 0, None, Some(&1), None, ctx)?;
			sh.config.debug_clear_error = true;
			sh.config.debug_do_next_error = true;

			let mut rhii = RawHashsetIterator {
				h: &sh,
				cur: sh.first_entry,
			};

			assert!(rhii.next().iter().next().is_none());

			let mut rhii = RawHashtableIterator {
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
			sh.insert_impl::<(), ()>(None, 0, Some(&key1[..]), None, None, ctx)?;
			assert!(sh
				.find_entry::<()>(None, Some(&key1[..]), 0, {
					tmp.clear();
					&mut tmp
				})
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
				.find_entry::<()>(None, Some(&key1[..]), 0, {
					tmp.clear();
					&mut tmp
				})
				.unwrap()
				.is_none());
			assert!(sh
				.find_entry::<()>(None, None, 0, {
					tmp.clear();
					&mut tmp
				})
				.is_err());
		}

		{
			let config = StaticHashtableConfig::default();
			let mut sh = StaticHashImpl::new(config.into(), None)?;

			// invalid insert
			let r = sh.insert_impl::<i32, i32>(None, 0, None, None, None, ctx);
			assert!(r.is_err());

			sh.insert_impl(Some(&1), 0, None, Some(&1), None, ctx)?;
			sh.config.debug_get_slab_error = true;
			let mut rhii = RawHashsetIterator {
				h: &sh,
				cur: sh.first_entry,
			};
			assert!(rhii.next().iter().next().is_none());

			let mut rhii = RawHashtableIterator {
				h: &sh,
				cur: sh.first_entry,
			};
			assert!(rhii.next().iter().next().is_none());
		}

		{
			let config = StaticHashsetConfig::default();
			let mut sh = StaticHashsetBuilder::build(config, None)?;
			sh.insert(ctx, &1)?;
			let mut iter = sh.into_iter();
			iter.debug_do_next_error = true;
			assert!(iter.next().is_none());
		}

		{
			let mut config = StaticHashsetConfig::default();
			config.debug_get_slab_error = true;
			let mut sh = StaticHashsetBuilder::build(config, None)?;
			sh.insert(ctx, &1)?;
			let mut iter = sh.into_iter();
			assert!(iter.next().is_none());
		}

		{
			let config = StaticHashtableConfig::default();
			let mut sh = StaticHashtableBuilder::build(config, None)?;
			sh.insert(ctx, &1, &2)?;
			let mut iter = sh.into_iter();
			iter.debug_do_next_error = true;
			assert!(iter.next().is_none());
		}

		{
			let mut config = StaticHashtableConfig::default();
			config.debug_get_slab_error = true;
			let mut sh = StaticHashtableBuilder::build(config, None)?;
			sh.insert(ctx, &1, &2)?;
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
			sh.insert(ctx, &1, &2)?;
			assert!(sh.insert(ctx, &2, &3).is_err());
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
			sh.insert(ctx, &1, &2)?;
			assert!(sh.insert(ctx, &2, &3).is_err());
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
		let ctx = ctx!();
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
		assert!(sh.insert(ctx, &80u32, &VarSer { len: 77 }).is_err());
		assert!(sh.insert(ctx, &90u32, &VarSer { len: 76 }).is_ok());
		sh.remove(ctx, &90u32)?;
		assert!(sh.insert(ctx, &80u32, &VarSer { len: 76 }).is_ok());
		assert_eq!(sh.get(ctx, &80u32)?, Some(VarSer { len: 76 }));

		Ok(())
	}

	#[test]
	fn test_load_factor() -> Result<(), Error> {
		let ctx = ctx!();
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
			sh.insert(ctx, &i, &0)?;
		}
		assert!(sh.insert(ctx, &100, &0).is_err());

		Ok(())
	}

	#[test]
	fn test_deleted_slot() -> Result<(), Error> {
		let ctx = ctx!();
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

		sh.insert(ctx, &1, &0)?;
		sh.remove(ctx, &1)?;
		assert!(sh.get(ctx, &1).unwrap().is_none());
		sh.insert(ctx, &1, &2)?;
		assert!(sh.get(ctx, &1).unwrap().is_some());

		Ok(())
	}

	#[test]
	fn test_max_iter() -> Result<(), Error> {
		let ctx = ctx!();
		initialize()?;
		let mut config: StaticHashConfig = StaticHashsetConfig::default().into();
		config.debug_max_iter = true;
		let mut sh = StaticHashImpl::new(config, None)?;

		let mut tmp = vec![];
		sh.insert_impl::<i32, ()>(Some(&1), 0, None, None, None, ctx)?;

		assert!(sh
			.find_entry(Some(&1), None, 0, {
				tmp.clear();
				&mut tmp
			})?
			.is_none());
		sh.config.debug_max_iter = false;
		assert!(sh
			.find_entry(Some(&1), None, 0, {
				tmp.clear();
				&mut tmp
			})?
			.is_some());

		Ok(())
	}

	#[test]
	fn test_hashtable_capacity() -> Result<(), Error> {
		let ctx = ctx!();
		for i in 1..100 {
			let max_load_factor = i as f64 / 100 as f64;
			let config = StaticHashtableConfig {
				max_entries: 10,
				max_load_factor,
				..StaticHashtableConfig::default()
			};
			let mut slabs = SlabAllocatorBuilder::build();
			slabs.init(SlabAllocatorConfig {
				slab_count: 1000,
				slab_size: 128,
				..SlabAllocatorConfig::default()
			})?;
			let mut sh = StaticHashtableBuilder::build(config, Some(slabs))?;

			for i in 0..10 {
				sh.insert(ctx, &i, &0)?;
			}
			assert!(sh.insert(ctx, &20, &20).is_err());
		}

		Ok(())
	}

	#[test]
	fn test_raw1() -> Result<(), Error> {
		let ctx = ctx!();
		initialize()?;
		let mut slabs1 = SlabAllocatorBuilder::build();
		slabs1.init(SlabAllocatorConfig::default())?;
		let mut sh = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;

		let mut hasher = DefaultHasher::new();
		(b"hi").hash(&mut hasher);
		let hash = hasher.finish();
		sh.insert_raw(ctx, b"hi", usize!(hash), b"ok")?;
		{
			let slab = sh.get_raw(ctx, b"hi", usize!(hash))?.unwrap();
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
		for x in sh.iter_raw(ctx) {
			info!("x={:?}", x.get())?;
			// key = 104/105 (hi), value = 111/107 (ok)
			assert_eq!(&x.get()[23..26], &[2, 104, 105]);
			count += 1;
		}

		assert_eq!(count, 1);

		assert!(sh.remove_raw(ctx, b"hi", usize!(hash))?);
		assert!(sh.get_raw(ctx, b"hi", usize!(hash))?.is_none());

		assert_eq!(sh.size(ctx), 0);
		assert!(!sh.remove_raw(ctx, b"hi", usize!(hash))?);
		// protect against 0 length keys
		assert!(sh.insert(ctx, &(), &()).is_err());

		Ok(())
	}

	#[derive(Hash)]
	struct VarSz {
		len: usize,
	}

	impl Serializable for VarSz {
		fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
			let len = reader.read_u8()? as usize;
			for _ in 0..len {
				reader.read_u8()?;
			}

			Ok(Self { len })
		}
		fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
			writer.write_u8(try_into!(self.len)?)?;
			for _ in 0..self.len {
				writer.write_u8(0)?;
			}
			Ok(())
		}
	}

	#[test]
	fn test_boundries() -> Result<(), Error> {
		initialize()?;
		let ctx = ctx!();

		// one slab
		{
			let slabs = slab_allocator!(64, 1)?;
			let mut hashtable = hashtable!(100, 1.0, slabs)?;

			// overhead is 16 bytes for the iterator list, 8 bytes for the key_len, 8 bytes
			// value_len, 8 bytes reserved for next pointer. So, total reserved is 40.

			// VarSz has 1 byte overhead + len bytes.

			// The key always has 1 byte total. The value is variable starting at 1 byte.
			// We should be able to store 24 bytes which is i = 22. (1 byte for key, 1 byte for
			// value len in the 64 byte slab.

			for i in 0..23 {
				hashtable.insert(ctx, &VarSz { len: 0 }, &VarSz { len: i })?;
				hashtable.clear(ctx)?;
				info!("value_len={} success!", i)?;
			}
			assert!(hashtable
				.insert(ctx, &VarSz { len: 0 }, &VarSz { len: 23 })
				.is_err());
		}

		// two slabs
		{
			let slabs = slab_allocator!(64, 2)?;
			let mut hashtable = hashtable!(100, 1.0, slabs)?;

			// overhead is 16 bytes for the iterator list, 8 bytes for the key_len, 8 bytes
			// value_len, 8 bytes reserved for next pointer. So, total reserved is 40.

			// VarSz has 1 byte overhead + len bytes.

			// The key always has 1 byte total. The value is variable starting at 1 byte.
			// We should be able to store 24 bytes which is i = 22. (1 byte for key, 1 byte for
			// value len in the 64 byte slab. With two slabs, the second slab has only
			// 8 bytes of overhead because the key/value and the iterator list pointers
			// are not stored there. So there should be a total of 56 bytes left for data.
			// This means i = 78 will fit.

			for i in 0..79 {
				hashtable.insert(ctx, &VarSz { len: 0 }, &VarSz { len: i })?;
				hashtable.clear(ctx)?;
				info!("value_len={} success!", i)?;
			}
			assert!(hashtable
				.insert(ctx, &VarSz { len: 0 }, &VarSz { len: 79 })
				.is_err());
		}

		// three slabs
		{
			let slabs = slab_allocator!(64, 3)?;
			let mut hashtable = hashtable!(100, 1.0, slabs)?;

			// overhead is 16 bytes for the iterator list, 8 bytes for the key_len, 8 bytes
			// value_len, 8 bytes reserved for next pointer. So, total reserved is 40.

			// VarSz has 1 byte overhead + len bytes.

			// The key always has 1 byte total. The value is variable starting at 1 byte.
			// We should be able to store 24 bytes which is i = 22. (1 byte for key, 1 byte for
			// value len in the 64 byte slab. With two slabs, the second slab has only
			// 8 bytes of overhead because the key/value and the iterator list pointers
			// are not stored there. So there should be a total of 56 bytes left for data.
			// This means i = 78 will fit. With the third slab, an additional 56 bytes
			// is available so i = 134 will fit.

			for i in 0..135 {
				hashtable.insert(ctx, &VarSz { len: 0 }, &VarSz { len: i })?;
				hashtable.clear(ctx)?;
				info!("value_len={} success!", i)?;
			}
			assert!(hashtable
				.insert(ctx, &VarSz { len: 0 }, &VarSz { len: 135 })
				.is_err());
		}

		Ok(())
	}

	#[test]
	fn test_remove_ser() -> Result<(), Error> {
		let mut h = hashtable!()?;
		let ctx = ctx!();
		h.insert(ctx, &1, &2)?;
		let x = h.remove(ctx, &2)?;
		assert_eq!(x, None);
		let x = h.remove(ctx, &1)?;
		assert_eq!(x, Some(2));
		let x = h.remove(ctx, &1)?;
		assert_eq!(x, None);

		Ok(())
	}
}
