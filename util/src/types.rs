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

use bmw_err::*;
use bmw_log::*;
use std::fmt::Debug;
use std::future::Future;
use std::hash::Hash;

use crate::slabs::SlabImpl;
use crate::slabs::SlabMutImpl;

info!();

/// The configuration struct for a [`StaticHashtable`]. This struct is passed
/// into the [`crate::StaticHashtableBuilder::build`] function. The [`std::default::Default`]
/// trait is implemented for this trait.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StaticHashtableConfig {
	/// The maximum number of entries that can exist in this [`StaticHashtable`].
	/// The default is 1_000_000. Note that the overhead for this value is 8 bytes
	/// per entry. The [`crate::StaticHashtableConfig::max_load_factor`] setting will
	/// also affect how much memory is used by the entry array.
	pub max_entries: usize,
	/// The maximum load factor for this [`crate::StaticHashtable`]. This number
	/// incidicates how full the hashtable can be. This is an array based hashtable
	/// and it is not possible to resize it after it is instantiated. The default value
	/// is 0.75.
	pub max_load_factor: f64,
	/// A debugging option used to test errors in the get slab function. This must be
	/// set to false except in testing scenarios.
	pub debug_get_slab_error: bool,
}

impl Serializable for StaticHashtableConfig {
	fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
		let max_entries = reader.read_usize()?;
		let max_load_factor = f64::read(reader)?;
		let debug_get_slab_error = match reader.read_u8()? {
			0 => false,
			_ => true,
		};
		let ret = Self {
			max_entries,
			max_load_factor,
			debug_get_slab_error,
		};
		Ok(ret)
	}
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
		writer.write_usize(self.max_entries)?;
		f64::write(&self.max_load_factor, writer)?;
		writer.write_u8(match self.debug_get_slab_error {
			true => 1,
			false => 0,
		})?;
		Ok(())
	}
}

/// The configuration struct for a [`StaticHashset`]. This struct is passed
/// into the [`crate::StaticHashsetBuilder::build`] function. The [`std::default::Default`]
/// trait is implemented for this trait.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StaticHashsetConfig {
	/// The maximum number of entries that can exist in this [`StaticHashset`].
	/// The default is 1_000_000. Note that the overhead for this value is 8 bytes
	/// per entry. So, by default 8 mb are allocated with this configuration.
	pub max_entries: usize,
	/// The maximum load factor for this [`crate::StaticHashset`]. This number
	/// incidicates how full the hashset can be. This is an array based hashset
	/// and it is not possible to resize it after it is instantiated. The default value
	/// is 0.75.
	pub max_load_factor: f64,
	/// A debugging option used to test errors in the get slab function. This must be
	/// set to false except in testing scenarios.
	pub debug_get_slab_error: bool,
}

impl Serializable for StaticHashsetConfig {
	fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
		let max_entries = reader.read_usize()?;
		let max_load_factor = f64::read(reader)?;
		let debug_get_slab_error = match reader.read_u8()? {
			0 => false,
			_ => true,
		};
		let ret = Self {
			max_entries,
			max_load_factor,
			debug_get_slab_error,
		};
		Ok(ret)
	}
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
		writer.write_usize(self.max_entries)?;
		f64::write(&self.max_load_factor, writer)?;
		writer.write_u8(match self.debug_get_slab_error {
			true => 1,
			false => 0,
		})?;
		Ok(())
	}
}

/// An iterator for iterating through raw data in this [`crate::StaticHashtable`].
/// This is distinct from the iterator that is implemented as [`crate::StaticHashtable`]'s
/// [`std::iter::IntoIterator`] trait which returns the serialized and not raw data.
///
/// # Examples
///
///```
/// use bmw_err::*;
/// use bmw_util::hashtable;
///
/// fn test() -> Result<(), Error> {
///     // create a hashtable
///     let mut hashtable = hashtable!()?;
///     // need to do a empty insert so that the types can be inferred. From here on we use raw
///     // operations
///     assert!(hashtable.insert(&(), &()).is_err());
///     let key = [0u8, 1u8, 123u8];
///     let value = [10u8];
///     let hash = 123usize;
///     hashtable.insert_raw(&key, hash, &value)?;
///
///     let key = [1u8, 1u8, 125u8];
///     let value = [14u8];
///     let hash = 125usize;
///     hashtable.insert_raw(&key, hash, &value)?;
///
///     let mut count = 0;
///     for (k,v) in hashtable.iter_raw() {
///         if v == [14u8] {
///             assert_eq!(k, [1u8, 1u8, 125u8]);
///             count += 1;
///         } else if v == [10u8] {
///             assert_eq!(k, [0u8, 1u8, 123u8]);
///             count += 1;
///         }
///     }
///
///     assert_eq!(count, 2);
///
///     Ok(())
/// }
///```
pub trait RawHashtableIterator<Item = (Vec<u8>, Vec<u8>)>: Iterator {}

