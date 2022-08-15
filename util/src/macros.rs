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

#[macro_export]
macro_rules! slab_allocator {
	() => {{
		let mut slabs = bmw_util::SlabAllocatorBuilder::build();
		match slabs.init(bmw_util::SlabAllocatorConfig::default())? {
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
