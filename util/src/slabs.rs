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
use bmw_err::{err, try_into, ErrKind, Error};
use bmw_log::*;
use std::cell::UnsafeCell;

info!();

// tarpaulin is not seeing this line as covered probably because it's thread_local.
// The GLOBAL_SLAB_ALLOCATOR is used in several tests. Not including in tarpaulin
// results.
#[cfg(not(tarpaulin_include))]
thread_local! {
	#[doc(hidden)]
	pub static GLOBAL_SLAB_ALLOCATOR: UnsafeCell<Box<dyn SlabAllocator>> =
				SlabAllocatorBuilder::build_unsafe();
}

pub struct SlabAllocatorBuilder {}

struct SlabMutImpl<'a> {
	pub data: &'a mut [u8],
	pub id: usize,
}

struct SlabImpl<'a> {
	pub data: &'a [u8],
	pub id: usize,
}

struct SlabAllocatorImpl {
	config: Option<SlabAllocatorConfig>,
	data: Vec<u8>,
	first_free: usize,
	free_count: usize,
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
	fn id(&self) -> usize {
		self.id
	}
}

impl<'a> Slab for SlabImpl<'a> {
	fn get(&self) -> &[u8] {
		&self.data
	}
	fn id(&self) -> usize {
		self.id
	}
}

