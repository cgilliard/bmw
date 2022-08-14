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

mod macros;
mod ser;
mod slabs;
mod static_hash;
mod types;

pub use crate::ser::{deserialize, serialize, BinReader, BinWriter};
pub use crate::slabs::{SlabAllocatorBuilder, GLOBAL_SLAB_ALLOCATOR};
pub use crate::static_hash::{
	StaticHashsetBuilder, StaticHashsetConfig, StaticHashtableBuilder, StaticHashtableConfig,
};
pub use crate::types::{
	RawHashsetIterator, RawHashtableIterator, Reader, Serializable, Slab, SlabAllocator,
	SlabAllocatorConfig, SlabMut, StaticHashset, StaticHashtable, StaticQueue, ThreadPool, Writer,
};
