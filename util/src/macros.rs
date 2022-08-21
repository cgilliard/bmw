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

/// Macro to initialize the global slab allocator. It is important to note that
/// this macro only affects the thread in which it is executed in and must be called
/// separately in each thread that you wish to initialize the global slab allocator
/// in. Also, the data structures, like [`crate::StaticHashtable`] and
/// [`crate::StaticHashset`], will initialized this slab allocator with default values
/// if this macro is not called first. Therefore, it makes sense to call this very soon
/// after starting a thread that will use it. The slab allocator is initialized with
/// `slab_size` and `slab_count` parameters respecitvely. `slab_size` is the size in bytes
/// of slabs. The default value is 1_024 bytes. `slab_count` is the number of slabs to
/// initialize. The default value is 10_240. The defaults will be used if this macro is
/// not called.
///
/// # Errors
///
/// [`init_slab_allocator`] returns a Ok(()) on success or returns an [`bmw_err::Error`]
/// if an error occurs.
///
/// * [`bmw_err::ErrorKind::IllegalArgument`] if the `slab_size` is less than 48 bytes or
/// if `slab_count` is equal to 0.
///
/// * [`bmw_err::ErrorKind::IllegalState`] if the thread local global slab allocator for this
/// thread has already been initialized.
///
/// # Examples
///```
///```
///
/// See [`crate::slab_allocator`] for details on how to create a new slab allocator as opposed
/// to the global thread local slab allocator.
///
#[macro_export]
macro_rules! init_slab_allocator {
	($slab_size:expr, $slab_count:expr) => {{
		bmw_util::GLOBAL_SLAB_ALLOCATOR.with(|f| -> Result<(), Error> {
			unsafe {
				f.get()
					.as_mut()
					.unwrap()
					.init(bmw_util::SlabAllocatorConfig {
						slab_count: $slab_count,
						slab_size: $slab_size,
						..Default::default()
					})?;
				Ok(())
			}
		})?;
	}};
}

/// The [`crate::slab_allocator`] macro allows for creation of a slab allocator that may be used
/// by the data structures in this crate. If no parameters are specified, the default values of
/// `slab_size` equal to 1_024 and `slab_count` of 10_240 are used. If one paramater is specifed,
/// it is used as `slab_count` and the default is used for `slab_size`. If two parameters are
/// specified, it is used as `slab_count` and `slab_size` respectively. The `slab_count` is
/// the total number of slabs in this slab allocator. It is important to note that additional
/// slabs may not be added after startup. `The slab_size` is the size in bytes of the slabs
/// in this instance.
///
/// # Errors
///
/// [`crate::slab_allocator`] returns a [`crate::SlabAllocator`] on success. On error,
/// a [`bmw_err::Error`] is returned.
///
/// * [`bmw_err::ErrorKind::IllegalArgument`] if the `slab_size` is less than 48 bytes or
/// if `slab_count` is equal to 0.
///
/// * [`bmw_err::ErrorKind::IllegalState`] if the thread local global slab allocator for this
/// thread has already been initialized.
///
/// # Examples
///
///```
///```
///
/// Note: the slab allocator returned by this macro is distinct from the global thread
/// local slab allocator that is initialized by the [`crate::init_slab_allocator`] macro.
#[macro_export]
macro_rules! slab_allocator {
	() => {{
		let mut slabs = bmw_util::SlabAllocatorBuilder::build();
		match slabs.init(bmw_util::SlabAllocatorConfig::default()) {
			Ok(_) => Ok(slabs),
			Err(e) => Err(bmw_err::err!(
				bmw_err::ErrKind::Configuration,
				format!("{}", e)
			)),
		}
	}};
	($slab_size:expr) => {{
		let config = bmw_util::SlabAllocatorConfig {
			slab_size: $slab_size,
			..Default::default()
		};
		let mut slabs = bmw_util::SlabAllocatorBuilder::build();
		match slabs.init(config) {
			Ok(_) => Ok(slabs),
			Err(e) => Err(bmw_err::err!(
				bmw_err::ErrKind::Configuration,
				format!("{}", e)
			)),
		}
	}};
	($slab_size:expr, $slab_count:expr) => {{
		let config = bmw_util::SlabAllocatorConfig {
			slab_count: $slab_count,
			slab_size: $slab_size,
			..Default::default()
		};
		let mut slabs = bmw_util::SlabAllocatorBuilder::build();
		match slabs.init(config) {
			Ok(_) => Ok(slabs),
			Err(e) => Err(bmw_err::err!(
				bmw_err::ErrKind::Configuration,
				format!("{}", e)
			)),
		}
	}};
}
