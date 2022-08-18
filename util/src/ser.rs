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

use crate::misc::{is_max, slice_to_usize, usize_to_slice};
use crate::{
	Reader, Serializable, Slab, SlabAllocator, SlabAllocatorConfig, SlabMut, Writer,
	GLOBAL_SLAB_ALLOCATOR,
};
use bmw_err::{err, ErrKind, Error};
use bmw_log::*;
use std::io::{Read, Write};
use std::str::from_utf8;
use std::thread;

info!();

macro_rules! impl_int {
	($int:ty, $w_fn:ident, $r_fn:ident) => {
		impl Serializable for $int {
			fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
				writer.$w_fn(*self)
			}
			fn read<R: Reader>(reader: &mut R) -> Result<$int, Error> {
				reader.$r_fn()
			}
		}
	};
}

impl_int!(u8, write_u8, read_u8);
impl_int!(u16, write_u16, read_u16);
impl_int!(u32, write_u32, read_u32);
impl_int!(i32, write_i32, read_i32);
impl_int!(u64, write_u64, read_u64);
impl_int!(i64, write_i64, read_i64);
impl_int!(i8, write_i8, read_i8);
impl_int!(i16, write_i16, read_i16);
impl_int!(u128, write_u128, read_u128);
impl_int!(i128, write_i128, read_i128);
impl_int!(usize, write_usize, read_usize);

impl Serializable for f64 {
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
		writer.write_fixed_bytes(self.to_be_bytes())?;
		Ok(())
	}
	fn read<R: Reader>(reader: &mut R) -> Result<f64, Error> {
		let mut b = [0u8; 8];
		reader.read_fixed_bytes(&mut b)?;
		Ok(f64::from_be_bytes(b))
	}
}

impl Serializable for () {
	fn write<W: Writer>(&self, _writer: &mut W) -> Result<(), Error> {
		Ok(())
	}
	fn read<R: Reader>(_reader: &mut R) -> Result<(), Error> {
		Ok(())
	}
}

impl<A: Serializable, B: Serializable> Serializable for (A, B) {
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
		Serializable::write(&self.0, writer)?;
		Serializable::write(&self.1, writer)
	}
	fn read<R: Reader>(reader: &mut R) -> Result<(A, B), Error> {
		Ok((Serializable::read(reader)?, Serializable::read(reader)?))
	}
}

impl<S: Serializable> Serializable for Vec<S> {
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
		let len = self.len();
		writer.write_usize(len)?;
		for i in 0..len {
			Serializable::write(&self[i], writer)?;
		}
		Ok(())
	}
	fn read<R: Reader>(reader: &mut R) -> Result<Vec<S>, Error> {
		let len = reader.read_usize()?;
		let mut v = Vec::with_capacity(len);
		for _ in 0..len {
			v.push(Serializable::read(reader)?);
		}
		Ok(v)
	}
}

impl Serializable for String {
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
		writer.write_usize(self.len())?;
		writer.write_fixed_bytes(self.as_bytes())?;
		Ok(())
	}
	fn read<R: Reader>(reader: &mut R) -> Result<String, Error> {
		let len = reader.read_usize()?;
		let mut v = Vec::with_capacity(len);
		for _ in 0..len {
			v.push(reader.read_u8()?);
		}
		Ok(from_utf8(&v)?.to_string())
	}
}

pub struct SlabWriter<'a> {
	slabs: Option<&'a mut Box<dyn SlabAllocator + Send + Sync>>,
	slab_id: usize,
	offset: usize,
	slab_size: usize,
	bytes_per_slab: usize,
}