/// An iterator for iterating through raw data in this [`crate::StaticHashtable`].
/// This is distinct from the iterator that is implemented as [`crate::StaticHashtable`]'s
/// [`std::iter::IntoIterator`] trait which returns the serialized and not raw data.
/// # Examples
///
///```
/// use bmw_err::*;
/// use bmw_util::hashset;
///
/// fn test() -> Result<(), Error> {
///     // create a hashset
///     let mut hashset = hashset!()?;
///     // need to do a empty insert so that the types can be inferred. From here on we use raw
///     // operations
///     assert!(hashset.insert(&()).is_err());
///     let key = [0u8, 1u8, 123u8];
///     let hash = 123usize;
///     hashset.insert_raw(&key, hash)?;
///     
///     let key = [1u8, 1u8, 125u8];
///     let hash = 125usize;
///     hashset.insert_raw(&key, hash)?;
///     
///     let mut count = 0;
///     for k in hashset.iter_raw() {
///         if k ==  [1u8, 1u8, 125u8] || k == [0u8, 1u8, 123u8] {
///             count += 1;
///         }
///     }
///
///     assert_eq!(count, 2);
///
///     Ok(())
/// }
///```
pub trait RawHashsetIterator<Item = Vec<u8>>: Iterator {}

/// Slab Allocator configuration struct. This struct is the input to the
/// [`crate::SlabAllocator::init`] function. The two parameters are `slab_size`
/// which is the size of the slabs in bytes allocated by this
/// [`crate::SlabAllocator`] and `slab_count` which is the number of slabs
/// that can be allocated by this [`crate::SlabAllocator`].
#[derive(Debug)]
pub struct SlabAllocatorConfig {
	/// The size, in bytes, of a slab
	pub slab_size: usize,
	/// The number of slabs that this slab allocator can allocate
	pub slab_count: usize,
}

