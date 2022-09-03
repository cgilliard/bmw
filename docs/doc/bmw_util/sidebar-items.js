window.SIDEBAR_ITEMS = {"enum":[["ConfigOption","Configuration options used throughout this crate via macro."],["PatternParam","The enum used to define patterns. See [`crate::pattern`] for more info."],["PoolResult","The result returned from a call to [`crate::ThreadPool::execute`]. This is similar to [`std::result::Result`] except that it implements [`std::marker::Send`] and [`std::marker::Sync`] so that it can be passed through threads. Also a type [`crate::PoolResult::Panic`] is returned if a thread panic occurs in the thread pool."],["SuffixParam","The enum used to define a suffix_tree. See [`crate::suffix_tree!`] for more info."]],"fn":[["deserialize","Deserializes a Serializable from any std::io::Read implementation."],["serialize","Serializes a Serializable into any std::io::Write implementation."]],"macro":[["array","The [`crate::array!`] macro builds an [`crate::Array`]. The macro takes the following parameters:"],["array_list","The [`crate::array_list`] macro builds an [`crate::ArrayList`] in the form of a impl SortableList. The macro takes the following parameters:"],["array_list_box","This macro is identical to [`crate::array_list`] except that the value is returned in a box. To be exact, the return value is `Box<dyn SortableList>`. The boxed version can then be used to store in structs and enums. See [`crate::array_list`] for more details and an example."],["block_on","Macro used to block until a thread pool has completed the task. See [`crate::ThreadPool`] for working examples."],["execute","Macro used to execute tasks in a thread pool. See [`crate::ThreadPool`] for working examples."],["global_slab_allocator","The `global_slab_allocator` macro initializes the global thread local slab allocator for the thread that it is executed in. It takes the following parameters:"],["hashset","The [`crate::hashset`] macro builds a [`crate::Hashset`] with the specified configuration and optionally the specified [`crate::SlabAllocator`]. The macro accepts the following parameters:"],["hashset_box","The [`crate::hashset_box`] macro is the same as the [`crate::hashset`] macro except that the hashset is returned in a box. See [`crate::hashset`]."],["hashset_sync","The hashset_sync macro is the same as [`crate::hashset`] except that the returned Hashset implements Send and Sync and can be safely passed through threads. See [`crate::hashtable_sync`] for further details."],["hashset_sync_box","The hashset_sync_box macro is the boxed version of the [`crate::hashset_sync`] macro. It is the same except that the returned [`crate::Hashset`] is in a Box so it can be added to structs and enums."],["hashtable","The [`crate::hashtable`] macro builds a [`crate::Hashtable`] with the specified configuration and optionally the specified [`crate::SlabAllocator`]. The macro accepts the following parameters:"],["hashtable_box","The [`crate::hashtable_box`] macro builds a [`crate::Hashtable`] with the specified configuration and optionally the specified [`crate::SlabAllocator`]. The only difference between this macro and the [`crate::hashtable`] macro is that the returned hashtable is inserted into a Box. Specifically, the return type is a `Box<dyn Hashtable>`. See [`crate::hashtable`] for further details."],["hashtable_sync","The difference between this macro and the [`crate::hashtable`] macro is that the returned [`crate::Hashtable`] implements the Send and Sync traits and is thread safe. With this hashtable you cannot specify a [`crate::SlabAllocator`] because they use [`std::cell::RefCell`] which is not thread safe. That is also why this macro returns an error if [`crate::ConfigOption::Slabs`] is specified. The parameters for this macro are:"],["hashtable_sync_box","This macro is the same as [`hashtable_sync`] except that the returned hashtable is in a Box. This macro can be used if the sync hashtable needs to be placed in a struct or an enum. See [`crate::hashtable`] and [`crate::hashtable_sync`] for further details."],["list","The list macro is used to create lists. This macro uses the global slab allocator. To use a specified slab allocator, see [`crate::Builder::build_list`]. It has the same syntax as the [`std::vec!`] macro. Note that this macro and the builder function both return an implementation of the [`crate::SortableList`] trait."],["list_append","Append list2 to list1."],["list_box","This is the boxed version of list. The returned value is `Box<dyn SortableList>`. Otherwise, this macro is identical to [`crate::list`]."],["list_eq","Compares equality of list1 and list2."],["list_sync","Like [`crate::hashtable_sync`] and [`crate::hashset_sync`] list has a ‘sync’ version. See those macros for more details and see the [`crate`] for an example of the sync version of a hashtable. Just as in that example the list can be put into a [`crate::lock!`] or [`crate::lock_box`] and passed between threads."],["list_sync_box","Box version of the [`crate::list_sync`] macro."],["lock","Macro to get a [`crate::Lock`]. Internally, the parameter passed in is wrapped in an Arc<Rwlock> wrapper that can be used to obtain read/write locks around any data structure."],["lock_box","The same as lock except that the value returned is in a Box<dyn LockBox> structure. See [`crate::LockBox`] for a working example."],["pattern","The pattern macro builds a [`crate::Pattern`] which is used by the [`crate::SuffixTree`]. The pattern macro takes the following parameters:"],["queue","This macro creates a [`crate::Queue`]. The parameters are"],["queue_box","This is the box version of [`crate::queue`]. It is identical other than the returned value is in a box `(Box<dyn Queue>)`."],["slab_allocator","The `slab_allocator` macro initializes a slab allocator with the specified parameters. It takes the following parameters:"],["stack","This macro creates a [`crate::Stack`]. The parameters are"],["stack_box","This is the box version of [`crate::stack`]. It is identical other than the returned value is in a box `(Box<dyn Stack>)`."],["suffix_tree","The `suffix_tree` macro builds a [`crate::SuffixTree`] which can be used to match multiple patterns for a given text in a performant way. The suffix_tree macro takes the following parameters:"],["thread_pool","Macro used to configure/build a thread pool. See [`crate::ThreadPool`] for working examples."]],"struct":[["Array","The [`crate::Array`] is essentially a wrapper around [`std::vec::Vec`]. It is mainly used to prevent post initialization heap allocations. In many cases resizing/growing of the vec is not needed and adding this wrapper assures that they do not happen. There are use cases where growing is useful and in fact in the library we use Vec for the suffix tree, but in most cases where contiguous memory is used, the primary purpose is for indexing. That can be done with an array. So we add this wrapper."],["ArrayIterator","An iterator for the [`crate::Array`]."],["ArrayList","[`crate::ArrayList`] is an array based implementation of the [`crate::List`] and [`crate::SortableList`] trait. It uses the [`crate::Array`] as it’s underlying storage mechanism. In most cases it is more performant than the LinkedList implementation, but it does do a heap allocation when created. See the module level documentation for an example."],["BinReader","Utility wrapper for an underlying byte Reader. Defines higher level methods to write numbers, byte vectors, hashes, etc."],["BinWriter","Utility wrapper for an underlying byte Writer. Defines higher level methods to write numbers, byte vectors, hashes, etc."],["Builder","The builder struct for the util library. All data structures are built through this interface. This is used by the macros as well."],["HashsetConfig","The configuration struct for a [`Hashset`]. This struct is passed into the [`crate::Builder::build_hashset`] function. The [`std::default::Default`] trait is implemented for this trait."],["HashsetIterator","An iterator for the [`crate::Hashset`]."],["HashtableConfig","The configuration struct for a [`Hashtable`]. This struct is passed into the [`crate::Builder::build_hashtable`] function. The [`std::default::Default`] trait is implemented for this trait."],["HashtableIterator","An iterator for the [`crate::Hashtable`]."],["ListConfig","Configuration for Lists currently there are no parameters, but it is still used to stay consistent with [`crate::Hashtable`] and [`crate::Hashset`]."],["ListIterator","An iterator for the [`crate::List`]."],["Match","A match which is returned by the suffix tree. See [`crate::suffix_tree!`]."],["Pattern","A pattern which is used with the suffix tree. See [`crate::suffix_tree!`]."],["RwLockReadGuardWrapper","Wrapper around the [`std::sync::RwLockReadGuard`]."],["RwLockWriteGuardWrapper","Wrapper around the [`std::sync::RwLockWriteGuard`]."],["Slab","Struct that is used as a immutable reference to data in a slab. See [`crate::SlabAllocator`] for further details."],["SlabAllocatorConfig","Slab Allocator configuration struct. This struct is the input to the [`crate::SlabAllocator::init`] function. The two parameters are `slab_size` which is the size of the slabs in bytes allocated by this [`crate::SlabAllocator`] and `slab_count` which is the number of slabs that can be allocated by this [`crate::SlabAllocator`]."],["SlabMut","Struct that is used as a mutable reference to data in a slab. See [`crate::SlabAllocator`] for further details."],["SlabReader","Utility to read from slabs using the [`bmw_ser::Reader`] trait."],["SlabWriter","Utility to write to slabs using the [`bmw_ser::Writer`] trait."],["ThreadPoolConfig","The configuration struct for a [`crate::ThreadPool`]. This struct is passed into the [`crate::Builder::build_thread_pool`] function or the [`crate::thread_pool`] macro. The [`std::default::Default`] trait is implemented for this trait. Also see [`crate::ConfigOption`] for details on configuring via macro."]],"trait":[["Hashset","A slab allocated Hashset. Most of the implementation is shared with [`crate::Hashtable`]. See [`crate::Hashtable`] for a discussion of the slab layout. The difference is that as is the case with hashsets, there is no value."],["Hashtable","A slab allocated hashtable. Data is stored in a [`crate::SlabAllocator`] defined by the user or using a global thread local slab allocator. All keys and values must implement the [`bmw_ser::Serializable`] trait which can be implemented with the [`bmw_derive::Serializable`] proc_macro."],["List","A trait that defines a list. Both an array list and a linked list are implemented the default [`crate::list`] macro returns the linked list. The [`crate::array_list`] macro returns the array list. Both implement this trait."],["Lock","Wrapper around the lock functionalities used by bmw in [`std::sync`] rust libraries. The main benefits are the simplified interface and the fact that if a thread attempts to obtain a lock twice, an error will be thrown instead of a thread panic. This is implemented through a thread local Hashset which keeps track of the guards used by the lock removes an entry for them when the guard is dropped."],["LockBox","[`crate::LockBox`] is the same as [`crate::Lock`] except that it is possible to build The LockBox into a Box<dyn LockBox> structure so that it is object safe. It can then be cloned using DynClone."],["Queue","This trait defines a queue. The implementation is a bounded queue. The queue uses an [`crate::Array`] as the underlying storage mechanism."],["Reader","Reader trait used for deserializing data."],["Serializable","This is the trait used by all data structures to serialize and deserialize data. Anything stored in them must implement this trait. Commonly needed implementations are built in the ser module in this crate. These include Vec, String, integer types among other things."],["SlabAllocator","This trait defines the public interface to the [`crate::SlabAllocator`]. The slab allocator is used by the other data structures in this crate to avoid dynamic heap allocations. By itself, the slab allocator is fairly simple. It only allocates and frees slabs. [`crate::SlabAllocator::get`] and [`crate::SlabAllocator::get_mut`] are also provided to obtain immutable and mutable references to a slab respectively. They only contain references to the data and not copies."],["SortableList","A trait that defines a sortable list. Both implementations implement this trait, although the linked list merely copies the data into an array list to sort. The array_list can natively sort with rust’s sort implementations using the functions associated with slice."],["Stack","This trait defines a stack. The implementation is a bounded stack. The stack uses an [`crate::Array`] as the underlying storage mechanism."],["SuffixTree","The suffix tree data structure. See [`crate::suffix_tree`]."],["ThreadPool","This trait defines the public interface to the ThreadPool. A pool can be configured via the [`crate::ThreadPoolConfig`] struct. The thread pool should be accessed through the macros under normal circumstances. See [`crate::thread_pool`], [`crate::execute`] and [`crate::block_on`] for additional details. The thread pool can be passed through threads via a [`crate::Lock`] or [`crate::LockBox`] so a single thread pool can service multiple worker threads. See examples below."],["Writer","Writer trait used to serializing data."]]};