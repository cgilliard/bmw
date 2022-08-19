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

use crate::ser::SlabReader;
use crate::slabs::Slab;
use crate::slabs::SlabMut;
use crate::static_hash::RawHashsetIterator;
use crate::static_hash::RawHashtableIterator;
use crate::static_list::StaticListIterator;

const STANDARD_BUF_CAPACITY: usize = 1024;

info!();

/// A context which is used in many of the methods in this crate. The reason for using
/// the context is to avoid creating new Vectors, and other structures that require heap
/// allocations at run time. Instead the context may be created at startup and used
/// throughout the lifecycle of the application. The [`crate::Context`] struct may be
/// conveniently built through the [`crate::ctx`] macro.
///
/// # Examples
///
///```
/// use bmw_util::{ctx, hashtable};
/// use bmw_err::Error;
///
/// fn main() -> Result<(), Error> {
///     let ctx = ctx!(); // create a context
///     let mut h = hashtable!()?; // create a hashtable
///     h.insert(ctx, &1, &2)?; // use the context in most of the functions
///     Ok(())
/// }
///```
pub struct Context {
	pub(crate) buf1: Vec<u8>,
	pub(crate) buf2: Vec<u8>,
	pub(crate) buf3: Vec<u8>,
}

impl Context {
	pub fn new() -> Self {
		Self {
			buf1: vec![],
			buf2: vec![],
			buf3: vec![],
		}
	}