/// The [`crate::StaticHashtable`] trait defines the public interface to the
/// static hashtable. The hashtable in this crate uses linear probing to handle
/// collisions and it cannot be resized after it is intialized. Configuration of the
/// hashtable is done via the [`crate::StaticHashtableConfig`] struct. The shared
/// implementation can be instantiated as a [`crate::StaticHashtable`] through the
/// [`crate::StaticHashtableBuilder::build`] function or as a [`crate::StaticHashset`]
/// through the [`crate::StaticHashsetBuilder::build`] function. Although there
/// is a different interface for each, they are very similar and share most of the
/// implementation code. In most cases, the datastructures in this crate should be
/// instantiated through the macros, but they can also be instantiated through the
/// builder structs as well.
///
/// # Examples
///```
/// use bmw_err::*;
/// use bmw_log::*;
/// use bmw_util::hashtable;
///
/// info!();
///
/// fn main() -> Result<(), Error> {
///     let mut hash = hashtable!()?;
///
///     hash.insert(&1, &"abc".to_string())?;
///     hash.insert(&3, &"def".to_string())?;
///
///     for (k,v) in &hash {
///         info!("k={},v={}", k, v)?;
///     }
///
///     Ok(())
/// }
///```
pub trait StaticHashtable<K, V>
where
	K: Serializable + Hash,
	V: Serializable,
{
	/// This function returns a copy of the underlying [`crate::StaticHashtableConfig`]
	/// struct.
	///
	/// # Examples
	///```
	/// use bmw_err::*;
	/// use bmw_log::*;
	/// use bmw_util::hashtable;
	///
	/// info!();
	///
	/// fn main() -> Result<(), Error> {
	///     let mut hash = hashtable!(1_000, 0.9)?;
	///
	///     hash.insert(&1, &"abc".to_string())?;
	///     let config = hash.config();
	///     assert_eq!(config.max_entries, 1_000);
	///     assert_eq!(config.max_load_factor, 0.9);
	///
	///     Ok(())
	/// }
	///```
	fn config(&self) -> StaticHashtableConfig;

	/// This function inserts the `key` `value` pair specified. Both K/V must implement the
	/// [`crate::Serializable`] trait and `key` must implement the [`std::hash::Hash`]
	/// trait. The key and value are both copied into the table. Note that after this point,
	/// changes the underlying key or value will not be reflected in the stored value. This
	/// is in contrast to the Rust standard Hashtable which uses references.
	///
	/// # Examples
	///```
	/// use bmw_err::*;
	/// use bmw_util::hashtable;
	///
	/// fn main() -> Result<(), Error> {
	///     let mut hash = hashtable!(1_000, 0.9)?;
	///
	///     // since Vec implements Serializable, we can insert vec.
	///     hash.insert(&1, &vec![1u8,2u8,3u8,4u8])?;
	///
	///     Ok(())
	/// }
	///```
	fn insert(&mut self, key: &K, value: &V) -> Result<(), Error>;

	/// Get an element in the [`crate::StaticHashtable`]. A copy of the serialized value
	/// is returned or None if it is not found.
	///
	/// # Examples
	///
	///```
	/// use bmw_err::*;
	/// use bmw_util::hashtable;
	///
	/// fn main() -> Result<(), Error> {
	///     let mut hash = hashtable!(1_000, 0.9)?;
	///
	///     // since Vec implements Serializable, we can insert a Vec.
	///     hash.insert(&1, &vec![1u8,2u8,3u8,4u8])?;
	///     let v = hash.get(&1)?;
	///     assert_eq!(v.unwrap(), vec![1u8,2u8,3u8,4u8]);
	///     assert!(hash.get(&2)?.is_none());
	///
	///     Ok(())
	/// }
	///```
	///
	fn get(&self, key: &K) -> Result<Option<V>, Error>;

	/// Remove an element from the [`crate::StaticHashtable`].
	fn remove(&mut self, key: &K) -> Result<bool, Error>;
	fn get_raw<'b>(&'b self, key: &[u8], hash: usize) -> Result<Option<SlabImpl<'b>>, Error>;
	fn get_raw_mut<'b>(
		&'b mut self,
		key: &[u8],
		hash: usize,
	) -> Result<Option<SlabMutImpl<'b>>, Error>;
	fn insert_raw(&mut self, key: &[u8], hash: usize, value: &[u8]) -> Result<(), Error>;
	fn remove_raw(&mut self, key: &[u8], hash: usize) -> Result<bool, Error>;
	fn iter_raw<'b>(&'b self) -> Box<dyn RawHashtableIterator<Item = (Vec<u8>, Vec<u8>)> + 'b>;
	fn size(&self) -> usize;
	fn first_entry(&self) -> usize;
	fn slab<'b>(&'b self, id: usize) -> Result<SlabImpl<'b>, Error>;
	fn read_kv(&self, slab_id: usize) -> Result<(K, V), Error>;
	fn get_array(&self) -> &Vec<usize>;
	fn clear(&mut self) -> Result<(), Error>;
}

pub trait StaticHashset<K>
where
	K: Serializable + Hash,
{
	fn config(&self) -> StaticHashsetConfig;
	fn insert(&mut self, key: &K) -> Result<(), Error>;
	fn contains(&self, key: &K) -> Result<bool, Error>;
	fn contains_raw(&self, key: &[u8], hash: usize) -> Result<bool, Error>;
	fn remove(&mut self, key: &K) -> Result<bool, Error>;
	fn insert_raw(&mut self, key: &[u8], hash: usize) -> Result<(), Error>;
	fn remove_raw(&mut self, key: &[u8], hash: usize) -> Result<bool, Error>;
	fn iter_raw<'b>(&'b self) -> Box<dyn RawHashsetIterator<Item = Vec<u8>> + 'b>;
	fn size(&self) -> usize;
	fn first_entry(&self) -> usize;
	fn slab<'b>(&'b self, id: usize) -> Result<SlabImpl<'b>, Error>;
	fn read_k(&self, slab_id: usize) -> Result<K, Error>;
	fn get_array(&self) -> &Vec<usize>;
	fn clear(&mut self) -> Result<(), Error>;
}
pub trait StaticQueue<V>
where
	V: Serializable,
{
	fn enqueue(&mut self, value: &V) -> Result<(), Error>;
	fn dequeue(&mut self) -> Result<Option<&V>, Error>;
	fn peek(&self) -> Result<Option<&V>, Error>;
}

