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

use crate::types::SlabAllocatorConfig;
use crate::{Slab, SlabAllocator, SlabMut};
use bmw_err::{err, ErrKind, Error};
use bmw_log::*;
use std::cell::UnsafeCell;

info!();

thread_local! {
		pub static GLOBAL_SLAB_ALLOCATOR: UnsafeCell<Box<dyn SlabAllocator>> =
					SlabAllocatorBuilder::build_unsafe();
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
	config: Option<SlabAllocatorConfig>,
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
		match &self.config {
			Some(config) => {
				debug!("allocate:self.config={:?}", config)?;
				if self.first_free == u64::MAX {
					return Err(err!(ErrKind::CapacityExceeded, "no more slabs available"));
				}

				let id = self.first_free;
				let offset = usize!((8 + config.slab_size) * id);
				self.first_free = u64!(u64::from_be_bytes(
					self.data[offset..offset + 8].try_into()?
				));

				let offset = usize!(offset + 8);
				let data = &mut self.data[offset..offset + config.slab_size as usize];
				self.free_count = self.free_count.saturating_sub(1);

				Ok(Box::new(SlabMutImpl { data, id }))
			}
			None => Err(err!(
				ErrKind::IllegalState,
				"slab allocator has not been initialied"
			)),
		}
	}
	fn free(&mut self, id: u64) -> Result<(), Error> {
		match &self.config {
			Some(config) => {
				if id >= config.slab_count {
					return Err(err!(
						ErrKind::ArrayIndexOutOfBounds,
						format!(
							"tried to free slab.id = {}, but only {} slabs exist",
							id, config.slab_count
						)
					));
				}
				debug!("free:self.config={:?},id={}", config, id)?;
				let offset = usize!((8 + config.slab_size) * id);
				self.data[offset..offset + 8].clone_from_slice(&self.first_free.to_be_bytes());
				self.first_free = id;
				self.free_count += 1;
				Ok(())
			}
			None => Err(err!(
				ErrKind::IllegalState,
				"slab allocator has not been initialied"
			)),
		}
	}
	fn get<'a>(&'a self, id: u64) -> Result<Box<dyn Slab + 'a>, Error> {
		match &self.config {
			Some(config) => {
				if id >= config.slab_count {
					return Err(err!(
						ErrKind::ArrayIndexOutOfBounds,
						format!(
							"tried to get slab.id = {}, but only {} slabs exist",
							id, config.slab_count
						)
					));
				}
				debug!("get:self.config={:?},id={}", config, id)?;
				let offset = usize!(8 + ((8 + config.slab_size) * id));
				let data = &self.data[offset..offset + usize!(config.slab_size)];
				Ok(Box::new(SlabImpl { data, id }))
			}
			None => Err(err!(
				ErrKind::IllegalState,
				"slab allocator has not been initialied"
			)),
		}
	}
	fn get_mut<'a>(&'a mut self, id: u64) -> Result<Box<dyn SlabMut + 'a>, Error> {
		match &self.config {
			Some(config) => {
				if id >= config.slab_count {
					return Err(err!(
						ErrKind::ArrayIndexOutOfBounds,
						format!(
							"tried to get slab.id = {}, but only {} slabs exist",
							id, config.slab_count
						)
					));
				}
				debug!("get_mut:self.config={:?},id={}", config, id)?;
				let offset = usize!(8 + ((8 + config.slab_size) * id));
				let data = &mut self.data[offset..offset + config.slab_size as usize];
				Ok(Box::new(SlabMutImpl { data, id }))
			}
			None => Err(err!(
				ErrKind::IllegalState,
				"slab allocator has not been initialied"
			)),
		}
	}

	fn free_count(&self) -> Result<u64, Error> {
		debug!("free_count:self.config={:?}", self.config)?;
		match &self.config {
			Some(_) => Ok(self.free_count),
			None => Err(err!(
				ErrKind::IllegalState,
				"slab allocator has not been initialied"
			)),
		}
	}

	fn slab_size(&self) -> Result<u64, Error> {
		debug!("slab_size:self.config={:?}", self.config)?;
		match &self.config {
			Some(config) => Ok(config.slab_size),
			None => Err(err!(
				ErrKind::IllegalState,
				"slab allocator has not been initialied"
			)),
		}
	}
	fn init(&mut self, config: SlabAllocatorConfig) -> Result<(), Error> {
		match &self.config {
			Some(_) => Err(err!(
				ErrKind::IllegalState,
				"slab allocator has already been initialied"
			)),
			None => {
				let mut data = vec![];
				data.resize(usize!(config.slab_count * (config.slab_size + 8)), 0u8);
				Self::build_free_list(&mut data, config.slab_count, config.slab_size)?;
				self.data = data;
				self.free_count = config.slab_count;
				self.config = Some(config);
				self.first_free = 0;
				Ok(())
			}
		}
	}
}

impl SlabAllocatorImpl {
	fn new() -> Self {
		Self {
			config: None,
			free_count: 0,
			data: vec![],
			first_free: 0,
		}
	}

	fn build_free_list(data: &mut Vec<u8>, slab_count: u64, slab_size: u64) -> Result<(), Error> {
		for i in 0..slab_count {
			let next_bytes = if i < slab_count - 1 {
				(i + 1).to_be_bytes()
			} else {
				u64::MAX.to_be_bytes()
			};

			let offset_next = usize!(i * (8 + slab_size));
			data[offset_next..offset_next + 8].clone_from_slice(&next_bytes);
		}
		Ok(())
	}
}

impl SlabAllocatorBuilder {
	pub fn build_unsafe() -> UnsafeCell<Box<dyn SlabAllocator>> {
		UnsafeCell::new(Box::new(SlabAllocatorImpl::new()))
	}

	pub fn build() -> Box<dyn SlabAllocator + Send + Sync> {
		Box::new(SlabAllocatorImpl::new())
	}
}

#[cfg(test)]
mod test {
	use crate::types::SlabAllocatorConfig;
	use crate::SlabAllocatorBuilder;
	use bmw_err::Error;
	use bmw_log::*;

	info!();

	#[test]
	fn test_simple() -> Result<(), Error> {
		let mut slabs = SlabAllocatorBuilder::build();
		slabs.init(SlabAllocatorConfig::default())?;

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

	#[test]
	fn test_static_slaballoc() -> Result<(), Error> {
		crate::slabs::GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(), Error> {
			unsafe {
				f.get()
					.as_mut()
					.unwrap()
					.init(SlabAllocatorConfig::default())?;
				Ok(())
			}
		})?;
		let slab = crate::slabs::GLOBAL_SLAB_ALLOCATOR.with(
			|f| -> Result<Box<dyn crate::SlabMut>, Error> {
				Ok(unsafe { f.get().as_mut().unwrap().allocate()? })
			},
		)?;
		info!("slab={:?}", slab.get())?;
		Ok(())
	}
}