	pub fn shrink(&mut self) {
		self.buf1.shrink_to(STANDARD_BUF_CAPACITY);
		self.buf2.shrink_to(STANDARD_BUF_CAPACITY);
		self.buf3.shrink_to(STANDARD_BUF_CAPACITY);
	}
}

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
/// implementation code. In most cases, the data structures in this crate should be
/// instantiated through the macros, but they can also be instantiated through the
/// builder structs as well. See [`crate::hashtable`].
///
/// # Examples
///```
/// use bmw_err::*;
/// use bmw_log::*;
/// use bmw_util::{ctx, hashtable};
///
/// info!();
///
/// fn main() -> Result<(), Error> {
///     let ctx = ctx!();
///     let mut hash = hashtable!()?;
///
///     hash.insert(ctx, &1, &"abc".to_string())?;
///     hash.insert(ctx, &3, &"def".to_string())?;
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
	/// use bmw_util::{ctx, hashtable};
	///
	/// info!();
	///
	/// fn main() -> Result<(), Error> {
	///     let ctx = ctx!();
	///     let mut hash = hashtable!(1_000, 0.9)?;
	///
	///     hash.insert(ctx, &1, &"abc".to_string())?;
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
	/// use bmw_util::{ctx, hashtable};
	///
	/// fn main() -> Result<(), Error> {
	///     let ctx = ctx!();
	///     let mut hash = hashtable!(1_000, 0.9)?;
	///
	///     // since Vec implements Serializable, we can insert vec.
	///     hash.insert(ctx, &1, &vec![1u8,2u8,3u8,4u8])?;
	///
	///     Ok(())
	/// }
	///```
	fn insert(&mut self, ctx: &mut Context, key: &K, value: &V) -> Result<(), Error>;

	/// Get an element in the [`crate::StaticHashtable`]. A copy of the serialized value
	/// is returned or None if it is not found.
	///
	/// # Examples
	///
	///```
	/// use bmw_err::*;
	/// use bmw_util::{ctx, hashtable};
	///
	/// fn main() -> Result<(), Error> {
	///     let ctx = ctx!(); // context
	///     let mut hash = hashtable!(1_000, 0.9)?;
	///
	///     // since Vec implements Serializable, we can insert a Vec.
	///     hash.insert(ctx, &1, &vec![1u8,2u8,3u8,4u8])?;
	///     let v = hash.get(ctx, &1)?;
	///     assert_eq!(v.unwrap(), vec![1u8,2u8,3u8,4u8]);
	///     assert!(hash.get(ctx, &2)?.is_none());
	///
	///     Ok(())
	/// }
	///```
	///
	fn get(&self, ctx: &mut Context, key: &K) -> Result<Option<V>, Error>;

	/// Remove an element from the [`crate::StaticHashtable`]. Upon success, the element
	/// removed from the [`crate::StaticHashtable`] is returned. If the `key` is not found
	/// in the [`crate::StaticHashtable`], None is returned. If an error occurs,
	/// [`bmw_err::Error`] is returned.
	///
	/// # Examples
	///
	///```
	/// use bmw_util::{ctx,hashtable};
	/// use bmw_err::Error;
	///
	/// fn hashtable_remove() -> Result<(), Error> {
	///     let ctx = ctx!();
	///     let mut sh = hashtable!()?;
	///     assert_eq!(sh.get(ctx, &1)?, None);
	///     sh.insert(ctx, &1, &100)?;
	///     assert_eq!(sh.get(ctx, &1)?, Some(100));
	///     sh.remove(ctx, &1)?;
	///     assert_eq!(sh.get(ctx, &1)?, None);
	///
	///     Ok(())
	/// }
	///```
	fn remove(&mut self, ctx: &mut Context, key: &K) -> Result<Option<V>, Error>;

	/// Get the raw [`crate::Slab`] data, if it exists, in this [`crate::StaticHashtable`].
	/// On success return the slab, if the slab is not found, return None or if an error
	/// occurs, return [`bmw_err::Error`].
	///
	/// # Examples
	///```
	/// use bmw_util::{hashtable, ctx, hashtable_set_raw};
	/// use bmw_err::Error;
	/// use std::collections::hash_map::DefaultHasher;
	/// use std::hash::{Hash, Hasher};
	/// use bmw_log::usize;
	///
	/// fn raw_ex() -> Result<(), Error> {
	///     let ctx = ctx!(); // create a context
	///     let mut sh = hashtable!()?; // create a hashtable
	///     hashtable_set_raw!(ctx, sh); // set raw mode
	///
	///     // generate our own hash manually
	///     let mut hasher = DefaultHasher::new();
	///     (b"hi").hash(&mut hasher);
	///     let hash = hasher.finish();
	///
	///     // insert
	///     sh.insert_raw(ctx, b"hi", usize!(hash), b"ok")?;
	///
	///     // call get_raw to lookup our entry
	///     let slab = sh.get_raw(ctx, b"hi", usize!(hash))?.unwrap();
	///
	///     // key = 104/105 (hi), value = 111/107 (ok)
	///     assert_eq!(
	///         slab.get()[0..36],
	///         [
	///             255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
	///             0, 0, 0, 0, 0, 0, 0, 2, 104, 105, 0, 0, 0, 0, 0, 0, 0, 2, 111, 107
	///         ]
	///     );
	///
	///     Ok(())
	/// }
	///```
	fn get_raw<'b>(
		&'b self,
		ctx: &mut Context,
		key: &[u8],
		hash: usize,
	) -> Result<Option<Slab<'b>>, Error>;

	/// Insert raw data into the [`crate::StaticHashtable`]. On success returns Ok(()), on
	/// failure returns [`bmw_err::Error`].
	///
	/// # Examples
	///```
	/// use bmw_util::{hashtable, ctx, hashtable_set_raw};
	/// use bmw_err::Error;
	/// use std::collections::hash_map::DefaultHasher;
	/// use std::hash::{Hash, Hasher};
	/// use bmw_log::usize;
	///
	/// fn raw_ex() -> Result<(), Error> {
	///     let ctx = ctx!(); // create a context
	///     let mut sh = hashtable!()?; // create a hashtable
	///     hashtable_set_raw!(ctx, sh); // set raw mode
	///
	///     // generate our own hash manually
	///     let mut hasher = DefaultHasher::new();
	///     (b"hi").hash(&mut hasher);
	///     let hash = hasher.finish();
	///
	///     // insert
	///     sh.insert_raw(ctx, b"hi", usize!(hash), b"ok")?;
	///
	///     // call get_raw to lookup our entry
	///     let slab = sh.get_raw(ctx, b"hi", usize!(hash))?.unwrap();
	///
	///     // key = 104/105 (hi), value = 111/107 (ok)
	///     assert_eq!(
	///         slab.get()[0..36],
	///         [
	///             255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
	///             0, 0, 0, 0, 0, 0, 0, 2, 104, 105, 0, 0, 0, 0, 0, 0, 0, 2, 111, 107
	///         ]
	///     );
	///
	///     Ok(())
	/// }
	///```
	fn insert_raw(
		&mut self,
		ctx: &mut Context,
		key: &[u8],
		hash: usize,
		value: &[u8],
	) -> Result<(), Error>;

	/// Remove raw data from the [`crate::StaticHashtable`]. On success returns Ok(bool), on
	/// failure returns [`bmw_err::Error`]. If the element is found, true is returned,
	/// otherwise, false.
	///
	/// # Examples
	///```
	/// use bmw_util::{hashtable, ctx, hashtable_set_raw};
	/// use bmw_err::Error;
	/// use std::collections::hash_map::DefaultHasher;
	/// use std::hash::{Hash, Hasher};
	/// use bmw_log::usize;
	///
	/// fn raw_ex() -> Result<(), Error> {
	///     let ctx = ctx!(); // create a context
	///     let mut sh = hashtable!()?; // create a hashtable
	///     hashtable_set_raw!(ctx, sh); // set raw mode
	///
	///     // generate our own hash manually
	///     let mut hasher = DefaultHasher::new();
	///     (b"hi").hash(&mut hasher);
	///     let hash = hasher.finish();
	///
	///     // insert
	///     sh.insert_raw(ctx, b"hi", usize!(hash), b"ok")?;
	///
	///     sh.remove_raw(ctx, b"hi", usize!(hash))?;
	///     // call get_raw to lookup our entry which is no longer in the table
	///     let slab = sh.get_raw(ctx, b"hi", usize!(hash))?;
	///     assert!(slab.is_none());
	///
	///     Ok(())
	/// }
	///```
	fn remove_raw(&mut self, ctx: &mut Context, key: &[u8], hash: usize) -> Result<bool, Error>;

	/// Returns an iterator for iterating through raw data in this [`crate::StaticHashtable`].
	/// This is distinct from the iterator that is implemented as [`crate::StaticHashtable`]'s
	/// [`std::iter::IntoIterator`] trait which returns the serialized and not raw data.
	/// An understanding of the structure of the raw data in the [`crate::StaticHashtable`]'s
	/// slabs is necessary to use this function.
	///
	/// # Examples
	///
	///```
	/// use bmw_err::*;
	/// use bmw_util::{hashtable, hashtable_set_raw, ctx};
	/// use bmw_log::*;
	///
	/// info!();
	///
	/// fn test() -> Result<(), Error> {
	///     // create a context
	///     let ctx = ctx!();
	///     // create a hashtable
	///     let mut hashtable = hashtable!()?;
	///     // need to do a empty insert so that the types can be inferred. From here on we use raw
	///     // operations. The hashtable_set_raw macro handles this.
	///     hashtable_set_raw!(ctx, hashtable);
	///     let key = [0u8, 1u8, 123u8];
	///     let value = [10u8];
	///     let hash = 123usize;
	///     hashtable.insert_raw(ctx, &key, hash, &value)?;
	///
	///     let key = [1u8, 1u8, 125u8];
	///     let value = [14u8];
	///     let hash = 125usize;
	///     hashtable.insert_raw(ctx, &key, hash, &value)?;
	///
	///     let mut count = 0;
	///     for slab in hashtable.iter_raw(ctx) {
	///         info!("slab={:?}", slab.get())?;
	///         count += 1;
	///     }
	///
	///     assert_eq!(count, 3);
	///
	///     Ok(())
	/// }
	///```
	fn iter_raw<'b>(&'b self, ctx: &mut Context) -> RawHashtableIterator<'b>;

	/// Return the current size of this [`crate::StaticHashtable`].
	fn size(&self, ctx: &mut Context) -> usize;

	/// Return the slab id of the first entry in this [`crate::StaticHashtable`].
	/// note: this function is mostly used internally by iterators.
	fn first_entry(&self, ctx: &mut Context) -> usize;

	/// Return this [`crate::Slab`] associated with this id.
	/// note: this function is mostly used internally by iterators.
	fn slab<'b>(&'b self, ctx: &mut Context, id: usize) -> Result<Slab<'b>, Error>;

	/// Return the key value pair associated with this slab id in serialized form.
	fn read_kv(&self, ctx: &mut Context, slab_id: usize) -> Result<(K, V), Error>;

	/// Return an immutable reference to the underlying array backing this
	/// [`crate::StaticHashtable`].
	/// note: this function is mostly used internally by iterators.
	fn get_array(&self, ctx: &mut Context) -> &Vec<usize>;

	/// Clear all elements in this [`crate::StaticHashtable`] and free any slabs
	/// that were associated with this table.
	fn clear(&mut self, ctx: &mut Context) -> Result<(), Error>;
}

