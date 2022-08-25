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
use crate::{Array, ArrayBuilder, ArrayList, List, Queue};
use bmw_deps::try_traits::clone::TryClone;
use bmw_err::{err, ErrKind, Error};
use bmw_log::*;
use std::alloc::{alloc, dealloc, Layout};
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};
use std::ptr::copy_nonoverlapping;

info!();

impl<T> Array<T> {
	fn new(size: usize) -> Result<Self, Error> {
		let n = ::std::mem::size_of::<T>();
		let layout = Layout::from_size_align(size * n, n).unwrap();
		let data = unsafe { alloc(layout) };
		debug!("n={}", n)?;
		Ok(Self {
			data,
			size_of_type: n,
			size,
			layout,
			_phantom_data: PhantomData,
		})
	}

	pub fn size(&self) -> usize {
		self.size
	}
}

impl<T> Debug for Array<T>
where
	T: Debug,
{
	fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), fmt::Error> {
		for i in 0..self.size {
			if i == 0 {
				write!(fmt, "[{:?}", self[i])?;
			} else {
				write!(fmt, ", {:?}", self[i])?;
			}
		}
		write!(fmt, "]")?;
		Ok(())
	}
}

impl<T> PartialEq for Array<T>
where
	T: PartialEq,
{
	fn eq(&self, rhs: &Self) -> bool {
		if self.size != rhs.size {
			false
		} else {
			for i in 0..self.size {
				let ptr1: *mut u8 = unsafe { self.data.add(i * self.size_of_type) };
				let ptr2: *mut u8 = unsafe { rhs.data.add(i * self.size_of_type) };
				let ref1: *mut T = ptr1 as *mut T;
				let ref2: *mut T = ptr2 as *mut T;
				let ref1: &T = unsafe { &*ref1 };
				let ref2: &T = unsafe { &*ref2 };
				if ref1 != ref2 {
					return false;
				}
			}
			true
		}
	}
}

impl<T> TryClone for Array<T> {
	type Error = Error;
	fn try_clone(&self) -> Result<Self, Error> {
		let layout = self.layout;
		let data = unsafe { alloc(layout) };
		let size = self.size;
		let size_of_type = self.size_of_type;

		unsafe {
			copy_nonoverlapping(self.data, data, size * size_of_type);
		}

		Ok(Self {
			data,
			size_of_type,
			layout,
			size,
			_phantom_data: PhantomData,
		})
	}
}

impl<T> Drop for Array<T> {
	fn drop(&mut self) {
		unsafe {
			dealloc(self.data, self.layout);
		}
	}
}

impl<T> IndexMut<usize> for Array<T> {
	fn index_mut(&mut self, index: usize) -> &mut <Self as Index<usize>>::Output {
		if index >= self.size {
			let fmt = format!(
				"ArrayIndexOutOfBounds: index = {}, size = {}",
				index, self.size
			);
			panic!("{}", fmt);
		}
		let ptr = unsafe { self.data.add(index * self.size_of_type) };
		unsafe { ptr.cast::<T>().as_mut().unwrap() }
	}
}

impl<T> Index<usize> for Array<T> {
	type Output = T;
	fn index(&self, index: usize) -> &<Self as Index<usize>>::Output {
		if index >= self.size {
			let fmt = format!(
				"ArrayIndexOutOfBounds: index = {}, size = {}",
				index, self.size
			);
			panic!("{}", fmt);
		}
		let ptr = unsafe { self.data.add(index * self.size_of_type) };
		unsafe { ptr.cast::<T>().as_ref().unwrap() }
	}
}

impl<T> ArrayList<T> {
	fn new(size: usize) -> Result<Self, Error> {
		let inner = Array::new(size)?;
		Ok(Self { inner, size: 0 })
	}
}

impl<T> Debug for ArrayList<T>
where
	T: Debug,
{
	fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
		write!(fmt, "{:?}", self.inner)
	}
}

impl<T> PartialEq for ArrayList<T>
where
	T: PartialEq,
{
	fn eq(&self, rhs: &ArrayList<T>) -> bool {
		for i in 0..self.size {
			if self.inner[i] != rhs.inner[i] {
				return false;
			}
		}

		true
	}
}

#[derive(PartialEq)]
enum Direction {
	Forward,
	Backward,
}