impl<'a> SlabWriter<'a> {
	pub fn new(
		slabs: Option<&'a mut Box<dyn SlabAllocator + Send + Sync>>,
		slab_id: usize,
	) -> Result<Self, Error> {
		debug!("new with slab_id = {}", slab_id)?;
		let (slab_size, slab_count) = match slabs.as_ref() {
			Some(slabs) => ((slabs.slab_size()?, slabs.slab_count()?)),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(usize, usize), Error> {
				let slabs = unsafe { f.get().as_mut().unwrap() };
				let slab_size = match slabs.slab_size() {
					Ok(slab_size) => slab_size,
					Err(_e) => {
						let th = thread::current();
						let n = th.name().unwrap_or("unknown");
						warn!(
							"Slab allocator was not initialized for thread '{}'. {}",
							n, "Initializing with default values.",
						)?;
						slabs.init(SlabAllocatorConfig::default())?;
						slabs.slab_size()?
					}
				};
				let slab_count = slabs.slab_count()?;
				Ok((slab_size, slab_count))
			})?,
		};
		let mut x = slab_count;
		let mut slab_ptr_size = 0;
		loop {
			if x == 0 {
				break;
			}
			x >>= 8;
			slab_ptr_size += 1;
		}

		let bytes_per_slab = slab_size.saturating_sub(slab_ptr_size);

		if bytes_per_slab == 0 {
			return Err(err!(
				ErrKind::Configuration,
				format!("slab size is too small: {}", slab_size)
			));
		}
		Ok(Self {
			slabs,
			slab_id,
			offset: 0,
			slab_size,
			bytes_per_slab,
		})
	}

	fn allocate(&mut self) -> Result<SlabMut, Error> {
		match &mut self.slabs {
			Some(slabs) => slabs.allocate(),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<SlabMut, Error> {
				let slabs = unsafe { f.get().as_mut().unwrap() };
				slabs.allocate()
			}),
		}
	}

	fn get_mut(&mut self, id: usize) -> Result<SlabMut, Error> {
		match &mut self.slabs {
			Some(slabs) => slabs.get_mut(id),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<SlabMut, Error> {
				let slabs = unsafe { f.get().as_mut().unwrap() };
				slabs.get_mut(id)
			}),
		}
	}
}

impl<'a> Writer for SlabWriter<'a> {
	fn write_fixed_bytes<T: AsRef<[u8]>>(&mut self, bytes: T) -> Result<(), Error> {
		let bytes = bytes.as_ref();
		let bytes_len = bytes.len();

		if bytes_len == 0 {
			return Ok(());
		}

		let mut bytes_offset = 0;
		let slab_id = self.slab_id;
		debug!("slab_id preloop = {}", slab_id)?;
		let bytes_per_slab = self.bytes_per_slab;
		let slab_size = self.slab_size;
		let mut alloc;

		loop {
			// if we've already written all of it break
			if bytes_offset >= bytes_len {
				break;
			}

			// get slab, if cur slab has more room get it otherwise allocate
			let self_offset;
			let mut slab = match self.offset < bytes_per_slab {
				true => {
					debug!("x: slab_id = {}", slab_id)?;
					self_offset = self.offset;
					alloc = false;
					self.get_mut(slab_id)?
				}
				false => {
					let nslab_id = {
						let nslab = self.allocate()?;
						nslab.id()
					};
					alloc = true;

					// update pointer to next
					{
						debug!(
							"set next_bytes for slab = {}, pointing to {}",
							self.slab_id, nslab_id
						)?;
						let mut prev = self.get_mut(self.slab_id)?;
						let prev_id = prev.id();

						let next_bytes = &mut prev.get_mut()[bytes_per_slab..slab_size];
						usize_to_slice(nslab_id, next_bytes)?;
						debug!("next_bytes = {:?},prev.id={}", next_bytes, prev_id)?;
					}

					self.offset = 0;
					self_offset = 0;
					debug!("setting self.slab_id = {}", nslab_id)?;
					self.slab_id = nslab_id;
					self.get_mut(self.slab_id)?
				}
			};
			let nid = slab.id();

			// how much is left in the buffer?
			let buffer_rem = bytes_len - bytes_offset;

			// we calculate the maximum we can write
			let wlen = if buffer_rem > bytes_per_slab - self_offset {
				bytes_per_slab - self_offset
			} else {
				buffer_rem
			};

			// write the data to the slab
			let slab_mut = slab.get_mut();
			debug!("writing wlen = {} bytes at offset = {}", wlen, self_offset)?;

			slab_mut[self_offset..self_offset + wlen]
				.clone_from_slice(&bytes[bytes_offset..bytes_offset + wlen]);

			bytes_offset += wlen;

			if alloc {
				// we're done, write 0xFF to set the next pointer so that we know it's
				// empty
				for i in bytes_per_slab..slab_size {
					slab_mut[i] = 0xFF;
				}
				debug!("setting slab {} to 0xFF", nid)?;
			}

			self.offset += wlen;
		}

		for i in 0..bytes.as_ref().len() {
			debug!("b[{}]={}", i, bytes.as_ref()[i])?;
		}
		Ok(())
	}
}

