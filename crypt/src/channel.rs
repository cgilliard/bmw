// Copyright (c) 2022, 37 Miners, LLC
// Some code and concepts from:
// * Grin: https://github.com/mimblewimble/grin
// * Arti: https://gitlab.torproject.org/tpo/core/arti
// * BitcoinMW: https://github.com/bitcoinmw/bitcoinmw
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

use crate::types::{ChannelImpl, TlsVerifier};
use crate::Peer;
use bmw_deps::lazy_static::lazy_static;
use bmw_deps::openssl::pkey::PKey;
use bmw_deps::pem;
use bmw_deps::rcgen::{
	date_time_ymd, Certificate, CertificateParams, DistinguishedName, KeyPair, PKCS_ED25519,
};
use bmw_deps::rustls::client::ClientConfig;
use bmw_deps::rustls::server::ServerConfig;
use bmw_deps::rustls::{
	Certificate as RustlsCertificate, ClientConnection, PrivateKey as RustlsPrivateKey,
	RootCertStore, ServerConnection,
};
use bmw_err::*;
use bmw_log::*;
use std::sync::{Arc, RwLock};

debug!();

lazy_static! {
	pub(crate) static ref SERVER_CONFIG: Arc<RwLock<Option<Arc<ServerConfig>>>> =
		Arc::new(RwLock::new(None));
	pub(crate) static ref CLIENT_CONFIG: Arc<RwLock<Option<Arc<ClientConfig>>>> =
		Arc::new(RwLock::new(None));
}

pub fn reset_tls_configs() -> Result<(), Error> {
	let mut guard = SERVER_CONFIG.write()?;
	*guard = None;

	let mut guard = CLIENT_CONFIG.write()?;
	*guard = None;

	Ok(())
}

impl ChannelImpl {
	pub fn new() -> Self {
		Self {
			tls_client: None,
			tls_server: None,
			peer: None,
			verified: false,
		}
	}

	pub fn connect(&mut self, peer: Peer) -> Result<(), Error> {
		debug!("connecting to peer: {:?}", peer)?;

		// localhost is used because we don't validate hostnames.
		self.tls_client = Some(ClientConnection::new(
			self.make_client_config()?,
			"localhost".try_into()?,
		)?);
		self.peer = Some(peer.clone());
		Ok(())
	}

	pub fn accept(&mut self) -> Result<(), Error> {
		debug!("accepting a connection1")?;
		self.tls_server = Some(ServerConnection::new(self.make_server_config()?)?);
		debug!("complete")?;
		Ok(())
	}

	// make a client config that only verifies the signatures and not hostname.
	// Identity is verified after tls tunnel is created.
	fn make_client_config(&self) -> Result<Arc<ClientConfig>, Error> {
		// only need to build this once. If it exists return it.
		match &*CLIENT_CONFIG.read()? {
			Some(config) => return Ok(config.clone()),
			None => {}
		}
		let root_store = RootCertStore::empty();

		let mut config = ClientConfig::builder()
			.with_safe_default_cipher_suites()
			.with_safe_default_kx_groups()
			.with_safe_default_protocol_versions()?
			.with_root_certificates(root_store)
			.with_no_client_auth();

		config
			.dangerous()
			.set_certificate_verifier(Arc::new(TlsVerifier {}));

		let mut guard = CLIENT_CONFIG.write()?;
		let ret = Arc::new(config);
		*guard = Some(ret.clone());

		Ok(ret)
	}

	// make a server config
	fn make_server_config(&self) -> Result<Arc<ServerConfig>, Error> {
		// only need to build this once. If it exists return it.
		match &*SERVER_CONFIG.read()? {
			Some(config) => return Ok(config.clone()),
			None => {}
		}

		let mut params: CertificateParams = Default::default();
		params.not_before = date_time_ymd(2021, 05, 19);
		params.not_after = date_time_ymd(4096, 01, 01);
		params.distinguished_name = DistinguishedName::new();
		params.alg = &PKCS_ED25519;
		let pkey: PKey<_> = PKey::generate_ed25519()?.try_into()?;
		let private_key = pkey.private_key_to_pem_pkcs8()?;
		let key_pair_pem = String::from_utf8(private_key.clone())?;
		let key_pair = KeyPair::from_pem(&key_pair_pem)?;
		params.key_pair = Some(key_pair);
		let private_key_der = pkey.private_key_to_der()?;
		let cert = Certificate::from_params(params)?;
		let pem_serialized = cert.serialize_pem()?;
		let der_serialized = pem::parse(&pem_serialized)?.contents;
		let config = ServerConfig::builder()
			.with_safe_defaults()
			.with_no_client_auth()
			.with_single_cert(
				vec![RustlsCertificate(der_serialized)],
				RustlsPrivateKey(private_key_der),
			)?;

		let mut guard = SERVER_CONFIG.write()?;
		let ret = Arc::new(config);
		*guard = Some(ret.clone());

		Ok(ret)
	}
}

#[cfg(test)]
mod test {
	use super::*;
	use bmw_deps::x25519_dalek::PublicKey;
	use bmw_test::port::pick_free_port;
	use std::io::{Read, Write};

	#[test]
	fn test_crypt_channel() -> Result<(), Error> {
		let port = pick_free_port()?;
		let addr = &format!("127.0.0.1:{}", port)[..];
		let server_pubkey = PublicKey::from([0u8; 32]);
		let _client_pubkey = PublicKey::from([1u8; 32]);
		let mut server = ChannelImpl::new();
		let mut client = ChannelImpl::new();

		let peer = Peer {
			sockaddr: addr.parse()?,
			pubkey: server_pubkey,
		};

		client.connect(peer)?;
		server.accept()?;

		let mut tls_client = client.tls_client.unwrap();
		let mut tls_server = server.tls_server.unwrap();

		let msg = b"abcd1234567890";
		// write message to the client
		tls_client.writer().write(msg)?;
		let io_state = tls_client.process_new_packets()?;
		info!("io_state={:?}", io_state)?;
		let mut buf = vec![];
		tls_client.write_tls(&mut buf)?;
		info!("buf.len() = {}", buf.len())?;

		// read the bytes into the server
		tls_server.read_tls(&mut &buf[..])?;
		let io_state = tls_server.process_new_packets()?;
		info!("io_state={:?}", io_state)?;
		let mut buf = vec![];
		tls_server.write_tls(&mut buf)?;
		info!("buf.len() = {}", buf.len())?;

		// read the bytes into the client
		tls_client.read_tls(&mut &buf[..])?;
		let io_state = tls_client.process_new_packets()?;
		info!("io_state={:?}", io_state)?;
		let mut buf = vec![];
		tls_client.write_tls(&mut buf)?;
		info!("buf.len() = {}", buf.len())?;

		// write the bytes into the server
		tls_server.read_tls(&mut &buf[..])?;
		let io_state = tls_server.process_new_packets()?;
		info!("io_state={:?}", io_state)?;

		let mut buf = vec![];
		buf.resize(msg.len(), 0u8);
		tls_server.reader().read(&mut buf)?;
		assert_eq!(&buf[..], msg);

		Ok(())
	}
}
