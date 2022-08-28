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

use crate::list_append;
use crate::misc::{set_max, slice_to_usize, usize_to_slice};
use crate::types::{Direction, HashImpl, HashImplSync};
use crate::{
	Builder, Hashset, HashsetConfig, HashsetIterator, Hashtable, HashtableConfig,
	HashtableIterator, List, ListConfig, ListIterator, Reader, Serializable, SlabAllocator,
	SlabAllocatorConfig, SlabReader, SlabWriter, SortableList, Writer, GLOBAL_SLAB_ALLOCATOR,
};
use bmw_err::*;
use bmw_log::*;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::hash_map::DefaultHasher;
use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::rc::Rc;
use std::thread;

const SLOT_EMPTY: usize = usize::MAX;
const SLOT_DELETED: usize = usize::MAX - 1;

info!();

impl<'a, K, V> Iterator for HashtableIterator<'a, K, V>
where
	K: Serializable + Clone,
	V: Serializable + Clone,
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
	K: Serializable + Clone,
{
	type Item = K;
	fn next(&mut self) -> Option<<Self as Iterator>::Item> {
		match self
			.hashset
			.get_next_slot(&mut self.cur, Direction::Backward, &mut self.slab_reader)
		{
			Ok(ret) => match ret {
				true => match K::read(&mut self.slab_reader) {
					Ok(k) => Some(k),
					Err(e) => {
						let _ = warn!("deserialization generated error: {}", e);
						None
					}
				},
				false => None,
			},
			Err(e) => {
				let _ = warn!("get_next_slot generated error: {}", e);
				None
			}
		}
	}
}

impl<'a, V> Iterator for ListIterator<'a, V>
where
	V: Serializable + Clone,
{
	type Item = V;
	fn next(&mut self) -> Option<<Self as Iterator>::Item> {
		match self.linked_list_ref {
			Some(list) => {
				let mut slab_reader = self.slab_reader.as_mut().unwrap();
				// linked list
				if list.size == 0 {
					return None;
				}
				let slot = self.cur;
				match list.get_next_slot(&mut self.cur, self.direction, &mut slab_reader) {
					Ok(ret) => match ret {
						true => match slab_reader.seek(slot, list.ptr_size * 2) {
							Ok(_) => match V::read(slab_reader) {
								Ok(v) => Some(v),
								Err(e) => {
									let _ = warn!("deserialization generated error: {}", e);
									None
								}
							},
							Err(e) => {
								let _ = warn!("slab_reader.seek generated error: {}", e);
								None
							}
						},
						false => None,
					},
					Err(e) => {
						let _ = warn!("get_next_slot generated error: {}", e);
						None
					}
				}
			}
			None => {
				// array list
				let array_list_ref = self.array_list_ref.unwrap();
				if array_list_ref.size == 0 {
					None
				} else if self.direction == Direction::Forward && self.cur >= array_list_ref.size {
					None
				} else if self.direction == Direction::Backward && self.cur <= 0 {
					None
				} else {
					let ret = Some(array_list_ref.inner[self.cur].clone());
					if self.direction == Direction::Forward {
						self.cur += 1;
					} else {
						self.cur = self.cur.saturating_sub(1);
					}
					ret
				}
			}
		}
	}
}

impl<'a, K, V> HashtableIterator<'a, K, V>
where
	K: Serializable + Clone,
{
	fn new(hashtable: &'a HashImpl<K>, cur: usize) -> Self {
		Self {
			hashtable,
			cur,
			_phantom_data: PhantomData,
		}
	}
}

impl<'a, K> HashsetIterator<'a, K>
where
	K: Serializable + Clone,
{
	fn new(hashset: &'a HashImpl<K>, cur: usize) -> Self {
		Self {
			hashset,
			cur,
			_phantom_data: PhantomData,
			slab_reader: hashset.slab_reader.clone(),
		}
	}
}

impl<'a, V> ListIterator<'a, V>
where
	V: Serializable + Clone,
{
	fn new(list: &'a HashImpl<V>, cur: usize, direction: Direction) -> Self {
		let _ = debug!("new list iter");
		Self {
			linked_list_ref: Some(list),
			cur,
			direction,
			_phantom_data: PhantomData,
			slab_reader: Some(list.slab_reader.clone()),
			array_list_ref: None,
		}
	}
}

impl Default for HashtableConfig {
	fn default() -> Self {
		Self {
			max_entries: 1_000_000,
			max_load_factor: 0.8,
		}
	}
}

impl Default for HashsetConfig {
	fn default() -> Self {
		Self {
			max_entries: 1_000_000,
			max_load_factor: 0.8,
		}
	}
}

impl Default for ListConfig {
	fn default() -> Self {
		Self {}
	}
}

impl<K> PartialEq for HashImpl<K>
where
	K: Serializable + PartialEq + Clone,
{
	fn eq(&self, rhs: &Self) -> bool {
		if self.size != rhs.size {
			false
		} else {
			let mut itt1 = ListIterator::new(self, self.head, Direction::Forward);
			let mut itt2 = ListIterator::new(rhs, self.head, Direction::Forward);
			loop {
				let next1 = itt1.next();
				let next2 = itt2.next();
				if next1 != next2 {
					return false;
				}
				if next1 == None {
					break;
				}
			}

			true
		}
	}
}

