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

use bmw_err::*;
use bmw_evh::EventHandlerConfig;

pub trait HttpServer {
	fn start(&mut self) -> Result<(), Error>;
	fn stop(&mut self) -> Result<(), Error>;
}

pub struct PlainConfig {
	pub domainnames: Vec<String>,
}

pub struct TlsConfig {
	pub cert_file: String,
	pub privkey_file: String,
	pub domainnames: Vec<String>,
}

pub enum HttpInstanceType {
	Plain(PlainConfig),
	Tls(TlsConfig),
}

pub struct HttpInstance {
	pub port: u16,
	pub http_dir: String,
	pub instance_type: HttpInstanceType,
}

pub struct HttpConfig {
	pub evh_config: EventHandlerConfig,
	pub instances: Vec<HttpInstance>,
}

pub struct HttpHeaders {}

pub struct Builder {}

// Crate local types
pub(crate) struct HttpServerImpl {
	pub(crate) config: HttpConfig,
}
