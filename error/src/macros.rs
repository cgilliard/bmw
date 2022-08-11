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

/// Build the specified [`crate::ErrorKind`] and convert it into an [`crate::Error`]. The desired
/// [`crate::ErrorKind`] is specified using the [`crate::ErrKind`] name enum.
///
/// Example:
///
///```
/// use bmw_err::{Error, ErrorKind, ErrKind, err};
///
/// fn show_err_kind(do_error: bool) -> Result<(), Error> {
///     let e = err!(ErrKind::Configuration, "invalid parameter name");
///
///     if do_error {
///         return Err(e);
///     }
///
///     Ok(())
/// }
///```
#[macro_export]
macro_rules! err {
	($kind:expr, $msg:expr) => {{
		match $kind {
			bmw_err::ErrKind::Configuration => {
				let error: bmw_err::Error =
					bmw_err::ErrorKind::Configuration($msg.to_string()).into();
				error
			}
			bmw_err::ErrKind::IO => {
				let error: bmw_err::Error = bmw_err::ErrorKind::IO($msg.to_string()).into();
				error
			}
			bmw_err::ErrKind::Log => {
				let error: bmw_err::Error = bmw_err::ErrorKind::Log($msg.to_string()).into();
				error
			}
			bmw_err::ErrKind::Utf8 => {
				let error: bmw_err::Error = bmw_err::ErrorKind::Utf8($msg.to_string()).into();
				error
			}
			bmw_err::ErrKind::ArrayIndexOutOfBounds => {
				let error: bmw_err::Error =
					bmw_err::ErrorKind::ArrayIndexOutOfBounds($msg.to_string()).into();
				error
			}
			bmw_err::ErrKind::Poison => {
				let error: bmw_err::Error = bmw_err::ErrorKind::Poison($msg.to_string()).into();
				error
			}
			bmw_err::ErrKind::CorruptedData => {
				let error: bmw_err::Error =
					bmw_err::ErrorKind::CorruptedData($msg.to_string()).into();
				error
			}
			bmw_err::ErrKind::Timeout => {
				let error: bmw_err::Error = bmw_err::ErrorKind::Timeout($msg.to_string()).into();
				error
			}
			bmw_err::ErrKind::CapacityExceeded => {
				let error: bmw_err::Error =
					bmw_err::ErrorKind::CapacityExceeded($msg.to_string()).into();
				error
			}
			bmw_err::ErrKind::UnexpectedEof => {
				let error: bmw_err::Error =
					bmw_err::ErrorKind::UnexpectedEof($msg.to_string()).into();
				error
			}
			bmw_err::ErrKind::IllegalArgument => {
				let error: bmw_err::Error =
					bmw_err::ErrorKind::IllegalArgument($msg.to_string()).into();
				error
			}
			bmw_err::ErrKind::Misc => {
				let error: bmw_err::Error = bmw_err::ErrorKind::Misc($msg.to_string()).into();
				error
			}
			bmw_err::ErrKind::IllegalState => {
				let error: bmw_err::Error =
					bmw_err::ErrorKind::IllegalState($msg.to_string()).into();
				error
			}
		}
	}};
}

/// Map the specified error into the [`crate::ErrKind`] enum name from this crate.
/// Optionally specify an additional message to be included in the error.
///
/// Example:
///
///```
/// use bmw_err::{Error, ErrorKind, ErrKind, map_err};
/// use std::fs::File;
/// use std::io::Write;
///
/// fn show_map_err(do_error: bool) -> Result<(), Error> {
///     let file = map_err!(File::open("/path/to/something"), ErrKind::IO, "file open failed")?;
///     println!("file_type={:?}", file.metadata()?.file_type());
///
///     let mut x = map_err!(File::open("/invalid/log/path.log"), ErrKind::Log)?;
///     x.write(b"test")?;
///
///     Ok(())
/// }
///```
#[macro_export]
macro_rules! map_err {
	($in_err:expr, $kind:expr) => {{
		map_err!($in_err, $kind, "")
	}};
	($in_err:expr, $kind:expr, $msg:expr) => {{
		$in_err.map_err(|e| {
			let error: bmw_err::Error = match $kind {
				bmw_err::ErrKind::Configuration => {
					bmw_err::ErrorKind::Configuration(format!("{}: {}", $msg, e)).into()
				}
				bmw_err::ErrKind::IO => bmw_err::ErrorKind::IO(format!("{}: {}", $msg, e)).into(),
				bmw_err::ErrKind::Log => bmw_err::ErrorKind::Log(format!("{}: {}", $msg, e)).into(),
				bmw_err::ErrKind::UnexpectedEof => {
					bmw_err::ErrorKind::UnexpectedEof(format!("{}: {}", $msg, e)).into()
				}
				bmw_err::ErrKind::Utf8 => {
					bmw_err::ErrorKind::Utf8(format!("{}: {}", $msg, e)).into()
				}
				bmw_err::ErrKind::ArrayIndexOutOfBounds => {
					bmw_err::ErrorKind::ArrayIndexOutOfBounds(format!("{}: {}", $msg, e)).into()
				}
				bmw_err::ErrKind::Timeout => {
					bmw_err::ErrorKind::Timeout(format!("{}: {}", $msg, e)).into()
				}
				bmw_err::ErrKind::CapacityExceeded => {
					bmw_err::ErrorKind::CapacityExceeded(format!("{}: {}", $msg, e)).into()
				}
				bmw_err::ErrKind::IllegalArgument => {
					bmw_err::ErrorKind::IllegalArgument(format!("{}: {}", $msg, e)).into()
				}
				bmw_err::ErrKind::Poison => {
					bmw_err::ErrorKind::Poison(format!("{}: {}", $msg, e)).into()
				}
				bmw_err::ErrKind::Misc => {
					bmw_err::ErrorKind::Misc(format!("{}: {}", $msg, e)).into()
				}
				bmw_err::ErrKind::CorruptedData => {
					bmw_err::ErrorKind::CorruptedData(format!("{}: {}", $msg, e)).into()
				}
				bmw_err::ErrKind::IllegalState => {
					bmw_err::ErrorKind::IllegalState(format!("{}: {}", $msg, e)).into()
				}
			};
			error
		})
	}};
}

#[cfg(test)]
mod test {
	use crate as bmw_err;
	use std::fs::File;

	#[test]
	fn test_ekinds() -> Result<(), crate::Error> {
		let err: bmw_err::Error = err!(bmw_err::ErrKind::Configuration, "anything");
		let _err_kind = err.kind();
		let raw: bmw_err::Error =
			bmw_err::ErrorKind::Configuration("configuration error".to_string()).into();
		assert!(matches!(raw.kind(), _err_kind));
		Ok(())
	}

	#[test]
	fn test_map_err() -> Result<(), crate::Error> {
		let res = map_err!(
			File::open("/path/to/nothing"),
			bmw_err::ErrKind::Configuration,
			"caused by"
		);
		assert!(matches!(
			res.as_ref().unwrap_err().kind(),
			crate::ErrorKind::Configuration(_),
		));

		let res = map_err!(
			File::open("/path/to/nothing"),
			bmw_err::ErrKind::Log,
			"another msg"
		);

		assert!(matches!(
			res.as_ref().unwrap_err().kind(),
			crate::ErrorKind::Log(_),
		));

		let res = map_err!(File::open("/path/to/nothing"), bmw_err::ErrKind::IO);
		assert!(matches!(
			res.as_ref().unwrap_err().kind(),
			crate::ErrorKind::IO(_),
		));

		Ok(())
	}
}
