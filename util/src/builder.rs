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

use crate::slabs::SlabAllocatorImpl;
use crate::suffix_tree::{MatchImpl, SuffixTreeImpl};
use crate::threadpool::ThreadPoolImpl;
use crate::types::Builder;
use crate::types::{StaticImpl, StaticImplSync};
use crate::{
	Array, ArrayList, List, ListConfig, Match, Pattern, Queue, Serializable, SlabAllocator,
	SlabAllocatorConfig, Stack, StaticHashset, StaticHashsetConfig, StaticHashtable,
	StaticHashtableConfig, SuffixTree, ThreadPool, ThreadPoolConfig,
};
use bmw_err::Error;
use std::cell::{RefCell, UnsafeCell};
use std::fmt::Debug;
use std::hash::Hash;
use std::rc::Rc;

impl Builder {
	/// Build a [`crate::ThreadPool`] based on the specified [`crate::ThreadPoolConfig`]. Note
	/// that the thread pool will not be usable until the [`crate::ThreadPool::start`] function
	/// has been called.
	pub fn build_thread_pool<T: 'static + Send + Sync>(
		config: ThreadPoolConfig,
	) -> Result<impl ThreadPool<T>, Error> {
		Ok(ThreadPoolImpl::new(config, None)?)
	}

	pub fn build_array<T>(size: usize) -> Result<Array<T>, Error>
	where
		T: Clone,
	{
		Array::new(size)
	}

	pub fn build_array_list<T>(size: usize) -> Result<impl List<T>, Error>
	where
		T: Clone + Debug + PartialEq,
	{
		ArrayList::new(size)
	}

	/*
	pub fn build_array_list_concrete<T>(size: usize) -> Result<Box<dyn List<T>>, Error>
	where
		T: Clone + Debug + PartialEq + 'static,
	{
		Ok(Box::new(build_array_list(size)?))
	}
		*/

	pub fn build_queue<T>(size: usize) -> Result<impl Queue<T>, Error>
	where
		T: Clone,
	{
		ArrayList::new(size)
	}

	pub fn build_queue_boxed<T>(size: usize) -> Result<Box<dyn Queue<T>>, Error>
	where
		T: Clone + 'static,
	{
		Ok(Box::new(ArrayList::new(size)?))
	}

	pub fn build_stack<T>(size: usize) -> Result<impl Stack<T>, Error>
	where
		T: Clone,
	{
		ArrayList::new(size)
	}

	pub fn build_stack_boxed<T>(size: usize) -> Result<Box<dyn Stack<T>>, Error>
	where
		T: Clone + 'static,
	{
		Ok(Box::new(ArrayList::new(size)?))
	}

	pub fn build_sync_hashtable<K, V>(
		config: StaticHashtableConfig,
		slab_config: SlabAllocatorConfig,
	) -> Result<impl StaticHashtable<K, V>, Error>
	where
		K: Serializable + Hash + PartialEq + Debug,
		V: Serializable,
	{
		StaticImplSync::new(Some(config), None, None, slab_config)
	}

	pub fn build_hashtable<K, V>(
		config: StaticHashtableConfig,
		slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	) -> Result<impl StaticHashtable<K, V>, Error>
	where
		K: Serializable + Hash + PartialEq + Debug,
		V: Serializable,
	{
		StaticImpl::new(Some(config), None, None, slabs)
	}

	pub fn build_sync_hashset<K>(
		config: StaticHashsetConfig,
		slab_config: SlabAllocatorConfig,
	) -> Result<impl StaticHashset<K>, Error>
	where
		K: Serializable + Hash + PartialEq + Debug,
	{
		StaticImplSync::new(None, Some(config), None, slab_config)
	}

	pub fn build_hashset<K>(
		config: StaticHashsetConfig,
		slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	) -> Result<impl StaticHashset<K>, Error>
	where
		K: Serializable + Hash + PartialEq + Debug,
	{
		StaticImpl::new(None, Some(config), None, slabs)
	}

	pub fn build_sync_list<V>(
		config: ListConfig,
		slab_config: SlabAllocatorConfig,
	) -> Result<impl List<V>, Error>
	where
		V: Serializable + Debug + PartialEq,
	{
		StaticImplSync::new(None, None, Some(config), slab_config)
	}

	pub fn build_list<V>(
		config: ListConfig,
		slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	) -> Result<impl List<V>, Error>
	where
		V: Serializable + Debug + PartialEq,
	{
		StaticImpl::new(None, None, Some(config), slabs)
	}

	pub fn _build_match(start: usize, end: usize, id: usize) -> impl Match {
		MatchImpl::_new(start, end, id)
	}

	pub fn _build_pattern(
		regex: &str,
		is_case_sensitive: bool,
		is_termination_pattern: bool,
		id: usize,
	) -> Pattern {
		Pattern::_new(regex, is_case_sensitive, is_termination_pattern, id)
	}

	pub fn _build_suffix_tree(patterns: impl List<Pattern>) -> Result<impl SuffixTree, Error> {
		SuffixTreeImpl::_new(patterns)
	}

	/// Build a slab allocator on the heap in an [`std::cell::UnsafeCell`].
	/// This function is used by the global thread local slab allocator to allocate
	/// thread local slab allocators. Note that it calls unsafe functions. This
	/// function should generally be called through the [`crate::init_slab_allocator`]
	/// macro.
	pub fn build_slabs_unsafe() -> UnsafeCell<Box<dyn SlabAllocator>> {
		UnsafeCell::new(Box::new(SlabAllocatorImpl::new()))
	}

	/// Build a slab allocator on the heap. This function is used by [`crate::slab_allocator`]
	/// to create slab allocators for use with the other macros.
	pub fn build_slabs_ref() -> Rc<RefCell<dyn SlabAllocator>> {
		Rc::new(RefCell::new(SlabAllocatorImpl::new()))
	}

	pub fn build_slabs() -> Box<dyn SlabAllocator> {
		Box::new(SlabAllocatorImpl::new())
	}
}
