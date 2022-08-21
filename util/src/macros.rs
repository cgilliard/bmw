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
macro_rules! init_slab_allocator {
( $( $config:expr ),* ) => {{
            let mut config = bmw_util::SlabAllocatorConfig::default();
            let mut error: Option<String> = None;
            let mut slab_size_specified = false;
            let mut slab_count_specified = false;

            // compiler sees macro as not used if it's not used in one part of the code
            // these lines make the warnings go away
            if config.slab_size == 0 { config.slab_size = 0; }
            if slab_count_specified { slab_count_specified = false; }
            if slab_size_specified { slab_size_specified = false; }
            if slab_count_specified {}
            if slab_size_specified {}
            if error.is_some() { error = None; }

            $(
                match $config {
                    bmw_util::ConfigOption::SlabSize(slab_size) => {
                        config.slab_size = slab_size;

                        if slab_size_specified {
                            error = Some("SlabSize was specified more than once!".to_string());
                        }
                        slab_size_specified = true;
                        if slab_size_specified {}

                    },
                    bmw_util::ConfigOption::SlabCount(slab_count) => {
                        config.slab_count = slab_count;

                        if slab_count_specified {
                            error = Some("SlabCount was specified more than once!".to_string());
                        }

                        slab_count_specified = true;
                        if slab_count_specified {}
                    },
                    bmw_util::ConfigOption::MaxEntries(_) => {
                        error = Some("Invalid configuration MaxEntries is not allowed for slab_allocator!".to_string());

                    }
                    bmw_util::ConfigOption::MaxLoadFactor(_) => {
                         error = Some("Invalid configuration MaxLoadFactor is not allowed for slab_allocator!".to_string());
                    }
                }
            )*
            match error {
                Some(error) => Err(bmw_err::err!(bmw_err::ErrKind::Configuration, error)),
                None => {
                        bmw_util::GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(), Error> {
                        unsafe {
                                f.get()
                                        .as_mut()
                                        .unwrap()
                                        .init(config)?;
                                Ok(())
                        }
                        })
                }
            }
        }
    }
}

#[macro_export]
macro_rules! slab_allocator {
( $( $config:expr ),* ) => {{
            let mut slabs = bmw_util::SlabAllocatorBuilder::build();
            let mut config = bmw_util::SlabAllocatorConfig::default();
            let mut error: Option<String> = None;
            let mut slab_size_specified = false;
            let mut slab_count_specified = false;

            // compiler sees macro as not used if it's not used in one part of the code
            // these lines make the warnings go away
            if config.slab_size == 0 { config.slab_size = 0; }
            if slab_count_specified { slab_count_specified = false; }
            if slab_size_specified { slab_size_specified = false; }
            if slab_count_specified {}
            if slab_size_specified {}
            if error.is_some() { error = None; }

            $(
                match $config {
                    bmw_util::ConfigOption::SlabSize(slab_size) => {
                        config.slab_size = slab_size;

                        if slab_size_specified {
                            error = Some("SlabSize was specified more than once!".to_string());
                        }
                        slab_size_specified = true;
                        if slab_size_specified {}

                    },
                    bmw_util::ConfigOption::SlabCount(slab_count) => {
                        config.slab_count = slab_count;

                        if slab_count_specified {
                            error = Some("SlabCount was specified more than once!".to_string());
                        }

                        slab_count_specified = true;
                        if slab_count_specified {}
                    },
                    bmw_util::ConfigOption::MaxEntries(_) => {
                        error = Some("Invalid configuration MaxEntries is not allowed for slab_allocator!".to_string());

                    }
                    bmw_util::ConfigOption::MaxLoadFactor(_) => {
                         error = Some("Invalid configuration MaxLoadFactor is not allowed for slab_allocator!".to_string());
                    }
                }
            )*
            match error {
                Some(error) => Err(bmw_err::err!(bmw_err::ErrKind::Configuration, error)),
                None => {
                        slabs.init(config)?;
                        Ok(slabs)},
                }
            }
      }
}

#[macro_export]
macro_rules! hashtable {
	( $( $config:expr ),* ) => {{
		let mut config = bmw_util::StaticHashtableConfig::default();
		let mut error: Option<String> = None;
		let mut max_entries_specified = false;
		let mut max_load_factor_specified = false;

		// compiler sees macro as not used if it's not used in one part of the code
		// these lines make the warnings go away
		if config.max_entries == 0 {
			config.max_entries = 0;
		}
		if max_entries_specified {
			max_entries_specified = false;
		}
                if max_load_factor_specified {
                        max_load_factor_specified = false;
                }
                if max_load_factor_specified {}
                if max_entries_specified {}
                if error.is_some() {
			error = None;
		}

                $(
                match $config {
                    bmw_util::ConfigOption::SlabSize(_) => {
                        error = Some("Invalid configuration SlabSize is not allowed for hashtable!".to_string());

                    },
                    bmw_util::ConfigOption::SlabCount(_) => {
                        error = Some("Invalid configuration SlabCount is not allowed for hashtable!".to_string());
                    },
                    bmw_util::ConfigOption::MaxEntries(max_entries) => {
                        config.max_entries = max_entries;

                        if max_entries_specified {
                            error = Some("MaxEntries was specified more than once!".to_string());
                        }

                        max_entries_specified = true;
                        if max_entries_specified {}
                    }
                    bmw_util::ConfigOption::MaxLoadFactor(max_load_factor) => {
                        config.max_load_factor = max_load_factor;

                        if max_load_factor_specified {
                            error = Some("MaxLoadFactor was specified more than once!".to_string());
                        }

                        max_load_factor_specified = true;
                        if max_load_factor_specified {}
                    }
                }
                )*

                match error {
                    Some(error) => Err(bmw_err::err!(bmw_err::ErrKind::Configuration, error)),
                    None => bmw_util::StaticBuilder::build_hashtable(config, None),
                }
	}};
}