/// The [`crate::StaticHashset`] trait defines the public interface to the
/// static hashset. The hashset in this crate uses linear probing to handle
/// collisions and it cannot be resized after it is intialized. Configuration of the
/// hashset is done via the [`crate::StaticHashsetConfig`] struct. The shared
/// implementation can be instantiated as a [`crate::StaticHashset`] through the
/// [`crate::StaticHashsetBuilder::build`] function or as a [`crate::StaticHashtable`]
/// through the [`crate::StaticHashtableBuilder::build`] function. Although there
/// is a different interface for each, they are very similar and share most of the
/// implementation code. In most cases, the data structures in this crate should be
/// instantiated through the macros, but they can also be instantiated through the
/// builder structs as well. See [`crate::hashset`].
///
/// # Examples
///```
/// use bmw_err::*;
/// use bmw_log::*;
/// use bmw_util::{ctx, hashset};
///
/// info!();
///
/// fn main() -> Result<(), Error> {
///     let ctx = ctx!();
///     let mut hash = hashset!()?;
///
///     hash.insert(ctx, &1)?;
///     hash.insert(ctx, &3)?;
///
///     for k in &hash {
///         info!("k={}", k)?;
///     }
///
///     Ok(())
/// }
///```
pub trait StaticHashset<K>
where
	K: Serializable + Hash,
{
	/// This function returns a copy of the underlying [`crate::StaticHashsetConfig`]
	/// struct.
	///
	/// # Examples
	///```
	/// use bmw_err::*;
	/// use bmw_log::*;
	/// use bmw_util::{ctx, hashset};
	///
	/// info!();
	///
	/// fn main() -> Result<(), Error> {
	///     let ctx = ctx!();
	///     let mut hash = hashset!(1_000, 0.9)?;
	///
	///     hash.insert(ctx, &1)?;
	///     let config = hash.config();
	///     assert_eq!(config.max_entries, 1_000);
	///     assert_eq!(config.max_load_factor, 0.9);
	///
	///     Ok(())
	/// }
	///```
	fn config(&self) -> StaticHashsetConfig;

	/// This function inserts the `key` specified into the [`crate::StaticHashset`]. `key` must implement the
	/// [`crate::Serializable`] trait and the [`std::hash::Hash`] trait. The key is copied into the set.
	///
	/// # Examples
	///```
	/// use bmw_err::*;
	/// use bmw_util::{ctx, hashset};
	/// use bmw_log::*;
	///
	/// info!();
	///
	/// fn main() -> Result<(), Error> {
	///     let ctx = ctx!();
	///     let mut hash = hashset!(1_000, 0.9)?;
	///
	///     hash.insert(ctx, &1)?;
	///     hash.insert(ctx, &2)?;
	///
	///     for k in &hash {
	///         info!("k={}", k)?;
	///     }
	///
	///     Ok(())
	/// }
	///```
	fn insert(&mut self, ctx: &mut Context, key: &K) -> Result<(), Error>;

	/// This function returns true if the `key` specified is a member of the [`crate::StaticHashset`].
	/// Otherwise, false is returned. In the event of an error a [`bmw_err::Error`] is
	/// returned.
	///
	/// # Examples
	///```
	/// use bmw_err::*;
	/// use bmw_util::{ctx, hashset};
	/// use bmw_log::*;
	///
	/// info!();
	///
	/// fn main() -> Result<(), Error> {
	///     let ctx = ctx!();
	///     let mut hash = hashset!(1_000, 0.9)?;
	///
	///     hash.insert(ctx, &1)?;
	///     hash.insert(ctx, &2)?;
	///
	///     assert!(hash.contains(ctx, &1)?);
	///     assert!(hash.contains(ctx, &2)?);
	///     assert!(!hash.contains(ctx, &3)?);
	///
	///     Ok(())
	/// }
	///```
	fn contains(&self, ctx: &mut Context, key: &K) -> Result<bool, Error>;

	/// This function is the raw version of the [`crate::StaticHashset::contains`] function. It
	/// is the same, except that the key is specified as a byte array instead of a serialized
	/// structure.
	///
	///```
	/// use bmw_err::*;
	/// use bmw_util::{ctx, hashset, hashset_set_raw};
	/// use bmw_log::*;
	///
	/// info!();
	///
	/// fn main() -> Result<(), Error> {
	///     let ctx = ctx!();
	///     let mut hash = hashset!()?;
	///     hashset_set_raw!(ctx, hash);
	///
	///     hash.insert_raw(ctx, &[1], 1)?;
	///     hash.insert_raw(ctx, &[2], 2)?;
	///
	///     assert!(hash.contains_raw(ctx, &[1], 1)?);
	///     assert!(hash.contains_raw(ctx, &[2], 2)?);
	///     assert!(!hash.contains_raw(ctx, &[3], 3)?);
	///
	///     Ok(())
	/// }
	///```
	fn contains_raw(&self, ctx: &mut Context, key: &[u8], hash: usize) -> Result<bool, Error>;

	/// Remove an element from the [`crate::StaticHashset`]. Upon success, the element
	/// is removed from the [`crate::StaticHashset`]. If the `key` was found and deleted,
	/// true is returned. Otherwise, false is returned. If an error occurs, [`bmw_err::Error`]
	/// is returned.
	///
	/// # Examples
	///
	///```
	/// use bmw_util::{ctx,hashset};
	/// use bmw_err::Error;
	///
	/// fn hashset() -> Result<(), Error> {
	///     let ctx = ctx!();
	///     let mut hash = hashset!()?;
	///     assert_eq!(hash.contains(ctx, &1)?, false);
	///     hash.insert(ctx, &1)?;
	///     assert_eq!(hash.contains(ctx, &1)?, true);
	///     hash.remove(ctx, &1)?;
	///     assert_eq!(hash.contains(ctx, &1)?, false);
	///
	///     Ok(())
	/// }
	///```
	fn remove(&mut self, ctx: &mut Context, key: &K) -> Result<bool, Error>;

	/// Insert raw data into the [`crate::StaticHashset`]. On success returns Ok(()), on
	/// failure returns [`bmw_err::Error`].
	///
	/// # Examples
	///```
	/// use bmw_util::{hashset, ctx, hashset_set_raw};
	/// use bmw_err::Error;
	/// use std::collections::hash_map::DefaultHasher;
	/// use std::hash::{Hash, Hasher};
	/// use bmw_log::usize;
	///
	/// fn raw_ex() -> Result<(), Error> {
	///     let ctx = ctx!(); // create a context
	///     let mut sh = hashset!()?; // create a hashset
	///     hashset_set_raw!(ctx, sh); // set raw mode
	///
	///     // generate our own hash manually
	///     let mut hasher = DefaultHasher::new();
	///     (b"hi").hash(&mut hasher);
	///     let hash = hasher.finish();
	///
	///     // insert
	///     sh.insert_raw(ctx, b"hi", usize!(hash))?;
	///
	///     // call contains_raw to lookup our entry
	///     assert_eq!(sh.contains_raw(ctx, b"hi", usize!(hash))?, true);
	///     assert_eq!(sh.contains_raw(ctx, b"other", usize!(hash))?, false);
	///
	///     Ok(())
	/// }
	///```
	fn insert_raw(&mut self, ctx: &mut Context, key: &[u8], hash: usize) -> Result<(), Error>;

	/// Remove raw data from the [`crate::StaticHashset`]. On success returns Ok(bool), on
	/// failure returns [`bmw_err::Error`]. If the element is found, true is returned,
	/// otherwise, false.
	///
	/// # Examples
	///```
	/// use bmw_util::{hashset, ctx, hashset_set_raw};
	/// use bmw_err::Error;
	/// use std::collections::hash_map::DefaultHasher;
	/// use std::hash::{Hash, Hasher};
	/// use bmw_log::usize;
	///
	/// fn raw_ex() -> Result<(), Error> {
	///     let ctx = ctx!(); // create a context
	///     let mut sh = hashset!()?; // create a hashset
	///     hashset_set_raw!(ctx, sh); // set raw mode
	///
	///     // generate our own hash manually
	///     let mut hasher = DefaultHasher::new();
	///     (b"hi").hash(&mut hasher);
	///     let hash = hasher.finish();
	///
	///     // insert
	///     sh.insert_raw(ctx, b"hi", usize!(hash))?;
	///     assert!(sh.contains_raw(ctx, b"hi", usize!(hash))?);
	///     sh.remove_raw(ctx, b"hi", usize!(hash))?;
	///     // call contains_raw to lookup our entry which is no longer in the set
	///     assert!(!sh.contains_raw(ctx, b"hi", usize!(hash))?);
	///
	///
	///     Ok(())
	/// }
	fn remove_raw(&mut self, ctx: &mut Context, key: &[u8], hash: usize) -> Result<bool, Error>;

	/// Returns an iterator for iterating through raw data in this [`crate::StaticHashset`].
	/// This is distinct from the iterator that is implemented as [`crate::StaticHashset`]'s
	/// [`std::iter::IntoIterator`] trait which returns the serialized and not raw data.
	/// An understanding of the structure of the raw data in the [`crate::StaticHashset`]'s
	/// slabs is necessary to use this function.
	///
	/// # Examples
	///
	///```
	/// use bmw_err::*;
	/// use bmw_util::{hashset, hashset_set_raw, ctx};
	/// use bmw_log::*;
	///
	/// info!();
	///
	/// fn test() -> Result<(), Error> {
	///     // create a context
	///     let ctx = ctx!();
	///     // create a hashset
	///     let mut hashset = hashset!()?;
	///     // need to do a empty insert so that the types can be inferred. From here on we use raw
	///     // operations. The hashset_set_raw macro handles this.
	///     hashset_set_raw!(ctx, hashset);
	///     let key = [0u8, 1u8, 123u8];
	///     let hash = 123usize;
	///     hashset.insert_raw(ctx, &key, hash)?;
	///
	///     let key = [1u8, 1u8, 125u8];
	///     let hash = 125usize;
	///     hashset.insert_raw(ctx, &key, hash)?;
	///
	///     let mut count = 0;
	///     for slab in hashset.iter_raw(ctx) {
	///         info!("slab={:?}", slab.get())?;
	///         count += 1;
	///     }
	///
	///     assert_eq!(count, 3);
	///
	///     Ok(())
	/// }
	///```
	fn iter_raw<'b>(&'b self, ctx: &mut Context) -> RawHashsetIterator<'b>;

	/// Return the current size of this [`crate::StaticHashset`].
	fn size(&self, ctx: &mut Context) -> usize;

	/// Return the slab id of the first entry in this [`crate::StaticHashset`].
	/// note: this function is mostly used internally by iterators.
	fn first_entry(&self, ctx: &mut Context) -> usize;

	/// Return this [`crate::Slab`] associated with this id.
	/// note: this function is mostly used internally by iterators.
	fn slab<'b>(&'b self, ctx: &mut Context, id: usize) -> Result<Slab<'b>, Error>;

	/// Return the key value associated with this slab id in serialized form.
	fn read_k(&self, ctx: &mut Context, slab_id: usize) -> Result<K, Error>;

	/// Return an immutable reference to the underlying array backing this
	/// [`crate::StaticHashset`].
	/// note: this function is mostly used internally by iterators.
	fn get_array(&self, ctx: &mut Context) -> &Vec<usize>;

	/// Clear all elements in this [`crate::StaticHashset`] and free any slabs
	/// that were associated with this set.
	fn clear(&mut self, ctx: &mut Context) -> Result<(), Error>;
}