pub struct SlabReader<'a> {
	slabs: Option<&'a Box<dyn SlabAllocator + Send + Sync>>,
	slab_id: usize,
	offset: usize,
	slab_size: usize,
	bytes_per_slab: usize,
}

impl<'a> SlabReader<'a> {
	pub fn new(
		slabs: Option<&'a Box<dyn SlabAllocator + Send + Sync>>,
		slab_id: usize,
	) -> Result<Self, Error> {
		let (slab_size, slab_count) = match slabs.as_ref() {
			Some(slabs) => ((slabs.slab_size()?, slabs.slab_count()?)),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(usize, usize), Error> {
				let slabs = unsafe { f.get().as_mut().unwrap() };
				let slab_size = match slabs.slab_size() {
					Ok(slab_size) => slab_size,
					Err(_e) => {
						let th = thread::current();
						let n = th.name().unwrap_or("unknown");
						warn!(
							"Slab allocator was not initialized for thread '{}'. {}",
							n, "Initializing with default values.",
						)?;
						slabs.init(SlabAllocatorConfig::default())?;
						slabs.slab_size()?
					}
				};
				let slab_count = slabs.slab_count()?;
				Ok((slab_size, slab_count))
			})?,
		};
		let mut x = slab_count + 1; // add one so we have an is_max that's not an valid slab_id
		let mut slab_ptr_size = 0;
		loop {
			if x == 0 {
				break;
			}
			x >>= 8;
			slab_ptr_size += 1;
		}

		let bytes_per_slab = slab_size.saturating_sub(slab_ptr_size);

		if bytes_per_slab <= slab_ptr_size {
			return Err(err!(
				ErrKind::Configuration,
				format!("slab size is too small: {}", slab_size)
			));
		}

		Ok(Self {
			slabs,
			slab_id,
			offset: 0,
			slab_size,
			bytes_per_slab,
		})
	}

	fn get(&self, id: usize) -> Result<Slab, Error> {
		match &self.slabs {
			Some(slabs) => slabs.get(id),
			None => GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<Slab, Error> {
				let slabs = unsafe { f.get().as_ref().unwrap() };
				slabs.get(id)
			}),
		}
	}

	pub fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error> {
		let mut buf_offset = 0;
		let buf_len = buf.len();

		loop {
			if buf_offset >= buf_len {
				break;
			}
			let buf_rem = buf_len - buf_offset;

			if self.offset >= self.bytes_per_slab {
				self.offset = 0;
				debug!("offset was greater than bytes_per_slab setting to 0")?;
				let slab = self.get(self.slab_id)?;
				let next_bytes = &slab.get()[self.bytes_per_slab..self.slab_size];
				if is_max(next_bytes) {
					return Err(err!(ErrKind::IO, "failed to fill whole buffer"));
				}
				self.slab_id = slice_to_usize(next_bytes)?;
			}

			debug!("getting slab_id={}", self.slab_id)?;
			let slab = self.get(self.slab_id)?;

			let mut rlen = self.bytes_per_slab - self.offset;
			if rlen > buf_rem {
				rlen = buf_rem;
			}

			debug!(
				"buf_offset={},rlen={},self.offset={},rlen+self.offset={},buf_rem={}",
				buf_offset,
				rlen,
				self.offset,
				(self.offset + rlen),
				buf_rem
			)?;
			buf[buf_offset..buf_offset + rlen]
				.clone_from_slice(&slab.get()[self.offset..(self.offset + rlen)]);
			buf_offset += rlen;
			self.offset += rlen;
			debug!("setting offset to {}", self.offset)?;
		}

		Ok(())
	}
}

