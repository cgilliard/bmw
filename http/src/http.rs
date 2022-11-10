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

use crate::types::HttpServerImpl;
use crate::{HttpConfig, HttpHeaders, HttpInstance, HttpInstanceType, HttpServer, PlainConfig};
use bmw_err::*;
use bmw_evh::{
	create_listeners, Builder, ConnData, ConnectionData, EventHandler, EventHandlerConfig,
	ServerConnection, ThreadContext, READ_SLAB_DATA_SIZE,
};
use bmw_log::*;
use std::any::Any;

info!();

impl Default for HttpConfig {
	fn default() -> Self {
		Self {
			evh_config: EventHandlerConfig::default(),
			instances: vec![HttpInstance {
				instance_type: HttpInstanceType::Plain(PlainConfig {
					domainnames: vec![],
				}),
				port: 8080,
				addr: "127.0.0.1".to_string(),
				listen_queue_size: 100,
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
		_ctx: &mut ThreadContext,
	) -> Result<(), Error> {
		let _h = HttpHeaders {};
		debug!("on read slab_offset = {}", conn_data.slab_offset())?;
		let first_slab = conn_data.first_slab();
		let last_slab = conn_data.last_slab();
		let slab_offset = conn_data.slab_offset();
		debug!("firstslab={},last_slab={}", first_slab, last_slab)?;
		let res = conn_data.borrow_slab_allocator(move |sa| {
			let mut slab_id = first_slab;
			let mut ret: Vec<u8> = vec![];
			loop {
				let slab = sa.get(slab_id.try_into()?)?;
				let slab_bytes = slab.get();
				let offset = if slab_id == last_slab {
					slab_offset as usize
				} else {
					READ_SLAB_DATA_SIZE
				};
				debug!("read bytes = {:?}", &slab.get()[0..offset as usize])?;
				ret.extend(&slab_bytes[0..offset]);

				if slab_id == last_slab {
					break;
				}
				slab_id = u32::from_be_bytes(try_into!(
					slab_bytes[READ_SLAB_DATA_SIZE..READ_SLAB_DATA_SIZE + 4]
				)?);
			}
			Ok(ret)
		})?;
		conn_data.clear_through(last_slab)?;
		let res_str = std::str::from_utf8(&res)?;
		info!("res={}", res_str)?;
		let res = b"HTTP/1.1 200 OK\r\n\
Date: Thu, 10 Nov 2022 22:31:52 GMT\r\n\
Content-Length: 11\r\n\r\nTest page\r\n";
		conn_data.write_handle().write(&res[..])?;
		Ok(())
	}

	fn process_on_accept(
		_conn_data: &mut ConnectionData,
		_ctx: &mut ThreadContext,
	) -> Result<(), Error> {
		Ok(())
	}

	fn process_on_close(
		_conn_data: &mut ConnectionData,
		_ctx: &mut ThreadContext,
	) -> Result<(), Error> {
		Ok(())
	}

	fn process_on_panic(_ctx: &mut ThreadContext, _e: Box<dyn Any + Send>) -> Result<(), Error> {
		Ok(())
	}

	fn process_housekeeper(_ctx: &mut ThreadContext) -> Result<(), Error> {
		Ok(())
	}
}

impl HttpServer for HttpServerImpl {
	fn start(&mut self) -> Result<(), Error> {
		if self.config.instances.len() == 0 {
			return Err(err!(
				ErrKind::IllegalArgument,
				"At least one instance must be specified"
			));
		}
		let mut evh = Builder::build_evh(self.config.evh_config.clone())?;

		evh.set_on_read(move |conn_data, ctx| Self::process_on_read(conn_data, ctx))?;
		evh.set_on_accept(move |conn_data, ctx| Self::process_on_accept(conn_data, ctx))?;
		evh.set_on_close(move |conn_data, ctx| Self::process_on_close(conn_data, ctx))?;
		evh.set_on_panic(move |ctx, e| Self::process_on_panic(ctx, e))?;
		evh.set_housekeeper(move |ctx| Self::process_housekeeper(ctx))?;

		evh.start()?;

		for instance in &self.config.instances {
			let port = instance.port;
			let addr = &instance.addr;

			debug!("creating listener for {}", port)?;
			let addr = format!("{}:{}", addr, port);
			let handles = create_listeners(self.config.evh_config.threads, &addr, 10, false)?;

			let sc = ServerConnection {
				tls_config: vec![],
				handles,
				is_reuse_port: false,
			};

			evh.add_server(sc)?;
		}

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
	use bmw_log::*;
	use std::io::Read;
	use std::io::Write;
	use std::net::TcpStream;
	use std::str::from_utf8;

	debug!();

	#[test]
	fn test_http_server_basic() -> Result<(), Error> {
		let config = HttpConfig {
			..Default::default()
		};
		let mut http = HttpServerImpl::new(config)?;
		http.start()?;
		std::thread::sleep(std::time::Duration::from_millis(1_000));
		let port = 8080;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let mut client = TcpStream::connect(addr)?;
		std::thread::sleep(std::time::Duration::from_millis(1_000));

		client.write(b"test")?;
		let mut buf = [0; 128];
		let len = client.read(&mut buf)?;
		let data = from_utf8(&buf)?;
		info!("len={}", len)?;
		info!("data='{}'", data)?;
		assert_eq!(len, 87);

		std::thread::sleep(std::time::Duration::from_millis(1_000));

		Ok(())
	}
}
