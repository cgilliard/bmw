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

/// Macro to initialize the global slab allocator. It is important to note that
/// this macro only affects the thread in which it is executed in and must be called
/// separately in each thread that you wish to initialize the global slab allocator
/// in. Also, the data structures, like [`crate::StaticHashtable`] and
/// [`crate::StaticHashset`], will initialized this slab allocator with default values
/// if this macro is not called first. Therefore, it makes sense to call this very soon
/// after starting a thread that will use it. The slab allocator is initialized with
/// `slab_size` and `slab_count` parameters respecitvely. `slab_size` is the size in bytes
/// of slabs. The default value is 1_024 bytes. `slab_count` is the number of slabs to
/// initialize. The default value is 10_240. The defaults will be used if this macro is
/// not called.
///
/// # Errors
///
/// [`init_slab_allocator`] returns a Ok(()) on success or returns an [`bmw_err::Error`]
/// if an error occurs.
///
/// * [`bmw_err::ErrorKind::IllegalArgument`] if the `slab_size` is less than 48 bytes or
/// if `slab_count` is equal to 0.
///
/// * [`bmw_err::ErrorKind::IllegalState`] if the thread local global slab allocator for this
/// thread has already been initialized.
///
/// # Examples
///```
/// use bmw_err::*;
/// use bmw_util::{hashtable, init_slab_allocator, SlabAllocatorConfig};
///
/// fn main() -> Result<(), Error> {
///     // initialize the slab allocator for this thread with 25_000 128 byte entries
///     init_slab_allocator!(128, 25_000);
///     // The hashtable will use this default thread local slab allocator
///     let mut hashtable = hashtable!()?;
///     hashtable.insert(&1, &2)?;
///     // ...
///
///     Ok(())
/// }
///```
///
/// See [`crate::slab_allocator`] for details on how to create a new slab allocator as opposed
/// to the global thread local slab allocator.
///
#[macro_export]
macro_rules! init_slab_allocator {
	($slab_size:expr, $slab_count:expr) => {{
		bmw_util::GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(), Error> {
			unsafe {
				f.get().as_mut().unwrap().init(SlabAllocatorConfig {
					slab_count: $slab_count,
					slab_size: $slab_size,
					..Default::default()
				})?;
				Ok(())
			}
		})?;
	}};
}

/// The [`crate::slab_allocator`] macro allows for creation of a slab allocator that may be used
/// by the data structures in this crate. If no parameters are specified, the default values of
/// `slab_size` equal to 1_024 and `slab_count` of 10_240 are used. If one paramater is specifed,
/// it is used as `slab_count` and the default is used for `slab_size`. If two parameters are
/// specified, it is used as `slab_count` and `slab_size` respectively. The `slab_count` is
/// the total number of slabs in this slab allocator. It is important to note that additional
/// slabs may not be added after startup. `The slab_size` is the size in bytes of the slabs
/// in this instance.
///
/// # Errors
///
/// [`crate::slab_allocator`] returns a [`crate::SlabAllocator`] on success. On error,
/// a [`bmw_err::Error`] is returned.
///
/// * [`bmw_err::ErrorKind::IllegalArgument`] if the `slab_size` is less than 48 bytes or
/// if `slab_count` is equal to 0.
///
/// * [`bmw_err::ErrorKind::IllegalState`] if the thread local global slab allocator for this
/// thread has already been initialized.
///
/// # Examples
///
///```
/// use bmw_err::*;
/// use bmw_util::{slab_allocator, hashtable};
///
/// fn main() -> Result<(), Error> {
///     let slab_allocator1 = slab_allocator!()?;
///     let slab_allocator2 = slab_allocator!(8_000)?;
///     let slab_allocator3 = slab_allocator!(3_000, 2_048)?;
///     let mut hashtable1 = hashtable!(1_000, 0.9, slab_allocator1)?;
///     let mut hashtable2 = hashtable!(5_000, 0.8, slab_allocator2)?;
///     let mut hashtable3 = hashtable!(3_000, 0.7, slab_allocator3)?;
///
///     hashtable1.insert(&1, &2)?;
///     hashtable2.insert(&10, &20)?;
///     hashtable3.insert(&100, &200)?;
///
///     // ...
///     Ok(())
/// }
///```
///
/// Note: the slab allocator returned by this macro is distinct from the global thread
/// local slab allocator that is initialized by the [`crate::init_slab_allocator`] macro.
#[macro_export]
macro_rules! slab_allocator {
	() => {{
		let mut slabs = bmw_util::SlabAllocatorBuilder::build();
		match slabs.init(bmw_util::SlabAllocatorConfig::default()) {
			Ok(_) => Ok(slabs),
			Err(e) => Err(err!(ErrKind::Configuration, format!("{}", e))),
		}
	}};
	($slab_count:expr) => {{
		let config = bmw_util::SlabAllocatorConfig {
			slab_count: $slab_count,
			..Default::default()
		};
		let mut slabs = bmw_util::SlabAllocatorBuilder::build();
		match slabs.init(config) {
			Ok(_) => Ok(slabs),
			Err(e) => Err(err!(ErrKind::Configuration, format!("{}", e))),
		}
	}};
	($slab_count:expr, $slab_size:expr) => {{
		let config = bmw_util::SlabAllocatorConfig {
			slab_count: $slab_count,
			slab_size: $slab_size,
			..Default::default()
		};
		let mut slabs = bmw_util::SlabAllocatorBuilder::build();
		match slabs.init(config) {
			Ok(_) => Ok(slabs),
			Err(e) => Err(err!(ErrKind::Configuration, format!("{}", e))),
		}
	}};
}

