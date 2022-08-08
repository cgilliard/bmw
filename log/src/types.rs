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

use bmw_err::Error;
use std::path::PathBuf;

pub enum RotationStatus {
	/// A rotation is not needed.
	NotNeeded,
	/// A rotation is needed.
	Needed,
	/// A rotation has occurred automatically.
	AutoRotated,
}

pub enum LogLevel {
	Trace,
	Debug,
	Info,
	Warn,
	Error,
	Fatal,
}

pub enum LogConfigOptionName {
	Colors,
	Stdout,
	MaxSizeBytes,
	MaxAgeMillis,
	Timestamp,
	Level,
	LineNum,
	ShowMillis,
	AutoRotate,
	FilePath,
	ShowBt,
	LineNumDataMaxLen,
	DeleteRotation,
	FileHeader,
}

#[derive(PartialEq, Debug)]
pub enum LogConfigOption {
	Colors(bool),
	Stdout(bool),
	MaxSizeBytes(u64),
	MaxAgeMillis(u64),
	Timestamp(bool),
	Level(bool),
	LineNum(bool),
	ShowMillis(bool),
	AutoRotate(bool),
	FilePath(Option<PathBuf>),
	ShowBt(bool),
	LineNumDataMaxLen(u64),
	DeleteRotation(bool),
	FileHeader(String),
}

pub struct LogConfig {
	pub colors: LogConfigOption,
	pub stdout: LogConfigOption,
	pub max_size_bytes: LogConfigOption,
	pub max_age_millis: LogConfigOption,
	pub timestamp: LogConfigOption,
	pub level: LogConfigOption,
	pub line_num: LogConfigOption,
	pub show_millis: LogConfigOption,
	pub auto_rotate: LogConfigOption,
	pub file_path: LogConfigOption,
}

impl Default for LogConfig {
	fn default() -> Self {
		Self {
			colors: LogConfigOption::Colors(true),
			stdout: LogConfigOption::Stdout(true),
			max_size_bytes: LogConfigOption::MaxSizeBytes(1024 * 1024),
			max_age_millis: LogConfigOption::MaxAgeMillis(1000 * 60 * 60),
			timestamp: LogConfigOption::Timestamp(true),
			level: LogConfigOption::Level(true),
			line_num: LogConfigOption::LineNum(true),
			show_millis: LogConfigOption::ShowMillis(true),
			auto_rotate: LogConfigOption::AutoRotate(true),
			file_path: LogConfigOption::FilePath(None),
		}
	}
}

pub trait Log {
	fn log(&mut self, level: LogLevel, line: &str) -> Result<(), Error>;
	fn rotate(&mut self) -> Result<(), Error>;
	fn rotation_status(&self) -> Result<RotationStatus, Error>;
	fn init(&mut self) -> Result<(), Error>;
	fn set_config_option(&mut self, value: LogConfigOption) -> Result<(), Error>;
	fn get_config_option(&self, option: LogConfigOptionName) -> Result<LogConfigOption, Error>;
}

#[cfg(test)]
mod test {
	use crate::types::{LogConfig, LogConfigOption};
	use bmw_err::Error;

	#[test]
	fn test_log_config() -> Result<(), Error> {
		let d = LogConfig::default();
		assert_eq!(d.colors, LogConfigOption::Colors(true));
		Ok(())
	}
}
