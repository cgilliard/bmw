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

use crate::Serializable;
use bmw_deps::dyn_clone::{clone_trait_object, DynClone};
use bmw_derive::Serializable;
use bmw_err::*;
use bmw_log::*;
use std::cell::RefCell;
use std::fmt::Debug;
use std::future::Future;
use std::io::{Read, Write};
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

info!();

/// Wrapper around the lock functionalities used by bmw in [`std::sync`] rust libraries.
/// The main benefits are the simplified interface and the fact that if a thread attempts
/// to obtain a lock twice, an error will be thrown instead of a thread panic. This is implemented
/// through a thread local Hashset which keeps track of the guards used by the lock removes an
/// entry for them when the guard is dropped.
///
/// # Examples
///
///```
/// use bmw_util::*;
/// use bmw_err::*;
/// use std::time::Duration;
/// use std::thread::{sleep,spawn};
///
/// fn test() -> Result<(), Error> {
///     let mut lock = lock!(1)?;
///     let lock_clone = lock.clone();
///
///     spawn(move || -> Result<(), Error> {
///         let mut x = lock.wlock()?;
///         assert_eq!(**(x.guard()), 1);
///         sleep(Duration::from_millis(3000));
///         **(x.guard()) = 2;
///         Ok(())
///     });
///
///     sleep(Duration::from_millis(1000));
///     let x = lock_clone.rlock()?;
///     assert_eq!(**(x.guard()), 2);
///     Ok(())
/// }
///```
pub trait Lock<T>: Send + Sync + Debug
where
	T: Send + Sync,
{
	/// obtain a write lock and corresponding [`std::sync::RwLockWriteGuard`] for this
	/// [`crate::Lock`].
	fn wlock(&mut self) -> Result<RwLockWriteGuardWrapper<'_, T>, Error>;
	/// obtain a read lock and corresponding [`std::sync::RwLockReadGuard`] for this
	/// [`crate::Lock`].
	fn rlock(&self) -> Result<RwLockReadGuardWrapper<'_, T>, Error>;
	/// Clone this [`crate::Lock`].
	fn clone(&self) -> Self;
}

/// [`crate::LockBox`] is the same as [`crate::Lock`] except that it is possible to build
/// The LockBox into a Box<dyn LockBox<T>> structure so that it is object safe. It can then
/// be cloned using DynClone.
///
/// # Examples
///
///```
///
/// use bmw_err::*;
/// use bmw_util::*;
/// use bmw_deps::dyn_clone::clone_box;
///
/// struct TestLockBox<T> {
///     lock_box: Box<dyn LockBox<T>>,
/// }
///
/// fn test_lock_box() -> Result<(), Error> {
///     let lock_box = lock_box!(1u32)?;
///     let mut lock_box2 = clone_box(&*lock_box);
///     let mut tlb = TestLockBox { lock_box };
///
///     {
///         let mut tlb = tlb.lock_box.wlock()?;
///         (**tlb.guard()) = 2u32;
///     }
///
///     {
///         let tlb = tlb.lock_box.rlock()?;
///         assert_eq!((**tlb.guard()), 2u32);
///     }
///
///     Ok(())
/// }
///
///```
pub trait LockBox<T>: Send + Sync + DynClone + Debug
where
	T: Send + Sync,
{
	/// obtain a write lock and corresponding [`std::sync::RwLockWriteGuard`] for this
	/// [`crate::Lock`].
	fn wlock(&mut self) -> Result<RwLockWriteGuardWrapper<'_, T>, Error>;
	/// obtain a read lock and corresponding [`std::sync::RwLockReadGuard`] for this
	/// [`crate::Lock`].
	fn rlock(&self) -> Result<RwLockReadGuardWrapper<'_, T>, Error>;
}

/// Wrapper around the [`std::sync::RwLockReadGuard`].
pub struct RwLockReadGuardWrapper<'a, T> {
	pub(crate) guard: RwLockReadGuard<'a, T>,
	pub(crate) id: u128,
	pub(crate) debug_err: bool,
}

/// Wrapper around the [`std::sync::RwLockWriteGuard`].
pub struct RwLockWriteGuardWrapper<'a, T> {
	pub(crate) guard: RwLockWriteGuard<'a, T>,
	pub(crate) id: u128,
	pub(crate) debug_err: bool,
}

