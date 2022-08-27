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

use crate::types::Direction;
use crate::{Array, ArrayList, List, Queue, Serializable, SortableList, Stack};
use bmw_err::{err, ErrKind, Error};
use bmw_log::*;
use std::alloc::{alloc, dealloc, Layout};
use std::fmt;
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};
use std::ptr::copy_nonoverlapping;
use std::slice::{from_raw_parts, from_raw_parts_mut};

info!();

impl<T> Array<T>
where
	T: Clone,
{
	pub(crate) fn new(size: usize) -> Result<Self, Error> {
		let n = ::std::mem::size_of::<T>();
		let layout = Layout::from_size_align(size * n, n).unwrap();
		let data = unsafe { alloc(layout) };
		if data.is_null() {
			return Err(err!(ErrKind::Alloc, "could not allocate memory for array"));
		}
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

	pub fn as_slice<'a>(&'a self) -> &'a [T] {
		let ptr: *mut T = self.data as *mut T;
		let slice = unsafe { from_raw_parts(ptr, self.size) };
		slice
	}

	pub fn as_mut<'a>(&'a mut self) -> &'a mut [T] {
		let ptr: *mut T = self.data as *mut T;
		let slice = unsafe { from_raw_parts_mut(ptr, self.size) };
		slice
	}
	pub fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = T> + 'a> {
		Box::new(ArrayIterator {
			cur: 0,
			array_ref: self,
		})
	}
}

unsafe impl<T> Send for Array<T> where T: Send {}

unsafe impl<T> Sync for Array<T> where T: Sync {}

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

