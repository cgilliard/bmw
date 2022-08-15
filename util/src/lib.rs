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

//! This utility crate provides data structures and other utilities used in bmw.
//! Currently, [`crate::hashtable`] and [`crate::hashset`] data structures
//! are supported. Both use a [`crate::SlabAllocator`] to allocate memory.
//! By default, the data structures use a global shared thread local slab allocator, but
//! if multi-threaded applications are needed, a dedicated thread-safe,
//! slab allocator may be specified. It is important to note that if the
//! default slab allocator is used (i.e. if you just call the
//! [`crate::hashtable`] macro without specifying a slab allocator, the values
//! stored in the data structure will be unique to each thread.
//!
//! # Motivation
//!
//! The advantage of these implementations is that they do not allocate memory on the
//! heap after initialization of the data structure. So, we can create a [`crate::hashtable`]
//! or a [`crate::hashset`] and do millions of inserts, gets, removes and iterations and
//! no heap memory will be allocated or deallocated other than some ephemeral data
//! structures used to do serialization. If the 'raw' versions of these functions are called,
//! no heap allocations are done for serialization either. Note: there are some minor exceptions
//! since things like the [`crate::Slab`] are returned in a Box which is stored on the heap, but
//! those data structures are just pointers so they are small and quickly allocated and deallocated
//! and don't pose any concerns. Dynamic heap allocations that are long-lived can cause
//! substantial problems and these data structures are intended to alleviate those problems.
//! The [`core::ops::Drop`] trait is also implemented so all slabs used by the data structure
//! are freed upon the data structure going out of scope.
//!
//! # Performance
//!
//! These data structures will never be as fast as the native Rust data structures because they
//! require serialization and deserialization of the entries on each operation. However, the
//! peformance is at least in the ballpark of the standard data structures. A performance tool
//! is included in the project in the etc directory
//! [hash_perf](https://github.com/37miners/bmw/tree/main/etc/hash_perf).
//!
//! Below is the output of some runs on a linux machine. All runs use get_raw/insert_raw on small
//! pieces of data.
//!
//! First, static hashtable 1000 inserts, and no gets. Performance is fairly close.
//!
//!```text
//! # ./target/release/hash_perf --do_static --count 1000 --size 2000 --slab_count 1000 --slab_size 64 --no_gets
//! [2022-08-14 16:44:19.387]: (INFO) Starting tests
//! [2022-08-14 16:44:19.458]: (INFO) Memory used (pre_drop) = 43.116107mb
//! [2022-08-14 16:44:19.458]: (INFO) Memory Allocated (pre_drop) = 66.940703mb
//! [2022-08-14 16:44:19.458]: (INFO) Memory De-allocated (pre_drop) = 23.827242mb
//! [2022-08-14 16:44:19.459]: (INFO) Memory used (post_drop) = 43.011571mb
//! [2022-08-14 16:44:19.459]: (INFO) Memory Allocated (post_drop) = 67.020615mb
//! [2022-08-14 16:44:19.459]: (INFO) Memory De-allocated (post_drop) = 24.011692mb
//! [2022-08-14 16:44:19.459]: (INFO) (StaticHash) Elapsed time = 0.51ms
//! ```
//!
//! With rust's standard hashmap for comparison.
//!
//!```text
//! # ./target/release/hash_perf --do_hash --count 1000 --no_gets
//! [2022-08-14 16:45:38.840]: (INFO) Starting tests
//! [2022-08-14 16:45:38.912]: (INFO) Memory used (pre_drop) = 43.062101mb
//! [2022-08-14 16:45:38.912]: (INFO) Memory Allocated (pre_drop) = 66.858088mb
//! [2022-08-14 16:45:38.912]: (INFO) Memory De-allocated (pre_drop) = 23.798633mb
//! [2022-08-14 16:45:38.912]: (INFO) Memory used (post drop) = 43.010885mb
//! [2022-08-14 16:45:38.912]: (INFO) Memory Allocated (post_drop) = 66.866024mb
//! [2022-08-14 16:45:38.912]: (INFO) Memory De-allocated (post_drop) = 23.857787mb
//! [2022-08-14 16:45:38.912]: (INFO) (HashMap) Elapsed time = 0.33ms
//!```
//!
//! With 100 gets per insert on static hashtable.
//!
//!```text
//! # ./target/release/hash_perf --do_static --count 1000 --size 2000 --slab_count 1000 --slab_size 64
//! [2022-08-14 16:47:04.933]: (INFO) Starting tests
//! [2022-08-14 16:47:05.014]: (INFO) Memory used (pre_drop) = 43.116075mb
//! [2022-08-14 16:47:05.014]: (INFO) Memory Allocated (pre_drop) = 71.5672mb
//! [2022-08-14 16:47:05.014]: (INFO) Memory De-allocated (pre_drop) = 28.453781mb
//! [2022-08-14 16:47:05.014]: (INFO) Memory used (post_drop) = 43.011539mb
//! [2022-08-14 16:47:05.014]: (INFO) Memory Allocated (post_drop) = 71.647142mb
//! [2022-08-14 16:47:05.014]: (INFO) Memory De-allocated (post_drop) = 28.638261mb
//! [2022-08-14 16:47:05.014]: (INFO) (StaticHash) Elapsed time = 9.49ms
//!```
//!
//! With rust's standard hashmap for comparison.
//!
//!```text
//! # ./target/release/hash_perf --do_hash --count 1000 --size 2000
//! [2022-08-14 16:47:16.677]: (INFO) Starting tests
//! [2022-08-14 16:47:16.750]: (INFO) Memory used (pre_drop) = 43.062201mb
//! [2022-08-14 16:47:16.750]: (INFO) Memory Allocated (pre_drop) = 66.858245mb
//! [2022-08-14 16:47:16.750]: (INFO) Memory De-allocated (pre_drop) = 23.79869mb
//! [2022-08-14 16:47:16.750]: (INFO) Memory used (post drop) = 43.010985mb
//! [2022-08-14 16:47:16.750]: (INFO) Memory Allocated (post_drop) = 66.866181mb
//! [2022-08-14 16:47:16.750]: (INFO) Memory De-allocated (post_drop) = 23.857844mb
//! [2022-08-14 16:47:16.750]: (INFO) (HashMap) Elapsed time = 2.06ms
//! ```
//!
//! So, as seen, the performance is somewhat comparable and keep in mind, that these operations are
//! being done in the sub millisecond time frame, so it's very unlikely that this will ultimately
//! impact the performance of the application unless it's an already very optimized application
//! that needs to be very scalable. It's important to note that the "non-raw" versions of these
//! operations are substantially slower, but if performance is needed in parts of the application,
//! raw access can be used and when performance doesn't matter as much the regular/convenient
//! serialization functions can be used. Mix and match is possible.
//!
//! # Use cases
//!
//! The main use case for these data structures is in server applications to avoid making dynamic
//! heap allocations, but they also offer some other interesting properties. For instance, with
//! the standard rust collections, the entries in the hashtable are just references so they must
//! stay in scope while they are in the hashtable. With this implementation, that is not required.
//! The inserted items can be dropped and they will remain in the hashtable/hashset. Also,
//! [`crate::StaticHashtable`] and [`crate::StaticHashset`] both implement the
//! [`crate::Serializable`] trait so they can be sent from one part of an app to another or even
//! sent over the network.
//!
//! # Examples
//!
//!```
//! use bmw_err::*; // errors
//! use bmw_log::*; // logging
//! use bmw_util::hashtable; // use hashtable macro
//!
//! info!(); // log at info level in this example
//!
//! fn test() -> Result<(), Error> {
//!     let mut hash = hashtable!()?; // hashtable with default settings
//!
//!     hash.insert(&1, &2)?; // insert 1 -> 2 (default is i32)
//!     assert_eq!(hash.get(&1)?.unwrap(), 2); // confirm that 1 is in the table
//!     hash.remove(&1)?; // remove
//!     assert!(hash.get(&1)?.is_none()); // confirm that 1 has been removed
//!
//!     Ok(())
//! }
//!
//! fn test_advanced() -> Result<(), Error> {
//!     // create a hashtable with maximum_entries of 10.
//!     let mut hash1 = hashtable!(10)?;
//!
//!     for i in 0..10 {
//!         let key = i as u128;
//!
//!         // insert with a u128 key and value of String.
//!         // note that anything inserted must implement the [`crate::Serializable`]
//!         // trait.
//!         hash1.insert(&key, &format!("something{}", i))?;
//!     }
//!
//!     assert!(hash1.insert(&100u128, &"anything".to_string()).is_err());
//!
//!     let mut count = 0;
//!     // The [`std::iter::IntoIterator`] trait is implemented, so iteration
//!     // is simple
//!     for (k,v) in &hash1 {
//!         info!("k={},v={}", k, v)?;
//!         count += 1;
//!     }
//!
//!     assert_eq!(count, 9);
//!
//!     // create a hash with a non-default maximum load capacity.
//!     let mut hash2 = hashtable!(10, 0.5)?;
//!
//!     for i in 0..10 {
//!         let key = i as u128;
//!
//!         // insert with a u128 key and value of String.
//!         // note that anything inserted must implement the [`crate::Serializable`]
//!         // trait.
//!         hash2.insert(&key, &format!("something{}", i))?;
//!     }
//!
//!     // note that hash2 appears to behave the same way as hash1. Both allow
//!     // for 10 entries regardless of the load factor (hash1 has a default load factor of
//!     // 0.75 and hash2 has a load factor configured to 0.5). The difference is that
//!     // when hash2 was instantiated, an entry array of length 20 would be used, whereas
//!     // with hash1, an array of 14 would have been used. Both ensure that exactly max_entries
//!     // items can be inserted. The main considerations for the user is memory consuption and
//!     // performance.
//!     assert!(hash2.insert(&100u128, &"anything".to_string()).is_err());
//!
//!     Ok(())
//! }
//!```

mod macros;
mod ser;
mod slabs;
mod static_hash;
mod types;

pub use crate::ser::{deserialize, serialize, BinReader, BinWriter};
pub use crate::slabs::{SlabAllocatorBuilder, GLOBAL_SLAB_ALLOCATOR};
pub use crate::static_hash::{StaticHashsetBuilder, StaticHashtableBuilder};
pub use crate::types::{
	RawHashsetIterator, RawHashtableIterator, Reader, Serializable, Slab, SlabAllocator,
	SlabAllocatorConfig, SlabMut, StaticHashset, StaticHashsetConfig, StaticHashtable,
	StaticHashtableConfig, StaticQueue, ThreadPool, Writer,
};