/// The enum used to define patterns. See [`crate::pattern`] for more info.
pub enum PatternParam<'a> {
	/// The regular expression for this pattern.
	Regex(&'a str),
	/// Whether or not this is a termination pattern.
	IsTerm(bool),
	/// Whether or not this is a multi-line pattern. (allowing multi-line wildcard matches).
	IsMulti(bool),
	/// Whether or not this pattern is case sensitive.
	IsCaseSensitive(bool),
	/// The id of this pattern.
	Id(usize),
}

/// The enum used to define a suffix_tree. See [`crate::suffix_tree`] for more info.
pub enum SuffixParam {
	/// The termination length of this suffix tree (length at which matching stops).
	TerminationLength(usize),
	/// The maximum length to look for wildcards.
	MaxWildcardLength(usize),
}

/// Configuration options used throughout this crate via macro.
#[derive(Clone, Debug)]
pub enum ConfigOption<'a> {
	/// The maximum number of entries for a data structure. See [`crate::Hashtable`] and
	/// [`crate::Hashset`].
	MaxEntries(usize),
	/// The maximum load factor for a data structure. See [`crate::Hashtable`] and
	/// [`crate::Hashset`].
	MaxLoadFactor(f64),
	/// The slab size for a slab allocator. See [`crate::SlabAllocator`].
	SlabSize(usize),
	/// The slab count for a slab allocator. See [`crate::SlabAllocator`].
	SlabCount(usize),
	/// The minimum number of threads for a thread pool. See [`crate::ThreadPool`].
	MinSize(usize),
	/// The maximum number of threads for a thread pool. See [`crate::ThreadPool`].
	MaxSize(usize),
	/// The size of the sync channel for a thread pool. See [`crate::ThreadPool`].
	SyncChannelSize(usize),
	/// Slab allocator to be used by this data structure.
	Slabs(&'a Rc<RefCell<dyn SlabAllocator>>),
}

/// Utility to write to slabs using the [`bmw_ser::Writer`] trait.
#[derive(Clone)]
pub struct SlabWriter {
	pub(crate) slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	pub(crate) slab_id: usize,
	pub(crate) offset: usize,
	pub(crate) slab_size: usize,
	pub(crate) bytes_per_slab: usize,
}

/// Utility to read from slabs using the [`bmw_ser::Reader`] trait.
#[derive(Clone)]
pub struct SlabReader {
	pub(crate) slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	pub(crate) slab_id: usize,
	pub(crate) offset: usize,
	pub(crate) slab_size: usize,
	pub(crate) bytes_per_slab: usize,
	pub(crate) max_value: usize,
}

/// Utility wrapper for an underlying byte Writer. Defines higher level methods
/// to write numbers, byte vectors, hashes, etc.
pub struct BinWriter<'a> {
	pub(crate) sink: &'a mut dyn Write,
}

/// Utility wrapper for an underlying byte Reader. Defines higher level methods
/// to write numbers, byte vectors, hashes, etc.
pub struct BinReader<'a, R: Read> {
	pub(crate) source: &'a mut R,
}

/// The configuration struct for a [`crate::ThreadPool`]. This struct is passed into the
/// [`crate::Builder::build_thread_pool`] function or the [`crate::thread_pool`] macro. The
/// [`std::default::Default`] trait is implemented for this trait. Also see [`crate::ConfigOption`]
/// for details on configuring via macro.
#[derive(Debug, Clone, Serializable)]
pub struct ThreadPoolConfig {
	/// The minimum number of threads that this thread_pool will use. The default value is 3.
	pub min_size: usize,
	/// The maximm number of threads that this thread_pool will use. The default value is 7.
	pub max_size: usize,
	/// The size of the sync_channel buffer. See [`std::sync::mpsc::sync_channel`] for more
	/// information. The default value is 7.
	pub sync_channel_size: usize,
}

/// The configuration struct for a [`Hashtable`]. This struct is passed
/// into the [`crate::Builder::build_hashtable`] function. The [`std::default::Default`]
/// trait is implemented for this trait.
#[derive(Debug, Clone, Copy, PartialEq, Serializable)]
pub struct HashtableConfig {
	/// The maximum number of entries that can exist in this [`Hashtable`].
	/// The default is 100_000. Note that the overhead for this value is 8 bytes
	/// per entry. The [`crate::HashtableConfig::max_load_factor`] setting will
	/// also affect how much memory is used by the entry array.
	pub max_entries: usize,
	/// The maximum load factor for this [`crate::Hashtable`]. This number
	/// incidicates how full the hashtable can be. This is an array based hashtable
	/// and it is not possible to resize it after it is instantiated. The default value
	/// is 0.75.
	pub max_load_factor: f64,
}

