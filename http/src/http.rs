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
use crate::{HttpConfig, HttpInstance, HttpInstanceType, HttpServer, PlainConfig};
use bmw_err::*;
use bmw_evh::{Builder, ConnectionData, EventHandler, EventHandlerConfig, ThreadContext};
use std::any::Any;

impl Default for HttpConfig {
	fn default() -> Self {
		Self {
			evh_config: EventHandlerConfig::default(),
			instances: vec![HttpInstance {
				instance_type: HttpInstanceType::Plain(PlainConfig {
					domainnames: vec![],
				}),
				port: 8080,
				http_dir: "~/.bmw/www".to_string(),
			}],
		}
	}
}

impl HttpServerImpl {
	pub(crate) fn new(config: HttpConfig) -> Result<HttpServerImpl, Error> {
		Ok(Self { config })
	}

	fn process_on_read(
		conn_data: &mut ConnectionData,
		ctx: &mut ThreadContext,
	) -> Result<(), Error> {
		Ok(())
	}

	fn process_on_accept(
		conn_data: &mut ConnectionData,
		ctx: &mut ThreadContext,
	) -> Result<(), Error> {
		Ok(())
	}

	fn process_on_close(
		conn_data: &mut ConnectionData,
		ctx: &mut ThreadContext,
	) -> Result<(), Error> {
		Ok(())
	}

	fn process_on_panic(ctx: &mut ThreadContext, e: Box<dyn Any + Send>) -> Result<(), Error> {
		Ok(())
	}

	fn process_housekeeper(ctx: &mut ThreadContext) -> Result<(), Error> {
		Ok(())
	}
}

impl HttpServer for HttpServerImpl {
	fn start(&mut self) -> Result<(), Error> {
		let mut evh = Builder::build_evh(self.config.evh_config.clone())?;

		evh.set_on_read(move |conn_data, ctx| Self::process_on_read(conn_data, ctx))?;
		evh.set_on_accept(move |conn_data, ctx| Self::process_on_accept(conn_data, ctx))?;
		evh.set_on_close(move |conn_data, ctx| Self::process_on_close(conn_data, ctx))?;
		evh.set_on_panic(move |ctx, e| Self::process_on_panic(ctx, e))?;
		evh.set_housekeeper(move |ctx| Self::process_housekeeper(ctx))?;

		evh.start()?;

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
