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

use crate::{Slab, SlabAllocator, SlabMut};
use bmw_err::{err, ErrKind, Error};
use bmw_log::*;
use std::cell::UnsafeCell;
use std::convert::TryInto;

info!();

#[derive(Debug)]
pub struct SlabAllocatorConfig {
	slab_size: u64,
	slab_count: u64,
}

pub struct SlabAllocatorBuilder {}

struct SlabMutImpl<'a> {
	pub data: &'a mut [u8],
	pub id: u64,
}

struct SlabImpl<'a> {
	pub data: &'a [u8],
	pub id: u64,
}

struct SlabAllocatorImpl {
	config: SlabAllocatorConfig,
	data: Vec<u8>,
	first_free: u64,
	free_count: u64,
}

impl Default for SlabAllocatorConfig {
	fn default() -> Self {
		Self {
			slab_size: 1024,
			slab_count: 10 * 1024,
		}
	}
}

impl<'a> SlabMut for SlabMutImpl<'a> {
	fn get(&self) -> &[u8] {
		&self.data
	}
	fn get_mut(&mut self) -> &mut [u8] {
		&mut self.data
	}
	fn id(&self) -> u64 {
		self.id
	}
}

impl<'a> Slab for SlabImpl<'a> {
	fn get(&self) -> &[u8] {
		&self.data
	}
	fn id(&self) -> u64 {
		self.id
	}
}

impl SlabAllocator for SlabAllocatorImpl {
	fn allocate<'a>(&'a mut self) -> Result<Box<dyn SlabMut + 'a>, Error> {
		debug!("allocate:self.config={:?}", self.config)?;
		if self.first_free == u64::MAX {
			return Err(err!(ErrKind::CapacityExceeded, "no more slabs available"));
		}

		let id = self.first_free;
		let offset = ((8 + self.config.slab_size) * id).try_into()?;
		self.first_free =
			u64::from_be_bytes(self.data[offset..offset + 8].try_into()?).try_into()?;

		let offset = (offset + 8).try_into()?;
		let data = &mut self.data[offset..offset + self.config.slab_size as usize];
		self.free_count = self.free_count.saturating_sub(1);

		Ok(Box::new(SlabMutImpl { data, id }))
	}
	fn free(&mut self, id: u64) -> Result<(), Error> {
		if id >= self.config.slab_count {
			return Err(err!(
				ErrKind::ArrayIndexOutOfBounds,
				format!(
					"tried to free slab.id = {}, but only {} slabs exist",
					id, self.config.slab_count
				)
			));
		}
		debug!("free:self.config={:?},id={}", self.config, id)?;
		let offset = ((8 + self.config.slab_size) * id).try_into()?;
		self.data[offset..offset + 8].clone_from_slice(&self.first_free.to_be_bytes());
		self.first_free = id;
		self.free_count += 1;
		Ok(())
	}
	fn get<'a>(&'a self, id: u64) -> Result<Box<dyn Slab + 'a>, Error> {
		if id >= self.config.slab_count {
			return Err(err!(
				ErrKind::ArrayIndexOutOfBounds,
				format!(
					"tried to get slab.id = {}, but only {} slabs exist",
					id, self.config.slab_count
				)
			));
		}
		debug!("get:self.config={:?},id={}", self.config, id)?;
		let offset = (8 + ((8 + self.config.slab_size) * id)).try_into()?;
		let data = &self.data[offset..offset + self.config.slab_size as usize];
		Ok(Box::new(SlabImpl { data, id }))
	}
	fn get_mut<'a>(&'a mut self, id: u64) -> Result<Box<dyn SlabMut + 'a>, Error> {
		if id >= self.config.slab_count {
			return Err(err!(
				ErrKind::ArrayIndexOutOfBounds,
				format!(
					"tried to get slab.id = {}, but only {} slabs exist",
					id, self.config.slab_count
				)
			));
		}
		debug!("get_mut:self.config={:?},id={}", self.config, id)?;
		let offset = (8 + ((8 + self.config.slab_size) * id)).try_into()?;
		let data = &mut self.data[offset..offset + self.config.slab_size as usize];
		Ok(Box::new(SlabMutImpl { data, id }))
	}

	fn free_count(&self) -> Result<u64, Error> {
		debug!("free_count:self.config={:?}", self.config)?;
		Ok(self.free_count)
	}

	fn slab_size(&self) -> Result<u64, Error> {
		debug!("slab_size:self.config={:?}", self.config)?;
		Ok(self.config.slab_size)
	}
}

impl SlabAllocatorImpl {
	fn new(config: SlabAllocatorConfig) -> Result<Self, Error> {
		let mut data = vec![];
		data.resize(
			(config.slab_count * (config.slab_size + 8)).try_into()?,
			0u8,
		);
		let first_free = 0;
		let free_count = config.slab_count;
		Self::build_free_list(&mut data, config.slab_count, config.slab_size)?;
		Ok(Self {
			config,
			free_count,
			data,
			first_free,
		})
	}

	fn build_free_list(data: &mut Vec<u8>, slab_count: u64, slab_size: u64) -> Result<(), Error> {
		for i in 0..slab_count {
			let next_bytes = if i < slab_count - 1 {
				(i + 1).to_be_bytes()
			} else {
				u64::MAX.to_be_bytes()
			};

			let offset_next: usize = (i * (8 + slab_size)).try_into().unwrap();
			data[offset_next..offset_next + 8].clone_from_slice(&next_bytes);
		}
		Ok(())
	}
}

impl SlabAllocatorBuilder {
	pub fn build_unsafe(
		config: SlabAllocatorConfig,
	) -> Result<UnsafeCell<Box<dyn SlabAllocator>>, Error> {
		Ok(UnsafeCell::new(Box::new(SlabAllocatorImpl::new(config)?)))
	}

	pub fn build(config: SlabAllocatorConfig) -> Result<Box<dyn SlabAllocator>, Error> {
		Ok(Box::new(SlabAllocatorImpl::new(config)?))
	}
}

#[cfg(test)]
mod test {
	use crate::{SlabAllocatorBuilder, SlabAllocatorConfig};
	use bmw_err::Error;

	#[test]
	fn test_simple() -> Result<(), Error> {
		let mut slabs = SlabAllocatorBuilder::build(SlabAllocatorConfig::default())?;

		let (id1, id2);
		{
			let mut slab = slabs.allocate()?;
			id1 = slab.id();
			slab.get_mut()[0] = 111;
		}

		{
			let mut slab = slabs.allocate()?;
			id2 = slab.id();
			slab.get_mut()[0] = 112;
		}

		assert_eq!(slabs.get(id1)?.get()[0], 111);
		assert_eq!(slabs.get(id2)?.get()[0], 112);

		Ok(())
	}
}