impl<'a> Reader for SlabReader<'a> {
	fn read_u8(&mut self) -> Result<u8, Error> {
		let mut b = [0u8; 1];
		self.read_exact(&mut b)?;
		Ok(b[0])
	}
	fn read_i8(&mut self) -> Result<i8, Error> {
		let mut b = [0u8; 1];
		self.read_exact(&mut b)?;
		Ok(b[0] as i8)
	}
	fn read_i16(&mut self) -> Result<i16, Error> {
		let mut b = [0u8; 2];
		self.read_exact(&mut b)?;
		Ok(i16::from_be_bytes(b))
	}
	fn read_u16(&mut self) -> Result<u16, Error> {
		let mut b = [0u8; 2];
		self.read_exact(&mut b)?;
		Ok(u16::from_be_bytes(b))
	}
	fn read_u32(&mut self) -> Result<u32, Error> {
		let mut b = [0u8; 4];
		self.read_exact(&mut b)?;
		Ok(u32::from_be_bytes(b))
	}
	fn read_i32(&mut self) -> Result<i32, Error> {
		let mut b = [0u8; 4];
		self.read_exact(&mut b)?;
		Ok(i32::from_be_bytes(b))
	}
	fn read_u64(&mut self) -> Result<u64, Error> {
		let mut b = [0u8; 8];
		self.read_exact(&mut b)?;
		Ok(u64::from_be_bytes(b))
	}
	fn read_i128(&mut self) -> Result<i128, Error> {
		let mut b = [0u8; 16];
		self.read_exact(&mut b)?;
		Ok(i128::from_be_bytes(b))
	}
	fn read_usize(&mut self) -> Result<usize, Error> {
		let mut b = [0u8; 8];
		self.read_exact(&mut b)?;
		Ok(usize::from_be_bytes(b))
	}

	fn read_u128(&mut self) -> Result<u128, Error> {
		let mut b = [0u8; 16];
		self.read_exact(&mut b)?;
		Ok(u128::from_be_bytes(b))
	}
	fn read_i64(&mut self) -> Result<i64, Error> {
		let mut b = [0u8; 8];
		self.read_exact(&mut b)?;
		Ok(i64::from_be_bytes(b))
	}

	fn read_fixed_bytes(&mut self, buf: &mut [u8]) -> Result<(), Error> {
		self.read_exact(buf)?;
		Ok(())
	}

	fn expect_u8(&mut self, val: u8) -> Result<u8, Error> {
		let b = self.read_u8()?;
		if b == val {
			Ok(b)
		} else {
			let fmt = format!("expected: {:?}, received: {:?}", val, b);
			Err(err!(ErrKind::CorruptedData, fmt))
		}
	}
}

/// Utility wrapper for an underlying byte Writer. Defines higher level methods
/// to write numbers, byte vectors, hashes, etc.
pub struct BinWriter<'a> {
	sink: &'a mut dyn Write,
}

impl<'a> BinWriter<'a> {
	/// Wraps a standard Write in a new BinWriter
	pub fn new(sink: &'a mut dyn Write) -> BinWriter<'a> {
		BinWriter { sink }
	}
}

impl<'a> Writer for BinWriter<'a> {
	fn write_fixed_bytes<T: AsRef<[u8]>>(&mut self, bytes: T) -> Result<(), Error> {
		self.sink.write_all(bytes.as_ref())?;
		Ok(())
	}
}

/// Utility wrapper for an underlying byte Reader. Defines higher level methods
/// to write numbers, byte vectors, hashes, etc.
pub struct BinReader<'a, R: Read> {
	source: &'a mut R,
}

impl<'a, R: Read> BinReader<'a, R> {
	/// Constructor for a new BinReader for the provided source
	pub fn new(source: &'a mut R) -> Self {
		BinReader { source }
	}
}