impl<T> Clone for Array<T> {
	fn clone(&self) -> Self {
		let layout = self.layout;
		let data = unsafe { alloc(layout) };
		if data.is_null() {
			panic!("could not allocate array. Not enough memory.");
		}
		let size = self.size;
		let size_of_type = self.size_of_type;

		unsafe {
			copy_nonoverlapping(self.data, data, size * size_of_type);
		}

		Self {
			data,
			size_of_type,
			layout,
			size,
			_phantom_data: PhantomData,
		}
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

impl<T> ArrayList<T>
where
	T: Clone,
{
	pub(crate) fn new(size: usize) -> Result<Self, Error> {
		let inner = Array::new(size)?;
		Ok(Self {
			inner,
			size: 0,
			head: 0,
			tail: 0,
		})
	}
}

impl<T> SortableList<T> for ArrayList<T>
where
	T: Clone + Debug + Serializable + PartialEq,
{
	fn sort(&mut self) -> Result<(), Error>
	where
		T: Ord,
	{
		let size = self.size();
		self.inner.as_mut()[0..size].sort();
		Ok(())
	}
	fn sort_unstable(&mut self) -> Result<(), Error>
	where
		T: Ord,
	{
		let size = self.size();
		self.inner.as_mut()[0..size].sort_unstable();
		Ok(())
	}
}

impl<T> Debug for ArrayList<T>
where
	T: Debug + Clone,
{
	fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
		let size = self.size;
		write!(fmt, "{:?}", &self.inner.as_slice()[0..size])
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

impl<T> List<T> for ArrayList<T>
where
	T: Clone + Debug + PartialEq,
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
	fn delete_head(&mut self) -> Result<(), Error> {
		return Err(err!(
			ErrKind::OperationNotSupported,
			"arraylist doesn't support delete_head"
		));
	}
	fn size(&self) -> usize {
		self.size
	}
	fn clear(&mut self) -> Result<(), Error> {
		self.size = 0;
		Ok(())
	}
}

impl<T> Queue<T> for ArrayList<T>
where
	T: Clone,
{
	fn enqueue(&mut self, value: T) -> Result<(), Error> {
		if self.size == self.inner.size {
			let fmt = format!("capacity ({}) exceeded.", self.inner.size);
			Err(err!(ErrKind::CapacityExceeded, fmt))
		} else {
			self.inner[self.tail] = value;
			self.tail = (self.tail + 1) % self.inner.size;
			self.size += 1;
			Ok(())
		}
	}
	fn dequeue(&mut self) -> Option<&T> {
		if self.size == 0 {
			None
		} else {
			let ret = &self.inner[self.head];
			self.head = (self.head + 1) % self.inner.size;
			self.size = self.size.saturating_sub(1);
			Some(ret)
		}
	}
	fn peek(&self) -> Option<&T> {
		if self.size == 0 {
			None
		} else {
			Some(&self.inner[self.head])
		}
	}
}

impl<T> Stack<T> for ArrayList<T>
where
	T: Clone,
{
	fn push(&mut self, value: T) -> Result<(), Error> {
		if self.size == self.inner.size {
			let fmt = format!("capacity ({}) exceeded.", self.inner.size);
			Err(err!(ErrKind::CapacityExceeded, fmt))
		} else {
			self.inner[self.tail] = value;
			self.tail = (self.tail + 1) % self.inner.size;
			self.size += 1;
			Ok(())
		}
	}
	fn pop(&mut self) -> Option<&T> {
		if self.size == 0 {
			None
		} else {
			if self.tail == 0 {
				self.tail = self.inner.size.saturating_sub(1);
			} else {
				self.tail = self.tail - 1;
			}
			let ret = &self.inner[self.tail];
			self.size = self.size.saturating_sub(1);
			Some(ret)
		}
	}
	fn peek(&self) -> Option<&T> {
		if self.size == 0 {
			None
		} else {
			let index = if self.tail == 0 {
				self.inner.size.saturating_sub(1)
			} else {
				self.tail - 1
			};
			Some(&self.inner[index])
		}
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

impl<'a, T> Iterator for ArrayIterator<'a, T>
where
	T: Clone,
{
	type Item = T;
	fn next(&mut self) -> Option<<Self as Iterator>::Item> {
		if self.cur >= self.array_ref.size.clone() {
			None
		} else {
			self.cur += 1;
			Some(self.array_ref[self.cur - 1].clone())
		}
	}
}

#[cfg(test)]
mod test {
	use crate as bmw_util;
	use crate::{
		block_on, execute, list, list_eq, thread_pool, Array, Builder, List, PoolResult, Queue,
		Stack, ThreadPool,
	};
	use bmw_deps::dyn_clone::clone_box;
	use bmw_err::{err, ErrKind, Error};
	use bmw_log::*;

	info!();

	struct TestStruct {
		arr: Array<u32>,
	}

	#[test]
	fn test_array_simple() -> Result<(), Error> {
		let mut arr = Builder::build_array(10)?;
		for i in 0..10 {
			arr[i] = i as u64;
		}
		for i in 0..10 {
			info!("arr[{}]={}", i, arr[i])?;
			assert_eq!(arr[i], i as u64);
		}

		let mut test = TestStruct {
			arr: Builder::build_array(40)?,
		};

		for i in 0..40 {
			test.arr[i] = i as u32;
		}

		let test2 = test.arr.clone();

		for i in 0..40 {
			info!("i={}", i)?;
			assert_eq!(test.arr[i], i as u32);
			assert_eq!(test2[i], i as u32);
		}

		Ok(())
	}

	#[test]
	fn test_array_iterator() -> Result<(), Error> {
		let mut arr = Builder::build_array(10)?;
		for i in 0..10 {
			arr[i] = i as u64;
		}

		let mut i = 0;
		for x in arr.iter() {
			assert_eq!(x, i as u64);
			i += 1;
		}
		Ok(())
	}

	#[test]
	fn test_array_index_out_of_bounds() -> Result<(), Error> {
		let tp = thread_pool!()?;

		let handle = execute!(tp, {
			let mut x = Builder::build_array(10)?;
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
			let mut x = Builder::build_array(10)?;
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
		let mut arr1 = Builder::build_array(10)?;
		let mut arr2 = Builder::build_array(11)?;

		for i in 0..10 {
			arr1[i] = 7;
		}

		for i in 0..11 {
			arr2[i] = 7;
		}

		assert_ne!(arr1, arr2);

		let mut arr3 = Builder::build_array(10)?;
		for i in 0..10 {
			arr3[i] = 8;
		}

		assert_ne!(arr3, arr1);

		let mut arr4 = Builder::build_array(10)?;
		for i in 0..10 {
			arr4[i] = 7;
		}

		assert_eq!(arr4, arr1);

		let mut arr5 = Builder::build_array(20)?;
		let mut arr6 = Builder::build_array(20)?;

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
		let mut list1 = Builder::build_array_list(10)?;
		let mut list2 = Builder::build_array_list(10)?;

		list1.push(1usize)?;
		list2.push(1usize)?;

		assert!(list_eq!(&list1, &list2));

		list1.push(2)?;
		assert!(!list_eq!(&list1, &list2));

		list2.push(2)?;
		assert!(list_eq!(&list1, &list2));

		list1.push(1)?;
		list2.push(3)?;
		assert!(!list_eq!(&list1, &list2));

		list1.clear()?;
		list2.clear()?;
		assert!(list_eq!(&list1, &list2));

		list1.push(10)?;
		list2.push(10)?;
		assert!(list_eq!(&list1, &list2));

		let mut list3 = Builder::build_array_list(10)?;

		for i in 0..5 {
			list3.push(i)?;
		}

		let mut list = Builder::build_array_list(50)?;

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

	#[test]
	fn test_as_slice_mut() -> Result<(), Error> {
		let data = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
		let mut array = Builder::build_array(data.len())?;
		array.as_mut().clone_from_slice(&data);

		assert_eq!(array[3], 3u8);
		assert_eq!(array.as_slice()[4], 4u8);
		Ok(())
	}

	#[test]
	fn test_queue() -> Result<(), Error> {
		let mut queue = Builder::build_queue(10)?;
		queue.enqueue(1)?;
		queue.enqueue(2)?;
		queue.enqueue(3)?;

		assert_eq!(queue.dequeue(), Some(&1));
		assert_eq!(queue.peek(), Some(&2));
		assert_eq!(queue.peek(), Some(&2));
		assert_eq!(queue.dequeue(), Some(&2));
		assert_eq!(queue.dequeue(), Some(&3));
		assert_eq!(queue.dequeue(), None);
		assert_eq!(queue.peek(), None);

		for i in 0..9 {
			queue.enqueue(i)?;
		}

		for i in 0..9 {
			assert_eq!(queue.dequeue(), Some(&i));
		}

		for i in 0..10 {
			queue.enqueue(i)?;
		}

		for i in 0..10 {
			assert_eq!(queue.dequeue(), Some(&i));
		}

		for i in 0..10 {
			queue.enqueue(i)?;
		}

		assert!(queue.enqueue(1).is_err());

		Ok(())
	}

	#[test]
	fn test_stack() -> Result<(), Error> {
		let mut stack = Builder::build_stack(10)?;
		stack.push(1)?;
		stack.push(2)?;
		stack.push(3)?;

		assert_eq!(stack.pop(), Some(&3));
		assert_eq!(stack.peek(), Some(&2));
		assert_eq!(stack.peek(), Some(&2));
		assert_eq!(stack.pop(), Some(&2));
		assert_eq!(stack.pop(), Some(&1));
		assert_eq!(stack.pop(), None);
		assert_eq!(stack.peek(), None);

		for i in 0..9 {
			stack.push(i)?;
		}

		for i in (0..9).rev() {
			assert_eq!(stack.pop(), Some(&i));
		}

		for i in 0..10 {
			stack.push(i)?;
		}

		for i in (0..10).rev() {
			assert_eq!(stack.pop(), Some(&i));
		}

		for i in 0..10 {
			stack.push(i)?;
		}

		assert!(stack.push(1).is_err());
		Ok(())
	}

	#[test]
	fn test_sync_array() -> Result<(), Error> {
		let mut array = Builder::build_array(10)?;
		array[0] = 1;

		let mut lock = lock!(array)?;
		let lock_clone = lock.clone();

		let tp = thread_pool!()?;

		let handle = execute!(tp, {
			let mut array = lock.wlock()?;
			assert_eq!((**array.guard())[0], 1);
			(**array.guard())[0] = 2;
			(**array.guard())[1] = 20;

			Ok(())
		})?;

		block_on!(handle);

		let array_processed = lock_clone.rlock()?;
		assert_eq!((**array_processed.guard())[0], 2);
		assert_eq!((**array_processed.guard())[1], 20);

		Ok(())
	}

	struct TestBoxedQueue {
		queue: Box<dyn Queue<u32>>,
	}

	#[test]
	fn test_queue_boxed() -> Result<(), Error> {
		let queue = Builder::build_queue_box(10)?;
		let mut test = TestBoxedQueue { queue };
		test.queue.enqueue(1)?;
		Ok(())
	}

	#[test]
	fn test_queue_clone() -> Result<(), Error> {
		let queue = Builder::build_queue_box(10)?;
		let mut test = TestBoxedQueue { queue };
		test.queue.enqueue(1)?;
		let mut test2 = clone_box(&*test.queue);

		assert_eq!(test.queue.dequeue(), Some(&1));
		assert_eq!(test.queue.dequeue(), None);
		assert_eq!(test2.dequeue(), Some(&1));
		assert_eq!(test2.dequeue(), None);

		Ok(())
	}

	#[test]
	fn test_sort() -> Result<(), Error> {
		use crate::SortableList;
		let mut list = Builder::build_array_list(10)?;

		list.push(1)?;
		list.push(3)?;
		list.push(2)?;

		let other_list = list![1, 3, 2];
		info!("list={:?}", list)?;
		assert!(list_eq!(list, other_list));

		list.sort()?;

		let other_list = list![1, 2, 3];
		info!("list={:?}", list)?;
		assert!(list_eq!(list, other_list));

		Ok(())
	}
}