/// The configuration struct for a [`Hashset`]. This struct is passed
/// into the [`crate::Builder::build_hashset`] function. The [`std::default::Default`]
/// trait is implemented for this trait.
#[derive(Debug, Clone, Copy, PartialEq, Serializable)]
pub struct HashsetConfig {
	/// The maximum number of entries that can exist in this [`Hashset`].
	/// The default is 100_000. Note that the overhead for this value is 8 bytes
	/// per entry. So, by default 8 mb are allocated with this configuration.
	pub max_entries: usize,
	/// The maximum load factor for this [`crate::Hashset`]. This number
	/// incidicates how full the hashset can be. This is an array based hashset
	/// and it is not possible to resize it after it is instantiated. The default value
	/// is 0.75.
	pub max_load_factor: f64,
}

/// Slab Allocator configuration struct. This struct is the input to the
/// [`crate::SlabAllocator::init`] function. The two parameters are `slab_size`
/// which is the size of the slabs in bytes allocated by this
/// [`crate::SlabAllocator`] and `slab_count` which is the number of slabs
/// that can be allocated by this [`crate::SlabAllocator`].
#[derive(Debug, Clone, Serializable)]
pub struct SlabAllocatorConfig {
	/// The size, in bytes, of a slab
	pub slab_size: usize,
	/// The number of slabs that this slab allocator can allocate
	pub slab_count: usize,
}

/// [`crate::ArrayList`] is an array based implementation of the [`crate::List`] and
/// [`crate::SortableList`] trait. It uses the [`crate::Array`] as it's underlying
/// storage mechanism. In most cases it is more performant than the LinkedList implementation,
/// but it does do a heap allocation when created. See the module level documentation for
/// an example.
#[derive(Clone)]
pub struct ArrayList<T> {
	pub(crate) inner: Array<T>,
	pub(crate) size: usize,
	pub(crate) head: usize,
	pub(crate) tail: usize,
}

/// The [`crate::Array`] is essentially a wrapper around [`std::vec::Vec`]. It is mainly used
/// to prevent post initialization heap allocations. In many cases resizing/growing of the vec
/// is not needed and adding this wrapper assures that they do not happen. There are use cases
/// where growing is useful and in fact in the library we use Vec for the suffix tree, but in most
/// cases where contiguous memory is used, the primary purpose is for indexing. That can be done
/// with an array. So we add this wrapper.
///
/// # Examples
///```
/// use bmw_err::Error;
/// use bmw_util::array;
///
/// fn main() -> Result<(), Error> {
///      let mut arr = array!(10, &0)?;
///      for i in 0..10 {
///          arr[i] = i;
///      }
///
///      for i in 0..10 {
///          assert_eq!(arr[i], i);
///      }
///      Ok(())
/// }
///```
pub struct Array<T> {
	pub(crate) data: Vec<T>,
}

/// Configuration for Lists currently there are no parameters, but it is still used
/// to stay consistent with [`crate::Hashtable`] and [`crate::Hashset`].
#[derive(Debug, Clone, Serializable)]
pub struct ListConfig {}

pub trait Hashtable<K, V>: Debug + DynClone
where
	K: Serializable + Clone,
	V: Serializable,
{
	fn max_load_factor(&self) -> f64;
	fn max_entries(&self) -> usize;
	fn insert(&mut self, key: &K, value: &V) -> Result<(), Error>;
	fn get(&self, key: &K) -> Result<Option<V>, Error>;
	fn remove(&mut self, key: &K) -> Result<Option<V>, Error>;
	fn size(&self) -> usize;
	fn clear(&mut self) -> Result<(), Error>;
	fn iter<'a>(&'a self) -> HashtableIterator<'a, K, V>;
}

pub trait Hashset<K>: Debug + DynClone
where
	K: Serializable + Clone,
{
	fn max_load_factor(&self) -> f64;
	fn max_entries(&self) -> usize;
	fn insert(&mut self, key: &K) -> Result<(), Error>;
	fn contains(&self, key: &K) -> Result<bool, Error>;
	fn remove(&mut self, key: &K) -> Result<bool, Error>;
	fn size(&self) -> usize;
	fn clear(&mut self) -> Result<(), Error>;
	fn iter<'a>(&'a self) -> HashsetIterator<'a, K>;
}

