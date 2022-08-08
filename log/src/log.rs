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

use crate::types::{
	Log, LogConfig, LogConfigOption, LogConfigOptionName, LogLevel, RotationStatus,
};
use bmw_err::Error;

struct LogImpl {
	config: LogConfig,
}

impl Log for LogImpl {
	fn log(&mut self, level: LogLevel, line: &str) -> Result<(), Error> {
		todo!()
	}
	fn rotate(&mut self) -> Result<(), Error> {
		todo!()
	}
	fn rotation_status(&self) -> Result<RotationStatus, Error> {
		todo!()
	}
	fn init(&mut self) -> Result<(), Error> {
		todo!()
	}
	fn set_config_option(&mut self, value: LogConfigOption) -> Result<(), Error> {
		todo!()
	}
	fn get_config_option(&self, option: LogConfigOptionName) -> Result<LogConfigOption, Error> {
		todo!()
	}
}

impl LogImpl {
	fn new(config: LogConfig) -> Result<Self, Error> {
		Self::check_config(&config)?;
		Ok(Self { config })
	}

	fn check_config(config: &LogConfig) -> Result<(), Error> {
		Ok(())
	}
}

pub struct LogBuilder {}

impl LogBuilder {
	pub fn build(config: LogConfig) -> Result<Box<dyn Log>, Error> {
		Ok(Box::new(LogImpl::new(config)?))
	}
}
