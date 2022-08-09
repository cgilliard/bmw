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

use crate::Log;
use bmw_deps::lazy_static::lazy_static;
use std::sync::{Arc, RwLock};

lazy_static! {
	#[doc(hidden)]
	pub static ref STATIC_LOG: Arc<RwLock<Option<Box<dyn Log + Send + Sync>>>> = Arc::new(RwLock::new(None));
}

#[macro_export]
macro_rules! fatal {
	() => {
		bmw_log::log!(bmw_log::types::LogLevel::Fatal);
	};
	($a:expr) => {
		{
                        bmw_log::log!(bmw_log::types::LogLevel::Fatal, $a)
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        bmw_log::log!(bmw_log::types::LogLevel::Fatal, $a, $($b)*)
		}
	};
}

#[macro_export]
macro_rules! fatal_plain {
	($a:expr) => {
		{
			bmw_log::log_plain!(bmw_log::types::LogLevel::Fatal, $a)
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
			bmw_log::log_plain!(bmw_log::types::LogLevel::Fatal, $a, $($b)*)
		}
	};
}

#[macro_export]
macro_rules! fatal_all {
	($a:expr) => {
		{
			bmw_log::log_all!(bmw_log::types::LogLevel::Fatal, $a)
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        bmw_log::log_all!(bmw_log::types::LogLevel::Fatal, $a, $($b)*)
		}
	};
}