impl<T> List<T> for ArrayList<T>
where
	T: PartialEq + Debug + Serializable + Clone,
{
	fn push(&mut self, value: T) -> Result<(), Error> {
		if self.size() >= self.inner.size {
			return Err(err!(
				ErrKind::CapacityExceeded,
				format!("ArrayList capacity exceeded: {}", self.inner.size)
			));
		}
		self.inner[self.size] = value;
		self.size += 1;
		Ok(())
	}
	fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = T> + 'a> {
		Box::new(ArrayListIterator {
			array_list_ref: &self,
			cur: 0,
			direction: Direction::Forward,
		})
	}
	fn iter_rev<'a>(&'a self) -> Box<dyn Iterator<Item = T> + 'a> {
		Box::new(ArrayListIterator {
			array_list_ref: &self,
			cur: self.size.saturating_sub(1),
			direction: Direction::Backward,
		})
	}
	fn size(&self) -> usize {
		self.size
	}
	fn clear(&mut self) -> Result<(), Error> {
		self.size = 0;
		Ok(())
	}
	fn append(&mut self, list: &impl List<T>) -> Result<(), Error> {
		for x in list.iter() {
			self.push(x)?;
		}
		Ok(())
	}
	fn copy(&self) -> Result<Self, Error> {
		todo!()
	}
}

impl<T> Queue<T> for ArrayList<T> {
	fn enqueue(&mut self, _value: &T) -> Result<(), Error> {
		todo!()
	}
	fn dequeue(&mut self) -> Result<Option<&T>, Error> {
		todo!()
	}
	fn peek(&self) -> Result<Option<&T>, Error> {
		todo!()
	}
}

struct ArrayListIterator<'a, T> {
	array_list_ref: &'a ArrayList<T>,
	cur: usize,
	direction: Direction,
}

pub struct ArrayIterator<'a, T> {
	array_ref: &'a Array<T>,
	cur: usize,
}

impl<'a, T> Iterator for ArrayListIterator<'a, T>
where
	T: Clone,
{
	type Item = T;
	fn next(&mut self) -> Option<<Self as Iterator>::Item> {
		if self.array_list_ref.size == 0 {
			None
		} else if self.direction == Direction::Forward && self.cur >= self.array_list_ref.size {
			None
		} else if self.direction == Direction::Backward && self.cur <= 0 {
			None
		} else {
			let ret = Some(self.array_list_ref.inner[self.cur].clone());
			if self.direction == Direction::Forward {
				self.cur += 1;
			} else {
				self.cur = self.cur.saturating_sub(1);
			}
			ret
		}
	}
}

impl<'a, T> Iterator for ArrayIterator<'a, T> {
	type Item = &'a T;
	fn next(&mut self) -> Option<<Self as Iterator>::Item> {
		if self.cur >= self.array_ref.size {
			None
		} else {
			self.cur += 1;
			Some(&self.array_ref[self.cur - 1])
		}
	}
}

impl<'a, T: 'static> IntoIterator for &'a Array<T> {
	type Item = &'a T;
	type IntoIter = ArrayIterator<'a, T>;
	fn into_iter(self) -> ArrayIterator<'a, T> {
		ArrayIterator {
			cur: 0,
			array_ref: self,
		}
	}
}

impl ArrayBuilder {
	pub fn build<T>(size: usize) -> Result<Array<T>, Error> {
		Array::new(size)
	}

	pub fn build_array_list<T>(size: usize) -> Result<ArrayList<T>, Error> {
		ArrayList::new(size)
	}
}

#[cfg(test)]
mod test {
	use crate as bmw_util;
	use crate::{
		block_on, execute, thread_pool, Array, ArrayBuilder, List, PoolResult, ThreadPool,
	};
	use bmw_deps::try_traits::clone::TryClone;
	use bmw_err::{err, ErrKind, Error};
	use bmw_log::*;

	info!();

	struct TestStruct {
		arr: Array<u32>,
	}

