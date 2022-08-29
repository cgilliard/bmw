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

use crate::types::{
	Builder, HashImpl, HashImplSync, MatchImpl, SlabAllocatorImpl, SuffixTreeImpl, ThreadPoolImpl,
};
use crate::{
	Array, ArrayList, Hashset, HashsetConfig, Hashtable, HashtableConfig, ListConfig, Match,
	Pattern, Queue, Serializable, SlabAllocator, SlabAllocatorConfig, SortableList, Stack,
	SuffixTree, ThreadPool, ThreadPoolConfig,
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

	pub fn build_array_list<T>(size: usize) -> Result<impl SortableList<T>, Error>
	where
		T: Clone + Debug + PartialEq + Serializable,
	{
		ArrayList::new(size)
	}

	pub fn build_array_list_box<T>(size: usize) -> Result<Box<dyn SortableList<T>>, Error>
	where
		T: Clone + Debug + PartialEq + Serializable + 'static,
	{
		Ok(Box::new(ArrayList::new(size)?))
	}

	pub fn build_queue<T>(size: usize) -> Result<impl Queue<T>, Error>
	where
		T: Clone,
	{
		ArrayList::new(size)
	}

	pub fn build_queue_box<T>(size: usize) -> Result<Box<dyn Queue<T>>, Error>
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

	pub fn build_stack_box<T>(size: usize) -> Result<Box<dyn Stack<T>>, Error>
	where
		T: Clone + 'static,
	{
		Ok(Box::new(ArrayList::new(size)?))
	}

	pub fn build_hashtable_sync<K, V>(
		config: HashtableConfig,
		slab_config: SlabAllocatorConfig,
	) -> Result<impl Hashtable<K, V>, Error>
	where
		K: Serializable + Hash + PartialEq + Debug + Clone,
		V: Serializable + Clone,
	{
		HashImplSync::new(Some(config), None, None, slab_config)
	}

	pub fn build_hashtable_sync_box<K, V>(
		config: HashtableConfig,
		slab_config: SlabAllocatorConfig,
	) -> Result<Box<dyn Hashtable<K, V> + Send + Sync>, Error>
	where
		K: Serializable + Hash + PartialEq + Debug + 'static + Clone,
		V: Serializable + Clone,
	{
		let ret = HashImplSync::new(Some(config), None, None, slab_config)?;
		let ret = Box::new(ret);
		Ok(ret)
	}

	pub fn build_hashtable<K, V>(
		config: HashtableConfig,
		slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	) -> Result<impl Hashtable<K, V>, Error>
	where
		K: Serializable + Hash + PartialEq + Debug + Clone,
		V: Serializable + Clone,
	{
		HashImpl::new(Some(config), None, None, slabs)
	}

	pub fn build_hashtable_box<K, V>(
		config: HashtableConfig,
		slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	) -> Result<Box<dyn Hashtable<K, V>>, Error>
	where
		K: Serializable + Hash + PartialEq + Debug + 'static + Clone,
		V: Serializable + Clone,
	{
		Ok(Box::new(HashImpl::new(Some(config), None, None, slabs)?))
	}

	pub fn build_hashset_sync<K>(
		config: HashsetConfig,
		slab_config: SlabAllocatorConfig,
	) -> Result<impl Hashset<K>, Error>
	where
		K: Serializable + Hash + PartialEq + Debug + Clone,
	{
		HashImplSync::new(None, Some(config), None, slab_config)
	}

	pub fn build_hashset_sync_box<K>(
		config: HashsetConfig,
		slab_config: SlabAllocatorConfig,
	) -> Result<Box<dyn Hashset<K> + Send + Sync>, Error>
	where
		K: Serializable + Hash + PartialEq + Debug + 'static + Clone,
	{
		let ret = HashImplSync::new(None, Some(config), None, slab_config)?;
		let ret = Box::new(ret);
		Ok(ret)
	}

	pub fn build_hashset<K>(
		config: HashsetConfig,
		slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	) -> Result<impl Hashset<K>, Error>
	where
		K: Serializable + Hash + PartialEq + Debug + Clone,
	{
		HashImpl::new(None, Some(config), None, slabs)
	}

	pub fn build_hashset_box<K>(
		config: HashsetConfig,
		slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	) -> Result<Box<dyn Hashset<K>>, Error>
	where
		K: Serializable + Hash + PartialEq + Debug + 'static + Clone,
	{
		Ok(Box::new(HashImpl::new(None, Some(config), None, slabs)?))
	}

	pub fn build_list_sync<V>(
		config: ListConfig,
		slab_config: SlabAllocatorConfig,
	) -> Result<impl SortableList<V>, Error>
	where
		V: Serializable + Debug + PartialEq + Clone,
	{
		HashImplSync::new(None, None, Some(config), slab_config)
	}

	pub fn build_list_sync_box<V>(
		config: ListConfig,
		slab_config: SlabAllocatorConfig,
	) -> Result<Box<dyn SortableList<V>>, Error>
	where
		V: Serializable + Debug + PartialEq + Clone + 'static,
	{
		let ret = HashImplSync::new(None, None, Some(config), slab_config)?;
		let ret = Box::new(ret);
		Ok(ret)
	}

	pub fn build_list<V>(
		config: ListConfig,
		slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	) -> Result<impl SortableList<V>, Error>
	where
		V: Serializable + Debug + Clone,
	{
		HashImpl::new(None, None, Some(config), slabs)
	}

	pub fn build_list_box<V>(
		config: ListConfig,
		slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	) -> Result<Box<dyn SortableList<V>>, Error>
	where
		V: Serializable + Debug + PartialEq + Clone + 'static,
	{
		Ok(Box::new(HashImpl::new(None, None, Some(config), slabs)?))
	}

	pub fn build_match(start: usize, end: usize, id: usize) -> impl Match {
		MatchImpl::new(start, end, id)
	}

	pub fn build_match_default() -> impl Match {
		MatchImpl::new(0, 0, 0)
	}

	pub fn build_pattern(
		regex: &str,
		is_case_sensitive: bool,
		is_termination_pattern: bool,
		is_multi_line: bool,
		id: usize,
	) -> Pattern {
		Pattern::new(
			regex,
			is_case_sensitive,
			is_termination_pattern,
			is_multi_line,
			id,
		)
	}

	pub fn build_suffix_tree(
		patterns: impl SortableList<Pattern>,
		termination_length: usize,
		max_wildcard_length: usize,
	) -> Result<impl SuffixTree, Error> {
		SuffixTreeImpl::new(patterns, termination_length, max_wildcard_length)
	}

	/// Build a slab allocator on the heap in an [`std::cell::UnsafeCell`].
	/// This function is used by the global thread local slab allocator to allocate
	/// thread local slab allocators. Note that it calls unsafe functions. This
	/// function should generally be called through the [`crate::init_slab_allocator`]
	/// macro.
	pub fn build_slabs_unsafe() -> UnsafeCell<Box<dyn SlabAllocator>> {
		UnsafeCell::new(Box::new(SlabAllocatorImpl::new()))
	}

	/// Build a slab allocator in a Rc/RefCell. This function is used by [`crate::slab_allocator`]
	/// to create slab allocators for use with the other macros.
	pub fn build_slabs_ref() -> Rc<RefCell<dyn SlabAllocator>> {
		Rc::new(RefCell::new(SlabAllocatorImpl::new()))
	}

	pub fn build_slabs() -> Box<dyn SlabAllocator> {
		Box::new(SlabAllocatorImpl::new())
	}
}

#[cfg(test)]
mod test {
	use crate::{Builder, ListConfig, Match, SlabAllocatorConfig};
	use bmw_err::*;

	#[test]
	fn test_builder() -> Result<(), Error> {
		let mut arrlist = Builder::build_array_list_box(10)?;
		arrlist.push(0)?;
		let mut i = 0;
		for x in arrlist.iter() {
			assert_eq!(x, 0);
			i += 1;
		}
		assert_eq!(i, 1);

		let mut list =
			Builder::build_list_sync_box(ListConfig::default(), SlabAllocatorConfig::default())?;
		list.push(0)?;
		assert_eq!(list.size(), 1);

		let nmatch = Builder::build_match(0, 1, 2);
		assert_eq!(nmatch.start(), 0);
		assert_eq!(nmatch.end(), 1);
		assert_eq!(nmatch.id(), 2);

		Ok(())
	}
}