#[macro_export]
macro_rules! error {
	() => {
		bmw_log::log!(bmw_log::types::LogLevel::Error);
	};
	($a:expr) => {
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal {
				bmw_log::log!(bmw_log::types::LogLevel::Error, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal {
				bmw_log::log!(bmw_log::types::LogLevel::Error, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! error_plain {
	($a:expr) => {
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal {
				bmw_log::log_plain!(bmw_log::types::LogLevel::Error, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal {
				bmw_log::log_plain!(bmw_log::types::LogLevel::Error, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! error_all {
	($a:expr) => {
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal {
				bmw_log::log_all!(bmw_log::types::LogLevel::Error, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal {
				bmw_log::log_all!(bmw_log::types::LogLevel::Error, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! warn {
	() => {
		bmw_log::log!(bmw_log::types::LogLevel::Warn);
	};
	($a:expr) => {
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error {
				bmw_log::log!(bmw_log::types::LogLevel::Warn, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error {
				bmw_log::log!(bmw_log::types::LogLevel::Warn, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! warn_plain {
	($a:expr) => {
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error {
				bmw_log::log_plain!(bmw_log::types::LogLevel::Warn, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error {
				bmw_log::log_plain!(bmw_log::types::LogLevel::Warn, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! warn_all {
	($a:expr) => {
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error {
				bmw_log::log_all!(bmw_log::types::LogLevel::Warn, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error {
				bmw_log::log_all!(bmw_log::types::LogLevel::Warn, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! info {
	() => {
		bmw_log::log!(bmw_log::types::LogLevel::Info);
	};
	($a:expr) => {
		{

                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn {
				bmw_log::log!(bmw_log::types::LogLevel::Info, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn {
				bmw_log::log!(bmw_log::types::LogLevel::Info, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! info_plain {
	($a:expr) => {
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn {
				bmw_log::log_plain!(bmw_log::types::LogLevel::Info, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn {
				bmw_log::log_plain!(bmw_log::types::LogLevel::Info, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! info_all {
	($a:expr) => {
                {
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn {
				bmw_log::log_all!(bmw_log::types::LogLevel::Info, $a)
                        } else {
                                Ok(())
                        }
                }
        };
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn {
				bmw_log::log_all!(bmw_log::types::LogLevel::Info, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! debug {
	() => {
		bmw_log::log!(bmw_log::types::LogLevel::Debug);
	};
	($a:expr) => {
		{
                    if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn && LOG_LEVEL != bmw_log::types::LogLevel::Info {
				bmw_log::log!(bmw_log::types::LogLevel::Debug, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                    if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn && LOG_LEVEL != bmw_log::types::LogLevel::Info {
				bmw_log::log_multi!(bmw_log::types::LogLevel::Debug, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! debug_plain {
	($a:expr) => {
		{
                    if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn && LOG_LEVEL != bmw_log::types::LogLevel::Info {
				bmw_log::log_plain!(bmw_log::types::LogLevel::Debug, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn && LOG_LEVEL != bmw_log::types::LogLevel::Info {
				bmw_log::log_plain!(bmw_log::types::LogLevel::Debug, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! debug_all {
	($a:expr) => {
		{
                    if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn && LOG_LEVEL != bmw_log::types::LogLevel::Info {
				bmw_log::log_all!(bmw_log::types::LogLevel::Debug, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn && LOG_LEVEL != bmw_log::types::LogLevel::Info {
                                bmw_log::log_all!(bmw_log::types::LogLevel::Debug, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! trace {
	() => {
		    bmw_log::log!(bmw_log::types::LogLevel::Trace);
	};
	($a:expr) => {
		{
			if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn && LOG_LEVEL != bmw_log::types::LogLevel::Info && LOG_LEVEL != bmw_log::types::LogLevel::Debug {
                                bmw_log::log!(bmw_log::types::LogLevel::Trace, $a)
			} else {
				Ok(())
			}
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn && LOG_LEVEL != bmw_log::types::LogLevel::Info && LOG_LEVEL != bmw_log::types::LogLevel::Debug {
			        bmw_log::log!(bmw_log::types::LogLevel::Trace, $a, $($b)*)
			} else {
		                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! trace_plain {
	($a:expr) => {
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn && LOG_LEVEL != bmw_log::types::LogLevel::Info && LOG_LEVEL != bmw_log::types::LogLevel::Debug {
				bmw_log::log_plain!(bmw_log::types::LogLevel::Trace, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn && LOG_LEVEL != bmw_log::types::LogLevel::Info && LOG_LEVEL != bmw_log::types::LogLevel::Debug {
				bmw_log::log_plain!(bmw_log::types::LogLevel::Trace, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! trace_all {
	($a:expr) => {
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn && LOG_LEVEL != bmw_log::types::LogLevel::Info && LOG_LEVEL != bmw_log::types::LogLevel::Debug {
				bmw_log::log_all!(bmw_log::types::LogLevel::Trace, $a)
                        } else {
                                Ok(())
                        }
		}
	};
	($a:expr,$($b:tt)*)=>{
		{
                        if LOG_LEVEL != bmw_log::types::LogLevel::Fatal && LOG_LEVEL != bmw_log::types::LogLevel::Error && LOG_LEVEL != bmw_log::types::LogLevel::Warn && LOG_LEVEL != bmw_log::types::LogLevel::Info && LOG_LEVEL != bmw_log::types::LogLevel::Debug {
				bmw_log::log_all!(bmw_log::types::LogLevel::Trace, $a, $($b)*)
                        } else {
                                Ok(())
                        }
		}
	};
}

#[macro_export]
macro_rules! log {
        ($level:expr) => {
                const LOG_LEVEL: bmw_log::LogLevel = $level;
        };
        ($level:expr, $a:expr) => {{
                bmw_log::do_log!(0, $level, $a)
        }};
        ($level:expr, $a:expr, $($b:tt)*) => {
                bmw_log::do_log!(0, $level, $a, $($b)*)
        }
}

#[macro_export]
macro_rules! log_plain {
        ($level:expr) => {
                const LOG_LEVEL: bmw_log::LogLevel = $level;
        };
        ($level:expr, $a:expr) => {{
                bmw_log::do_log!(1, $level, $a)
        }};
        ($level:expr, $a:expr, $($b:tt)*) => {
                bmw_log::do_log!(1, $level, $a, $($b)*)
        }
}

#[macro_export]
macro_rules! log_all {
        ($level:expr) => {
                const LOG_LEVEL: bmw_log::LogLevel = $level;
        };
        ($level:expr, $a:expr) => {{
                bmw_log::do_log!(2, $level, $a)
        }};
        ($level:expr, $a:expr, $($b:tt)*) => {
                bmw_log::do_log!(2, $level, $a, $($b)*)
        }
}

#[macro_export]
macro_rules! do_log {
	($flavor:expr, $level:expr, $a:expr) => {{
		let mut static_log = bmw_log::lockw!(bmw_log::STATIC_LOG)?;
		let (ret, log) = match (*static_log).as_mut() {
			Some(log) => {
                            if $flavor == 0 {
                                (log.log($level, $a, None), None)
                            } else if $flavor == 1 {
                                (log.log_plain($level, $a, None), None)
                            } else {
                                (log.log_all($level, $a, None), None)
                            }
                        },
			None => {
				let mut log = bmw_log::LogBuilder::build(bmw_log::LogConfig::default())?;
				log.init()?;
                                if $flavor == 0 {
				    (log.log($level, $a, None), Some(log))
                                } else if $flavor == 1 {
                                    (log.log_plain($level, $a, None), None)
                                } else {
                                    (log.log_all($level, $a, None), None)
                                }
			}
		};

		if log.is_some() {
			(*static_log) = log;
		}

		ret
	}};
	($flavor:expr, $level:expr, $a:expr, $($b:tt)*) => {
		do_log!($flavor, $level, &format!($a, $($b)*)[..])
	};
}

#[macro_export]
macro_rules! get_log_option {
	($option:expr) => {{
		let static_log = bmw_log::lockr!(bmw_log::STATIC_LOG)?;
		match (*static_log).as_ref() {
			Some(log) => Ok(log.get_config_option($option)?.clone()),
			None => Err(bmw_err::errkind!(
				bmw_err::ErrKind::Log,
				"log not initialized"
			)),
		}
	}};
}

#[macro_export]
macro_rules! set_log_option {
	($option:expr) => {{
		let mut static_log = bmw_log::lockw!(bmw_log::STATIC_LOG)?;
		match (*static_log).as_mut() {
			Some(log) => log.set_config_option($option),
			None => Err(bmw_err::errkind!(
				bmw_err::ErrKind::Log,
				"log not initialized"
			)),
		}
	}};
}

#[macro_export]
macro_rules! log_init {
	() => {{
		let config = bmw_log::LogConfig::default();
		log_init!(config)
	}};
	($config:expr) => {{
		let mut static_log = bmw_log::lockw!(bmw_log::STATIC_LOG)?;
		if (*static_log).is_some() {
			return Err(bmw_err::errkind!(
				bmw_err::ErrKind::Log,
				"already initialized"
			));
		}

		let mut log = bmw_log::LogBuilder::build($config)?;
		log.init()?;
		*static_log = Some(log);
	}};
}

#[macro_export]
macro_rules! log_rotate {
	() => {{
		let mut static_log = bmw_log::lockw!(bmw_log::STATIC_LOG)?;
		match (*static_log).as_mut() {
			Some(log) => log.rotate(),
			None => Err(bmw_err::errkind!(
				bmw_err::ErrKind::Log,
				"log not initialized"
			)),
		}
	}};
}

#[macro_export]
macro_rules! need_rotate {
	() => {{
		let static_log = bmw_log::lockr!(bmw_log::STATIC_LOG)?;
		match (*static_log).as_ref() {
			Some(log) => log.need_rotate(None),
			None => Err(bmw_err::errkind!(
				bmw_err::ErrKind::Log,
				"log not initialized"
			)),
		}
	}};
}

/// A macro that is used to lock a rwlock in write mode and return the appropriate error if the lock is poisoned.
#[macro_export]
macro_rules! lockw {
	($a:expr) => {{
		let res = $a.write().map_err(|e| {
			let error: bmw_err::Error =
				bmw_err::ErrorKind::Poison(format!("Poison Error: {}", e.to_string())).into();
			error
		});

		res
	}};
}

/// A macro that is used to lock a rwlock in read mode and return the appropriate error if the lock is poisoned.
#[macro_export]
macro_rules! lockr {
	($a:expr) => {{
		let res = $a.read().map_err(|e| {
			let error: bmw_err::Error =
				bmw_err::ErrorKind::Poison(format!("Poison Error: {}", e.to_string())).into();
			error
		});

		res
	}};
}

#[cfg(test)]
mod test {
	use crate as bmw_log;
	use crate::{LogConfigOption, LogConfigOptionName};
	use bmw_err::Error;
	use bmw_log::LogConfig;
	use bmw_test::testdir::{setup_test_dir, tear_down_test_dir};
	use std::fs::read_to_string;
	use std::path::PathBuf;
	use std::sync::{Arc, RwLock};
	use std::thread::spawn;

	trace!();

	#[test]
	fn test_lock() -> Result<(), Error> {
		let v: u32 = 0;
		let x = Arc::new(RwLock::new(v));
		let x_clone = x.clone();

		let jh = spawn(move || {
			let p: Option<u32> = None;
			let _x = lockw!(x_clone).unwrap();
			p.unwrap(); // cause thread panic and poison this lock
		});

		// this will be an error but we ignore for the purposes of this test
		let _ = jh.join();

		// try to get the lock here and it's a poison error
		let res = lockw!(x);
		assert!(res.is_err());
		let res = lockr!(x);
		assert!(res.is_err());

		Ok(())
	}

	#[test]
	fn test_log_macros() -> Result<(), Error> {
		let test_dir = ".test_macros.bmw";
		setup_test_dir(test_dir)?;

		let log_file = format!("{}/test.log", test_dir);
		log_init!(LogConfig {
			file_path: LogConfigOption::FilePath(Some(PathBuf::from(log_file.clone()))),
			max_size_bytes: LogConfigOption::MaxSizeBytes(100),
			auto_rotate: LogConfigOption::AutoRotate(false),
			show_bt: LogConfigOption::ShowBt(false),
			..Default::default()
		});

		trace!("1")?;
		debug!("2")?;
		info!("3")?;
		warn!("4")?;
		error!("5")?;
		fatal!("6")?;
		let contents = read_to_string(log_file.clone())?;
		assert_eq!(contents.len(), 406);

		set_log_option!(LogConfigOption::ShowMillis(false))?;
		let show_millis = get_log_option!(LogConfigOptionName::ShowMillis)?;
		info!("show_millis={:?}", show_millis)?;
		assert_eq!(show_millis, LogConfigOption::ShowMillis(false));

		trace!("1")?;
		debug!("2")?;
		info!("3")?;
		warn!("4")?;
		error!("5")?;
		fatal!("6")?;

		trace_plain!("1")?;
		debug_plain!("2")?;
		info_plain!("3")?;
		warn_plain!("4")?;
		error_plain!("5")?;
		fatal_plain!("6")?;

		let need_rotate = need_rotate!()?;
		info!("need_rotate={}", need_rotate)?;
		assert_eq!(need_rotate, true);
		log_rotate!()?;
		let need_rotate = need_rotate!()?;
		assert_eq!(need_rotate, false);

		tear_down_test_dir(test_dir)?;
		Ok(())
	}
}
