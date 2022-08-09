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

use crate::types::{Log, LogConfig, LogConfigOption, LogConfigOptionName, LogLevel};
use crate::LogConfigOption::{
	AutoRotate, Colors, DeleteRotation, FileHeader, FilePath, Level, LineNum, LineNumDataMaxLen,
	MaxAgeMillis, MaxSizeBytes, ShowBt, ShowMillis, Stdout, Timestamp,
};
use bmw_deps::backtrace;
use bmw_deps::backtrace::Backtrace;
use bmw_deps::chrono::{DateTime, Local};
use bmw_deps::colored::Colorize;
use bmw_deps::rand::random;
use bmw_err::{errkind, ErrKind, Error};
use std::convert::TryInto;
use std::fs::{canonicalize, remove_file, rename, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

const NEWLINE: &[u8] = &['\n' as u8];

struct LogImpl {
	config: LogConfig,
	init: bool,
	file: Option<File>,
	cur_size: u64,
	last_rotation: Instant,
}

impl Log for LogImpl {
	fn log(&mut self, level: LogLevel, line: &str, now: Option<Instant>) -> Result<(), Error> {
		if !self.init {
			return Err(errkind!(ErrKind::Log, "log not initialized"));
		}

		self.rotate_if_needed(now)?;

		let show_stdout = match self.config.stdout {
			Stdout(s) => s,
			_ => {
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"stdout must be of the form Stdout(bool)"
				));
			}
		};

		let show_timestamp = match self.config.timestamp {
			Timestamp(t) => t,
			_ => {
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"timestamp must be of the form Timestamp(bool)"
				));
			}
		};

		let show_colors = match self.config.colors {
			Colors(c) => c,
			_ => {
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"colors must be of the form Colors(bool)"
				));
			}
		};

		let show_log_level = match self.config.level {
			Level(l) => l,
			_ => {
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"log_level must be of the form LogLevel(bool)"
				));
			}
		};

		let show_line_num = match self.config.line_num {
			LineNum(l) => l,
			_ => {
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"line_num must be of the form LineNUm(bool)"
				));
			}
		};

		let show_millis = match self.config.show_millis {
			ShowMillis(m) => m,
			_ => {
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"show_millis must be of the form ShowMillis(bool)"
				));
			}
		};

		let show_bt = match self.config.show_bt {
			ShowBt(b) => b && (level == LogLevel::Error || level == LogLevel::Fatal),
			_ => {
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"show_bt must be of the form ShowBt(bool)"
				));
			}
		};

		let file_is_some = self.file.is_some();

		if show_timestamp {
			let date = Local::now();
			let millis = date.timestamp_millis() % 1_000;
			let formatted_timestamp = if show_millis {
				format!("{}.{}", date.format("%Y-%m-%d %H:%M:%S"), millis)
			} else {
				format!("{}", date.format("%Y-%m-%d %H:%M:%S"))
			};

			if file_is_some {
				let formatted_timestamp = format!("[{}]: ", formatted_timestamp);
				let formatted_timestamp = formatted_timestamp.as_bytes();
				self.file.as_ref().unwrap().write(formatted_timestamp)?;
				let formatted_len: u64 = formatted_timestamp.len().try_into()?;
				self.cur_size += formatted_len;
			}

			if show_stdout {
				if show_colors {
					print!("[{}]: ", formatted_timestamp.to_string().dimmed());
				} else {
					print!("[{}]: ", formatted_timestamp);
				}
			}
		}

		if show_log_level {
			if file_is_some {
				let formatted_level = format!("({}) ", level);
				let formatted_level = formatted_level.as_bytes();
				self.file.as_ref().unwrap().write(formatted_level)?;
				let formatted_len: u64 = formatted_level.len().try_into()?;
				self.cur_size += formatted_len;
			}

			if show_stdout {
				if show_colors {
					match level {
						LogLevel::Trace | LogLevel::Debug => {
							print!("({}) ", format!("{}", level).magenta());
						}
						LogLevel::Info => {
							print!("({}) ", format!("{}", level).green());
						}
						LogLevel::Warn => {
							print!("({}) ", format!("{}", level).yellow());
						}
						LogLevel::Error | LogLevel::Fatal => {
							print!("({}) ", format!("{}", level).red());
						}
					}
				} else {
					print!("({}) ", level);
				}
			}
		}

		if show_line_num {
			let mut found_logger = false;
			let mut found_frame = false;
			let mut logged_from_file = "unknown".to_string();
			backtrace::trace(|frame| {
				backtrace::resolve_frame(frame, |symbol| {
					#[cfg(debug_assertions)]
					if let Some(filename) = symbol.filename() {
						let filename = filename.display().to_string();
						let lineno = match symbol.lineno() {
							Some(lineno) => lineno.to_string(),
							None => "".to_string(),
						};

						if filename.find("/log/src/log.rs").is_some() {
							found_logger = true;
						}
						if filename.find("/log/src/log.rs").is_none() && found_logger {
							logged_from_file = format!("{}:{}", filename, lineno);
							found_frame = true;
						}
					}
					#[cfg(not(debug_assertions))]
					if let Some(name) = symbol.name() {
						let name = name.to_string();
						if name.find("bmw_log::LogImpl::log").is_some() {
							found_logger = true;
						}
						if name.find("bmw_log::LogImpl::log").is_none() && found_logger {
							let pos = name.rfind(':');
							let name = match pos {
								Some(pos) => match pos > 1 {
									true => &name[0..pos - 1],
									false => &name[0..pos],
								},
								None => &name[..],
							};
							logged_from_file = format!("{}", name);
							found_frame = true;
						}
					}
				});
				!found_frame
			});

			let max_len = match self.config.line_num_data_max_len {
				LineNumDataMaxLen(max_len) => max_len,
				_ => 25, // should not be possible but just use default here
			};

			let len = logged_from_file.len();
			if len > max_len {
				let start = len.saturating_sub(max_len);
				logged_from_file = format!("..{}", &logged_from_file[start..]);
			}

			if file_is_some {
				let mut file = self.file.as_ref().unwrap();
				let logged_from_file = format!("[{}]: ", logged_from_file);
				let logged_from_file = logged_from_file.as_bytes();
				file.write(logged_from_file)?;
				let logged_from_file_len: u64 = logged_from_file.len().try_into()?;
				self.cur_size += logged_from_file_len;
			}

			if show_stdout {
				if show_colors {
					print!("[{}]: ", logged_from_file.yellow());
				} else {
					print!("[{}]: ", logged_from_file);
				}
			}
		}

		if file_is_some {
			let line_bytes = line.as_bytes();
			let mut file = self.file.as_ref().unwrap();
			file.write(line_bytes)?;
			file.write(NEWLINE)?;
			let line_bytes_len: u64 = line_bytes.len().try_into()?;
			self.cur_size += line_bytes_len;

			if show_bt {
				let bt = Backtrace::new();
				let bt_text = format!("{:?}", bt);
				let bt_bytes: &[u8] = bt_text.as_bytes();
				file.write(bt_bytes)?;
				let bt_bytes_len: u64 = bt_bytes.len().try_into()?;
				self.cur_size += bt_bytes_len;
			}
		}

		if show_stdout {
			println!("{}", line);
			if show_bt {
				let bt = Backtrace::new();
				let bt_text = format!("{:?}", bt);
				print!("{}", bt_text);
			}
		}

		Ok(())
	}

	fn rotate(&mut self) -> Result<(), Error> {
		if !self.init {
			return Err(errkind!(ErrKind::Log, "log not initialized"));
		}

		if self.file.is_none() {
			// no file, nothing to rotate
			return Ok(());
		}

		let now: DateTime<Local> = Local::now();
		let rotation_string = now.format(".r_%m_%d_%Y_%T").to_string().replace(":", "-");

		let original_file_path = match self.config.file_path.clone() {
			FilePath(file_path) => match file_path {
				Some(file_path) => file_path,
				None => {
					return Err(errkind!(
						ErrKind::IllegalArgument,
						"file_path must be of the for FilePath(Option<PathBuf>)"
					))
				}
			},
			_ => {
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"file_path must be of the for FilePath(Option<PathBuf>)"
				));
			}
		};

		// get the parent directory and the file name
		let parent = match original_file_path.parent() {
			Some(parent) => parent,
			None => {
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"file_path has an unexpected illegal value of None for parent"
				));
			}
		};

		let file_name = match original_file_path.file_name() {
			Some(file_name) => file_name,
			None => {
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"file_path has an unexpected illegal value of None for file_name"
				))
			}
		};

		let file_name = match file_name.to_str() {
			Some(file_name) => file_name,
			None => {
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"file_path has an unexpected illegal value of None for file_name"
				))
			}
		};

		let mut new_file_path_buf = parent.to_path_buf();
		let file_name = match file_name.rfind(".") {
			Some(pos) => &file_name[0..pos],
			_ => &file_name,
		};
		let file_name = format!("{}{}_{}.log", file_name, rotation_string, random::<u64>());
		new_file_path_buf.push(file_name);

		match self.config.delete_rotation {
			DeleteRotation(delete_rotation) => {
				if delete_rotation {
					remove_file(&original_file_path)?;
				} else {
					rename(&original_file_path, new_file_path_buf.clone())?;
				}
			}
			_ => {
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"delete_rotation has an unexpected illegal value"
				))
			}
		}

		let mut file = OpenOptions::new()
			.append(true)
			.create(true)
			.open(&original_file_path)?;
		self.check_open(&mut file, &original_file_path)?;
		self.file = Some(file);

		Ok(())
	}

	fn rotation_needed(&self, now: Option<Instant>) -> Result<bool, Error> {
		if !self.init {
			return Err(errkind!(ErrKind::Log, "log not initialized"));
		}

		let now = now.unwrap_or(Instant::now());

		let max_age_millis = match self.config.max_age_millis {
			MaxAgeMillis(m) => m,
			_ => {
				// should not get here, but return an error if we do
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"max_age_millis must be of form MaxAgeMillis(u128)"
				));
			}
		};
		let max_size_bytes = match self.config.max_size_bytes {
			MaxSizeBytes(m) => m,
			_ => {
				// should not get here, but return an error if we do
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"max_size_bytes must be of form MaxSizeBytes(u64)"
				));
			}
		};

		if now.duration_since(self.last_rotation).as_millis() > max_age_millis
			|| self.cur_size > max_size_bytes
		{
			Ok(true)
		} else {
			Ok(false)
		}
	}

	fn init(&mut self) -> Result<(), Error> {
		if self.init {
			return Err(errkind!(ErrKind::Log, "log already initialized"));
		}

		match self.config.file_path.clone() {
			FilePath(file_path) => match file_path {
				Some(file_path) => {
					println!("opening file = {:?}", file_path);
					let mut file = if file_path.exists() {
						OpenOptions::new().append(true).open(file_path.clone())
					} else {
						OpenOptions::new()
							.append(true)
							.create(true)
							.open(file_path.clone())
					}?;
					self.check_open(&mut file, &file_path)?;
					self.file = Some(file);
				}
				None => {}
			},
			_ => {
				// should not be able to get here
				return Err(errkind!(
					ErrKind::Log,
					"file_path must be of the form FilePath(Option<PathBuf>)"
				));
			}
		}

		self.init = true;

		Ok(())
	}
	fn set_config_option(&mut self, value: LogConfigOption) -> Result<(), Error> {
		match value {
			Colors(_) => {
				self.config.colors = value;
			}
			Stdout(_) => {
				self.config.stdout = value;
			}
			MaxSizeBytes(_) => {
				self.config.max_size_bytes = value;
			}
			MaxAgeMillis(_) => {
				self.config.max_age_millis = value;
			}
			Timestamp(_) => {
				self.config.timestamp = value;
			}
			Level(_) => {
				self.config.level = value;
			}
			LineNum(_) => {
				self.config.line_num = value;
			}
			ShowMillis(_) => {
				self.config.show_millis = value;
			}
			AutoRotate(_) => {
				self.config.auto_rotate = value;
			}
			FilePath(_) => {
				self.config.file_path = value;
			}
			ShowBt(_) => {
				self.config.show_bt = value;
			}
			LineNumDataMaxLen(_) => {
				self.config.line_num_data_max_len = value;
			}
			DeleteRotation(_) => {
				self.config.delete_rotation = value;
			}
			FileHeader(_) => {
				self.config.file_header = value;
			}
		}

		Ok(())
	}
	fn get_config_option(&self, option: LogConfigOptionName) -> Result<&LogConfigOption, Error> {
		Ok(match option {
			LogConfigOptionName::Colors => &self.config.colors,
			LogConfigOptionName::Stdout => &self.config.stdout,
			LogConfigOptionName::MaxSizeBytes => &self.config.max_size_bytes,
			LogConfigOptionName::MaxAgeMillis => &self.config.max_age_millis,
			LogConfigOptionName::Timestamp => &self.config.timestamp,
			LogConfigOptionName::Level => &self.config.level,
			LogConfigOptionName::LineNum => &self.config.line_num,
			LogConfigOptionName::ShowMillis => &self.config.show_millis,
			LogConfigOptionName::AutoRotate => &self.config.auto_rotate,
			LogConfigOptionName::FilePath => &self.config.file_path,
			LogConfigOptionName::ShowBt => &self.config.show_bt,
			LogConfigOptionName::LineNumDataMaxLen => &self.config.line_num_data_max_len,
			LogConfigOptionName::DeleteRotation => &self.config.delete_rotation,
			LogConfigOptionName::FileHeader => &self.config.file_header,
		})
	}
}

