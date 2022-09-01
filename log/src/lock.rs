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

use bmw_deps::dyn_clone::DynClone;
use bmw_deps::rand::random;
use bmw_err::{err, map_err, ErrKind, Error};
use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[cfg(not(tarpaulin_include))]
thread_local! {
	pub static LOCKS: RefCell<HashSet<u128>> = RefCell::new(HashSet::new());
}

/// Macro to get a [`crate::Lock`]. Internally, the parameter passed in is wrapped in
/// an Arc<Rwlock<T>> wrapper that can be used to obtain read/write locks around any
/// data structure.
///
/// # Examples
///
///```
/// use bmw_err::*;
/// use bmw_log::*;
/// use std::time::Duration;
/// use std::thread::{sleep, spawn};
///
/// #[derive(Debug, PartialEq)]
/// struct MyStruct {
///     id: u128,
///     name: String,
/// }
///
/// impl MyStruct {
///     fn new(id: u128, name: String) -> Self {
///         Self { id, name }
///     }
/// }
///
/// fn main() -> Result<(), Error> {
///     let v = MyStruct::new(1234, "joe".to_string());
///     let mut vlock = lock!(v)?;
///     let vlock_clone = vlock.clone();
///
///     spawn(move || -> Result<(), Error> {
///         let mut x = vlock.wlock()?;
///         assert_eq!((**(x.guard())).id, 1234);
///         sleep(Duration::from_millis(3000));
///         (**(x.guard())).id = 4321;
///         Ok(())
///     });
///
///     sleep(Duration::from_millis(1000));
///     let x = vlock_clone.rlock()?;
///     assert_eq!((**(x.guard())).id, 4321);
///
///     Ok(())
/// }
///```
#[macro_export]
macro_rules! lock {
	($value:expr) => {{
		bmw_log::LockBuilder::build($value)
	}};
}

/// The same as lock except that the value returned is in a Box<dyn LockBox<T>> structure.
/// See [`crate::LockBox`] for a working example.
#[macro_export]
macro_rules! lock_box {
	($value:expr) => {{
		bmw_log::LockBuilder::build_box($value)
	}};
}

/// Wrapper around the lock functionalities used by bmw in [`std::sync`] rust libraries.
/// The main benefits are the simplified interface and the fact that if a thread attempts
/// to obtain a lock twice, an error will be thrown instead of a thread panic. This is implemented
/// through a thread local Hashset which keeps track of the guards used by the lock removes an
/// entry for them when the guard is dropped.
///
/// # Examples
///
///```
/// use bmw_log::*;
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
/// use bmw_log::*;
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
	guard: RwLockReadGuard<'a, T>,
	id: u128,
	debug_err: bool,
}

impl<'a, T> RwLockReadGuardWrapper<'a, T>
where
	T: Send + Sync,
{
	#[cfg(not(tarpaulin_include))]
	pub fn guard(&self) -> &RwLockReadGuard<'a, T> {
		&self.guard
	}
}

impl<T> Drop for RwLockReadGuardWrapper<'_, T> {
	fn drop(&mut self) {
		let id = self.id;

		let res = LOCKS.with(|f| -> Result<(), Error> {
			(*f.borrow_mut()).remove(&id);
			Ok(())
		});
		if res.is_err() || self.debug_err {
			println!("error dropping read lock: {:?}", res);
		}
	}
}

/// Wrapper around the [`std::sync::RwLockWriteGuard`].
pub struct RwLockWriteGuardWrapper<'a, T> {
	guard: RwLockWriteGuard<'a, T>,
	id: u128,
	debug_err: bool,
}

impl<'a, T> RwLockWriteGuardWrapper<'a, T> {
	#[cfg(not(tarpaulin_include))]
	pub fn guard(&mut self) -> &mut RwLockWriteGuard<'a, T> {
		&mut self.guard
	}
}