	#[test]
	fn test_array_simple() -> Result<(), Error> {
		let mut arr = ArrayBuilder::build(10)?;
		for i in 0..10 {
			arr[i] = i as u64;
		}
		for i in 0..10 {
			info!("arr[{}]={}", i, arr[i])?;
			assert_eq!(arr[i], i as u64);
		}

		let mut test = TestStruct {
			arr: ArrayBuilder::build(40)?,
		};

		for i in 0..40 {
			test.arr[i] = i as u32;
		}

		let test2 = test.arr.try_clone()?;

		for i in 0..40 {
			info!("i={}", i)?;
			assert_eq!(test.arr[i], i as u32);
			assert_eq!(test2[i], i as u32);
		}

		Ok(())
	}

	#[test]
	fn test_array_iterator() -> Result<(), Error> {
		let mut arr = ArrayBuilder::build(10)?;
		for i in 0..10 {
			arr[i] = i as u64;
		}

		let mut i = 0;
		for x in &arr {
			assert_eq!(*x, i as u64);
			i += 1;
		}
		Ok(())
	}

	#[test]
	fn test_array_index_out_of_bounds() -> Result<(), Error> {
		let tp = thread_pool!()?;

		let handle = execute!(tp, {
			let mut x = ArrayBuilder::build(10)?;
			for i in 0..11 {
				x[i] = i;
			}
			Ok(())
		})?;

		assert_eq!(
			block_on!(handle),
			PoolResult::Err(err!(
				ErrKind::ThreadPanic,
				"thread pool panic: receiving on a closed channel"
			))
		);

		let handle = execute!(tp, {
			let mut x = ArrayBuilder::build(10)?;
			for i in 0..10 {
				x[i] = i;
			}
			Ok(())
		})?;

		assert_eq!(block_on!(handle), PoolResult::Ok(()));

		Ok(())
	}

	#[test]
	fn test_array_partial_eq() -> Result<(), Error> {
		let mut arr1 = ArrayBuilder::build(10)?;
		let mut arr2 = ArrayBuilder::build(11)?;

		for i in 0..10 {
			arr1[i] = 7;
		}

		for i in 0..11 {
			arr2[i] = 7;
		}

		assert_ne!(arr1, arr2);

		let mut arr3 = ArrayBuilder::build(10)?;
		for i in 0..10 {
			arr3[i] = 8;
		}

		assert_ne!(arr3, arr1);

		let mut arr4 = ArrayBuilder::build(10)?;
		for i in 0..10 {
			arr4[i] = 7;
		}

		assert_eq!(arr4, arr1);

		let mut arr5 = ArrayBuilder::build(20)?;
		let mut arr6 = ArrayBuilder::build(20)?;

		info!("test 128")?;
		for i in 0..20 {
			arr5[i] = 71u128;
		}
		for i in 0..20 {
			arr6[i] = 71u128;
		}

		assert_eq!(arr5, arr6);

		arr5[3] = 100;

		assert_ne!(arr5, arr6);

		Ok(())
	}

	#[test]
	fn test_array_list() -> Result<(), Error> {
		let mut list1 = ArrayBuilder::build_array_list(10)?;
		let mut list2 = ArrayBuilder::build_array_list(10)?;

		list1.push(1usize)?;
		list2.push(1usize)?;

		assert_eq!(list1, list2);

		list1.push(2)?;
		assert_ne!(list1, list2);

		list2.push(2)?;
		assert_eq!(list1, list2);

		list1.push(1)?;
		list2.push(3)?;
		assert_ne!(list1, list2);

		list1.clear()?;
		list2.clear()?;
		assert_eq!(list1, list2);

		list1.push(10)?;
		list2.push(10)?;
		assert_eq!(list1, list2);

		let mut list3 = ArrayBuilder::build_array_list(10)?;

		for i in 0..5 {
			list3.push(i)?;
		}

		list1.append(&list3)?;
		assert_ne!(list1, list2);
		list2.append(&list3)?;
		assert_eq!(list1, list2);

		let mut list = ArrayBuilder::build_array_list(50)?;

		let mut i = 0u64;
		for _ in list.iter() {
			i += 1;
		}
		for _ in list.iter_rev() {
			i += 1;
		}

		for i in 0..5 {
			list.push(i as u64)?;
		}

		for x in list.iter() {
			assert_eq!(x, i);
			i += 1;
		}

		assert_eq!(i, 5);
		for x in list.iter_rev() {
			i -= 1;
			assert_eq!(x, i);
		}

		Ok(())
	}
}