impl<'a, R: Read> Reader for BinReader<'a, R> {
	fn read_u8(&mut self) -> Result<u8, Error> {
		let mut b = [0u8; 1];
		self.source.read_exact(&mut b)?;
		Ok(b[0])
	}
	fn read_i8(&mut self) -> Result<i8, Error> {
		let mut b = [0u8; 1];
		self.source.read_exact(&mut b)?;
		Ok(b[0] as i8)
	}
	fn read_i16(&mut self) -> Result<i16, Error> {
		let mut b = [0u8; 2];
		self.source.read_exact(&mut b)?;
		Ok(i16::from_be_bytes(b))
	}
	fn read_u16(&mut self) -> Result<u16, Error> {
		let mut b = [0u8; 2];
		self.source.read_exact(&mut b)?;
		Ok(u16::from_be_bytes(b))
	}
	fn read_u32(&mut self) -> Result<u32, Error> {
		let mut b = [0u8; 4];
		self.source.read_exact(&mut b)?;
		Ok(u32::from_be_bytes(b))
	}
	fn read_i32(&mut self) -> Result<i32, Error> {
		let mut b = [0u8; 4];
		self.source.read_exact(&mut b)?;
		Ok(i32::from_be_bytes(b))
	}
	fn read_u64(&mut self) -> Result<u64, Error> {
		let mut b = [0u8; 8];
		self.source.read_exact(&mut b)?;
		Ok(u64::from_be_bytes(b))
	}
	fn read_i128(&mut self) -> Result<i128, Error> {
		let mut b = [0u8; 16];
		self.source.read_exact(&mut b)?;
		Ok(i128::from_be_bytes(b))
	}
	fn read_usize(&mut self) -> Result<usize, Error> {
		let mut b = [0u8; 8];
		self.source.read_exact(&mut b)?;
		Ok(usize::from_be_bytes(b))
	}

	fn read_u128(&mut self) -> Result<u128, Error> {
		let mut b = [0u8; 16];
		self.source.read_exact(&mut b)?;
		Ok(u128::from_be_bytes(b))
	}
	fn read_i64(&mut self) -> Result<i64, Error> {
		let mut b = [0u8; 8];
		self.source.read_exact(&mut b)?;
		Ok(i64::from_be_bytes(b))
	}

	fn read_fixed_bytes(&mut self, buf: &mut [u8]) -> Result<(), Error> {
		self.source.read_exact(buf)?;
		Ok(())
	}

	fn expect_u8(&mut self, val: u8) -> Result<u8, Error> {
		let b = self.read_u8()?;
		if b == val {
			Ok(b)
		} else {
			let fmt = format!("expected: {:?}, received: {:?}", val, b);
			Err(err!(ErrKind::CorruptedData, fmt))
		}
	}
}

/// Serializes a Serializable into any std::io::Write implementation.
pub fn serialize<W: Serializable>(sink: &mut dyn Write, thing: &W) -> Result<(), Error> {
	let mut writer = BinWriter::new(sink);
	thing.write(&mut writer)
}

/// Deserializes a Serializable from any std::io::Read implementation.
pub fn deserialize<T: Serializable, R: Read>(source: &mut R) -> Result<T, Error> {
	let mut reader = BinReader::new(source);
	T::read(&mut reader)
}

#[cfg(test)]
mod test {
	use crate as bmw_util;
	use crate::ser::{SlabReader, SlabWriter};
	use crate::*;
	use bmw_deps::rand;
	use bmw_err::*;
	use bmw_log::*;
	use std::fmt::Debug;

	info!();

	#[derive(Debug, PartialEq)]
	struct SerErr {
		exp: u8,
		empty: u8,
	}