impl<T> Drop for RwLockWriteGuardWrapper<'_, T> {
	fn drop(&mut self) {
		let id = self.id;

		let res = LOCKS.with(|f| -> Result<(), Error> {
			(*f.borrow_mut()).remove(&id);
			Ok(())
		});

		if res.is_err() || self.debug_err {
			println!("error dropping write lock: {:?}", res);
		}
	}
}

#[derive(Clone)]
struct LockImpl<T> {
	t: Arc<RwLock<T>>,
	id: u128,
}

impl<T> Debug for LockImpl<T> {
	fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
		write!(f, "LockImpl<{}>", self.id)
	}
}

impl<T> Lock<T> for LockImpl<T>
where
	T: Send + Sync,
{
	fn wlock(&mut self) -> Result<RwLockWriteGuardWrapper<'_, T>, Error> {
		self.do_wlock()
	}

	fn rlock(&self) -> Result<RwLockReadGuardWrapper<'_, T>, Error> {
		self.do_rlock()
	}

	fn clone(&self) -> Self {
		Self {
			t: self.t.clone(),
			id: self.id,
		}
	}
}

impl<T> LockBox<T> for LockImpl<T>
where
	T: Send + Sync + Clone,
{
	fn wlock(&mut self) -> Result<RwLockWriteGuardWrapper<'_, T>, Error> {
		self.do_wlock()
	}

	fn rlock(&self) -> Result<RwLockReadGuardWrapper<'_, T>, Error> {
		self.do_rlock()
	}
}

impl<T> LockImpl<T> {
	fn new(t: T) -> Self {
		Self {
			t: Arc::new(RwLock::new(t)),
			id: random(),
		}
	}

	fn do_wlock(&mut self) -> Result<RwLockWriteGuardWrapper<'_, T>, Error> {
		let contains = LOCKS.with(|f| -> Result<bool, Error> {
			let ret = (*f.borrow()).contains(&self.id);
			(*f.borrow_mut()).insert(self.id);

			Ok(ret)
		})?;
		if contains {
			Err(err!(ErrKind::Poison, "would deadlock"))
		} else {
			let guard = map_err!(self.t.write(), ErrKind::Poison)?;
			let id = self.id;
			let debug_err = false;
			let ret = RwLockWriteGuardWrapper {
				guard,
				id,
				debug_err,
			};
			Ok(ret)
		}
	}

	fn do_rlock(&self) -> Result<RwLockReadGuardWrapper<'_, T>, Error> {
		let contains = LOCKS.with(|f| -> Result<bool, Error> {
			let ret = (*f.borrow()).contains(&self.id);
			(*f.borrow_mut()).insert(self.id);

			Ok(ret)
		})?;
		if contains {
			Err(err!(ErrKind::Poison, "would deadlock"))
		} else {
			let guard = map_err!(self.t.read(), ErrKind::Poison)?;
			let id = self.id;
			let debug_err = false;
			let ret = RwLockReadGuardWrapper {
				guard,
				id,
				debug_err,
			};
			Ok(ret)
		}
	}
}

/// Builder for [`crate::Lock`]. This is the only way that a [`crate::Lock`] can be built from
/// outside this crate.
pub struct LockBuilder {}

impl LockBuilder {
	pub fn build<T>(t: T) -> Result<impl Lock<T>, Error>
	where
		T: Send + Sync,
	{
		Ok(LockImpl::new(t))
	}

	pub fn build_box<T>(t: T) -> Result<Box<dyn LockBox<T>>, Error>
	where
		T: Send + Sync + Clone + 'static,
	{
		Ok(Box::new(LockImpl::new(t)))
	}
}

#[cfg(test)]
mod test {
	use crate as bmw_log;
	use crate::lock::Lock;
	use crate::lock::LockBox;
	use crate::lock::LockBuilder;
	use crate::{RwLockReadGuardWrapper, RwLockWriteGuardWrapper};
	use bmw_deps::dyn_clone::clone_box;
	use bmw_err::Error;
	use bmw_log::lock;
	use std::sync::{Arc, RwLock};
	use std::thread::{sleep, spawn};
	use std::time::Duration;