impl LogImpl {
	fn new(config: LogConfig) -> Result<Self, Error> {
		Self::check_config(&config)?;
		Ok(Self {
			config,
			init: false,
			file: None,
			cur_size: 0,
			last_rotation: Instant::now(),
		})
	}

	fn rotate_if_needed(&mut self, now_param: Option<Instant>) -> Result<(), Error> {
		match self.config.auto_rotate {
			AutoRotate(r) => {
				if !r {
					return Ok(()); // auto rotate not enabled
				}
			}
			_ => {
				// should not get here, but return an error if we do
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"autorotate must be of form AutoRotate(bool)"
				));
			}
		}
		let now = match now_param {
			Some(now) => now,
			None => Instant::now(),
		};

		let max_age_millis = match self.config.max_age_millis {
			MaxAgeMillis(m) => m,
			_ => {
				// should not get here, but return an error if we do
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"max_age_millis must be of form MaxAgeMillis(u128)"
				));
			}
		};
		let max_size_bytes = match self.config.max_size_bytes {
			MaxSizeBytes(m) => m,
			_ => {
				// should not get here, but return an error if we do
				return Err(errkind!(
					ErrKind::IllegalArgument,
					"max_size_bytes must be of form MaxSizeBytes(u64)"
				));
			}
		};

		if now.duration_since(self.last_rotation).as_millis() > max_age_millis
			|| self.cur_size > max_size_bytes
		{
			self.rotate()?;
		}

		Ok(())
	}

	fn check_config(config: &LogConfig) -> Result<(), Error> {
		match &config.file_path {
			FilePath(file_path) => match &file_path {
				Some(file_path) => {
					match file_path.parent() {
						Some(parent) => {
							if !parent.exists() {
								return Err(errkind!(
									ErrKind::Configuration,
									"file_path parent must exist"
								));
							}
						}
						None => {
							return Err(errkind!(
								ErrKind::Configuration,
								"if file_path specifies a PathBuf, the PathBuf must\
not terminate in a root or a prefix"
							));
						}
					}

					if file_path.is_dir() {
						return Err(errkind!(
							ErrKind::Configuration,
							"file_path must not be a directory"
						));
					}

					match file_path.file_name() {
						Some(_) => {}
						None => {
							return Err(errkind!(
								ErrKind::Configuration,
								"file_path file_name must not terminate in a root or a prefix"
							))
						}
					}
				}
				None => {}
			},
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"file_path must be of the form FilePath(Option<PathBuf>)"
				));
			}
		}
		match config.colors {
			Colors(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"colors must be of the form Colors(bool)"
				));
			}
		}
		match config.stdout {
			Stdout(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"stdout must be of the form Stdout(bool)"
				));
			}
		}
		match config.max_size_bytes {
			MaxSizeBytes(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"max_size_bytes must be of the form MaxSizeBytes(u64)"
				));
			}
		}
		match config.max_age_millis {
			MaxAgeMillis(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"max_age_millis must be of the form MaxAgeMillis(u64)"
				));
			}
		}

		match config.timestamp {
			Timestamp(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"timestamp must be of the form Timestamp(bool)"
				));
			}
		}
		match config.level {
			Level(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"level must be of the form Level(bool)"
				));
			}
		}
		match config.line_num {
			LineNum(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"line_num must be of the form LineNum(bool)"
				));
			}
		}
		match config.show_millis {
			ShowMillis(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"show_millis must be of the form ShowMillis(bool)"
				));
			}
		}
		match config.auto_rotate {
			AutoRotate(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"auto_rotate must be of the form AutoRotate(bool)"
				));
			}
		}
		match config.show_bt {
			ShowBt(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"show_bt must be of the form ShowBt(bool)"
				));
			}
		}
		match config.line_num_data_max_len {
			LineNumDataMaxLen(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"line_num_data_max_len must be of the form LineNumDataMaxLen(u64)"
				));
			}
		}
		match config.delete_rotation {
			DeleteRotation(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"delete_rotation must be of the form DeleteRotation(bool)"
				));
			}
		}

		match config.file_header {
			FileHeader(_) => {}
			_ => {
				return Err(errkind!(
					ErrKind::Configuration,
					"file_header must be of the form FileHeader(String)"
				));
			}
		}

		Ok(())
	}

	fn check_open(&mut self, file: &mut File, path: &PathBuf) -> Result<(), Error> {
		let metadata = match file.metadata() {
			Ok(metadata) => metadata,
			Err(_) => {
				return Err(errkind!(
					ErrKind::Log,
					format!("failed to retreive metadata for file: {}", path.display())
				));
			}
		};

		let len = metadata.len();
		if len == 0 {
			// do we need to add the file header?
			match &self.config.file_header {
				FileHeader(header) => {
					if header.len() > 0 {
						// there's a header. We need to append it.
						file.write(header.as_bytes())?;
						file.write(NEWLINE)?;
						let header_len: u64 = header.len().try_into()?;
						self.cur_size = header_len + 1;
					} else {
						self.cur_size = 0;
					}
				}
				_ => {
					self.cur_size = 0;
				}
			}
		} else {
			self.cur_size = len;
		}

		// update our canonicalization. This makes it so if we change directories later we
		// have the full path to the log file.
		self.config.file_path = FilePath(Some(canonicalize(path)?));
		self.last_rotation = Instant::now();
		Ok(())
	}
}

