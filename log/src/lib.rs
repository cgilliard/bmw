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

//! Logging crate used by other crates in bmw. The crate has an extensive macro
//! library that allows for logging at the standard 6 levels and also
//! allows for specifying a log file and various options. All options can
//! be seen in the [`crate::LogConfig`] struct. This crate is largely compatible
//! with the [log](https://docs.rs/log/latest/log/) crate. So any code
//! that was written to work with that crate will work with this crate with minor
//! adjustments. In addition to the [`trace`], [`debug`], [`info`], [`warn`], [`error`]
//! and [`fatal`] log levels, this crate provides an 'all' version and 'plain'
//! version of each macro. For example: [`info_all`] and [`info_plain`].
//! These macros allow for logging to standard out, no matter how the log is
//! configured and log without the timestamp respectively. The main difference
//! from the rust log crate is that this crate returns errors so you will have
//! to add the error handling which can be as simple as using the question mark operator.
//!
//! The default output will look something like this:
//!
//! ```text
//! [2022-02-24 13:52:24]: (FATAL) [..ioruntime/src/main.rs:116]: fatal
//! [2022-02-24 13:52:24]: (ERROR) [..ioruntime/src/main.rs:120]: error
//! [2022-02-24 13:52:24]: (WARN) [..ioruntime/src/main.rs:124]: warn
//! [2022-02-24 13:52:24]: (INFO) [..ioruntime/src/main.rs:128]: info
//! [2022-02-24 13:52:24]: (DEBUG) [..ioruntime/src/main.rs:132]: debug
//! [2022-02-24 13:52:24]: (TRACE) [..ioruntime/src/main.rs:136]: trace
//! ```
//!
//! If enabled color coding is included as well.

mod log;
mod macros;
mod types;

pub use crate::log::LogBuilder;

pub use crate::macros::STATIC_LOG;
pub use crate::types::{Log, LogConfig, LogConfigOption, LogConfigOptionName, LogLevel};
