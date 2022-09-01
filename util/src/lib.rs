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

//! The util crate provides the data structures that are used in bmw. The data structures
//! implemented include [`crate::Hashtable`], [`crate::Hashset`], [`crate::List`], [`crate::Array`],
//! [`crate::ArrayList`], [`crate::Stack`], [`crate::Queue`], and [`crate::SuffixTree`]. Additionally
//! a [`crate::ThreadPool`] implementation is provided. Each data structure can be instantiated in
//! a impl and Box form using the [`crate::Builder`] or through macros. The impls completely stack
//! based and the box forms are Box<dyn ..>'s that can be stored in other structs and Enums.
//!
//! # Motivation
//!
//! The advantage of these implementations is that they do not allocate memory on the
//! heap after initialization of the data structure. So, we can create a [`crate::hashtable`],
//! [`crate::List`] or a [`crate::hashset`] and do millions of operations and
//! no heap memory will be allocated or deallocated. Dynamic heap allocations that are long-lived can cause
//! substantial problems like slowness and memory fragmentation and even system crashes and these data structures
//! are intended to alleviate those problems. The [`core::ops::Drop`] trait is also implemented so all
//! slabs used by the data structure are freed when the data structure goes out of scope.
//!
//! # Performance
//!
//! The hashtable/set are not as fast as the native Rust data structures because they
//! require serialization and deserialization of the entries on each operation. However, the
//! peformance is at least in the ballpark of the standard data structures. The array, arraylist,
//! queue, and stack are faster for insert, slower for initialization and about the same for
//! iteration and drop. A performance tool is included in the project in the etc directory
//! [ds_perf](https://github.com/37miners/bmw/tree/main/etc/ds_perf).
//!
//! Below is the output of some runs on a linux machine.
//!
//!```text
//! $ ./target/release/perf  --help
//! ds_perf 1.0
//! 37 Miners, LLC
//!
//! USAGE:
//!    perf [FLAGS]
//!
//! FLAGS:
//!     --array           run tests for array
//!     --array_string    run tests for array with strings
//!     --arraylist       run tests for array list
//!     --hashmap         run tests for standard rust library hashmap
//!     --hashtable       run tests for hashtable
//! -h, --help            Prints help information
//! -V, --version         Prints version information
//!     --vec             run tests for standard rust library vec
//!     --vec_string      run tests for vec with strings
//!
//! $ ./target/release/perf  --array_string
//! [2022-09-01 13:02:41.974]: Starting ds_perf
//! [2022-09-01 13:02:41.974]: Testing array string
//! [2022-09-01 13:02:41.974]: array init: alloc: 240,000, dealloc: 0, alloc_qty: 1, dealloc_qty: 0, delta: 240,000, elapsed: 180.336µs
//! [2022-09-01 13:02:41.975]: array insert: alloc: 110,000, dealloc: 0, alloc_qty: 10,000, dealloc_qty: 0, delta: 110,000, elapsed: 332.891µs
//! [2022-09-01 13:02:41.975]: array iter: alloc: 0, dealloc: 0, alloc_qty: 0, dealloc_qty: 0, delta: 0, elapsed: 27ns
//! [2022-09-01 13:02:41.975]: array drop: alloc: 0, dealloc: 350,000, alloc_qty: 0, dealloc_qty: 10,001, delta: -350,000, elapsed: 121.148µs
//!
//! $ ./target/release/perf  --vec_string
//! [2022-09-01 13:02:38.151]: Starting ds_perf
//! [2022-09-01 13:02:38.151]: Testing vec string
//! [2022-09-01 13:02:38.151]: vec init: alloc: 240,000, dealloc: 0, alloc_qty: 1, dealloc_qty: 0, delta: 240,000, elapsed: 177.351µs
//! [2022-09-01 13:02:38.151]: vec insert: alloc: 110,000, dealloc: 0, alloc_qty: 10,000, dealloc_qty: 0, delta: 110,000, elapsed: 324.671µs
//! [2022-09-01 13:02:38.151]: vec iter: alloc: 0, dealloc: 0, alloc_qty: 0, dealloc_qty: 0, delta: 0, elapsed: 40ns
//! [2022-09-01 13:02:38.152]: vec drop: alloc: 0, dealloc: 350,000, alloc_qty: 0, dealloc_qty: 10,001, delta: -350,000, elapsed: 121.868µs
//!
//! $ ./target/release/perf  --array
//! [2022-09-01 13:00:22.190]: Starting ds_perf
//! [2022-09-01 13:00:22.190]: Testing array
//! [2022-09-01 13:00:22.190]: array init: alloc: 40,000, dealloc: 0, alloc_qty: 1, dealloc_qty: 0, delta: 40,000, elapsed: 27.454µs
//! [2022-09-01 13:00:22.190]: array insert: alloc: 0, dealloc: 0, alloc_qty: 0, dealloc_qty: 0, delta: 0, elapsed: 999ns
//! [2022-09-01 13:00:22.190]: array iter: alloc: 0, dealloc: 0, alloc_qty: 0, dealloc_qty: 0, delta: 0, elapsed: 29ns
//! [2022-09-01 13:00:22.190]: array drop: alloc: 0, dealloc: 40,000, alloc_qty: 0, dealloc_qty: 1, delta: -40,000, elapsed: 121ns
//!
//! $ ./target/release/perf  --vec
//! [2022-09-01 13:00:27.820]: Starting ds_perf
//! [2022-09-01 13:00:27.820]: testing vec
//! [2022-09-01 13:00:27.820]: vec init: alloc: 0, dealloc: 0, alloc_qty: 0, dealloc_qty: 0, delta: 0, elapsed: 120ns
//! [2022-09-01 13:00:27.820]: vec insert: alloc: 131,056, dealloc: 65,520, alloc_qty: 13, dealloc_qty: 12, delta: 65,536, elapsed: 48.554µs
//! [2022-09-01 13:00:27.820]: vec iter: alloc: 0, dealloc: 0, alloc_qty: 0, dealloc_qty: 0, delta: 0, elapsed: 29ns
//! [2022-09-01 13:00:27.820]: vec drop: alloc: 0, dealloc: 65,536, alloc_qty: 0, dealloc_qty: 1, delta: -65,536, elapsed: 5.096µs
//!
//! $ ./target/release/perf  --hashtable
//! [2022-09-01 13:07:30.861]: Starting ds_perf
//! [2022-09-01 13:07:30.861]: Testing hashtable
//! [2022-09-01 13:07:30.861]: hashtable init: alloc: 100,096, dealloc: 96, alloc_qty: 2, dealloc_qty: 1, delta: 100,000, elapsed: 55.208µs
//! [2022-09-01 13:07:30.863]: hashtable insert: alloc: 0, dealloc: 0, alloc_qty: 0, dealloc_qty: 0, delta: 0, elapsed: 2.296961ms
//! [2022-09-01 13:07:30.865]: hashtable get: alloc: 0, dealloc: 0, alloc_qty: 0, dealloc_qty: 0, delta: 0, elapsed: 1.462402ms
//! [2022-09-01 13:07:30.865]: hashtable drop: alloc: 100,000, dealloc: 200,000, alloc_qty: 1, dealloc_qty: 2, delta: -100,000, elapsed: 512.423µs
//!
//! $ ./target/release/perf  --hashmap
//! [2022-09-01 13:07:39.269]: Starting ds_perf
//! [2022-09-01 13:07:39.269]: Testing hashmap
//! [2022-09-01 13:07:39.269]: hashmap init: alloc: 0, dealloc: 0, alloc_qty: 0, dealloc_qty: 0, delta: 0, elapsed: 69ns
//! [2022-09-01 13:07:39.270]: hashmap insert: alloc: 688,252, dealloc: 344,172, alloc_qty: 26, dealloc_qty: 24, delta: 344,080, elapsed: 611.785µs
//! [2022-09-01 13:07:39.270]: hashmap get: alloc: 0, dealloc: 0, alloc_qty: 0, dealloc_qty: 0, delta: 0, elapsed: 199.428µs
//! [2022-09-01 13:07:39.270]: hashmap drop: alloc: 0, dealloc: 344,080, alloc_qty: 0, dealloc_qty: 2, delta: -344,080, elapsed: 25.252µs
//!
//! $ ./target/release/perf  --arraylist
//! [2022-09-01 13:08:08.434]: Starting ds_perf
//! [2022-09-01 13:08:08.435]: testing arraylist
//! [2022-09-01 13:08:08.435]: arraylist init: alloc: 40,000, dealloc: 0, alloc_qty: 1, dealloc_qty: 0, delta: 40,000, elapsed: 27.827µs
//! [2022-09-01 13:08:08.435]: arraylist insert: alloc: 0, dealloc: 0, alloc_qty: 0, dealloc_qty: 0, delta: 0, elapsed: 18.963µs
//! [2022-09-01 13:08:08.435]: arraylist iter: alloc: 24, dealloc: 24, alloc_qty: 1, dealloc_qty: 1, delta: 0, elapsed: 20.341µs
//! [2022-09-01 13:08:08.435]: arraylist drop: alloc: 1,856, dealloc: 41,856, alloc_qty: 27, dealloc_qty: 28, delta: -40,000, elapsed: 110ns
//!
//!
//! ```
//!
//! So, as seen, the performance is somewhat comparable and keep in mind, that these operations are
//! being done in the sub millisecond time frame, so it's very unlikely that this will ultimately
//! impact the performance of the application unless it's an already very optimized application
//! that needs to be very scalable.
//!
//! # Using bmw in your project
//!
//! To use the crates in bmw in your project, add the following to your Cargo.toml
//!```
//!
//!```
//!
//! # Use cases
//!
//! The main use case for these data structures is in server applications to avoid making dynamic
//! heap allocations as runtime, but they also offer some other interesting properties. For instance, with
//! the standard rust collections, the entries in the hashmap are just references so they must
//! stay in scope while they are in the hashmap. With this implementation, that is not required.
//! The inserted items can be dropped and they will remain in the hashtable/hashset. Also,
//! [`crate::Hashtable`] and [`crate::Hashset`] both implement the
//! [`bmw_ser::Serializable`] trait so they can be sent from one part of an app to another or even
//! sent over the network.
//!
//! # Examples
//!

mod array;
mod builder;
mod hash;
mod macros;
mod misc;
mod ser;
mod slabs;
mod suffix_tree;
mod threadpool;
mod types;

pub use crate::ser::{deserialize, serialize};
pub use crate::slabs::GLOBAL_SLAB_ALLOCATOR;
pub use crate::types::ConfigOption::*;
pub use crate::types::PatternParam::*;
pub use crate::types::SuffixParam::*;
pub use crate::types::{
	Array, ArrayIterator, ArrayList, BinReader, BinWriter, Builder, ConfigOption, Hashset,
	HashsetConfig, HashsetIterator, Hashtable, HashtableConfig, HashtableIterator, List,
	ListConfig, ListIterator, Match, Pattern, PatternParam, PoolResult, Queue, Slab, SlabAllocator,
	SlabAllocatorConfig, SlabMut, SlabReader, SlabWriter, SortableList, Stack, SuffixParam,
	SuffixTree, ThreadPool, ThreadPoolConfig,
};
pub use bmw_ser::{Reader, Serializable, Writer};