pub struct LogBuilder {}

impl LogBuilder {
	pub fn build(config: LogConfig) -> Result<Box<dyn Log>, Error> {
		Ok(Box::new(LogImpl::new(config)?))
	}
}

#[cfg(test)]
mod test {
	use crate::LogConfigOption::{
		AutoRotate, Colors, DeleteRotation, FileHeader, FilePath, Level, LineNum,
		LineNumDataMaxLen, MaxAgeMillis, MaxSizeBytes, ShowBt, ShowMillis, Stdout, Timestamp,
	};
	use crate::{LogBuilder, LogConfig, LogConfigOptionName, LogLevel};
	use bmw_err::Error;
	use bmw_test::testdir::{setup_test_dir, tear_down_test_dir};
	use std::path::PathBuf;

	#[test]
	fn test_bad_configs() -> Result<(), Error> {
		assert!(LogBuilder::build(LogConfig::default()).is_ok());

		assert!(LogBuilder::build(LogConfig {
			colors: Colors(false),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			colors: DeleteRotation(false),
			..Default::default()
		})
		.is_err());

		assert!(LogBuilder::build(LogConfig {
			stdout: Stdout(false),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			stdout: DeleteRotation(false),
			..Default::default()
		})
		.is_err());

		assert!(LogBuilder::build(LogConfig {
			max_size_bytes: MaxSizeBytes(1_000_000),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			max_size_bytes: Level(false),
			..Default::default()
		})
		.is_err());

		assert!(LogBuilder::build(LogConfig {
			max_age_millis: MaxAgeMillis(3_600_000),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			max_age_millis: Colors(true),
			..Default::default()
		})
		.is_err());

		assert!(LogBuilder::build(LogConfig {
			timestamp: Timestamp(false),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			timestamp: FilePath(None),
			..Default::default()
		})
		.is_err());

		assert!(LogBuilder::build(LogConfig {
			level: Level(false),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			level: ShowBt(true),
			..Default::default()
		})
		.is_err());

		assert!(LogBuilder::build(LogConfig {
			line_num: LineNum(false),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			line_num: FileHeader("file_header_value".to_string()),
			..Default::default()
		})
		.is_err());

		assert!(LogBuilder::build(LogConfig {
			show_millis: ShowMillis(true),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			show_millis: DeleteRotation(false),
			..Default::default()
		})
		.is_err());

		assert!(LogBuilder::build(LogConfig {
			auto_rotate: AutoRotate(true),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			auto_rotate: Stdout(false),
			..Default::default()
		})
		.is_err());

		let mut path_buf = PathBuf::new();
		path_buf.push("./anything");
		assert!(LogBuilder::build(LogConfig {
			file_path: FilePath(Some(path_buf)),
			..Default::default()
		})
		.is_ok());

		// error because it's a root path
		let path_buf = PathBuf::new();
		assert!(LogBuilder::build(LogConfig {
			file_path: FilePath(Some(path_buf)),
			..Default::default()
		})
		.is_err());

		// error because it's a directory
		let mut path_buf = PathBuf::new();
		path_buf.push(".");
		assert!(LogBuilder::build(LogConfig {
			file_path: FilePath(Some(path_buf)),
			..Default::default()
		})
		.is_err());

		// none is ok (stdout only)
		assert!(LogBuilder::build(LogConfig {
			file_path: FilePath(None),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			file_path: Stdout(false),
			..Default::default()
		})
		.is_err());

		assert!(LogBuilder::build(LogConfig {
			show_bt: ShowBt(true),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			show_bt: Stdout(false),
			..Default::default()
		})
		.is_err());

		assert!(LogBuilder::build(LogConfig {
			line_num_data_max_len: LineNumDataMaxLen(30),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			line_num_data_max_len: FileHeader("".to_string()),
			..Default::default()
		})
		.is_err());

		assert!(LogBuilder::build(LogConfig {
			delete_rotation: DeleteRotation(false),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			delete_rotation: Colors(false),
			..Default::default()
		})
		.is_err());

		assert!(LogBuilder::build(LogConfig {
			file_header: FileHeader("some_value".to_string()),
			..Default::default()
		})
		.is_ok());

		assert!(LogBuilder::build(LogConfig {
			file_header: AutoRotate(false),
			..Default::default()
		})
		.is_err());

		Ok(())
	}