pub trait Queue<V>: DynClone {
	fn enqueue(&mut self, value: V) -> Result<(), Error>;
	fn dequeue(&mut self) -> Option<&V>;
	fn peek(&self) -> Option<&V>;
	fn length(&self) -> usize;
}

pub trait Stack<V>: DynClone {
	fn push(&mut self, value: V) -> Result<(), Error>;
	fn pop(&mut self) -> Option<&V>;
	fn peek(&self) -> Option<&V>;
	fn length(&self) -> usize;
}

pub trait List<V>: DynClone + Debug {
	fn push(&mut self, value: V) -> Result<(), Error>;
	fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = V> + 'a>
	where
		V: Serializable + Clone;
	fn iter_rev<'a>(&'a self) -> Box<dyn Iterator<Item = V> + 'a>
	where
		V: Serializable + Clone;
	fn delete_head(&mut self) -> Result<(), Error>;
	fn size(&self) -> usize;
	fn clear(&mut self) -> Result<(), Error>;
}

pub trait SortableList<V>: List<V> + DynClone {
	fn sort(&mut self) -> Result<(), Error>
	where
		V: Ord;
	fn sort_unstable(&mut self) -> Result<(), Error>
	where
		V: Ord;
}

clone_trait_object!(<V>Queue<V>);
clone_trait_object!(<V>Stack<V>);
clone_trait_object!(<V>Hashset<V>);
clone_trait_object!(<K, V>Hashtable<K, V>);
clone_trait_object!(<V>List<V>);
clone_trait_object!(<V>SortableList<V>);

/// The result returned from a call to [`crate::ThreadPool::execute`]. This is
/// similar to [`std::result::Result`] except that it implements [`std::marker::Send`]
/// and [`std::marker::Sync`] so that it can be passed through threads. Also a type
/// [`crate::PoolResult::Panic`] is returned if a thread panic occurs in the thread pool.
#[derive(Debug, PartialEq)]
pub enum PoolResult<T, E> {
	Ok(T),
	Err(E),
	Panic,
}

/// This trait defines the public interface to the ThreadPool. A pool can be configured
/// via the [`crate::ThreadPoolConfig`] struct. The thread pool should be accessed through the
/// macros under normal circumstances. See [`crate::thread_pool`], [`crate::execute`] and
/// [`crate::block_on`] for additional details. The thread pool can be passed through threads via a
/// [`crate::Lock`] or [`crate::LockBox`] so a single thread pool can service multiple
/// worker threads. See examples below.
///
/// # Examples
///
///```
/// use bmw_err::*;
/// use bmw_log::*;
/// use bmw_util::*;
/// use bmw_util::ConfigOption::{MaxSize, MinSize};
///
/// info!();
///
/// fn thread_pool() -> Result<(), Error> {
///     let tp = thread_pool!()?; // create a thread pool using default settings
///
///     // create a shared variable protected by the [`crate::lock`] macro.
///     let mut shared = lock!(0)?; // we use an integer 0, but any struct can be used.
///     let shared_clone = shared.clone();
///
///     // execute a task
///     let handle = execute!(tp, {
///         // obtain the lock in write mode
///         let mut shared = shared.wlock()?;
///         // increment the counter
///         (**shared.guard()) += 1;
///         // return the counter value (now it's 1)
///         Ok((**shared.guard()))
///     })?;
///
///     // block on the returned handle until it's task is complete and assert that the
///     // return value is 1.
///     assert_eq!(block_on!(handle), PoolResult::Ok(1));
///
///     // check our shared value which was updated in the thread pool.
///     let shared_clone = shared_clone.rlock()?;
///     assert_eq!((**shared_clone.guard()), 1);
///
///     Ok(())
/// }
///
/// fn thread_pool2() -> Result<(), Error> {
///     // create a thread pool with the specified max/min size. See [`crate::ThreadPoolConfig`]
///     // for further details.
///     let tp = thread_pool!(MaxSize(10), MinSize(5))?;
///
///     // put the thread pool in a [`crate::Lock`].
///     let tp = lock!(tp)?;
///
///     // spawn 6 worker threads
///     for _ in 0..6 {
///         // create a clone of our locked thread pool for each worker thread
///         let tp = tp.clone();
///         std::thread::spawn(move || -> Result<(), Error> {
///             // do some work here and pass other work to the thread pool.
///             // ...
///
///             // obtain a read lock on the thread pool
///             let tp = tp.rlock()?;
///
///             // execute the task in the thread pool, the returned handle can be used
///             // if desired or ignored
///             execute!((**tp.guard()), {
///                 info!("executing in thread pool")?;
///                 Ok(1)
///             })?;
///             Ok(())
///         });
///     }
///     Ok(())
/// }
///
///```
pub trait ThreadPool<T> {
	/// Execute a task in the thread pool. This task will run to completion
	/// on the first available thread in the pool. The return value is a receiver
	/// which will be sent a [`crate::PoolResult`] on completion of the task. If
	/// an error occurs, [`bmw_err::Error`] will be returned.
	fn execute<F>(&self, f: F) -> Result<Receiver<PoolResult<T, Error>>, Error>
	where
		F: Future<Output = Result<T, Error>> + Send + Sync + 'static;