/// TODO: not implemented
pub trait StaticQueue<V>
where
	V: Serializable,
{
	fn enqueue(&mut self, value: &V) -> Result<(), Error>;
	fn dequeue(&mut self) -> Result<Option<&V>, Error>;
	fn peek(&self) -> Result<Option<&V>, Error>;
}

/// TODO: not implemented
pub trait StaticStack<V>
where
	V: Serializable,
{
	fn push(&mut self, value: &V) -> Result<(), Error>;
	fn pop(&mut self) -> Result<Option<V>, Error>;
	fn peek(&self) -> Result<Option<V>, Error>;
}

#[derive(Debug, Clone)]
pub struct StaticListConfig {}

pub trait StaticList<V>
where
	V: Serializable,
{
	fn config(&self) -> StaticListConfig;
	fn push(&mut self, value: &V) -> Result<(), Error>;
	fn pop(&mut self) -> Result<Option<V>, Error>;
	fn push_front(&mut self, value: &V) -> Result<(), Error>;
	fn pop_front(&mut self) -> Result<Option<V>, Error>;
	fn iter<'a>(&'a self) -> StaticListIterator<'a, V>;
	fn iter_rev<'a>(&'a self) -> StaticListIterator<'a, V>;
	fn size(&self) -> usize;
	fn clear(&mut self) -> Result<(), Error>;
	fn head(&self) -> usize;
	fn tail(&self) -> usize;
	fn slab(&self, slab_id: usize) -> Result<Slab, Error>;
	fn swap(&mut self, slab_id1: usize, slab_id2: usize) -> Result<(), Error>;
	fn merge(&mut self, ptr1: usize, ptr2: usize) -> Result<(), Error>;
	fn ptr_size(&self) -> usize;
	fn get_reader(&mut self, slab_id: usize) -> Result<SlabReader, Error>;
	fn max_value(&self) -> usize;
}