impl<V> SortableList<V> for HashImpl<V>
where
	V: Serializable + Debug + Clone,
{
	fn sort(&mut self) -> Result<(), Error>
	where
		V: Ord,
	{
		let mut list = Builder::build_array_list::<V>(self.size)?;
		list_append!(list, self);
		list.sort()?;
		self.clear()?;
		list_append!(self, list);
		Ok(())
	}
	fn sort_unstable(&mut self) -> Result<(), Error>
	where
		V: Ord,
	{
		let mut list = Builder::build_array_list::<V>(self.size)?;
		list_append!(list, self);
		list.sort_unstable()?;
		self.clear()?;
		list_append!(self, list);
		Ok(())
	}
}

impl<K> Debug for HashImpl<K>
where
	K: Serializable + Debug + Clone,
{
	fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
		if self.entry_array.is_some() {
			let itt = HashsetIterator::new(self, self.tail);
			write!(f, "[")?;
			let mut i = 0;
			for x in itt {
				if i == 0 {
					write!(f, "{:?}", x)?;
				} else {
					write!(f, ", {:?}", x)?;
				}
				i += 1;
			}
			write!(f, "]")?;
		} else {
			let itt = ListIterator::new(self, self.head, Direction::Forward);
			write!(f, "[")?;
			let mut i = 0;
			for x in itt {
				if i == 0 {
					write!(f, "{:?}", x)?;
				} else {
					write!(f, ", {:?}", x)?;
				}
				i += 1;
			}
			write!(f, "]")?;
		}
		Ok(())
	}
}

unsafe impl<K> Send for HashImplSync<K> where K: Serializable + Clone {}

unsafe impl<K> Sync for HashImplSync<K> where K: Serializable + Clone {}

impl<V> SortableList<V> for HashImplSync<V>
where
	V: Clone + PartialEq + Debug + Serializable,
{
	fn sort(&mut self) -> Result<(), Error>
	where
		V: Ord,
	{
		self.static_impl.sort()
	}
	fn sort_unstable(&mut self) -> Result<(), Error>
	where
		V: Ord,
	{
		self.static_impl.sort_unstable()
	}
}

impl<K> PartialEq for HashImplSync<K>
where
	K: Serializable + PartialEq + Clone,
{
	fn eq(&self, rhs: &Self) -> bool {
		self.static_impl == rhs.static_impl
	}
}

impl<K> Debug for HashImplSync<K>
where
	K: Serializable + Debug + Clone,
{
	fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
		write!(f, "{:?}", self.static_impl)
	}
}

impl<K> HashImplSync<K>
where
	K: Serializable + Clone,
{
	pub(crate) fn new(
		hashtable_config: Option<HashtableConfig>,
		hashset_config: Option<HashsetConfig>,
		list_config: Option<ListConfig>,
		slab_allocator_config: SlabAllocatorConfig,
	) -> Result<Self, Error> {
		let slabs = Builder::build_slabs_ref();
		{
			let mut slabs: RefMut<_> = slabs.borrow_mut();
			slabs.init(slab_allocator_config)?;
		}

		let static_impl =
			HashImpl::new(hashtable_config, hashset_config, list_config, Some(slabs))?;
		Ok(Self { static_impl })
	}
}

impl<K, V> Hashtable<K, V> for HashImplSync<K>
where
	K: Serializable + Hash + PartialEq + Debug + Clone,
	V: Serializable + Clone,
{
	fn insert(&mut self, key: &K, value: &V) -> Result<(), Error> {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		self.static_impl
			.insert_hash_impl(Some(key), Some(value), hash)
	}
	fn get(&self, key: &K) -> Result<Option<V>, Error> {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		match self.static_impl.get_impl(key, hash)? {
			Some((_entry, mut reader)) => Ok(Some(V::read(&mut reader)?)),
			None => Ok(None),
		}
	}
	fn remove(&mut self, key: &K) -> Result<Option<V>, Error> {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		match self.static_impl.get_impl(key, hash)? {
			Some((entry, mut reader)) => {
				let v = V::read(&mut reader)?;
				self.static_impl.remove_impl(entry)?;
				Ok(Some(v))
			}
			None => Ok(None),
		}
	}
	fn size(&self) -> usize {
		self.static_impl.size
	}
	fn clear(&mut self) -> Result<(), Error> {
		self.static_impl.clear_impl()
	}

	fn iter<'b>(&'b self) -> HashtableIterator<'b, K, V> {
		HashtableIterator::new(&self.static_impl, self.static_impl.tail)
	}
}

impl<K> Hashset<K> for HashImplSync<K>
where
	K: Serializable + Hash + PartialEq + Debug + Clone,
{
	fn insert(&mut self, key: &K) -> Result<(), Error> {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		self.static_impl
			.insert_hash_impl::<K>(Some(key), None, hash)
	}
	fn contains(&self, key: &K) -> Result<bool, Error> {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		match self.static_impl.get_impl(key, hash)? {
			Some(_) => Ok(true),
			None => Ok(false),
		}
	}
	fn remove(&mut self, key: &K) -> Result<bool, Error> {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		match self.static_impl.get_impl(key, hash)? {
			Some((entry, _reader)) => {
				self.static_impl.remove_impl(entry)?;
				Ok(true)
			}
			None => Ok(false),
		}
	}
	fn size(&self) -> usize {
		self.static_impl.size
	}
	fn clear(&mut self) -> Result<(), Error> {
		self.static_impl.clear_impl()
	}

	fn iter<'b>(&'b self) -> HashsetIterator<'b, K> {
		HashsetIterator::new(&self.static_impl, self.static_impl.tail)
	}
}