	#[test]
	fn test_locks() -> Result<(), Error> {
		let mut lock = LockBuilder::build(1)?;
		let mut lock2 = lock.clone();
		{
			let x = lock.rlock()?;
			println!("x={}", *x.guard());
		}
		{
			let mut y = lock.wlock()?;
			**(y.guard()) = 2;

			assert!(lock2.wlock().is_err());
		}

		{
			let mut z = lock.wlock()?;
			assert_eq!(**(z.guard()), 2);
		}

		Ok(())
	}

	#[test]
	fn test_read_deadlock() -> Result<(), Error> {
		let mut lock = LockBuilder::build(1)?;
		let lock2 = lock.clone();
		{
			let x = lock.rlock()?;
			println!("x={}", *x.guard());
		}
		{
			let mut y = lock.wlock()?;
			**(y.guard()) = 2;

			assert!(lock2.rlock().is_err());
		}

		{
			let mut z = lock.wlock()?;
			assert_eq!(**(z.guard()), 2);
		}
		Ok(())
	}

	#[test]
	fn test_lock_threads() -> Result<(), Error> {
		let mut lock = LockBuilder::build(1)?;
		let mut lock_clone = lock.clone();

		spawn(move || -> Result<(), Error> {
			let mut x = lock.wlock()?;
			sleep(Duration::from_millis(3000));
			**(x.guard()) = 2;
			Ok(())
		});

		sleep(Duration::from_millis(1000));
		let mut x = lock_clone.wlock()?;
		assert_eq!(**(x.guard()), 2);

		Ok(())
	}

	#[test]
	fn test_lock_macro() -> Result<(), Error> {
		let mut lock = lock!(1)?;
		let lock_clone = lock.clone();
		println!("lock={:?}", lock);

		spawn(move || -> Result<(), Error> {
			let mut x = lock.wlock()?;
			assert_eq!(**(x.guard()), 1);
			sleep(Duration::from_millis(3000));
			**(x.guard()) = 2;
			Ok(())
		});

		sleep(Duration::from_millis(1000));
		let x = lock_clone.rlock()?;
		assert_eq!(**(x.guard()), 2);

		Ok(())
	}

	struct TestLockBox<T> {
		lock_box: Box<dyn LockBox<T>>,
	}

	#[test]
	fn test_lock_box() -> Result<(), Error> {
		let lock_box = lock_box!(1u32)?;
		let mut lock_box2 = clone_box(&*lock_box);
		let mut tlb = TestLockBox { lock_box };
		{
			let mut tlb = tlb.lock_box.wlock()?;
			(**tlb.guard()) = 2u32;
		}

		{
			let tlb = tlb.lock_box.rlock()?;
			assert_eq!((**tlb.guard()), 2u32);
		}

		{
			let mut tlb = lock_box2.wlock()?;
			assert_eq!((**tlb.guard()), 2u32);
			(**tlb.guard()) = 3u32;
		}

		{
			let tlb = tlb.lock_box.rlock()?;
			assert_eq!((**tlb.guard()), 3u32);
		}

		Ok(())
	}

	#[test]
	fn test_rw_guards() -> Result<(), Error> {
		{
			let lock = Arc::new(RwLock::new(1));
			let guard = lock.read().unwrap();
			let x = RwLockReadGuardWrapper {
				guard,
				id: 0,
				debug_err: true,
			};
			let guard = x.guard();
			assert_eq!(**guard, 1);
		}
		{
			let lock = Arc::new(RwLock::new(1));
			let guard = lock.write().unwrap();
			let mut x = RwLockWriteGuardWrapper {
				guard,
				id: 0,
				debug_err: true,
			};
			let guard = x.guard();
			assert_eq!(**guard, 1);
		}
		Ok(())
	}
}
