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

use crate::types::Serializable;
use crate::types::{Reader, Writer};
use bmw_err::{err, map_err, ErrKind, Error};
use bmw_log::*;
use std::io::{Read, Write};
use std::str::from_utf8;

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

impl Serializable for String {
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
		writer.write_usize(self.len())?;
		writer.write_fixed_bytes(self.as_bytes())?;
		Ok(())
	}
	fn read<R: Reader>(reader: &mut R) -> Result<String, Error> {
		Ok(from_utf8(&reader.read_bytes_len_prefix()?)?.to_string())
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

/// Utility to read from a binary source
pub struct BinReader<'a, R: Read> {
	source: &'a mut R,
}

impl<'a, R: Read> BinReader<'a, R> {
	/// Constructor for a new BinReader for the provided source
	pub fn new(source: &'a mut R) -> Self {
		BinReader { source }
	}
}

/// Utility wrapper for an underlying byte Reader. Defines higher level methods
/// to read numbers, byte vectors, hashes, etc.
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
	/// Read a variable size vector from the underlying Read. Expects a usize
	fn read_bytes_len_prefix(&mut self) -> Result<Vec<u8>, Error> {
		let len = self.read_usize()?;
		self.read_fixed_bytes(len)
	}

	/// Read a fixed number of bytes.
	fn read_fixed_bytes(&mut self, len: usize) -> Result<Vec<u8>, Error> {
		let mut buf = vec![0; len];
		map_err!(self.source.read_exact(&mut buf), ErrKind::IO)?;
		Ok(buf)
	}

	fn expect_u8(&mut self, val: u8) -> Result<u8, Error> {
		let b = self.read_u8()?;
		if b == val {
			Ok(b)
		} else {
			let fmt = format!("expected: {:?}, received: {:?}", vec![val], vec![b]);
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
	use crate::*;
	use bmw_deps::rand;

	#[derive(Debug, PartialEq)]
	struct SerErr {
		exp: u8,
	}

	impl Serializable for SerErr {
		fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
			reader.expect_u8(99)?;
			Ok(Self { exp: 99 })
		}
		fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
			writer.write_u8(self.exp)?;
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
			Ok(())
		}
	}

	fn ser_helper<S: Serializable + PartialEq>(ser_out: S) -> Result<(), Error> {
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

		let ser_out = SerErr { exp: 100 };
		let mut v: Vec<u8> = vec![];
		serialize(&mut v, &ser_out)?;
		let ser_in: Result<SerErr, Error> = deserialize(&mut &v[..]);
		assert!(ser_in.is_err());

		let ser_out = SerErr { exp: 99 };
		let mut v: Vec<u8> = vec![];
		serialize(&mut v, &ser_out)?;
		let ser_in: Result<SerErr, Error> = deserialize(&mut &v[..]);
		assert!(ser_in.is_ok());

		Ok(())
	}
}