impl<V> List<V> for HashImplSync<V>
where
	V: Serializable + Debug + PartialEq + Clone,
{
	fn push(&mut self, value: V) -> Result<(), Error> {
		self.static_impl.insert_impl::<V>(Some(&value), None, None)
	}

	fn iter<'b>(&'b self) -> ListIterator<'b, V> {
		ListIterator::new(&self.static_impl, self.static_impl.head, Direction::Forward)
	}
	fn iter_rev<'b>(&'b self) -> ListIterator<'b, V> {
		ListIterator::new(
			&self.static_impl,
			self.static_impl.tail,
			Direction::Backward,
		)
	}
	fn delete_head(&mut self) -> Result<(), Error> {
		self.static_impl.delete_head()
	}
	fn size(&self) -> usize {
		self.static_impl.size
	}
	fn clear(&mut self) -> Result<(), Error> {
		self.static_impl.clear_impl()
	}
}

impl<K> HashImpl<K>
where
	K: Serializable + Clone,
{
	pub(crate) fn new(
		hashtable_config: Option<HashtableConfig>,
		hashset_config: Option<HashsetConfig>,
		list_config: Option<ListConfig>,
		slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	) -> Result<Self, Error> {
		let (slab_size, slab_count) = match slabs.as_ref() {
			Some(slabs) => {
				let slabs: Ref<_> = slabs.borrow();
				(slabs.slab_size()?, slabs.slab_count()?)
			}
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

		if slab_size > 256 * 256 {
			return Err(err!(
				ErrKind::Configuration,
				"slab_size must be equal to or less than 65,536"
			));
		}

		if slab_count > 281_474_976_710_655 {
			return Err(err!(
				ErrKind::Configuration,
				"slab_count must be equal to or less than 281_474_976_710_655"
			));
		}

		let (max_entries, max_load_factor) = match hashtable_config {
			Some(config) => {
				if config.max_entries == 0 {
					return Err(err!(
						ErrKind::Configuration,
						"MaxEntries must be greater than 0"
					));
				}
				if config.max_load_factor <= 0.0 || config.max_load_factor > 1.0 {
					return Err(err!(
						ErrKind::Configuration,
						"MaxLoadFactor must be greater than 0 and less than or equal to 1.0"
					));
				}
				(config.max_entries, config.max_load_factor)
			}
			None => {
				match hashset_config {
					Some(config) => {
						if config.max_entries == 0 {
							return Err(err!(
								ErrKind::Configuration,
								"MaxEntries must be greater than 0"
							));
						}
						if config.max_load_factor <= 0.0 || config.max_load_factor > 1.0 {
							return Err(err!(
                                                                ErrKind::Configuration,
                                                                "MaxLoadFactor must be greater than 0 and less than or equal to 1.0"
                                                                ));
						}

						(config.max_entries, config.max_load_factor)
					}
					None => (0, 1.0), // for lists it's ignored
				}
			}
		};

		let (entry_array, ptr_size) = match list_config {
			Some(_) => {
				let mut x = slab_count;
				let mut ptr_size = 0;
				loop {
					if x == 0 {
						break;
					}
					x >>= 8;
					ptr_size += 1;
				}
				debug!("ptr_size={}", ptr_size)?;
				(None, ptr_size)
			}
			None => {
				let size: usize = (max_entries as f64 / max_load_factor).ceil() as usize;
				let mut entry_array = Builder::build_array(size)?;
				debug!("entry array init to size = {}", size)?;
				for i in 0..size {
					entry_array[i] = SLOT_EMPTY
				}
				//entry_array.resize(size, SLOT_EMPTY);
				let mut x = entry_array.size() + 2; // two more, one for deleted and one for empty
				let mut ptr_size = 0;
				loop {
					if x == 0 {
						break;
					}
					x >>= 8;
					ptr_size += 1;
				}
				debug!("ptr_size={}", ptr_size)?;
				(Some(entry_array), ptr_size)
			}
		};
		let mut ptr = [0u8; 8];
		set_max(&mut ptr[0..ptr_size]);
		let max_value = slice_to_usize(&ptr[0..ptr_size])?;

		let bytes_per_slab = slab_size.saturating_sub(ptr_size);
		if slab_size < ptr_size * 4 {
			return Err(err!(
				ErrKind::Configuration,
				format!("SlabSize is too small. Must be at least {}", ptr_size * 4)
			));
		}

		let slab_reader = SlabReader::new(slabs.clone(), 0)?;
		let slab_writer = SlabWriter::new(slabs.clone(), 0)?;

		Ok(Self {
			slabs,
			entry_array,
			bytes_per_slab,
			max_value,
			slab_size,
			ptr_size,
			max_load_factor,
			size: 0,
			head: max_value,
			tail: max_value,
			slab_reader,
			slab_writer,
			_phantom_data: PhantomData,
		})
	}

	fn get_next<V>(
		&self,
		cur: &mut usize,
	) -> Result<Option<<HashtableIterator<K, V> as Iterator>::Item>, Error>
	where
		V: Serializable + Clone,
	{
		let mut reader = self.slab_reader.clone();
		match self.get_next_slot(cur, Direction::Backward, &mut reader)? {
			true => Ok(Some((K::read(&mut reader)?, V::read(&mut reader)?))),
			false => Ok(None),
		}
	}

	fn get_next_slot(
		&self,
		cur: &mut usize,
		direction: Direction,
		reader: &mut SlabReader,
	) -> Result<bool, Error> {
		debug!("cur={}", *cur)?;
		if *cur >= self.max_value {
			return Ok(false);
		}
		let slot = match &self.entry_array {
			Some(entry_array) => entry_array[*cur],
			None => *cur,
		};
		debug!("slot={}", slot)?;

		let mut ptrs = [0u8; 8];
		let ptr_size = self.ptr_size;

		*cur = match direction {
			Direction::Backward => {
				reader.seek(slot, ptr_size)?;
				reader.read_fixed_bytes(&mut ptrs[0..ptr_size])?;
				slice_to_usize(&ptrs[0..ptr_size])?
			}
			Direction::Forward => {
				reader.seek(slot, 0)?;
				reader.read_fixed_bytes(&mut ptrs[0..ptr_size])?;
				slice_to_usize(&ptrs[0..ptr_size])?
			}
		};
		debug!("read cur = {}", cur)?;
		Ok(true)
	}

	fn delete_head_impl(&mut self) -> Result<(), Error> {
		if self.size != 0 {
			self.remove_impl(self.head)?;
		}
		Ok(())
	}

	fn clear_impl(&mut self) -> Result<(), Error> {
		let mut cur = self.tail;
		loop {
			if cur == SLOT_EMPTY || cur == SLOT_DELETED {
				break;
			}

			if self.entry_array.is_none() && cur >= self.max_value {
				break;
			}
			debug!("clear impl cur={}", cur)?;

			if cur < self.max_value {
				let entry = self.lookup_entry(cur);
				debug!("free chain = {}", entry)?;
				self.free_chain(entry)?;
			} else {
				break;
			}

			let last_cur = cur;
			let additional =
				self.get_next_slot(&mut cur, Direction::Backward, &mut self.slab_reader.clone())?;
			match self.entry_array.as_mut() {
				Some(entry_array) => {
					if !additional {
						debug!("setting entry_array[{}]={}", last_cur, SLOT_EMPTY)?;
						entry_array[last_cur] = SLOT_EMPTY;
						break;
					}
					debug!("setting entry_array[{}]={}", last_cur, SLOT_EMPTY)?;
					entry_array[last_cur] = SLOT_EMPTY
				}
				None => {}
			}
		}
		debug!("set size to 0")?;
		self.size = 0;
		self.tail = SLOT_EMPTY;
		self.head = SLOT_EMPTY;

		// clear the entry array to get rid of SLOT_DELETED
		if self.entry_array.is_some() {
			let size = self.entry_array.as_ref().unwrap().size();
			let mut entry_array = Builder::build_array(size)?;
			for i in 0..size {
				entry_array[i] = SLOT_EMPTY
			}
			self.entry_array = Some(entry_array);
		}

		Ok(())
	}

	fn get_impl(&self, key: &K, hash: usize) -> Result<Option<(usize, SlabReader)>, Error>
	where
		K: Serializable + PartialEq + Clone,
	{
		let entry_array_len = match self.entry_array.as_ref() {
			Some(e) => e.size(),
			None => {
				return Err(err!(
					ErrKind::IllegalState,
					"get_impl called with no entry array"
				));
			}
		};
		let mut entry = hash
			% match &self.entry_array {
				Some(entry_array) => entry_array.size(),
				None => 1,
			};

		let mut i = 0;
		loop {
			if i >= entry_array_len {
				let msg = "HashImpl: Capacity exceeded";
				return Err(err!(ErrKind::CapacityExceeded, msg));
			}
			if self.lookup_entry(entry) == SLOT_EMPTY {
				debug!("slot empty at {}", entry)?;
				return Ok(None);
			}

			// does the current key match ours?
			if self.lookup_entry(entry) != SLOT_DELETED {
				match self.read_key(self.lookup_entry(entry))? {
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

	fn insert_hash_impl<V>(
		&mut self,
		key: Option<&K>,
		value: Option<&V>,
		hash: usize,
	) -> Result<(), Error>
	where
		K: Serializable + Hash + PartialEq + Clone,
		V: Serializable + Clone,
	{
		let entry_array_len = match self.entry_array.as_ref() {
			Some(e) => e.size(),
			None => 0,
		};

		let entry = match key {
			Some(key) => {
				let mut entry = hash
					% match &self.entry_array {
						Some(entry_array) => entry_array.size(),
						None => 1,
					};

				// check the load factor
				if (self.size + 1) as f64 > self.max_load_factor * entry_array_len as f64 {
					let fmt = format!("load factor ({}) exceeded", self.max_load_factor);
					return Err(err!(ErrKind::CapacityExceeded, fmt));
				}

				let mut i = 0;
				loop {
					if i >= entry_array_len {
						let msg = "HashImpl: Capacity exceeded";
						return Err(err!(ErrKind::CapacityExceeded, msg));
					}
					let entry_value = self.lookup_entry(entry);
					if entry_value == SLOT_EMPTY || entry_value == SLOT_DELETED {
						break;
					}

					// does the current key match ours?
					match self.read_key(entry_value)? {
						Some((k, _reader)) => {
							if &k == key {
								self.size = self.size.saturating_sub(1);
								self.free_chain(entry_value)?;
								break;
							}
						}
						None => {}
					}

					entry = (entry + 1) % entry_array_len;
					i += 1;
				}

				entry
			}
			None => 0,
		};

		self.insert_impl(key, value, Some(entry))
	}

	fn insert_impl<V>(
		&mut self,
		key: Option<&K>,
		value: Option<&V>,
		entry: Option<usize>,
	) -> Result<(), Error>
	where
		V: Serializable + Clone,
	{
		let ptr_size = self.ptr_size;
		let max_value = self.max_value;
		let tail = self.tail;
		let slab_id = self.allocate()?;
		self.slab_writer.seek(slab_id, 0)?;

		// for lists we use the slab_id as the entry
		let entry = match entry {
			Some(entry) => entry,
			None => slab_id,
		};
		let mut ptrs = [0u8; 16];
		debug!("slab_id={}", slab_id)?;
		// update head/tail pointers
		usize_to_slice(SLOT_EMPTY, &mut ptrs[0..ptr_size])?;
		usize_to_slice(tail, &mut ptrs[ptr_size..ptr_size * 2])?;
		debug!(
			"updating slab id {} with next = {}, prev = {}",
			slab_id, max_value, tail
		)?;

		self.slab_writer.write_fixed_bytes(&ptrs[0..ptr_size * 2])?;
		debug!("key write")?;
		match key {
			Some(key) => match key.write(&mut self.slab_writer) {
				Ok(_) => {}
				Err(e) => {
					warn!("writing key generated error: {}", e)?;
					self.free_chain(slab_id)?;
					return Err(err!(
						ErrKind::CapacityExceeded,
						format!("writing key generated error: {}", e)
					));
				}
			},
			None => {}
		}
		match value {
			Some(value) => match value.write(&mut self.slab_writer) {
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

		match self.entry_array.as_mut() {
			Some(entry_array) => {
				// for hash based structures we use the entry index
				if self.tail < max_value {
					if entry_array[self.tail] < max_value {
						let entry_value = self.lookup_entry(self.tail);
						self.slab_writer.seek(entry_value, 0)?;
						usize_to_slice(entry, &mut ptrs[0..ptr_size])?;
						self.slab_writer.write_fixed_bytes(&ptrs[0..ptr_size])?;
					}
				}
			}
			None => {
				// for list based structures we use the slab_id directly
				if self.tail < max_value {
					self.slab_writer.seek(self.tail, 0)?;
					usize_to_slice(entry, &mut ptrs[0..ptr_size])?;
					self.slab_writer.write_fixed_bytes(&ptrs[0..ptr_size])?;
				}
			}
		}

		self.tail = entry;

		if self.head >= max_value {
			self.head = entry;
		}

		match self.entry_array.as_mut() {
			Some(entry_array) => {
				debug!("setting entry_array[{}]={}", entry, slab_id)?;
				entry_array[entry] = slab_id;
			}
			None => {}
		}

		self.size += 1;

		Ok(())
	}

	fn read_key(&self, slab_id: usize) -> Result<Option<(K, SlabReader)>, Error> {
		let ptr_size = self.ptr_size;
		// get a reader, we have to clone the rc because we are not mutable
		let mut reader = self.slab_reader.clone();
		// seek past the ptr data
		reader.seek(slab_id, ptr_size * 2)?;
		// read our serailized struct
		Ok(Some((K::read(&mut reader)?, reader)))
	}

	fn allocate(&mut self) -> Result<usize, Error> {
		match &mut self.slabs {
			Some(slabs) => {
				let mut slabs: RefMut<_> = slabs.borrow_mut();
				let mut slab = slabs.allocate()?;
				let slab_mut = slab.get_mut();
				// set next pointer to none
				for i in self.bytes_per_slab..self.slab_size {
					slab_mut[i] = 0xFF;
				}
				Ok(slab.id())
			}
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
				let slabs = unsafe { f.get().as_mut().unwrap() };
				let mut slab = slabs.allocate()?;
				let slab_mut = slab.get_mut();
				// set next pointer to none
				for i in self.bytes_per_slab..self.slab_size {
					slab_mut[i] = 0xFF;
				}
				Ok(slab.id())
			}),
		}
	}

	fn free(&mut self, slab_id: usize) -> Result<(), Error> {
		match &mut self.slabs {
			Some(slabs) => {
				let mut slabs: RefMut<_> = slabs.borrow_mut();
				slabs.free(slab_id)
			}
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(), Error> {
				let slabs = unsafe { f.get().as_mut().unwrap() };
				slabs.free(slab_id)
			}),
		}
	}

	fn free_chain(&mut self, slab_id: usize) -> Result<(), Error> {
		let bytes_per_slab = self.bytes_per_slab;
		let slab_size = self.slab_size;
		let mut next_bytes = slab_id.clone();
		loop {
			let id = next_bytes.clone();
			next_bytes = match &self.slabs {
				Some(slabs) => {
					let slabs: Ref<_> = slabs.borrow();
					let slab = slabs.get(next_bytes)?;
					slice_to_usize(&slab.get()[bytes_per_slab..slab_size])
				}
				None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
					let slabs = unsafe { f.get().as_mut().unwrap() };
					let slab = slabs.get(next_bytes)?;
					slice_to_usize(&slab.get()[bytes_per_slab..slab_size])
				}),
			}?
			.clone();
			debug!("free id = {}", id)?;
			self.free(id)?;

			if next_bytes >= self.max_value {
				break;
			}
		}
		Ok(())
	}
	fn lookup_entry(&self, entry: usize) -> usize {
		match self.entry_array.as_ref() {
			Some(entry_array) => entry_array[entry],
			None => entry,
		}
	}

	fn free_iter_list(&mut self, entry: usize) -> Result<(), Error> {
		let slab_id = self.lookup_entry(entry);
		let mut next = [0u8; 8];
		let mut prev = [0u8; 8];
		let ptr_size = self.ptr_size;
		self.slab_reader.seek(slab_id, 0)?;
		self.slab_reader.read_fixed_bytes(&mut next[0..ptr_size])?;
		self.slab_reader.read_fixed_bytes(&mut prev[0..ptr_size])?;

		let next_usize_entry = slice_to_usize(&next[0..ptr_size])?;
		let prev_usize_entry = slice_to_usize(&prev[0..ptr_size])?;

		if self.head == entry {
			self.head = next_usize_entry;
		}
		if self.tail == entry {
			self.tail = prev_usize_entry;
		}

		if next_usize_entry < self.max_value {
			let next_usize = self.lookup_entry(next_usize_entry);
			if next_usize < self.max_value {
				let mut ptrs = [0u8; 8];
				self.slab_reader.seek(next_usize, 0)?;
				self.slab_reader
					.read_fixed_bytes(&mut ptrs[0..ptr_size * 2])?;
				usize_to_slice(prev_usize_entry, &mut ptrs[ptr_size..ptr_size * 2])?;
				self.slab_writer.seek(next_usize, 0)?;
				self.slab_writer.write_fixed_bytes(&ptrs[0..ptr_size * 2])?;
			}
		}

		if prev_usize_entry < self.max_value {
			let prev_usize = self.lookup_entry(prev_usize_entry);
			if prev_usize < self.max_value {
				let mut next = [0u8; 8];
				let mut prev = [0u8; 8];
				self.slab_reader.seek(prev_usize, 0)?;
				self.slab_reader.read_fixed_bytes(&mut next[0..ptr_size])?;
				self.slab_reader.read_fixed_bytes(&mut prev[0..ptr_size])?;
				usize_to_slice(next_usize_entry, &mut next[0..ptr_size])?;
				self.slab_writer.seek(prev_usize, 0)?;
				self.slab_writer.write_fixed_bytes(&next[0..ptr_size])?;
				self.slab_writer.write_fixed_bytes(&prev[0..ptr_size])?;
			}
		}

		Ok(())
	}
	fn remove_impl(&mut self, entry: usize) -> Result<(), Error> {
		debug!("remove impl {}", entry)?;
		self.free_iter_list(entry)?;
		self.free_chain(self.lookup_entry(entry))?;
		match self.entry_array.as_mut() {
			Some(entry_array) => {
				debug!("setting entry_array[{}]={}", entry, SLOT_DELETED)?;
				entry_array[entry] = SLOT_DELETED
			}
			None => {}
		}
		self.size = self.size.saturating_sub(1);

		Ok(())
	}
}

impl<K> Drop for HashImpl<K>
where
	K: Serializable + Clone,
{
	fn drop(&mut self) {
		match self.clear_impl() {
			Ok(_) => {}
			Err(e) => {
				let _ = warn!("unexpected error in drop: {}", e);
			}
		}
	}
}

impl<K, V> Hashtable<K, V> for HashImpl<K>
where
	K: Serializable + Hash + PartialEq + Debug + Clone,
	V: Serializable + Clone,
{
	fn insert(&mut self, key: &K, value: &V) -> Result<(), Error> {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		self.insert_hash_impl(Some(key), Some(value), hash)
	}
	fn get(&self, key: &K) -> Result<Option<V>, Error> {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		match self.get_impl(key, hash)? {
			Some((_entry, mut reader)) => Ok(Some(V::read(&mut reader)?)),
			None => Ok(None),
		}
	}
	fn remove(&mut self, key: &K) -> Result<Option<V>, Error> {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		match self.get_impl(key, hash)? {
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

	fn iter<'b>(&'b self) -> HashtableIterator<'b, K, V> {
		HashtableIterator::new(self, self.tail)
	}
}

impl<K> Hashset<K> for HashImpl<K>
where
	K: Serializable + Hash + PartialEq + Debug + Clone,
{
	fn insert(&mut self, key: &K) -> Result<(), Error> {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		self.insert_hash_impl::<K>(Some(key), None, hash)
	}
	fn contains(&self, key: &K) -> Result<bool, Error> {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		match self.get_impl(key, hash)? {
			Some(_) => Ok(true),
			None => Ok(false),
		}
	}
	fn remove(&mut self, key: &K) -> Result<bool, Error> {
		let mut hasher = DefaultHasher::new();
		key.hash(&mut hasher);
		let hash = hasher.finish() as usize;
		match self.get_impl(key, hash)? {
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

	fn iter<'b>(&'b self) -> HashsetIterator<'b, K> {
		HashsetIterator::new(self, self.tail)
	}
}

impl<V> List<V> for HashImpl<V>
where
	V: Serializable + Debug + Clone,
{
	fn push(&mut self, value: V) -> Result<(), Error> {
		self.insert_impl::<V>(Some(&value), None, None)
	}

	fn iter<'b>(&'b self) -> ListIterator<'b, V> {
		ListIterator::new(self, self.head, Direction::Forward)
	}
	fn iter_rev<'b>(&'b self) -> ListIterator<'b, V> {
		ListIterator::new(self, self.tail, Direction::Backward)
	}
	fn delete_head(&mut self) -> Result<(), Error> {
		self.delete_head_impl()
	}
	fn size(&self) -> usize {
		self.size
	}
	fn clear(&mut self) -> Result<(), Error> {
		self.clear_impl()
	}
	/*
	fn append(&mut self, list: &impl List<V>) -> Result<(), Error> {
		for x in list.iter() {
			self.push(x)?;
		}
		Ok(())
	}
		*/
}