pub trait StaticStack<V>
where
	V: Serializable,
{
	fn push(&mut self, value: &V) -> Result<(), Error>;
	fn pop(&mut self) -> Result<Option<V>, Error>;
	fn peek(&self) -> Result<Option<V>, Error>;
}

pub trait StaticList<'a, V>
where
	V: Serializable,
{
	fn push(&mut self, value: &V) -> Result<(), Error>;
	fn pop(&mut self) -> Result<Option<V>, Error>;
	fn push_front(&mut self, value: V) -> Result<(), Error>;
	fn pop_front(&mut self) -> Result<Option<V>, Error>;
	fn pop_raw(&mut self) -> Result<SlabImpl, Error>;
}

pub trait Array<V>
where
	V: Serializable,
{
}

pub trait BitVec {}

pub trait ThreadPool {
	fn execute<F>(&self, f: F) -> Result<(), Error>
	where
		F: Future<Output = Result<(), Error>> + Send + Sync + 'static;
}

/*
/// The public interface to a Slab stored by the [`crate::SlabAllocator`].
/// [`crate::Slab`] is an immutable reference and [`crate::SlabMut`] is a mutable
/// reference.
pub trait Slab {
	/// Get an immutable reference to the underlying data in this slab.
	fn get(&self) -> &[u8];
	/// A unique id for this [`crate::Slab`]. This id can be used to lookup
	/// the slab later.
	fn id(&self) -> usize;
}

/// The public interface to a Slab stored by the [`crate::SlabAllocator`].
/// [`crate::Slab`] is an immutable reference and [`crate::SlabMut`] is a mutable
/// reference.
pub trait SlabMut {
	/// Get an immutable reference to the underlying data in this slab.
	fn get(&self) -> &[u8];
	/// Get a mutable reference to the underlying data in this slab.
	fn get_mut(&mut self) -> &mut [u8];
	/// A unique id for this [`crate::Slab`]. This id can be used to lookup
	/// the slab later.
	fn id(&self) -> usize;
}
*/

