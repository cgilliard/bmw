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

use crate::constants::*;
use bmw_deps::rustls::{ClientConnection, ServerConnection};
use bmw_deps::x25519_dalek::PublicKey;
use bmw_err::*;
use std::io::{Read, Write};
use std::net::SocketAddr;

pub struct Cell {}

#[derive(Debug, Clone)]
pub struct Cert {
	pub cert_type: u8,
	pub cert_bytes: Vec<u8>,
}

pub struct CryptState {}

pub enum ChannelDirection {
	Inbound,
	Outbound,
}

pub(crate) struct ChannelImpl {
	tls_client: Option<ClientConnection>,
	tls_server: Option<ServerConnection>,
	dest: Node,
	verified: bool,
	remote_certs: Option<Vec<Cert>>,
}

pub trait Channel {
	fn direction(&self) -> ChannelDirection;
	fn is_verified(&self) -> bool;
	fn start() -> Result<(), Error>;
	fn read_tor(&mut self, rd: &mut dyn Read) -> Result<usize, Error>;
	fn write_tor(&mut self, wr: &mut dyn Write) -> Result<usize, Error>;
	fn send_cell(&mut self, cell: Cell) -> Result<(), Error>;
	fn process_new_packets(&mut self, state: &mut CryptState) -> Result<(), Error>;
}

pub struct Node {
	pub(crate) sockaddr: SocketAddr,
	pub(crate) ed_identity: Ed25519Identity,
	pub(crate) onion_pubkey: PublicKey,
}

/// A relay's identity, as an unchecked, unvalidated Ed25519 key.
#[derive(Clone, Copy, Hash)]
pub struct Ed25519Identity {
	/// A raw unchecked Ed25519 public key.
	id: [u8; ED25519_ID_LEN],
}

/// A vector of bytes that gets cleared when it's dropped.
pub(crate) type SecretBytes = bmw_deps::zeroize::Zeroizing<Vec<u8>>;