impl SlabAllocator for SlabAllocatorImpl {
	fn allocate<'a>(&'a mut self) -> Result<Box<dyn SlabMut + 'a>, Error> {
		if self.config.is_none() {
			return Err(err!(ErrKind::IllegalState, "not initialied"));
		}
		let config = self.config.as_ref().unwrap();
		debug!("allocate:self.config={:?}", config)?;
		if self.first_free == usize::MAX {
			return Err(err!(ErrKind::CapacityExceeded, "no more slabs available"));
		}

		let id = self.first_free;
		let offset = (8 + config.slab_size) * id;
		self.first_free = usize::from_be_bytes(try_into!(self.data[offset..offset + 8])?);

		let offset = offset + 8;
		// mark it as not free
		self.data[(8 + config.slab_size) * id..(8 + config.slab_size) * id + 8]
			.clone_from_slice(&usize::MAX.to_be_bytes());
		let data = &mut self.data[offset..offset + config.slab_size as usize];
		self.free_count = self.free_count.saturating_sub(1);

		Ok(Box::new(SlabMutImpl { data, id }))
	}
	fn free(&mut self, id: usize) -> Result<(), Error> {
		match &self.config {
			Some(config) => {
				if id >= config.slab_count {
					let fmt = format!("slab.id = {}, total slabs = {}", id, config.slab_count);
					return Err(err!(ErrKind::ArrayIndexOutOfBounds, fmt));
				}
				let offset = (8 + config.slab_size) * id;

				// check that it's currently allocated
				if self.data[offset..offset + 8] != [255, 255, 255, 255, 255, 255, 255, 255] {
					// double free error
					let fmt = format!("slab.id = {} has been freed when not allocated", id);
					return Err(err!(ErrKind::IllegalState, fmt));
				}

				debug!("free:self.config={:?},id={}", config, id)?;

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
	fn get<'a>(&'a self, id: usize) -> Result<Box<dyn Slab + 'a>, Error> {
		if self.config.is_none() {
			return Err(err!(ErrKind::IllegalState, "not initialied"));
		}
		let config = self.config.as_ref().unwrap();
		if id >= config.slab_count {
			let fmt = format!("slab.id = {}, total slabs = {}", id, config.slab_count);
			return Err(err!(ErrKind::ArrayIndexOutOfBounds, fmt));
		}
		debug!("get:self.config={:?},id={}", config, id)?;
		let offset = 8 + ((8 + config.slab_size) * id);
		let data = &self.data[offset..offset + config.slab_size];
		Ok(Box::new(SlabImpl { data, id }))
	}
	fn get_mut<'a>(&'a mut self, id: usize) -> Result<Box<dyn SlabMut + 'a>, Error> {
		if self.config.is_none() {
			return Err(err!(ErrKind::IllegalState, "not initialied"));
		}
		let config = self.config.as_ref().unwrap();
		if id >= config.slab_count {
			let fmt = format!("slab.id = {}, total slabs = {}", id, config.slab_count);
			return Err(err!(ErrKind::ArrayIndexOutOfBounds, fmt));
		}
		debug!("get_mut:self.config={:?},id={}", config, id)?;
		let offset = 8 + ((8 + config.slab_size) * id);
		let data = &mut self.data[offset..offset + config.slab_size as usize];
		Ok(Box::new(SlabMutImpl { data, id }))
	}

	fn free_count(&self) -> Result<usize, Error> {
		if self.config.is_none() {
			return Err(err!(ErrKind::IllegalState, "not initialied"));
		}
		let config = self.config.as_ref().unwrap();
		debug!("free_count:self.config={:?}", config)?;
		Ok(self.free_count)
	}

	fn slab_size(&self) -> Result<usize, Error> {
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
				if config.slab_size < 48 {
					return Err(err!(
						ErrKind::IllegalArgument,
						"slab_size must be at least 48 bytes"
					));
				}
				if config.slab_count == 0 {
					return Err(err!(
						ErrKind::IllegalArgument,
						"slab_count must be greater than 0"
					));
				}
				let mut data = vec![];
				data.resize(config.slab_count * (config.slab_size + 8), 0u8);
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

	fn build_free_list(
		data: &mut Vec<u8>,
		slab_count: usize,
		slab_size: usize,
	) -> Result<(), Error> {
		for i in 0..slab_count {
			let next_bytes = if i < slab_count - 1 {
				(i + 1).to_be_bytes()
			} else {
				usize::MAX.to_be_bytes()
			};

			let offset_next = i * (8 + slab_size);
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

	#[test]
	fn test_capacity() -> Result<(), Error> {
		let mut slabs = SlabAllocatorBuilder::build();
		slabs.init(SlabAllocatorConfig {
			slab_count: 10,
			..SlabAllocatorConfig::default()
		})?;
		for _ in 0..10 {
			slabs.allocate()?;
		}
		assert!(slabs.allocate().is_err());

		slabs.free(0)?;
		assert!(slabs.allocate().is_ok());
		assert!(slabs.allocate().is_err());
		Ok(())
	}

	#[test]
	fn test_error_conditions() -> Result<(), Error> {
		let mut slabs = SlabAllocatorBuilder::build();
		assert!(slabs.allocate().is_err());
		assert!(slabs.free(0).is_err());
		assert!(slabs.get(0).is_err());
		assert!(slabs.get_mut(0).is_err());
		assert!(slabs.free_count().is_err());
		slabs.init(SlabAllocatorConfig::default())?;
		assert!(slabs.allocate().is_ok());
		assert!(slabs.free_count().is_ok());
		assert!(slabs.free(usize::MAX).is_err());
		assert!(slabs.get(usize::MAX).is_err());
		assert!(slabs.get_mut(usize::MAX).is_err());
		assert!(slabs.init(SlabAllocatorConfig::default()).is_err());
		Ok(())
	}

	#[test]
	fn test_double_free() -> Result<(), Error> {
		let mut slabs = SlabAllocatorBuilder::build();
		slabs.init(SlabAllocatorConfig::default())?;
		let id = {
			let slab = slabs.allocate()?;
			slab.id()
		};
		slabs.free(id)?;
		assert!(slabs.free(id).is_err());
		let id2 = {
			let slab = slabs.allocate()?;
			slab.id()
		};
		slabs.free(id2)?;
		assert!(slabs.free(id2).is_err());
		// we know id and id2 are equal because when you free a slab it's added to the
		// front of the list
		assert_eq!(id, id2);
		Ok(())
	}

	#[test]
	fn test_other_slabs_configs() -> Result<(), Error> {
		assert!(SlabAllocatorBuilder::build()
			.init(SlabAllocatorConfig::default())
			.is_ok());

		assert!(SlabAllocatorBuilder::build()
			.init(SlabAllocatorConfig {
				slab_size: 100,
				..SlabAllocatorConfig::default()
			})
			.is_ok());

		assert!(SlabAllocatorBuilder::build()
			.init(SlabAllocatorConfig {
				slab_size: 48,
				..SlabAllocatorConfig::default()
			})
			.is_ok());

		assert!(SlabAllocatorBuilder::build()
			.init(SlabAllocatorConfig {
				slab_size: 47,
				..SlabAllocatorConfig::default()
			})
			.is_err());
		assert!(SlabAllocatorBuilder::build()
			.init(SlabAllocatorConfig {
				slab_count: 0,
				..SlabAllocatorConfig::default()
			})
			.is_err());

		let mut sh = SlabAllocatorBuilder::build();
		sh.init(SlabAllocatorConfig {
			slab_count: 1,
			..SlabAllocatorConfig::default()
		})?;

		let slab = sh.allocate();
		assert!(slab.is_ok());
		let id = slab.unwrap().id();
		assert!(sh.allocate().is_err());
		sh.free(id)?;
		assert!(sh.allocate().is_ok());

		Ok(())
	}
}