pub trait SortableList<V>: StaticList<V>
where
	V: Serializable + Ord + Debug,
{
	fn sort(&mut self) -> Result<(), Error>;
}

/// TODO: not implemented
pub trait Array<V>
where
	V: Serializable,
{
}

/// TODO: not implemented
pub trait BitVec {}

/// TODO: not implemented
pub trait ThreadPool {
	fn execute<F>(&self, f: F) -> Result<(), Error>
	where
		F: Future<Output = Result<(), Error>> + Send + Sync + 'static;
}

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
	fn allocate<'a>(&'a mut self) -> Result<SlabMut<'a>, Error>;

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
	fn get<'a>(&'a self, id: usize) -> Result<Slab<'a>, Error>;

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
	fn get_mut<'a>(&'a mut self, id: usize) -> Result<SlabMut<'a>, Error>;

	/// Returns the number of free slabs this [`crate::SlabAllocator`] has remaining.
	fn free_count(&self) -> Result<usize, Error>;

	/// Returns the configured `slab_size` for this [`crate::SlabAllocator`].
	fn slab_size(&self) -> Result<usize, Error>;

	/// Returns the configured `slab_count` for this [`crate::SlabAllocator`].
	fn slab_count(&self) -> Result<usize, Error>;

	/// Initializes the [`crate::SlabAllocator`] with the given `config`. See
	/// [`crate::SlabAllocatorConfig`] for further details.
	fn init(&mut self, config: SlabAllocatorConfig) -> Result<(), Error>;
}