/// The [`crate::hashtable`] macro is used to instantiate a [`crate::StaticHashtable`]. If no
/// parameters are specified, default values of `max_entries` = 1_000_000 and `max_load_factor` =
/// 0.75 are used. If one parameter is specified, it will be use for `max_entries`. If two
/// parameters are specified, they will be used for `max_entries` and `max_load_factor`
/// respectively. The `max_entries` value is the maximum number of entries that can exist in this
/// [`crate::StaticHashtable`]. The `max_load_factor` is the maximum load factor that
/// this [`crate::StaticHashtable`] can support. The [`crate::StaticHashtable`] will calculate
/// the size of the entry array based on these values. With a lower load factor a larger entry
/// array must be used to support `max_entries` so the lower the load factor the more resources
/// are required.
///
/// # Errors
///
/// On success [`crate::hashtable`] will return a [`crate::StaticHashtable`]. On failure,
/// a [`bmw_err::Error`] is returned.
///
/// * [`bmw_err::ErrorKind::IllegalArgument`] if the `max_load_factor` is greater than 1 or less than
/// or equal to 0 or `max_entries` if is 0.
///
/// # Examples
///```
/// use bmw_err::*;
/// use bmw_util::{hashtable, init_slab_allocator, slab_allocator, SlabAllocatorConfig};
///
/// fn main() -> Result<(), Error> {
///     // initialize the default global thread local slab allocator for this thread.
///     init_slab_allocator!(1_000_000, 64);
///
///     // intialize an additional slab allocator to be used by hashtabl4 below.
///     // The other hashtables will share the default global thread local slab allocator.
///     let slabs = slab_allocator!(100_000, 256)?;
///
///     // all default values
///     let mut hashtable1 = hashtable!()?;
///
///     // max_enries of 15,000 with default values for the rest
///     let mut hashtable2 = hashtable!(15_000)?;
///
///     // max_entries of 13,000 and max_load_factor of 0.95
///     let mut hashtable3 = hashtable!(13_000, 0.95)?;
///
///     // max_entries of 20,000, max_load_factor of 0.9 and an owned/dedicated slab allocator
///     let mut hashtable4 = hashtable!(20_000, 0.9, slabs)?;
///
///     hashtable1.insert(&1, &100)?;
///     hashtable2.insert(&2, &100)?;
///     hashtable3.insert(&3, &100)?;
///     hashtable4.insert(&4, &100)?;
///     
///
///     Ok(())
/// }
///```
#[macro_export]
macro_rules! hashtable {
	() => {{
		bmw_util::StaticHashtableBuilder::build(bmw_util::StaticHashtableConfig::default(), None)
	}};
	($max_entries:expr) => {{
		let config = bmw_util::StaticHashtableConfig {
			max_entries: $max_entries,
			..Default::default()
		};
		bmw_util::StaticHashtableBuilder::build(config, None)
	}};
	($max_entries:expr, $max_load_factor:expr) => {{
		let config = bmw_util::StaticHashtableConfig {
			max_entries: $max_entries,
			max_load_factor: $max_load_factor,
			..Default::default()
		};
		bmw_util::StaticHashtableBuilder::build(config, None)
	}};
	($max_entries:expr, $max_load_factor:expr, $slab_allocator:expr) => {{
		let config = bmw_util::StaticHashtableConfig {
			max_entries: $max_entries,
			max_load_factor: $max_load_factor,
			..Default::default()
		};
		bmw_util::StaticHashtableBuilder::build(config, Some($slab_allocator))
	}};
}

