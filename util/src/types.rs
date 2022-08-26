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

use crate::slabs::Slab;
use crate::slabs::SlabMut;
use crate::{SlabReader, SlabWriter};
use bmw_err::*;
use bmw_log::*;
use std::alloc::Layout;
use std::cell::RefCell;
use std::fmt::Debug;
use std::future::Future;
use std::hash::Hash;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::mpsc::Receiver;

info!();

/// Configuration options used throughout this crate via macro.
#[derive(Debug)]
pub enum ConfigOption {
	/// The maximum number of entries for a data structure. See [`crate::StaticHashtable`] and
	/// [`crate::StaticHashset`].
	MaxEntries(usize),
	/// The maximum load factor for a data structure. See [`crate::StaticHashtable`] and
	/// [`crate::StaticHashset`].
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
}

/// The configuration struct for a [`crate::ThreadPool`]. This struct is passed into the
/// [`crate::ThreadPoolBuilder::build`] function or the [`crate::thread_pool`] macro. The
/// [`std::default::Default`] trait is implemented for this trait. Also see [`crate::ConfigOption`]
/// for details on configuring via macro.
#[derive(Debug, Clone)]
pub struct ThreadPoolConfig {
	/// The minimum number of threads that this thread_pool will use. The default value is 3.
	pub min_size: usize,
	/// The maximm number of threads that this thread_pool will use. The default value is 7.
	pub max_size: usize,
	/// The size of the sync_channel buffer. See [`std::sync::mpsc::sync_channel`] for more
	/// information. The default value is 7.
	pub sync_channel_size: usize,
}

/// The configuration struct for a [`StaticHashtable`]. This struct is passed
/// into the [`crate::StaticBuilder::build_hashtable`] function. The [`std::default::Default`]
/// trait is implemented for this trait.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StaticHashtableConfig {
	/// The maximum number of entries that can exist in this [`StaticHashtable`].
	/// The default is 1_000_000. Note that the overhead for this value is 8 bytes
	/// per entry. The [`crate::StaticHashtableConfig::max_load_factor`] setting will
	/// also affect how much memory is used by the entry array.
	pub max_entries: usize,
	/// The maximum load factor for this [`crate::StaticHashtable`]. This number
	/// incidicates how full the hashtable can be. This is an array based hashtable
	/// and it is not possible to resize it after it is instantiated. The default value
	/// is 0.75.
	pub max_load_factor: f64,
}

/// The configuration struct for a [`StaticHashset`]. This struct is passed
/// into the [`crate::StaticBuilder::build_hashset`] function. The [`std::default::Default`]
/// trait is implemented for this trait.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StaticHashsetConfig {
	/// The maximum number of entries that can exist in this [`StaticHashset`].
	/// The default is 1_000_000. Note that the overhead for this value is 8 bytes
	/// per entry. So, by default 8 mb are allocated with this configuration.
	pub max_entries: usize,
	/// The maximum load factor for this [`crate::StaticHashset`]. This number
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
#[derive(Debug)]
pub struct SlabAllocatorConfig {
	/// The size, in bytes, of a slab
	pub slab_size: usize,
	/// The number of slabs that this slab allocator can allocate
	pub slab_count: usize,
}

pub struct ArrayList<T> {
	pub(crate) inner: Array<T>,
	pub(crate) size: usize,
	pub(crate) head: usize,
	pub(crate) tail: usize,
}

pub struct Array<T> {
	pub(crate) data: *mut u8,
	pub(crate) size_of_type: usize,
	pub(crate) size: usize,
	pub(crate) layout: Layout,
	pub(crate) _phantom_data: PhantomData<T>,
}

pub struct ArrayBuilder {}

#[derive(Debug, Clone)]
pub struct ListConfig {}