/// TODO: not implemented
pub trait Match {
	fn start(&self) -> usize;
	fn end(&self) -> usize;
	fn id(&self) -> u128;
	fn set_start(&mut self, start: usize) -> Result<(), Error>;
	fn set_end(&mut self, end: usize) -> Result<(), Error>;
	fn set_id(&mut self, id: u128) -> Result<(), Error>;
}

/// TODO: not implemented
pub trait Pattern {
	fn regex(&self) -> String;
	fn is_case_sensitive(&self) -> bool;
	fn is_termination_pattern(&self) -> bool;
	fn id(&self) -> u128;
}

/// TODO: not implemented
pub trait SuffixTree {
	fn add_pattern(&mut self, pattern: &dyn Pattern) -> Result<(), Error>;
	fn run_matches(
		&mut self,
		text: &[u8],
		matches: &mut Vec<Box<dyn Match>>,
	) -> Result<usize, Error>;
}

/// Writer trait used to serializing data.
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
		for _ in 0..length {
			self.write_u8(0)?;
		}
		Ok(())
	}
}

/// Reader trait used for deserializing data.
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
	fn read_fixed_bytes(&mut self, buf: &mut [u8]) -> Result<(), Error>;
	fn read_usize(&mut self) -> Result<usize, Error>;
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

/// This is the trait used by all data structures to serialize and deserialize data.
/// Anthing stored in them must implement this trait. Commonly needed implementations
/// are built in the ser module in this crate. These include Vec, String, integer types among
/// other things.
pub trait Serializable
where
	Self: Sized,
{
	/// read data from the reader and build the underlying type represented by that
	/// data.
	fn read<R: Reader>(reader: &mut R) -> Result<Self, Error>;
	/// write data to the writer representing the underlying type.
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error>;
}