/// The [`crate::hashset`] macro is used to instantiate a [`crate::StaticHashset`]. If no
/// parameters are specified, default values of `max_entries` = 1_000_000 and `max_load_factor` =
/// 0.75 are used. If one parameter is specified, it will be use for `max_entries`. If two
/// parameters are specified, they will be used for `max_entries` and `max_load_factor`
/// respectively. The `max_entries` value is the maximum number of entries that can exist in this
/// [`crate::StaticHashset`]. The `max_load_factor` is the maximum load factor that
/// this [`crate::StaticHashset`] can support. The [`crate::StaticHashset`] will calculate
/// the size of the entry array based on these values. With a lower load factor a larger entry
/// array must be used to support `max_entries` so the lower the load factor the more resources
/// are required.
///
/// # Errors
///
/// On success [`crate::hashset`] will return a [`crate::StaticHashset`]. On failure,
/// a [`bmw_err::Error`] is returned.
///
/// * [`bmw_err::ErrorKind::IllegalArgument`] if the `max_load_factor` is greater than 1 or less than
/// or equal to 0 or `max_entries` if is 0.
///
/// # Examples
///```
/// use bmw_err::*;
/// use bmw_util::{hashset, init_slab_allocator, slab_allocator, SlabAllocatorConfig};
///
/// fn main() -> Result<(), Error> {
///     // initialize the default global thread local slab allocator for this thread.
///     init_slab_allocator!(1_000_000, 64);
///
///     // intialize an additional slab allocator to be used by hashtabl4 below.
///     // The other hashsets will share the default global thread local slab allocator.
///     let slabs = slab_allocator!(100_000, 256)?;
///
///     // all default values
///     let mut hashset1 = hashset!()?;
///
///     // max_enries of 15,000 with default values for the rest
///     let mut hashset2 = hashset!(15_000)?;
///
///     // max_entries of 13,000 and max_load_factor of 0.95
///     let mut hashset3 = hashset!(13_000, 0.95)?;
///
///     // max_entries of 20,000, max_load_factor of 0.9 and an owned/dedicated slab allocator
///     let mut hashset4 = hashset!(20_000, 0.9, slabs)?;
///
///     hashset1.insert(&1)?;
///     hashset2.insert(&2)?;
///     hashset3.insert(&3)?;
///     hashset4.insert(&4)?;
///     
///
///     Ok(())
/// }
///```
#[macro_export]
macro_rules! hashset {
	() => {{
		bmw_util::StaticHashsetBuilder::build(bmw_util::StaticHashsetConfig::default(), None)
	}};
	($max_entries:expr) => {{
		let config = bmw_util::StaticHashsetConfig {
			max_entries: $max_entries,
			..Default::default()
		};
		bmw_util::StaticHashsetBuilder::build(config, None)
	}};
	($max_entries:expr, $max_load_factor:expr) => {{
		let config = bmw_util::StaticHashsetConfig {
			max_entries: $max_entries,
			max_load_factor: $max_load_factor,
			..Default::default()
		};
		bmw_util::StaticHashsetBuilder::build(config, None)
	}};
	($max_entries:expr, $max_load_factor:expr, $slab_allocator:expr) => {{
		let config = bmw_util::StaticHashsetConfig {
			max_entries: $max_entries,
			max_load_factor: $max_load_factor,
			..Default::default()
		};
		bmw_util::StaticHashsetBuilder::build(config, Some($slab_allocator))
	}};
}

#[cfg(test)]
mod test {
	use crate as bmw_util;
	use bmw_err::*;
	use bmw_log::*;

	debug!();

	#[test]
	fn test_hashtable_macro() -> Result<(), Error> {
		let mut hash = hashtable!()?;
		hash.insert(&1, &2)?;
		assert_eq!(hash.get(&1)?.unwrap(), 2);

		let mut hash = hashtable!(10)?;
		for i in 0..10 {
			hash.insert(&i, &100)?;
		}
		assert!(hash.insert(&100, &100).is_err());

		let mut hash = hashtable!(10, 0.85)?;
		for i in 0..10 {
			hash.insert(&i, &100)?;
		}
		assert!(hash.insert(&100, &100).is_err());

		let slabs = slab_allocator!(10)?;
		let mut hash = hashtable!(100, 0.85, slabs)?;
		for i in 0..10 {
			hash.insert(&i, &100)?;
		}
		assert!(hash.insert(&100, &100).is_err());

		Ok(())
	}

	#[test]
	fn test_hashset_macro() -> Result<(), Error> {
		let mut hash = hashset!()?;
		hash.insert(&1)?;
		assert!(hash.contains(&1)?);

		let mut hash = hashset!(10)?;
		for i in 0..10 {
			hash.insert(&i)?;
		}
		assert!(hash.insert(&100).is_err());

		let mut hash = hashset!(10, 0.85)?;
		for i in 0..10 {
			hash.insert(&i)?;
		}
		assert!(hash.insert(&100).is_err());

		let slabs = slab_allocator!(10)?;
		let mut hash = hashset!(100, 0.85, slabs)?;
		for i in 0..10 {
			hash.insert(&i)?;
		}
		assert!(hash.insert(&100).is_err());

		Ok(())
	}
}