/// This trait defines the public interface to the [`crate::SlabAllocator`]. The slab
/// allocator is used by the other data structures in this crate to avoid dynamic heap
/// allocations. By itself, the slab allocator is fairly simple. It only allocates and frees
/// slabs. [`crate::SlabAllocator::get`] and [`crate::SlabAllocator::get_mut`] are also
/// provided to obtain immutable and mutable references to a slab respectively. They only
/// contain references to the data and not copies.
///
/// # Examples
///
///```
/// use bmw_err::*;
/// use bmw_util::slab_allocator;
///
/// fn main() -> Result<(), Error> {
///     // build a slab allocator, in this case with defaults
///     let mut slabs = slab_allocator!()?;
///
///     let id = {
///         // allocate a slab. [`crate::SlabAllocator::allocate`] returns [`crate::SlabMut`]
///         // which contains a mutable reference to the underlying data in the slab.
///         let mut slab = slabs.allocate()?;
///
///         // get the id for this slab
///         let id = slab.id();
///         // get_mut returns a mutable reference to the data in owned by the
///         // [`crate::SlabAllocator`]
///         slab.get_mut()[0] = 101;
///         id
///     };
///
///     // now we can get an immutable reference to this slab
///     let slab = slabs.get(id)?;
///     assert_eq!(slab.get()[0], 101);
///
///     Ok(())
/// }
///```
pub trait SlabAllocator {
	/// Allocate a slab and return a [`crate::SlabMut`] on success.
	/// On failure, return an [`bmw_err::Error`].
	///
	/// * [`bmw_err::ErrorKind::IllegalState`] if the [`crate::SlabAllocator::init`]
	/// function has not been called.
	///
	/// * [`bmw_err::ErrorKind::CapacityExceeded`] if the capacity of this
	/// [`crate::SlabAllocator`] has been exceeded.
	fn allocate<'a>(&'a mut self) -> Result<SlabMutImpl<'a>, Error>;

	/// Free a slab that has previously been allocated by this slab allocator.
	/// `id` is the id of the slab to free. It can be obtained through the
	/// [`crate::SlabMut::id`] or [`crate::Slab::id`] function. Return a
	/// [`bmw_err::Error`] on failure.
	///
	/// *  [`bmw_err::ErrorKind::ArrayIndexOutOfBounds`] if this slab entry is
	/// too big for this instance.
	///
	/// * [`bmw_err::ErrorKind::IllegalState`] if the [`crate::SlabAllocator::init`]
	/// function has not been called or this slab was not allocated.
	///
	/// # Examples
	///
	///```
	/// use bmw_err::*;
	/// use bmw_util::slab_allocator;
	///
	/// fn main() -> Result<(), Error> {
	///     // instantiate a slab allocator with a slab count of 1,000.
	///     let mut slabs = slab_allocator!(1_000, 1_000)?;
	///
	///     // assert that there are 1,000 free slabs.
	///     assert_eq!(slabs.free_count()?, 1_000);
	///
	///     let slab_id = {
	///         // allocate a slab.
	///         let slab = slabs.allocate()?;
	///         slab.id()
	///     };
	///
	///     // assert that the free count has decreased by 1.
	///     assert_eq!(slabs.free_count()?, 999);
	///
	///
	///     // free the slab that was allocated
	///     slabs.free(slab_id)?;
	///
	///     // assert that the free count has returnred to the initial value of 1,000.
	///     assert_eq!(slabs.free_count()?, 1_000);
	///
	///     Ok(())
	/// }
	///```
	fn free(&mut self, id: usize) -> Result<(), Error>;

	/// Get an immutable reference to a slab that has previously been allocated by the
	/// [`crate::SlabAllocator`]. On success a [`crate::Slab`] is returned. On failure,
	/// a [`bmw_err::Error`] is returned.
	///
	/// *  [`bmw_err::ErrorKind::ArrayIndexOutOfBounds`] if this slab entry is
	/// too big for this instance.
	///
	/// * [`bmw_err::ErrorKind::IllegalState`] if the [`crate::SlabAllocator::init`]
	/// function has not been called or this slab was not allocated.
	///
	/// # Examples
	///
	///```
	/// use bmw_err::*;
	/// use bmw_log::*;
	/// use bmw_util::slab_allocator;
	///
	/// info!();
	///
	/// fn main() -> Result<(), Error> {
	///     // instantiate a slab allocator with a slab count of 1,000.
	///     let mut slabs = slab_allocator!(1_000, 1_000)?;
	///
	///     // assert that there are 1,000 free slabs.
	///     assert_eq!(slabs.free_count()?, 1_000);
	///
	///     let slab_id = {
	///         // allocate a slab.
	///         let slab = slabs.allocate()?;
	///         slab.id()
	///     };
	///
	///     // assert that the free count has decreased by 1.
	///     assert_eq!(slabs.free_count()?, 999);
	///
	///
	///     // get the slab that was allocated
	///     let slab = slabs.get(slab_id)?;
	///
	///     info!("slab data = {:?}", slab.get())?;
	///
	///     Ok(())
	/// }
	///```
	fn get<'a>(&'a self, id: usize) -> Result<SlabImpl<'a>, Error>;

	/// Get an mutable reference to a slab that has previously been allocated by the
	/// [`crate::SlabAllocator`]. On success a [`crate::SlabMut`] is returned. On failure,
	/// a [`bmw_err::Error`] is returned.
	///
	/// *  [`bmw_err::ErrorKind::ArrayIndexOutOfBounds`] if this slab entry is
	/// too big for this instance.
	///
	/// * [`bmw_err::ErrorKind::IllegalState`] if the [`crate::SlabAllocator::init`]
	/// function has not been called or this slab was not allocated.
	///
	/// # Examples
	///
	///```
	/// use bmw_err::*;
	/// use bmw_log::*;
	/// use bmw_util::slab_allocator;
	///
	/// info!();
	///
	/// fn main() -> Result<(), Error> {
	///     // instantiate a slab allocator with a slab count of 1,000.
	///     let mut slabs = slab_allocator!(1_000, 1_000)?;
	///
	///     // assert that there are 1,000 free slabs.
	///     assert_eq!(slabs.free_count()?, 1_000);
	///
	///     let slab_id = {
	///         // allocate a slab.
	///         let slab = slabs.allocate()?;
	///         slab.id()
	///     };
	///
	///     // assert that the free count has decreased by 1.
	///     assert_eq!(slabs.free_count()?, 999);
	///
	///
	///     // get the slab that was allocated
	///     let mut slab = slabs.get_mut(slab_id)?;
	///
	///     info!("slab data = {:?}", slab.get_mut())?;
	///
	///     Ok(())
	/// }
	///```
	fn get_mut<'a>(&'a mut self, id: usize) -> Result<SlabMutImpl<'a>, Error>;

	/// Returns the number of free slabs this [`crate::SlabAllocator`] has remaining.
	fn free_count(&self) -> Result<usize, Error>;

	/// Returns the configured `slab_size` for this [`crate::SlabAllocator`].
	fn slab_size(&self) -> Result<usize, Error>;

	/// Initializes the [`crate::SlabAllocator`] with the given `config`. See
	/// [`crate::SlabAllocatorConfig`] for further details.
	fn init(&mut self, config: SlabAllocatorConfig) -> Result<(), Error>;
}

