// Copyright (c) 2022, 37 Miners, LLC
// Some code and concepts from:
// * Grin: https://github.com/mimblewimble/grin
// * Arti: https://gitlab.torproject.org/tpo/core/arti
// * BitcoinMW: https://github.com/bitcoinmw/bitcoinmw
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

use crate::types::{HttpHeaders, HttpServerImpl};
use crate::{HttpConfig, HttpServer};
use bmw_err::*;

impl Default for HttpConfig {
	fn default() -> Self {
		Self { threads: 6 }
	}
}

impl HttpServerImpl {
	pub(crate) fn new(_config: HttpConfig) -> Result<HttpServerImpl, Error> {
		let _headers = HttpHeaders {};
		Ok(Self {})
	}
}

impl HttpServer for HttpServerImpl {
	fn start(&mut self) -> Result<(), Error> {
		Ok(())
	}

	fn stop(&mut self) -> Result<(), Error> {
		Ok(())
	}
}

#[cfg(test)]
mod test {
	use crate::types::HttpServerImpl;
	use crate::{HttpConfig, HttpServer};
	use bmw_err::*;

	#[test]
	fn test_http_server_basic() -> Result<(), Error> {
		let config = HttpConfig::default();
		let mut http = HttpServerImpl::new(config)?;
		http.start()?;
		Ok(())
	}
}
