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

use bmw_deps::old_rand_core::RngCore as OldRngCore;
use bmw_deps::rand_core::RngCore;
use bmw_deps::rustls::{ClientConnection, ServerConnection};
use bmw_deps::x25519_dalek::PublicKey;
use bmw_err::*;

use std::io::{Read, Write};
use std::net::SocketAddr;

/// Extension trait for the _current_ versions of [`RngCore`]; adds a
/// compatibility-wrapper function.
pub trait RngCompatExt: RngCore {
	/// Wrapper type returned by this trait.
	type Wrapper: RngCore + OldRngCore;
	/// Return a version of this Rng that can be used with older versions
	/// of the rand_core and rand libraries, as well as the current
	/// version.
	fn rng_compat(self) -> Self::Wrapper;
}

/// A new-style Rng, wrapped for backward compatibility.
///
/// This object implements both the current (0.6.2) version of [`RngCore`],
/// as well as the version from 0.5.1 that the dalek-crypto functions expect.
///
/// To get an RngWrapper, use the [`RngCompatExt`] extension trait:
/// ```
/// use bmw_crypt::RngCompatExt;
///
/// let mut wrapped_rng = bmw_deps::rand::thread_rng().rng_compat();
/// ```
pub struct RngWrapper<T>(pub(crate) T);

pub struct TlsVerifier {}

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
	pub(crate) tls_client: Option<ClientConnection>,
	pub(crate) tls_server: Option<ServerConnection>,
	pub(crate) peer: Option<Peer>,
	pub(crate) verified: bool,
}

pub trait Channel {
	fn direction(&self) -> ChannelDirection;
	fn is_verified(&self) -> bool;
	fn read_crypt(&mut self, rd: &mut dyn Read) -> Result<usize, Error>;
	fn write_crypt(&mut self, wr: &mut dyn Write) -> Result<usize, Error>;
	fn send_cell(&mut self, cell: Cell) -> Result<(), Error>;
	fn process_new_packets(&mut self, state: &mut CryptState) -> Result<(), Error>;
}

#[derive(Debug, Clone)]
pub struct Peer {
	pub(crate) sockaddr: SocketAddr,
	pub(crate) pubkey: PublicKey,
}

/// A vector of bytes that gets cleared when it's dropped.
pub(crate) type SecretBytes = bmw_deps::zeroize::Zeroizing<Vec<u8>>;
