// Copyright (c) 2022, 37 Miners, LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! This crate includes the main error structs, enums and macros used
//! in bmw for building and mapping errors. This crate offers
//! wrappers around the rust failure crate. The [`crate::map_err`]
//! macro can be used to conveniently map errors from 3rd party crates
//! into [`crate::ErrorKind`] in this crate. The [`crate::errkind`] macro
//! can be used to generate errors. In most cases errors should be created
//! using one of these two macros.

use bmw_deps::failure;

mod error;
mod macros;

pub use crate::error::{ErrKind, Error, ErrorKind};