#[macro_export]
macro_rules! hashset {
	( $( $config:expr ),* ) => {{
		let mut config = bmw_util::StaticHashsetConfig::default();
		let mut error: Option<String> = None;
		let mut max_entries_specified = false;
		let mut max_load_factor_specified = false;

		// compiler sees macro as not used if it's not used in one part of the code
		// these lines make the warnings go away
		if config.max_entries == 0 {
			config.max_entries = 0;
		}
		if max_entries_specified {
			max_entries_specified = false;
		}
                if max_load_factor_specified {
                        max_load_factor_specified = false;
                }
                if max_load_factor_specified {}
                if max_entries_specified {}
                if error.is_some() {
			error = None;
		}

                $(
                match $config {
                    bmw_util::ConfigOption::SlabSize(_) => {
                        error = Some("Invalid configuration SlabSize is not allowed for hashset!".to_string());

                    },
                    bmw_util::ConfigOption::SlabCount(_) => {
                        error = Some("Invalid configuration SlabCount is not allowed for hashset!".to_string());
                    },
                    bmw_util::ConfigOption::MaxEntries(max_entries) => {
                        config.max_entries = max_entries;

                        if max_entries_specified {
                            error = Some("MaxEntries was specified more than once!".to_string());
                        }

                        max_entries_specified = true;
                        if max_entries_specified {}
                    }
                    bmw_util::ConfigOption::MaxLoadFactor(max_load_factor) => {
                        config.max_load_factor = max_load_factor;

                        if max_load_factor_specified {
                            error = Some("MaxLoadFactor was specified more than once!".to_string());
                        }

                        max_load_factor_specified = true;
                        if max_load_factor_specified {}
                    }
                }
                )*

                match error {
                    Some(error) => Err(bmw_err::err!(bmw_err::ErrKind::Configuration, error)),
                    None => bmw_util::StaticBuilder::build_hashset(config, None),
                }
	}};
}

#[macro_export]
macro_rules! list {
    ( $( $x:expr ),* ) => {
        {
            let mut temp_list = bmw_util::StaticBuilder::build_list(bmw_util::StaticListConfig::default(), None)?;
            $(
                temp_list.push($x)?;
            )*
            temp_list
        }
    };
}

#[cfg(test)]
mod test {
	use crate as bmw_util;
	use crate::{StaticHashset, StaticHashtable, StaticList};
	use bmw_log::*;
	use bmw_util::ConfigOption::*;

	info!();

	#[test]
	fn test_slab_allocator_macro() -> Result<(), bmw_err::Error> {
		let mut slabs = slab_allocator!()?;
		let mut slabs2 = slab_allocator!(SlabSize(128), SlabCount(1))?;

		let slab = slabs.allocate()?;
		assert_eq!(
			slab.get().len(),
			bmw_util::SlabAllocatorConfig::default().slab_size
		);
		let slab = slabs2.allocate()?;
		assert_eq!(slab.get().len(), 128);
		assert!(slabs2.allocate().is_err());
		assert!(slabs.allocate().is_ok());

		assert!(slab_allocator!(SlabSize(128), SlabSize(64)).is_err());
		assert!(slab_allocator!(SlabCount(128), SlabCount(64)).is_err());
		assert!(slab_allocator!(MaxEntries(128)).is_err());
		assert!(slab_allocator!(MaxLoadFactor(128.0)).is_err());
		Ok(())
	}

	#[test]
	fn test_hashtable_macro() -> Result<(), bmw_err::Error> {
		let mut hashtable = hashtable!()?;
		hashtable.insert(&1, &2)?;
		assert_eq!(hashtable.get(&1).unwrap().unwrap(), 2);
		let mut hashtable = hashtable!(MaxEntries(100), MaxLoadFactor(0.9))?;
		hashtable.insert(&"test".to_string(), &1)?;
		assert_eq!(hashtable.size(), 1);
		Ok(())
	}

	#[test]
	fn test_hashset_macro() -> Result<(), bmw_err::Error> {
		let mut hashset = hashset!()?;
		hashset.insert(&1)?;
		assert_eq!(hashset.contains(&1).unwrap(), true);
		assert_eq!(hashset.contains(&2).unwrap(), false);
		let mut hashset = hashset!(MaxEntries(100), MaxLoadFactor(0.9))?;
		hashset.insert(&"test".to_string())?;
		assert_eq!(hashset.size(), 1);
		assert!(hashset.contains(&"test".to_string())?);
		Ok(())
	}

	#[test]
	fn test_list_macro() -> Result<(), bmw_err::Error> {
		let mut list1 = list!['1', '2', '3'];
		let list2 = list!['a', 'b', 'c'];
		list1.append(&list2)?;
		info!("list={:?}", list1)?;
		assert_eq!(list1, list!['a', 'b', 'c', '1', '2', '3']);
		assert_ne!(list1, list!['a', 'b', 'c', '1', '2']);

		let list3 = list![1, 2, 3, 4, 5];
		info!("list={:?}", list3)?;
		Ok(())
	}
}
