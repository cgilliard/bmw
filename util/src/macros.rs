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
/// use bmw_util::{ctx, hashtable, init_slab_allocator, SlabAllocatorConfig};
///
/// fn main() -> Result<(), Error> {
///     // create a context
///     let ctx = ctx!();
///
///     // initialize the slab allocator for this thread with 25_000 128 byte entries
///     init_slab_allocator!(25_000, 128);
///
///     // The hashtable will use this default thread local slab allocator
///     let mut hashtable = hashtable!()?;
///
///     hashtable.insert(ctx, &1, &2)?;
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
				f.get()
					.as_mut()
					.unwrap()
					.init(bmw_util::SlabAllocatorConfig {
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
/// use bmw_util::{ctx, slab_allocator, hashtable};
///
/// fn main() -> Result<(), Error> {
///     let ctx = ctx!();
///     let slab_allocator1 = slab_allocator!()?;
///     let slab_allocator2 = slab_allocator!(8_000)?;
///     let slab_allocator3 = slab_allocator!(3_000, 2_048)?;
///     let mut hashtable1 = hashtable!(1_000, 0.9, slab_allocator1)?;
///     let mut hashtable2 = hashtable!(5_000, 0.8, slab_allocator2)?;
///     let mut hashtable3 = hashtable!(3_000, 0.7, slab_allocator3)?;
///
///     hashtable1.insert(ctx, &1, &2)?;
///     hashtable2.insert(ctx, &10, &20)?;
///     hashtable3.insert(ctx, &100, &200)?;
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
			Err(e) => Err(bmw_err::err!(
				bmw_err::ErrKind::Configuration,
				format!("{}", e)
			)),
		}
	}};
	($slab_size:expr) => {{
		let config = bmw_util::SlabAllocatorConfig {
			slab_size: $slab_size,
			..Default::default()
		};
		let mut slabs = bmw_util::SlabAllocatorBuilder::build();
		match slabs.init(config) {
			Ok(_) => Ok(slabs),
			Err(e) => Err(bmw_err::err!(
				bmw_err::ErrKind::Configuration,
				format!("{}", e)
			)),
		}
	}};
	($slab_size:expr, $slab_count:expr) => {{
		let config = bmw_util::SlabAllocatorConfig {
			slab_count: $slab_count,
			slab_size: $slab_size,
			..Default::default()
		};
		let mut slabs = bmw_util::SlabAllocatorBuilder::build();
		match slabs.init(config) {
			Ok(_) => Ok(slabs),
			Err(e) => Err(bmw_err::err!(
				bmw_err::ErrKind::Configuration,
				format!("{}", e)
			)),
		}
	}};
}
/*
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
/// use bmw_util::{ctx, hashtable, init_slab_allocator, slab_allocator, SlabAllocatorConfig};
///
/// fn main() -> Result<(), Error> {
///     // create a context
///     let ctx = ctx!();
///
///     // initialize the default global thread local slab allocator for this thread.
///     init_slab_allocator!(64, 1_000_000);
///
///     // intialize an additional slab allocator to be used by hashtabl4 below.
///     // The other hashtables will share the default global thread local slab allocator.
///     let slabs = slab_allocator!(256, 100_000)?;
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
///     hashtable1.insert(ctx, &1, &100)?;
///     hashtable2.insert(ctx, &2, &100)?;
///     hashtable3.insert(ctx, &3, &100)?;
///     hashtable4.insert(ctx, &4, &100)?;
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
/// use bmw_util::{ctx, hashset, init_slab_allocator, slab_allocator, SlabAllocatorConfig};
///
/// fn main() -> Result<(), Error> {
///     // initialize the default global thread local slab allocator for this thread.
///     init_slab_allocator!(64, 1_000_000);
///
///     // intialize an additional slab allocator to be used by hashtabl4 below.
///     // The other hashsets will share the default global thread local slab allocator.
///     let slabs = slab_allocator!(256, 100_000)?;
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
///     // get a context
///     let ctx = ctx!();
///
///     hashset1.insert(ctx, &1)?;
///     hashset2.insert(ctx, &2)?;
///     hashset3.insert(ctx, &3)?;
///     hashset4.insert(ctx, &4)?;
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

/// Macro to create a context which is used by the data structures.
///
/// # Examples
///```
///    use bmw_err::Error;
///    use bmw_util::{ctx, hashtable};
///
///    fn test() -> Result<(), Error> {
///        let ctx = ctx!();
///        let mut h = hashtable!()?;
///        h.insert(ctx, &1, &2)?;
///        let v = h.get(ctx, &1)?;
///        assert_eq!(v, Some(2));
///
///        Ok(())
///    }
///```
#[macro_export]
macro_rules! ctx {
	() => {{
		&mut bmw_util::Context::new()
	}};
}

/// This macro is used to put a hashtable into 'raw' mode. After this
/// function is called, only the 'raw' functions (i.e. [`crate::StaticHashtable::insert_raw`],
/// [`crate::StaticHashtable::get_raw`], [`crate::StaticHashtable::remove_raw`] and
/// [`crate::StaticHashtable::iter_raw`]) will return useful values.
///
/// # Examples
///
///```
///    use bmw_err::Error;
///    use bmw_util::{ctx, hashtable, hashtable_set_raw};
///    use std::collections::hash_map::DefaultHasher;
///    use std::hash::{Hash, Hasher};
///    use bmw_log::*;
///
///    info!();
///
///    fn test() -> Result<(), Error> {
///        let ctx = ctx!();
///        let mut h = hashtable!()?;
///        hashtable_set_raw!(ctx, h);
///
///        let mut hasher = DefaultHasher::new();
///        (b"test").hash(&mut hasher);
///        let hash = hasher.finish();
///        h.insert_raw(ctx, b"test", usize!(hash), b"123")?;
///
///        Ok(())
///    }
///```
#[macro_export]
macro_rules! hashtable_set_raw {
	($ctx:expr, $hashtable:expr) => {{
		let _ = $hashtable.insert($ctx, &(), &());
	}};
}

/// This macro is used to put a hashset into 'raw' mode. After this
/// function is called, only the 'raw' functions (i.e. [`crate::StaticHashset::insert_raw`],
/// [`crate::StaticHashset::contains_raw`], [`crate::StaticHashset::remove_raw`] and
/// [`crate::StaticHashset::iter_raw`]) will return useful values.
///
/// # Examples
///
///```
///    use bmw_err::Error;
///    use bmw_util::{ctx, hashset, hashset_set_raw};
///    use std::collections::hash_map::DefaultHasher;
///    use std::hash::{Hash, Hasher};
///    use bmw_log::*;
///
///    info!();
///
///    fn test() -> Result<(), Error> {
///        let ctx = ctx!();
///        let mut h = hashset!()?;
///        hashset_set_raw!(ctx, h);
///
///        let mut hasher = DefaultHasher::new();
///        (b"test").hash(&mut hasher);
///        let hash = hasher.finish();
///        h.insert_raw(ctx, b"test", usize!(hash))?;
///
///        Ok(())
///    }
///```
#[macro_export]
macro_rules! hashset_set_raw {
	($ctx:expr, $hashset:expr) => {{
		let _ = $hashset.insert($ctx, &());
	}};
}
*/
/*
#[macro_export]
macro_rules! list {
	() => {{
		bmw_util::StaticListBuilder::build(bmw_util::StaticListConfig::default(), None)?
	}};
	($slab_allocator:expr) => {{
		bmw_util::StaticListBuilder::build(
			bmw_util::StaticListConfig::default(),
			Some($slab_allocator),
		)
	}};
		( $( $x:expr ),* ) => {
		{
			let mut temp_list = bmw_util::StaticListBuilder::build(bmw_util::StaticListConfig::default(), None)?;
			$(
				temp_list.push(&$x)?;
			)*
			temp_list
		}
		};
}
*/