pub trait StaticHashtable<K, V>: PartialEq + Debug
where
	K: Serializable + Hash + PartialEq + Debug,
	V: Serializable,
{
	fn insert(&mut self, key: &K, value: &V) -> Result<(), Error>;
	fn get(&self, key: &K) -> Result<Option<V>, Error>;
	fn remove(&mut self, key: &K) -> Result<Option<V>, Error>;
	fn size(&self) -> usize;
	fn clear(&mut self) -> Result<(), Error>;
	fn iter<'a>(&'a self) -> HashtableIterator<'a, K, V>;
	fn copy(&self) -> Result<Self, Error>
	where
		Self: Sized;
}

pub trait StaticHashset<K>: PartialEq + Debug
where
	K: Serializable + Hash + PartialEq + Debug,
{
	fn insert(&mut self, key: &K) -> Result<(), Error>;
	fn contains(&self, key: &K) -> Result<bool, Error>;
	fn remove(&mut self, key: &K) -> Result<bool, Error>;
	fn size(&self) -> usize;
	fn clear(&mut self) -> Result<(), Error>;
	fn iter<'a>(&'a self) -> HashsetIterator<K>;
	fn copy(&self) -> Result<Self, Error>
	where
		Self: Sized;
}

pub trait Queue<V> {
	fn enqueue(&mut self, value: V) -> Result<(), Error>;
	fn dequeue(&mut self) -> Option<&V>;
	fn peek(&self) -> Option<&V>;
}

pub trait Stack<V> {
	fn push(&mut self, value: V) -> Result<(), Error>;
	fn pop(&mut self) -> Option<&V>;
	fn peek(&self) -> Option<&V>;
}

pub trait List<V>: PartialEq + Debug {
	fn push(&mut self, value: V) -> Result<(), Error>;
	fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = V> + 'a>;
	fn iter_rev<'a>(&'a self) -> Box<dyn Iterator<Item = V> + 'a>;
	fn size(&self) -> usize;
	fn clear(&mut self) -> Result<(), Error>;
	fn append(&mut self, list: &impl List<V>) -> Result<(), Error>;
	fn copy(&self) -> Result<Self, Error>
	where
		Self: Sized;
}

pub trait SortableList<V>: List<V>
where
	V: Serializable + Ord,
{
	fn sort(&mut self) -> Result<(), Error>;
}

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

unsafe impl<T, E> Send for PoolResult<T, E> {}
unsafe impl<T, E> Sync for PoolResult<T, E> {}

/// This trait defines the public interface to the ThreadPool. A pool can be configured
/// via the [`crate::ThreadPoolConfig`] struct. The thread pool should be accessed through the
/// macros under normal circumstances. See [`crate::thread_pool`], [`crate::execute`] and
/// [`crate::block_on`] for additional details. The thread pool can be passed through threads via a
/// [`bmw_log::Lock`] or [`bmw_log::LockBox`] so a single thread pool can service multiple
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
///     // create a shared variable protected by the [`bmw_log::lock`] macro.
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
///     // put the thread pool in a [`bmw_log::Lock`].
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
pub trait SlabAllocator {
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

pub trait Match {
	fn start(&self) -> usize;
	fn end(&self) -> usize;
	fn id(&self) -> usize;
	fn set_start(&mut self, start: usize) -> Result<(), Error>;
	fn set_end(&mut self, end: usize) -> Result<(), Error>;
	fn set_id(&mut self, id: usize) -> Result<(), Error>;
}

#[derive(Debug, PartialEq)]
pub struct Pattern {
	pub(crate) regex: String,
	pub(crate) is_case_sensitive: bool,
	pub(crate) is_termination_pattern: bool,
	pub(crate) id: usize,
}

pub trait SuffixTree {
	fn run_matches(&mut self, text: &[u8], matches: &mut [Box<dyn Match>]) -> Result<usize, Error>;
}

pub struct MatchBuilder {}

/// Writer trait used to serializing data.
pub trait Writer {
	fn write_u8(&mut self, n: u8) -> Result<(), Error> {
		self.write_fixed_bytes(&[n])
	}

	fn write_i8(&mut self, n: i8) -> Result<(), Error> {
		self.write_fixed_bytes(&[n as u8])
	}

