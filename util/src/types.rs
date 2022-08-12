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

use bmw_deps::byteorder::{BigEndian, ByteOrder};
use bmw_err::{err, ErrKind, Error};
use std::fmt::Debug;
use std::future::Future;

pub trait StaticIterator<'a, K>
where
	K: Serializable,
{
	fn next(&mut self) -> Option<K>;
}

pub trait StaticHashtable<K, V>
where
	K: Serializable,
	V: Serializable,
{
	fn insert(&mut self, key: &K, value: &V) -> Result<(), Error>;
	fn get(&self, key: &K) -> Result<Option<V>, Error>;
	fn remove(&mut self, key: &K) -> Result<(), Error>;
	fn iter(&self) -> Result<Box<dyn StaticIterator<'_, (K, V)>>, Error>;
	fn get_raw<'b>(&'b self, key: &[u8], hash: u64) -> Result<Option<Box<dyn Slab + 'b>>, Error>;
	//fn get_raw_mut<'a>(&'a mut self, key: &[u8], hash: u64) -> Result<Option<&'a mut [u8]>, Error>;
	fn insert_raw(&mut self, key: &[u8], hash: u64, value: &[u8]) -> Result<(), Error>;
}
pub trait StaticHashset<K>
where
	K: Serializable,
{
	fn insert(&mut self, key: &K) -> Result<(), Error>;
	fn contains(&self, key: &K) -> Result<bool, Error>;
	fn remove(&mut self, key: &K) -> Result<(), Error>;
	fn iter(&self) -> Result<Box<dyn StaticIterator<'_, K>>, Error>;
	fn insert_raw(&mut self, key: &[u8]) -> Result<(), Error>;
}
pub trait StaticQueue<V>
where
	V: Serializable,
{
	fn enqueue(&mut self, value: &V) -> Result<(), Error>;
	fn dequeue(&mut self) -> Result<Option<&V>, Error>;
	fn iter(&self) -> Result<Box<dyn StaticIterator<'_, V>>, Error>;
	fn iter_rev(&self) -> Result<Box<dyn StaticIterator<'_, V>>, Error>;
}

pub trait StaticStack<V>
where
	V: Serializable,
{
	fn push(&mut self, value: &V) -> Result<(), Error>;
	fn pop(&mut self) -> Result<Option<&V>, Error>;
	fn peek(&self) -> Result<Option<&V>, Error>;
}

pub trait ThreadPool {
	fn execute<F>(&self, f: F) -> Result<(), Error>
	where
		F: Future<Output = Result<(), Error>> + Send + Sync + 'static;
}

pub trait Slab {
	fn get(&self) -> &[u8];
	fn id(&self) -> u64;
}

pub trait SlabMut {
	fn get(&self) -> &[u8];
	fn get_mut(&mut self) -> &mut [u8];
	fn id(&self) -> u64;
}

pub trait SlabAllocator {
	fn allocate<'a>(&'a mut self) -> Result<Box<dyn SlabMut + 'a>, Error>;
	fn free(&mut self, id: u64) -> Result<(), Error>;
	fn get<'a>(&'a self, id: u64) -> Result<Box<dyn Slab + 'a>, Error>;
	fn get_mut<'a>(&'a mut self, id: u64) -> Result<Box<dyn SlabMut + 'a>, Error>;
	fn free_count(&self) -> Result<u64, Error>;
	fn slab_size(&self) -> Result<u64, Error>;
}

pub trait Match {
	fn start(&self) -> usize;
	fn end(&self) -> usize;
	fn id(&self) -> u128;
	fn set_start(&mut self, start: usize) -> Result<(), Error>;
	fn set_end(&mut self, end: usize) -> Result<(), Error>;
	fn set_id(&mut self, id: u128) -> Result<(), Error>;
}

pub trait Pattern {
	fn regex(&self) -> String;
	fn is_case_sensitive(&self) -> bool;
	fn is_termination_pattern(&self) -> bool;
	fn id(&self) -> u128;
}

pub trait SuffixTree {
	fn add_pattern(&mut self, pattern: &dyn Pattern) -> Result<(), Error>;
	fn run_matches(
		&mut self,
		text: &[u8],
		matches: &mut Vec<Box<dyn Match>>,
	) -> Result<usize, Error>;
}

pub trait Writer {
	fn write_u8(&mut self, n: u8) -> Result<(), Error> {
		self.write_fixed_bytes(&[n])
	}

	fn write_i8(&mut self, n: i8) -> Result<(), Error> {
		self.write_fixed_bytes(&[n as u8])
	}

	fn write_u16(&mut self, n: u16) -> Result<(), Error> {
		let mut bytes = [0; 2];
		BigEndian::write_u16(&mut bytes, n);
		self.write_fixed_bytes(&bytes)
	}

	fn write_i16(&mut self, n: i16) -> Result<(), Error> {
		let mut bytes = [0; 2];
		BigEndian::write_i16(&mut bytes, n);
		self.write_fixed_bytes(&bytes)
	}

	fn write_u32(&mut self, n: u32) -> Result<(), Error> {
		let mut bytes = [0; 4];
		BigEndian::write_u32(&mut bytes, n);
		self.write_fixed_bytes(&bytes)
	}

	fn write_i32(&mut self, n: i32) -> Result<(), Error> {
		let mut bytes = [0; 4];
		BigEndian::write_i32(&mut bytes, n);
		self.write_fixed_bytes(&bytes)
	}

	fn write_u64(&mut self, n: u64) -> Result<(), Error> {
		let mut bytes = [0; 8];
		BigEndian::write_u64(&mut bytes, n);
		self.write_fixed_bytes(&bytes)
	}

	fn write_i128(&mut self, n: i128) -> Result<(), Error> {
		let mut bytes = [0; 16];
		BigEndian::write_i128(&mut bytes, n);
		self.write_fixed_bytes(&bytes)
	}

	fn write_u128(&mut self, n: u128) -> Result<(), Error> {
		let mut bytes = [0; 16];
		BigEndian::write_u128(&mut bytes, n);
		self.write_fixed_bytes(&bytes)
	}

	fn write_i64(&mut self, n: i64) -> Result<(), Error> {
		let mut bytes = [0; 8];
		BigEndian::write_i64(&mut bytes, n);
		self.write_fixed_bytes(&bytes)
	}

	fn write_bytes<T: AsRef<[u8]>>(&mut self, bytes: T) -> Result<(), Error> {
		self.write_u64(bytes.as_ref().len() as u64)?;
		self.write_fixed_bytes(bytes)
	}

	fn write_fixed_bytes<T: AsRef<[u8]>>(&mut self, bytes: T) -> Result<(), Error>;

	fn write_empty_bytes(&mut self, length: usize) -> Result<(), Error> {
		self.write_fixed_bytes(vec![0u8; length])
	}
}

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
	fn read_bytes_len_prefix(&mut self) -> Result<Vec<u8>, Error>;
	fn read_fixed_bytes(&mut self, length: usize) -> Result<Vec<u8>, Error>;
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

pub trait Serializable
where
	Self: Sized + Debug,
{
	fn read<R: Reader>(reader: &mut R) -> Result<Self, Error>;
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error>;
}