	impl Serializable for SerErr {
		fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
			reader.expect_u8(99)?;
			reader.read_empty_bytes(1)?;
			Ok(Self { exp: 99, empty: 0 })
		}
		fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
			writer.write_u8(self.exp)?;
			writer.write_u8(self.empty)?;
			Ok(())
		}
	}

	#[derive(Debug, PartialEq)]
	struct SerAll {
		a: u8,
		b: i8,
		c: u16,
		d: i16,
		e: u32,
		f: i32,
		g: u64,
		h: i64,
		i: u128,
		j: i128,
		k: usize,
	}

	impl Serializable for SerAll {
		fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
			let a = reader.read_u8()?;
			let b = reader.read_i8()?;
			let c = reader.read_u16()?;
			let d = reader.read_i16()?;
			let e = reader.read_u32()?;
			let f = reader.read_i32()?;
			let g = reader.read_u64()?;
			let h = reader.read_i64()?;
			let i = reader.read_u128()?;
			let j = reader.read_i128()?;
			let k = reader.read_usize()?;
			reader.expect_u8(100)?;
			assert_eq!(reader.read_u64()?, 4);
			reader.read_u8()?;
			reader.read_u8()?;
			reader.read_u8()?;
			reader.read_u8()?;
			reader.read_empty_bytes(10)?;

			let ret = Self {
				a,
				b,
				c,
				d,
				e,
				f,
				g,
				h,
				i,
				j,
				k,
			};

			Ok(ret)
		}
		fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
			writer.write_u8(self.a)?;
			writer.write_i8(self.b)?;
			writer.write_u16(self.c)?;
			writer.write_i16(self.d)?;
			writer.write_u32(self.e)?;
			writer.write_i32(self.f)?;
			writer.write_u64(self.g)?;
			writer.write_i64(self.h)?;
			writer.write_u128(self.i)?;
			writer.write_i128(self.j)?;
			writer.write_usize(self.k)?;
			writer.write_u8(100)?;
			writer.write_bytes([1, 2, 3, 4])?;
			writer.write_empty_bytes(10)?;
			Ok(())
		}
	}

	fn ser_helper<S: Serializable + Debug + PartialEq>(ser_out: S) -> Result<(), Error> {
		let mut v: Vec<u8> = vec![];
		serialize(&mut v, &ser_out)?;
		let ser_in: S = deserialize(&mut &v[..])?;
		assert_eq!(ser_in, ser_out);
		Ok(())
	}

	#[test]
	fn test_serialization() -> Result<(), Error> {
		let ser_out = SerAll {
			a: rand::random(),
			b: rand::random(),
			c: rand::random(),
			d: rand::random(),
			e: rand::random(),
			f: rand::random(),
			g: rand::random(),
			h: rand::random(),
			i: rand::random(),
			j: rand::random(),
			k: rand::random(),
		};
		ser_helper(ser_out)?;
		ser_helper(())?;
		ser_helper((rand::random::<u32>(), rand::random::<i128>()))?;
		ser_helper(("hi there".to_string(), 123))?;

		let ser_out = SerErr { exp: 100, empty: 0 };
		let mut v: Vec<u8> = vec![];
		serialize(&mut v, &ser_out)?;
		let ser_in: Result<SerErr, Error> = deserialize(&mut &v[..]);
		assert!(ser_in.is_err());

		let ser_out = SerErr { exp: 99, empty: 0 };
		let mut v: Vec<u8> = vec![];
		serialize(&mut v, &ser_out)?;
		let ser_in: Result<SerErr, Error> = deserialize(&mut &v[..]);
		assert!(ser_in.is_ok());

		let ser_out = SerErr { exp: 99, empty: 1 };
		let mut v: Vec<u8> = vec![];
		serialize(&mut v, &ser_out)?;
		let ser_in: Result<SerErr, Error> = deserialize(&mut &v[..]);
		assert!(ser_in.is_err());

		let v = vec!["test1".to_string(), "a".to_string(), "okokok".to_string()];
		ser_helper(v)?;

		Ok(())
	}

	#[test]
	fn test_hashtable_ser() -> Result<(), Error> {
		let ctx = ctx!();
		let mut hashtable = StaticHashtableBuilder::build(StaticHashtableConfig::default(), None)?;
		hashtable.insert(ctx, &1u32, &4u64)?;
		let mut v: Vec<u8> = vec![];
		serialize(&mut v, &hashtable)?;
		let ser_in: Result<Box<dyn StaticHashtable<u32, u64>>, Error> = deserialize(&mut &v[..]);
		let ser_in = ser_in.unwrap();
		assert_eq!(ser_in.get(ctx, &1u32).unwrap().unwrap(), 4u64);

		let config = StaticHashtableConfig {
			debug_get_slab_error: true,
			..StaticHashtableConfig::default()
		};
		ser_helper(config)?;

		Ok(())
	}

	#[test]
	fn test_hashset_ser() -> Result<(), Error> {
		let ctx = ctx!();
		let mut hashset = StaticHashsetBuilder::build(
			StaticHashsetConfig {
				..StaticHashsetConfig::default()
			},
			None,
		)?;
		hashset.insert(ctx, &1u32)?;
		let mut v: Vec<u8> = vec![];
		serialize(&mut v, &hashset)?;
		let ser_in: Result<Box<dyn StaticHashset<u32>>, Error> = deserialize(&mut &v[..]);
		let ser_in = ser_in.unwrap();
		assert!(ser_in.contains(ctx, &1u32).unwrap());
		assert!(!ser_in.contains(ctx, &2u32).unwrap());
		assert_eq!(ser_in.size(ctx), 1);

		let config = StaticHashsetConfig {
			debug_get_slab_error: true,
			..StaticHashsetConfig::default()
		};
		ser_helper(config)?;

		Ok(())
	}

	#[test]
	fn test_slab_rw() -> Result<(), Error> {
		let mut slabs = slab_allocator!()?;

		let slab_id = {
			let slab = slabs.allocate()?;
			slab.id()
		};

		let mut slab_writer = SlabWriter::new(Some(&mut slabs), slab_id)?;
		slab_writer.write_u64(123)?;
		slab_writer.write_u128(123)?;

		let mut slab_reader = SlabReader::new(Some(&mut slabs), slab_id)?;
		assert_eq!(slab_reader.read_u64()?, 123);
		assert_eq!(slab_reader.read_u128()?, 123);

		Ok(())
	}

	#[test]
	fn test_multi_slabs() -> Result<(), Error> {
		let mut slabs = slab_allocator!()?;
		let slab_id = {
			let slab = slabs.allocate()?;
			slab.id()
		};

		let mut slab_writer = SlabWriter::new(Some(&mut slabs), slab_id)?;
		let r = 10_100;
		for i in 0..r {
			slab_writer.write_u128(i)?;
		}

		let mut slab_reader = SlabReader::new(Some(&mut slabs), slab_id)?;
		for i in 0..r {
			assert_eq!(slab_reader.read_u128()?, i);
		}

		let mut v = vec![];
		v.resize(1024 * 2, 0u8);
		// we can't read anymore
		assert!(slab_reader.read_fixed_bytes(&mut v).is_err());

		Ok(())
	}

	#[test]
	fn test_alternate_sized_slabs() -> Result<(), Error> {
		for i in 0..1_000 {
			let mut slabs = slab_allocator!(48 + i, 10)?;

			let slab_id = {
				let slab = slabs.allocate()?;
				slab.id()
			};
			let mut slab_writer = SlabWriter::new(Some(&mut slabs), slab_id)?;

			let mut v = [0u8; 256];
			for i in 0..v.len() {
				v[i] = (i % 256) as u8;
			}
			slab_writer.write_fixed_bytes(v)?;

			let mut slab_reader = SlabReader::new(Some(&mut slabs), slab_id)?;
			let mut v_back = [1u8; 256];
			slab_reader.read_fixed_bytes(&mut v_back)?;
			assert_eq!(v, v_back);
		}

		// test capacity exceeded

		// 470 is ok because only 1 byte overhead per slab.
		let mut slabs = slab_allocator!(48, 10)?;
		let slab_id = {
			let slab = slabs.allocate()?;
			slab.id()
		};
		let mut slab_writer = SlabWriter::new(Some(&mut slabs), slab_id)?;
		let mut v = [0u8; 470];
		for i in 0..v.len() {
			v[i] = (i % 256) as u8;
		}
		assert!(slab_writer.write_fixed_bytes(v).is_ok());

		// 471 is one too many and returns error (note: user responsible for cleanup)
		let mut slabs = slab_allocator!(48, 10)?;
		let slab_id = {
			let slab = slabs.allocate()?;
			slab.id()
		};
		let mut slab_writer = SlabWriter::new(Some(&mut slabs), slab_id)?;
		let mut v = [0u8; 471];
		for i in 0..v.len() {
			v[i] = (i % 256) as u8;
		}
		assert!(slab_writer.write_fixed_bytes(v).is_err());

		Ok(())
	}
}
