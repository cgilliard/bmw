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

mod impls;
mod macros;
mod misc;
mod ser;
mod slabs;
mod threadpool;
mod types;

pub use crate::ser::{deserialize, serialize, BinReader, BinWriter, SlabReader, SlabWriter};
pub use crate::slabs::{Slab, SlabAllocatorBuilder, SlabMut, GLOBAL_SLAB_ALLOCATOR};
pub use crate::types::{
	ConfigOption, ExecutionResponse, HashsetIterator, HashtableIterator, ListIterator, Reader,
	Serializable, SlabAllocator, SlabAllocatorConfig, SortableList, StaticBuilder, StaticHashset,
	StaticHashsetConfig, StaticHashtable, StaticHashtableConfig, StaticList, StaticListConfig,
	StaticQueue, ThreadPool, ThreadPoolBuilder, ThreadPoolConfig, Writer,
};