	/// Start the pool. If macros are used, this call is unnecessary.
	fn start(&mut self) -> Result<(), Error>;

	/// Stop the thread pool. Note that this function is automatically called by
	/// the [`std::ops::Drop`] handler, but if needed, it can be explicitly called.
	/// This function, whether called through drop or directly, will ensure no new
	/// tasks are processed in the ThreadPool and that the threads will be stopped once they
	/// become idle again. It however, does not ensure that any tasks currently running in the thread pool are stopped
	/// immediately. That is the responsibility of the user.
	fn stop(&mut self) -> Result<(), Error>;

	/// Returns the current size of the thread pool which will be between
	/// [`crate::ThreadPoolConfig::min_size`] and [`crate::ThreadPoolConfig::max_size`].
	fn size(&self) -> Result<usize, Error>;
}

/// Struct that is used as a mutable refernce to data in a slab. See [`crate::SlabAllocator`] for
/// further details.
pub struct SlabMut<'a> {
	pub(crate) data: &'a mut [u8],
	pub(crate) id: usize,
}

/// Struct that is used as a immutable refernce to data in a slab. See [`crate::SlabAllocator`] for
/// further details.
pub struct Slab<'a> {
	pub(crate) data: &'a [u8],
	pub(crate) id: usize,
}

/// This trait defines the public interface to the [`crate::SlabAllocator`]. The slab
/// allocator is used by the other data structures in this crate to avoid dynamic heap
/// allocations. By itself, the slab allocator is fairly simple. It only allocates and frees
/// slabs. [`crate::SlabAllocator::get`] and [`crate::SlabAllocator::get_mut`] are also
/// provided to obtain immutable and mutable references to a slab respectively. They only
/// contain references to the data and not copies.
///
/// # Examples
///
///```
/// use bmw_err::*;
/// use bmw_util::slab_allocator;
/// use std::cell::{RefMut,Ref};
///
/// fn main() -> Result<(), Error> {
///     // build a slab allocator, in this case with defaults
///     let slabs = slab_allocator!()?;
///
///     let id = {
///         // slab allocator is stored in an Rc<RefCell>. This allows for it to be used by
///         // multiple data structures at the same time.
///         let mut slabs: RefMut<_> = slabs.borrow_mut();
///         // allocate a slab. [`crate::SlabAllocator::allocate`] returns [`crate::SlabMut`]
///         // which contains a mutable reference to the underlying data in the slab.
///         let mut slab = slabs.allocate()?;
///
///         // get the id for this slab
///         let id = slab.id();
///         // get_mut returns a mutable reference to the data in owned by the
///         // [`crate::SlabAllocator`]
///         slab.get_mut()[0] = 101;
///         id
///     };
///
///     // borrow, this time with a Ref instead of refmut since it's an immutable call.
///     let slabs: Ref<_> = slabs.borrow();
///     // now we can get an immutable reference to this slab
///     let slab = slabs.get(id)?;
///     assert_eq!(slab.get()[0], 101);
///
///     Ok(())
/// }
///```
pub trait SlabAllocator: DynClone + Debug {
	/// Allocate a slab and return a [`crate::SlabMut`] on success.
	/// On failure, return an [`bmw_err::Error`].
	///
	/// * [`bmw_err::ErrorKind::IllegalState`] if the [`crate::SlabAllocator::init`]
	/// function has not been called.
	///
	/// * [`bmw_err::ErrorKind::CapacityExceeded`] if the capacity of this
	/// [`crate::SlabAllocator`] has been exceeded.
	fn allocate<'a>(&'a mut self) -> Result<SlabMut<'a>, Error>;

	/// Free a slab that has previously been allocated by this slab allocator.
	/// `id` is the id of the slab to free. It can be obtained through the
	/// [`crate::SlabMut::id`] or [`crate::Slab::id`] function. Return a
	/// [`bmw_err::Error`] on failure.
	///
	/// *  [`bmw_err::ErrorKind::ArrayIndexOutOfBounds`] if this slab entry is
	/// too big for this instance.
	///
	/// * [`bmw_err::ErrorKind::IllegalState`] if the [`crate::SlabAllocator::init`]
	/// function has not been called or this slab was not allocated.
	///
	/// # Examples
	///
	///```
	/// use bmw_err::*;
	/// use bmw_util::slab_allocator;
	/// use bmw_util::ConfigOption::{SlabSize, SlabCount};
	///
	/// fn main() -> Result<(), Error> {
	///     // instantiate a slab allocator with a slab count of 1,000.
	///     let mut slabs = slab_allocator!(SlabSize(1_000), SlabCount(1_000))?;
	///
	///     // borrow a mutable refernce
	///     let mut slabs = slabs.borrow_mut();
	///
	///     // assert that there are 1,000 free slabs.
	///     assert_eq!(slabs.free_count()?, 1_000);
	///
	///     let slab_id = {
	///         // allocate a slab.
	///         let slab = slabs.allocate()?;
	///         slab.id()
	///     };
	///
	///     // assert that the free count has decreased by 1.
	///     assert_eq!(slabs.free_count()?, 999);
	///
	///
	///     // free the slab that was allocated
	///     slabs.free(slab_id)?;
	///
	///     // assert that the free count has returnred to the initial value of 1,000.
	///     assert_eq!(slabs.free_count()?, 1_000);
	///
	///     Ok(())
	/// }
	///```
	fn free(&mut self, id: usize) -> Result<(), Error>;

	/// Get an immutable reference to a slab that has previously been allocated by the
	/// [`crate::SlabAllocator`]. On success a [`crate::Slab`] is returned. On failure,
	/// a [`bmw_err::Error`] is returned.
	///
	/// *  [`bmw_err::ErrorKind::ArrayIndexOutOfBounds`] if this slab entry is
	/// too big for this instance.
	///
	/// * [`bmw_err::ErrorKind::IllegalState`] if the [`crate::SlabAllocator::init`]
	/// function has not been called or this slab was not allocated.
	///
	/// # Examples
	///
	///```
	/// use bmw_err::*;
	/// use bmw_log::*;
	/// use bmw_util::slab_allocator;
	/// use bmw_util::ConfigOption::{SlabSize,SlabCount};
	///
	/// info!();
	///
	/// fn main() -> Result<(), Error> {
	///     // instantiate a slab allocator with a slab count of 1,000.
	///     let mut slabs = slab_allocator!(SlabCount(1_000), SlabSize(1_000))?;
	///     
	///     // borrow a mutable refernce
	///     let mut slabs = slabs.borrow_mut();
	///
	///     // assert that there are 1,000 free slabs.
	///     assert_eq!(slabs.free_count()?, 1_000);
	///
	///     let slab_id = {
	///         // allocate a slab.
	///         let slab = slabs.allocate()?;
	///         slab.id()
	///     };
	///
	///     // assert that the free count has decreased by 1.
	///     assert_eq!(slabs.free_count()?, 999);
	///
	///
	///     // get the slab that was allocated
	///     let slab = slabs.get(slab_id)?;
	///
	///     info!("slab data = {:?}", slab.get())?;
	///
	///     Ok(())
	/// }
	///```
	fn get<'a>(&'a self, id: usize) -> Result<Slab<'a>, Error>;

	/// Get an mutable reference to a slab that has previously been allocated by the
	/// [`crate::SlabAllocator`]. On success a [`crate::SlabMut`] is returned. On failure,
	/// a [`bmw_err::Error`] is returned.
	///
	/// *  [`bmw_err::ErrorKind::ArrayIndexOutOfBounds`] if this slab entry is
	/// too big for this instance.
	///
	/// * [`bmw_err::ErrorKind::IllegalState`] if the [`crate::SlabAllocator::init`]
	/// function has not been called or this slab was not allocated.
	///
	/// # Examples
	///
	///```
	/// use bmw_err::*;
	/// use bmw_log::*;
	/// use bmw_util::slab_allocator;
	/// use bmw_util::ConfigOption::{SlabSize, SlabCount};
	///
	/// info!();
	///
	/// fn main() -> Result<(), Error> {
	///     // instantiate a slab allocator with a slab count of 1,000.
	///     let mut slabs = slab_allocator!(SlabSize(1_000), SlabCount(1_000))?;
	///
	///     // borrow a mutable refernce
	///     let mut slabs = slabs.borrow_mut();
	///
	///     // assert that there are 1,000 free slabs.
	///     assert_eq!(slabs.free_count()?, 1_000);
	///
	///     let slab_id = {
	///         // allocate a slab.
	///         let slab = slabs.allocate()?;
	///         slab.id()
	///     };
	///
	///     // assert that the free count has decreased by 1.
	///     assert_eq!(slabs.free_count()?, 999);
	///
	///
	///     // get the slab that was allocated
	///     let mut slab = slabs.get_mut(slab_id)?;
	///
	///     info!("slab data = {:?}", slab.get_mut())?;
	///
	///     Ok(())
	/// }
	///```
	fn get_mut<'a>(&'a mut self, id: usize) -> Result<SlabMut<'a>, Error>;

	/// Returns the number of free slabs this [`crate::SlabAllocator`] has remaining.
	fn free_count(&self) -> Result<usize, Error>;

	/// Returns the configured `slab_size` for this [`crate::SlabAllocator`].
	fn slab_size(&self) -> Result<usize, Error>;

	/// Returns the configured `slab_count` for this [`crate::SlabAllocator`].
	fn slab_count(&self) -> Result<usize, Error>;

	/// Initializes the [`crate::SlabAllocator`] with the given `config`. See
	/// [`crate::SlabAllocatorConfig`] for further details.
	fn init(&mut self, config: SlabAllocatorConfig) -> Result<(), Error>;
}