pub trait Match {
	fn start(&self) -> usize;
	fn end(&self) -> usize;
	fn id(&self) -> u128;
	fn set_start(&mut self, start: usize) -> Result<(), Error>;
	fn set_end(&mut self, end: usize) -> Result<(), Error>;
	fn set_id(&mut self, id: u128) -> Result<(), Error>;
}

pub trait Pattern {
	fn regex(&self) -> String;
	fn is_case_sensitive(&self) -> bool;
	fn is_termination_pattern(&self) -> bool;
	fn id(&self) -> u128;
}

pub trait SuffixTree {
	fn add_pattern(&mut self, pattern: &dyn Pattern) -> Result<(), Error>;
	fn run_matches(
		&mut self,
		text: &[u8],
		matches: &mut Vec<Box<dyn Match>>,
	) -> Result<usize, Error>;
}

pub trait Writer {
	fn write_u8(&mut self, n: u8) -> Result<(), Error> {
		self.write_fixed_bytes(&[n])
	}

	fn write_i8(&mut self, n: i8) -> Result<(), Error> {
		self.write_fixed_bytes(&[n as u8])
	}

	fn write_u16(&mut self, n: u16) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_i16(&mut self, n: i16) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_u32(&mut self, n: u32) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_i32(&mut self, n: i32) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_u64(&mut self, n: u64) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_i128(&mut self, n: i128) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_u128(&mut self, n: u128) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_i64(&mut self, n: i64) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_usize(&mut self, n: usize) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_bytes<T: AsRef<[u8]>>(&mut self, bytes: T) -> Result<(), Error> {
		self.write_u64(bytes.as_ref().len() as u64)?;
		self.write_fixed_bytes(bytes)
	}

	fn write_fixed_bytes<T: AsRef<[u8]>>(&mut self, bytes: T) -> Result<(), Error>;

	fn write_empty_bytes(&mut self, length: usize) -> Result<(), Error> {
		self.write_fixed_bytes(vec![0u8; length])
	}
}

pub trait Reader {
	fn read_u8(&mut self) -> Result<u8, Error>;
	fn read_i8(&mut self) -> Result<i8, Error>;
	fn read_i16(&mut self) -> Result<i16, Error>;
	fn read_u16(&mut self) -> Result<u16, Error>;
	fn read_u32(&mut self) -> Result<u32, Error>;
	fn read_u64(&mut self) -> Result<u64, Error>;
	fn read_u128(&mut self) -> Result<u128, Error>;
	fn read_i128(&mut self) -> Result<i128, Error>;
	fn read_i32(&mut self) -> Result<i32, Error>;
	fn read_i64(&mut self) -> Result<i64, Error>;
	fn read_usize(&mut self) -> Result<usize, Error>;
	fn read_bytes_len_prefix(&mut self) -> Result<Vec<u8>, Error>;
	fn read_fixed_bytes(&mut self, length: usize) -> Result<Vec<u8>, Error>;
	fn expect_u8(&mut self, val: u8) -> Result<u8, Error>;

	fn read_empty_bytes(&mut self, length: usize) -> Result<(), Error> {
		for _ in 0..length {
			if self.read_u8()? != 0u8 {
				return Err(err!(ErrKind::CorruptedData, "expected 0u8"));
			}
		}
		Ok(())
	}
}

pub trait Serializable
where
	Self: Sized,
{
	fn read<R: Reader>(reader: &mut R) -> Result<Self, Error>;
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error>;
}