	fn write_u16(&mut self, n: u16) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_i16(&mut self, n: i16) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_u32(&mut self, n: u32) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_i32(&mut self, n: i32) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_u64(&mut self, n: u64) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_i128(&mut self, n: i128) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_u128(&mut self, n: u128) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_i64(&mut self, n: i64) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_usize(&mut self, n: usize) -> Result<(), Error> {
		self.write_fixed_bytes(n.to_be_bytes())
	}

	fn write_bytes<T: AsRef<[u8]>>(&mut self, bytes: T) -> Result<(), Error> {
		self.write_u64(bytes.as_ref().len() as u64)?;
		self.write_fixed_bytes(bytes)
	}

	fn write_fixed_bytes<T: AsRef<[u8]>>(&mut self, bytes: T) -> Result<(), Error>;

	fn write_empty_bytes(&mut self, length: usize) -> Result<(), Error> {
		for _ in 0..length {
			self.write_u8(0)?;
		}
		Ok(())
	}
}

/// Reader trait used for deserializing data.
pub trait Reader {
	fn read_u8(&mut self) -> Result<u8, Error>;
	fn read_i8(&mut self) -> Result<i8, Error>;
	fn read_i16(&mut self) -> Result<i16, Error>;
	fn read_u16(&mut self) -> Result<u16, Error>;
	fn read_u32(&mut self) -> Result<u32, Error>;
	fn read_u64(&mut self) -> Result<u64, Error>;
	fn read_u128(&mut self) -> Result<u128, Error>;
	fn read_i128(&mut self) -> Result<i128, Error>;
	fn read_i32(&mut self) -> Result<i32, Error>;
	fn read_i64(&mut self) -> Result<i64, Error>;
	fn read_fixed_bytes(&mut self, buf: &mut [u8]) -> Result<(), Error>;
	fn read_usize(&mut self) -> Result<usize, Error>;
	fn expect_u8(&mut self, val: u8) -> Result<u8, Error>;

	fn read_empty_bytes(&mut self, length: usize) -> Result<(), Error> {
		for _ in 0..length {
			if self.read_u8()? != 0u8 {
				return Err(err!(ErrKind::CorruptedData, "expected 0u8"));
			}
		}
		Ok(())
	}
}

/// This is the trait used by all data structures to serialize and deserialize data.
/// Anthing stored in them must implement this trait. Commonly needed implementations
/// are built in the ser module in this crate. These include Vec, String, integer types among
/// other things.
pub trait Serializable
where
	Self: Sized,
{
	/// read data from the reader and build the underlying type represented by that
	/// data.
	fn read<R: Reader>(reader: &mut R) -> Result<Self, Error>;
	/// write data to the writer representing the underlying type.
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error>;
}

pub struct StaticBuilder {}

/// The builder for [`ThreadPool`].
pub struct ThreadPoolBuilder {}

pub struct HashtableIterator<'a, K, V>
where
	K: Serializable,
{
	pub(crate) hashtable: &'a StaticImpl<K>,
	pub(crate) cur: usize,
	pub(crate) _phantom_data: PhantomData<(K, V)>,
}

pub struct HashsetIterator<'a, K>
where
	K: Serializable,
{
	pub(crate) hashset: &'a StaticImpl<K>,
	pub(crate) cur: usize,
	pub(crate) _phantom_data: PhantomData<K>,
	pub(crate) slab_reader: SlabReader,
}

pub struct ListIterator<'a, V>
where
	V: Serializable,
{
	pub(crate) list: &'a StaticImpl<V>,
	pub(crate) cur: usize,
	pub(crate) direction: Direction,
	pub(crate) _phantom_data: PhantomData<V>,
	pub(crate) slab_reader: SlabReader,
}

pub(crate) struct StaticImpl<K>
where
	K: Serializable,
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
	pub(crate) _phantom_data: PhantomData<K>,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Direction {
	Forward,
	Backward,
}