clone_trait_object!(SlabAllocator);

#[derive(Debug, PartialEq, Clone)]
pub struct Pattern {
	pub(crate) regex: String,
	pub(crate) is_case_sensitive: bool,
	pub(crate) is_termination_pattern: bool,
	pub(crate) is_multi_line: bool,
	pub(crate) id: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Match {
	pub(crate) start: usize,
	pub(crate) end: usize,
	pub(crate) id: usize,
}

pub trait SuffixTree {
	fn tmatch(&mut self, text: &[u8], matches: &mut [Match]) -> Result<usize, Error>;
}

/// The builder struct for the util library. All data structures are built through this
/// interface. This is used by the macros as well.
pub struct Builder {}

pub struct HashtableIterator<'a, K, V>
where
	K: Serializable + Clone,
{
	pub(crate) hashtable: &'a HashImpl<K>,
	pub(crate) cur: usize,
	pub(crate) _phantom_data: PhantomData<(K, V)>,
}

pub struct HashsetIterator<'a, K>
where
	K: Serializable + Clone,
{
	pub(crate) hashset: &'a HashImpl<K>,
	pub(crate) cur: usize,
	pub(crate) _phantom_data: PhantomData<K>,
	pub(crate) slab_reader: SlabReader,
}

pub struct ListIterator<'a, V>
where
	V: Serializable + Clone,
{
	pub(crate) linked_list_ref: Option<&'a HashImpl<V>>,
	pub(crate) array_list_ref: Option<&'a ArrayList<V>>,
	pub(crate) cur: usize,
	pub(crate) direction: Direction,
	pub(crate) _phantom_data: PhantomData<V>,
	pub(crate) slab_reader: Option<SlabReader>,
}

pub struct ArrayListIterator<'a, T> {
	pub(crate) array_list_ref: &'a ArrayList<T>,
	pub(crate) direction: Direction,
	pub(crate) cur: usize,
}