#[cfg(test)]
mod test {
	use crate as bmw_util;
	use bmw_err::*;
	use bmw_log::*;

	debug!();

	/*
	#[test]
	fn test_hashtable_macro() -> Result<(), Error> {
		let ctx = ctx!();
		let mut hash = hashtable!()?;
		hash.insert(ctx, &1, &2)?;
		assert_eq!(hash.get(ctx, &1)?.unwrap(), 2);

		let mut hash = hashtable!(10)?;
		for i in 0..10 {
			hash.insert(ctx, &i, &100)?;
		}
		assert!(hash.insert(ctx, &100, &100).is_err());

		let mut hash = hashtable!(10, 0.85)?;
		for i in 0..10 {
			hash.insert(ctx, &i, &100)?;
		}
		assert!(hash.insert(ctx, &100, &100).is_err());

		let slabs = slab_allocator!(1024, 10)?;
		let mut hash = hashtable!(100, 0.85, slabs)?;
		for i in 0..10 {
			hash.insert(ctx, &i, &100)?;
		}
		assert!(hash.insert(ctx, &100, &100).is_err());

		Ok(())
	}
		*/
	/*
		#[test]
		fn test_hashset_macro() -> Result<(), Error> {
			let ctx = ctx!();
			let mut hash = hashset!()?;
			hash.insert(ctx, &1)?;
			assert!(hash.contains(ctx, &1)?);

			let mut hash = hashset!(10)?;
			for i in 0..10 {
				hash.insert(ctx, &i)?;
			}
			assert!(hash.insert(ctx, &100).is_err());

			let mut hash = hashset!(10, 0.85)?;
			for i in 0..10 {
				hash.insert(ctx, &i)?;
			}
			assert!(hash.insert(ctx, &100).is_err());

			let slabs = slab_allocator!(1024, 10)?;
			let mut hash = hashset!(100, 0.85, slabs)?;
			for i in 0..10 {
				hash.insert(ctx, &i)?;
			}
			assert!(hash.insert(ctx, &100).is_err());

			Ok(())
		}
	*/

	#[test]
	fn test_list_macro() -> Result<(), Error> {
		let mut list = list!();
		list.push(&1)?;
		for x in &list {
			info!("x={}", x)?;
		}
		assert_eq!(list.pop()?, Some(1));

		let slabs = slab_allocator!(48, 10)?;
		let mut list = list!(slabs)?;
		list.push(&"str1".to_string())?;
		list.push(&"another string".to_string())?;
		list.push(&"".to_string())?;
		list.push(&"hi".to_string())?;
		list.push(&"hi".to_string())?;

		let mut count = 0;
		for x in &list {
			info!("x={:?}", x)?;
			count += 1;
		}

		assert_eq!(count, 5);
		Ok(())
	}

	#[test]
	fn test_list_braces() -> Result<(), Error> {
		let list = list![1, 2, 3];
		let mut i = 0;
		for x in &list {
			info!("list[{}]={}", i, x)?;
			i += 1;
			assert_eq!(x, i);
		}
		assert_eq!(i, 3);

		let list = list!["ok".to_string(), "abc".to_string(), "def".to_string()];
		i = 0;
		for x in &list {
			info!("list[{}]={}", i, x)?;
			i += 1;
		}

		Ok(())
	}
}