	#[test]
	fn test_config_options() -> Result<(), Error> {
		let mut log = LogBuilder::build(LogConfig::default())?;

		assert_eq!(
			log.get_config_option(LogConfigOptionName::Colors)?,
			&Colors(true)
		);
		log.set_config_option(Colors(false))?;
		assert_eq!(
			log.get_config_option(LogConfigOptionName::Colors)?,
			&Colors(false)
		);

		assert_eq!(
			log.get_config_option(LogConfigOptionName::Stdout)?,
			&Stdout(true)
		);
		log.set_config_option(Stdout(false))?;
		assert_eq!(
			log.get_config_option(LogConfigOptionName::Stdout)?,
			&Stdout(false)
		);

		assert_eq!(
			log.get_config_option(LogConfigOptionName::MaxAgeMillis)?,
			&MaxAgeMillis(60 * 60 * 1000)
		);
		log.set_config_option(MaxAgeMillis(30 * 60 * 1000))?;

		assert_eq!(
			log.get_config_option(LogConfigOptionName::MaxAgeMillis)?,
			&MaxAgeMillis(30 * 60 * 1000)
		);

		assert_eq!(
			log.get_config_option(LogConfigOptionName::Timestamp)?,
			&Timestamp(true)
		);
		log.set_config_option(Timestamp(false))?;
		assert_eq!(
			log.get_config_option(LogConfigOptionName::Timestamp)?,
			&Timestamp(false)
		);

		assert_eq!(
			log.get_config_option(LogConfigOptionName::Level)?,
			&Level(true)
		);
		log.set_config_option(Level(false))?;
		assert_eq!(
			log.get_config_option(LogConfigOptionName::Level)?,
			&Level(false)
		);

		assert_eq!(
			log.get_config_option(LogConfigOptionName::LineNum)?,
			&LineNum(true)
		);
		log.set_config_option(LineNum(false))?;
		assert_eq!(
			log.get_config_option(LogConfigOptionName::LineNum)?,
			&LineNum(false)
		);

		assert_eq!(
			log.get_config_option(LogConfigOptionName::ShowMillis)?,
			&ShowMillis(true)
		);
		log.set_config_option(ShowMillis(false))?;
		assert_eq!(
			log.get_config_option(LogConfigOptionName::ShowMillis)?,
			&ShowMillis(false)
		);

		assert_eq!(
			log.get_config_option(LogConfigOptionName::AutoRotate)?,
			&AutoRotate(true)
		);
		log.set_config_option(AutoRotate(false))?;
		assert_eq!(
			log.get_config_option(LogConfigOptionName::AutoRotate)?,
			&AutoRotate(false)
		);

		assert_eq!(
			log.get_config_option(LogConfigOptionName::FilePath)?,
			&FilePath(None)
		);
		let mut path = PathBuf::new();
		path.push("./anything");
		log.set_config_option(FilePath(Some(path.clone())))?;
		assert_eq!(
			log.get_config_option(LogConfigOptionName::FilePath)?,
			&FilePath(Some(path))
		);

		assert_eq!(
			log.get_config_option(LogConfigOptionName::ShowBt)?,
			&ShowBt(true)
		);
		log.set_config_option(ShowBt(false))?;
		assert_eq!(
			log.get_config_option(LogConfigOptionName::ShowBt)?,
			&ShowBt(false)
		);

		assert_eq!(
			log.get_config_option(LogConfigOptionName::LineNumDataMaxLen)?,
			&LineNumDataMaxLen(25)
		);
		log.set_config_option(LineNumDataMaxLen(35))?;
		assert_eq!(
			log.get_config_option(LogConfigOptionName::LineNumDataMaxLen)?,
			&LineNumDataMaxLen(35)
		);

		assert_eq!(
			log.get_config_option(LogConfigOptionName::DeleteRotation)?,
			&DeleteRotation(false)
		);
		log.set_config_option(DeleteRotation(true))?;
		assert_eq!(
			log.get_config_option(LogConfigOptionName::DeleteRotation)?,
			&DeleteRotation(true)
		);

		assert_eq!(
			log.get_config_option(LogConfigOptionName::FileHeader)?,
			&FileHeader("".to_string())
		);
		log.set_config_option(FileHeader("anything".to_string()))?;
		assert_eq!(
			log.get_config_option(LogConfigOptionName::FileHeader)?,
			&FileHeader("anything".to_string())
		);

		Ok(())
	}

	#[test]
	fn test_init() -> Result<(), Error> {
		let test_dir = ".test_init.bmw";
		setup_test_dir(test_dir)?;

		let config = LogConfig {
			file_path: FilePath(Some(PathBuf::from(format!("{}/test.log", test_dir)))),
			..Default::default()
		};
		let mut log = LogBuilder::build(config)?;
		log.init()?;
		log.log(LogLevel::Info, "test", None)?;
		log.log(LogLevel::Debug, "test2", None)?;

		tear_down_test_dir(test_dir)?;
		Ok(())
	}
}