pub struct ArrayIterator<'a, T> {
	pub(crate) array_ref: &'a Array<T>,
	pub(crate) cur: usize,
}

// crate local structs/enums

#[derive(Clone, Debug)]
pub(crate) struct Node {
	pub(crate) next: [u32; 257],
	pub(crate) pattern_id: usize,
	pub(crate) is_multi: bool,
	pub(crate) is_term: bool,
	pub(crate) is_start_only: bool,
	pub(crate) is_multi_line: bool,
}

pub(crate) struct Dictionary {
	pub(crate) nodes: Vec<Node>,
	pub(crate) next: u32,
}

pub(crate) struct SuffixTreeImpl {
	pub(crate) dictionary_case_insensitive: Dictionary,
	pub(crate) dictionary_case_sensitive: Dictionary,
	pub(crate) termination_length: usize,
	pub(crate) max_wildcard_length: usize,
	pub(crate) branch_stack: Box<dyn Stack<(usize, usize)>>,
}

#[derive(Clone)]
pub(crate) struct HashImplSync<K>
where
	K: Serializable + Clone,
{
	pub(crate) static_impl: HashImpl<K>,
}

#[derive(Clone)]
pub(crate) struct HashImpl<K>
where
	K: Serializable + Clone,
{
	pub(crate) slabs: Option<Rc<RefCell<dyn SlabAllocator>>>,
	pub(crate) slab_reader: SlabReader,
	pub(crate) slab_writer: SlabWriter,
	pub(crate) max_value: usize,
	pub(crate) bytes_per_slab: usize,
	pub(crate) slab_size: usize,
	pub(crate) ptr_size: usize,
	pub(crate) entry_array: Option<Array<usize>>,
	pub(crate) size: usize,
	pub(crate) head: usize,
	pub(crate) tail: usize,
	pub(crate) max_load_factor: f64,
	pub(crate) max_entries: usize,
	pub(crate) is_hashtable: bool,
	pub(crate) _phantom_data: PhantomData<K>,
	pub(crate) debug_get_next_slot_error: bool,
	pub(crate) debug_entry_array_len: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Direction {
	Forward,
	Backward,
}

#[derive(Clone, Debug)]
pub(crate) struct SlabAllocatorImpl {
	pub(crate) config: Option<SlabAllocatorConfig>,
	pub(crate) data: Array<u8>,
	pub(crate) first_free: usize,
	pub(crate) free_count: usize,
	pub(crate) ptr_size: usize,
	pub(crate) max_value: usize,
}

#[derive(Clone)]
pub(crate) struct LockImpl<T> {
	pub(crate) t: Arc<RwLock<T>>,
	pub(crate) id: u128,
}

#[derive(Debug, Clone, Serializable)]
pub(crate) struct ThreadPoolState {
	pub(crate) waiting: usize,
	pub(crate) cur_size: usize,
	pub(crate) config: ThreadPoolConfig,
	pub(crate) stop: bool,
}

pub(crate) struct FutureWrapper<T> {
	pub(crate) f: Pin<Box<dyn Future<Output = Result<T, Error>> + Send + Sync + 'static>>,
	pub(crate) tx: SyncSender<PoolResult<T, Error>>,
}

pub(crate) struct ThreadPoolImpl<T: 'static + Send + Sync> {
	pub(crate) config: ThreadPoolConfig,
	pub(crate) rx: Option<Arc<Mutex<Receiver<FutureWrapper<T>>>>>,
	pub(crate) tx: Option<SyncSender<FutureWrapper<T>>>,
	pub(crate) state: Box<dyn LockBox<ThreadPoolState>>,
	pub(crate) test_config: Option<ThreadPoolTestConfig>,
}