#[cfg(test)]
mod test {
	use crate as bmw_util;
	use crate::types::{Hashset, List};
	use crate::ConfigOption::SlabSize;
	use crate::{
		block_on, execute, list, list_append, list_eq, slab_allocator, thread_pool, Builder,
		HashsetConfig, Hashtable, HashtableConfig, ListConfig, SlabAllocatorConfig, SortableList,
		ThreadPool, GLOBAL_SLAB_ALLOCATOR,
	};
	use bmw_deps::rand::random;
	use bmw_err::*;
	use bmw_log::*;
	use std::cell::RefMut;
	use std::collections::HashMap;

	info!();

	#[test]
	fn test_static_hashtable() -> Result<(), Error> {
		let mut hashtable = Builder::build_hashtable(
			HashtableConfig {
				max_entries: 100,
				..Default::default()
			},
			None,
		)?;
		hashtable.insert(&1, &2)?;
		let v = hashtable.get(&1)?;
		assert_eq!(v.unwrap(), 2);
		assert_eq!(hashtable.size(), 1);
		Ok(())
	}

	#[test]
	fn test_remove_static_hashtable() -> Result<(), Error> {
		let mut hashtable = Builder::build_hashtable(HashtableConfig::default(), None)?;
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
		let mut hashtable = Builder::build_hashtable(HashtableConfig::default(), None)?;
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
		let mut hashtable = Builder::build_hashtable(HashtableConfig::default(), None)?;
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
		let mut hashtable = Builder::build_hashtable(HashtableConfig::default(), None)?;
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
			let mut hashtable = Builder::build_hashtable(HashtableConfig::default(), None)?;
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
	fn test_hashset1() -> Result<(), Error> {
		let mut hashset = Builder::build_hashset::<i32>(HashsetConfig::default(), None)?;
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

	#[test]
	fn test_list1() -> Result<(), Error> {
		let mut list = Builder::build_list(ListConfig::default(), None)?;
		list.push(1)?;
		list.push(2)?;
		list.push(3)?;
		list.push(4)?;
		list.push(5)?;
		list.push(6)?;
		let mut i = 0;
		for x in list.iter() {
			info!("valuetest_fwd={}", x)?;
			i += 1;
			if i > 10 {
				break;
			}
		}

		let mut i = 0;
		for x in list.iter_rev() {
			info!("valuetest_rev={}", x)?;
			i += 1;
			if i > 10 {
				break;
			}
		}

		Ok(())
	}

	/*
	#[test]
	fn test_append() -> Result<(), Error> {
		let mut list = Builder::build_list(ListConfig::default(), None)?;
		list.push(1)?;
		list.push(2)?;
		list.push(3)?;
		list.push(4)?;
		list.push(5)?;
		list.push(6)?;

		let mut list2 = Builder::build_list(ListConfig::default(), None)?;
		list2.push(7)?;
		list2.push(8)?;
		list2.push(9)?;

		list.append(&list2)?;

		let mut i = 0;
		for x in list.iter() {
			i += 1;
			info!("i={}", i)?;
			assert_eq!(x, i);
		}

		assert_eq!(i, 9);

		Ok(())
	}
		*/

	#[test]
	fn test_small_slabs() -> Result<(), Error> {
		let slabs = slab_allocator!(SlabSize(8))?;
		let mut table = Builder::build_hashtable(
			HashtableConfig {
				max_entries: 100,
				..Default::default()
			},
			Some(slabs),
		)?;

		table.insert(&1u8, &1u8)?;
		table.insert(&2u8, &2u8)?;

		let mut count = 0;
		for (k, v) in table.iter() {
			match k {
				1u8 => assert_eq!(v, 1u8),
				2u8 => assert_eq!(v, 2u8),
				_ => assert!(false),
			}
			count += 1;
		}

		assert_eq!(count, 2);

		Ok(())
	}

	#[test]
	fn test_small_config() -> Result<(), Error> {
		let slab_size = 12;
		let slabs = Builder::build_slabs_ref();
		let config = SlabAllocatorConfig {
			slab_size,
			slab_count: 1,
			..Default::default()
		};
		{
			let mut slabs: RefMut<_> = slabs.borrow_mut();
			slabs.init(config)?;
		}

		{
			let config = HashtableConfig {
				max_entries: 1,
				..Default::default()
			};
			let mut h = Builder::build_hashtable(config, Some(slabs.clone()))?;

			info!("insert 1")?;
			assert!(h.insert(&2u64, &6u64).is_err());
			info!("insert 2")?;
			let mut h = Builder::build_hashtable(config, Some(slabs.clone()))?;
			h.insert(&2000u32, &1000u32)?;
		}
		Ok(())
	}
	#[test]
	fn test_sync_hashtable() -> Result<(), Error> {
		let slab_config = SlabAllocatorConfig {
			slab_size: 1024,
			slab_count: 1024,
			..Default::default()
		};

		let config = HashtableConfig {
			max_entries: 1024,
			..Default::default()
		};

		let h = Builder::build_hashtable_sync(config, slab_config)?;
		let mut h = lock!(h)?;
		let h_clone = h.clone();

		let tp = thread_pool!()?;

		{
			let h2 = h_clone.rlock()?;
			assert_eq!((**h2.guard()).get(&2u64)?, None);
		}

		let handle = execute!(tp, {
			let mut h = h.wlock()?;
			(**h.guard()).insert(&2u64, &6u64)?;
			Ok(())
		})?;

		block_on!(handle);

		let h = h_clone.rlock()?;
		assert_eq!((**h.guard()).get(&2u64)?, Some(6u64));

		Ok(())
	}

	#[test]
	fn test_sync_hashset() -> Result<(), Error> {
		let slab_config = SlabAllocatorConfig {
			slab_size: 1024,
			slab_count: 1024,
			..Default::default()
		};

		let config = HashsetConfig {
			max_entries: 1024,
			..Default::default()
		};

		let h = Builder::build_hashset_sync(config, slab_config)?;
		let mut h = lock!(h)?;
		let h_clone = h.clone();

		let tp = thread_pool!()?;

		{
			let h2 = h_clone.rlock()?;
			assert_eq!((**h2.guard()).contains(&2u64)?, false);
		}

		let handle = execute!(tp, {
			let mut h = h.wlock()?;
			(**h.guard()).insert(&2u64)?;
			Ok(())
		})?;

		block_on!(handle);

		let h = h_clone.rlock()?;
		assert_eq!((**h.guard()).contains(&2u64)?, true);

		Ok(())
	}

	#[test]
	fn test_sync_list() -> Result<(), Error> {
		let slab_config = SlabAllocatorConfig {
			slab_size: 1024,
			slab_count: 1024,
			..Default::default()
		};

		let config = ListConfig {};

		let h = Builder::build_list_sync(config, slab_config)?;
		let mut h = lock!(h)?;
		let h_clone = h.clone();

		let tp = thread_pool!()?;

		{
			let h = h_clone.rlock()?;
			assert_eq!((**h.guard()).size(), 0);
		}

		let handle = execute!(tp, {
			let mut h = h.wlock()?;
			(**h.guard()).push(2u64)?;
			Ok(())
		})?;

		block_on!(handle);

		let h = h_clone.rlock()?;
		assert_eq!((**h.guard()).size(), 1);

		Ok(())
	}

	struct TestHashtableBox {
		h: Box<dyn Hashtable<u32, u32>>,
	}

	#[test]
	fn test_hashtable_box() -> Result<(), Error> {
		let config = HashtableConfig {
			..Default::default()
		};

		let h = Builder::build_hashtable_box(config, None)?;
		let mut thtb = TestHashtableBox { h };

		let x = 1;
		thtb.h.insert(&x, &2)?;
		assert_eq!(thtb.h.get(&x)?, Some(2));

		Ok(())
	}

	#[test]
	fn test_list_boxed() -> Result<(), Error> {
		let mut list1 = Builder::build_list_box(ListConfig {}, None)?;
		list1.push(1)?;
		list1.push(2)?;

		let mut list2 = Builder::build_list(ListConfig {}, None)?;
		list2.push(1)?;
		list2.push(2)?;

		//list_append!(list1, list2);

		assert!(list_eq!(list1, list2));

		let list3 = list![1, 2, 1, 2];
		list_append!(list1, list2);
		assert!(list_eq!(list1, list3));

		let mut list4 = Builder::build_array_list(100)?;
		list4.push(1)?;
		list4.push(2)?;
		list4.push(1)?;
		list4.push(2)?;
		assert!(list_eq!(list1, list4));

		Ok(())
	}

	#[test]
	fn test_delete_head() -> Result<(), Error> {
		let free_count1;
		{
			let mut list = list![1, 2, 3, 4];
			free_count1 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
				Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
			})? + 4;

			list.delete_head()?;
		}

		let free_count2 = GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<usize, Error> {
			Ok(unsafe { f.get().as_ref().unwrap().free_count()? })
		})?;

		assert_eq!(free_count1, free_count2);
		Ok(())
	}

	#[test]
	fn test_sort_linked() -> Result<(), Error> {
		let mut list = list![1, 2, 3, 7, 5];
		list.sort()?;
		info!("list={:?}", list)?;

		let other_list = list![1, 2, 3, 5, 7];
		assert!(list_eq!(other_list, list));
		Ok(())
	}
}
