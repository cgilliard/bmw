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

use bmw_deps::failure::{Backtrace, Context, Fail};
use std::fmt::{Display, Formatter, Result};

/// Base Error struct which is used throught this crate
#[derive(Debug, Fail)]
pub struct Error {
	inner: Context<ErrorKind>,
}

/// Kinds of errors that can occur
#[derive(Clone, Eq, PartialEq, Debug, Fail)]
pub enum ErrorKind {
	/// IOError Error
	#[fail(display = "IOError Error: {}", _0)]
	IOError(String),
	/// UTF8 Error
	#[fail(display = "UTF8 Error: {}", _0)]
	Utf8Error(String),
	/// ArrayIndexOutOfBounds
	#[fail(display = "ArrayIndexOutofBounds: {}", _0)]
	ArrayIndexOutofBounds(String),
	/// EventHandlerConfigurationError
	#[fail(display = "Configuration Error: {}", _0)]
	Configuration(String),
	/// Poison error multiple locks
	#[fail(display = "Poison Error: {}", _0)]
	Poison(String),
	/// CorruptedDataError
	#[fail(display = "Corrupted Data Error: {}", _0)]
	CorruptedData(String),
	/// Timeout
	#[fail(display = "Timeout: {}", _0)]
	Timeout(String),
	/// Capacity Exceeded
	#[fail(display = "Capacity Exceeded: {}", _0)]
	CapacityExceeded(String),
	/// UnexpectedEof error
	#[fail(display = "UnexpectedEOF: {}", _0)]
	UnexpectedEof(String),
	/// IllegalArgument
	#[fail(display = "IllegalArgument: {}", _0)]
	IllegalArgument(String),
}

impl Display for Error {
	fn fmt(&self, f: &mut Formatter<'_>) -> Result {
		let cause = match self.cause() {
			Some(c) => format!("{}", c),
			None => String::from("Unknown"),
		};
		let backtrace = match self.backtrace() {
			Some(b) => format!("{}", b),
			None => String::from("Unknown"),
		};
		let output = format!(
			"{} \n Cause: {} \n Backtrace: {}",
			self.inner, cause, backtrace
		);
		Display::fmt(&output, f)
	}
}

impl Error {
	/// get kind
	pub fn kind(&self) -> ErrorKind {
		self.inner.get_context().clone()
	}
	/// get cause
	pub fn cause(&self) -> Option<&dyn Fail> {
		self.inner.cause()
	}
	/// get backtrace
	pub fn backtrace(&self) -> Option<&Backtrace> {
		self.inner.backtrace()
	}

	/// get inner
	pub fn inner(&self) -> String {
		self.inner.to_string()
	}
}

impl From<ErrorKind> for Error {
	fn from(kind: ErrorKind) -> Error {
		Error {
			inner: Context::new(kind),
		}
	}
}

impl From<std::io::Error> for Error {
	fn from(e: std::io::Error) -> Error {
		Error {
			inner: Context::new(ErrorKind::IOError(format!("{}", e))),
		}
	}
}