pub(crate) struct ThreadPoolTestConfig {
	pub(crate) debug_drop_error: bool,
}

#[cfg(test)]
mod test {
	use crate as bmw_util;
	use crate::types::ConfigOption;
	use crate::{deserialize, serialize, slab_allocator};
	use bmw_err::*;

	#[test]
	fn test_config_option_ser() -> Result<(), Error> {
		let config_option = ConfigOption::MinSize(10);
		let mut v: Vec<u8> = vec![];
		serialize(&mut v, &config_option)?;
		let ser_in: ConfigOption = deserialize(&mut &v[..])?;
		assert!(matches!(ser_in, ConfigOption::MinSize(_)));

		let config_option = ConfigOption::MaxSize(10);
		let mut v: Vec<u8> = vec![];
		serialize(&mut v, &config_option)?;
		let ser_in: ConfigOption = deserialize(&mut &v[..])?;
		assert!(matches!(ser_in, ConfigOption::MaxSize(_)));

		let config_option = ConfigOption::SyncChannelSize(10);
		let mut v: Vec<u8> = vec![];
		serialize(&mut v, &config_option)?;
		let ser_in: ConfigOption = deserialize(&mut &v[..])?;
		assert!(matches!(ser_in, ConfigOption::SyncChannelSize(_)));

		let v = vec![7u8]; // 7 is illegal
		let err: Result<ConfigOption, Error> = deserialize(&mut &v[..]);
		assert!(err.is_err());

		let slabs = slab_allocator!()?;
		let config_option = ConfigOption::Slabs(&slabs);
		let mut v: Vec<u8> = vec![];
		assert!(serialize(&mut v, &config_option).is_err());

		Ok(())
	}
}
