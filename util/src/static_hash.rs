// Copyright (c) 2022, 37 Miners, LLC
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

use crate::types::StaticHashset;
use crate::{Serializable, StaticHashtable, StaticIterator};
use bmw_err::Error;
use bmw_log::*;

debug!();

#[derive(Debug)]
pub struct StaticHashConfig {}

impl From<StaticHashsetConfig> for StaticHashConfig {
	fn from(config: StaticHashsetConfig) -> Self {
		let _ = debug!("converting {:?}", config);
		Self {}
	}
}

impl From<StaticHashtableConfig> for StaticHashConfig {
	fn from(config: StaticHashtableConfig) -> Self {
		let _ = debug!("converting {:?}", config);
		Self {}
	}
}

#[derive(Debug)]
pub struct StaticHashtableConfig {}

impl Default for StaticHashtableConfig {
	fn default() -> Self {
		Self {}
	}
}

#[derive(Debug)]
pub struct StaticHashsetConfig {}

impl Default for StaticHashsetConfig {
	fn default() -> Self {
		Self {}
	}
}

struct StaticHashImpl {
	config: StaticHashConfig,
}

impl StaticHashImpl {
	fn new(config: StaticHashConfig) -> Result<Self, Error> {
		Ok(StaticHashImpl { config })
	}
}

impl<K, V> StaticHashtable<K, V> for StaticHashImpl
where
	K: Serializable,
	V: Serializable,
{
	fn insert(&mut self, key: &K, value: &V) -> Result<(), Error> {
		debug!(
			"insert:self.config={:?},k={:?},v={:?}",
			self.config, key, value
		)?;
		todo!()
	}
	fn get(&self, key: &K) -> Result<&V, Error> {
		debug!("get:self.config={:?},k={:?}", self.config, key)?;
		todo!()
	}
	fn get_mut(&mut self, key: &K) -> Result<&mut V, Error> {
		debug!("get_mut:self.config={:?},k={:?}", self.config, key)?;
		todo!()
	}
	fn remove(&mut self, key: &K) -> Result<&V, Error> {
		debug!("remove:self.config={:?},k={:?}", self.config, key)?;
		todo!()
	}
	fn iter(&self) -> Result<Box<dyn StaticIterator<'_, (K, V)>>, Error> {
		debug!("iter:self.config={:?}", self.config)?;
		todo!()
	}
}

impl<K> StaticHashset<K> for StaticHashImpl
where
	K: Serializable,
{
	fn insert(&mut self, key: &K) -> Result<(), Error> {
		debug!("insert:self.config={:?},k={:?}", self.config, key)?;
		todo!()
	}
	fn contains(&self, key: &K) -> Result<bool, Error> {
		debug!("contains:self.config={:?},k={:?}", self.config, key)?;
		todo!()
	}
	fn remove(&mut self, key: &K) -> Result<(), Error> {
		debug!("remove:self.config={:?},k={:?}", self.config, key)?;
		todo!()
	}
	fn iter(&self) -> Result<Box<dyn StaticIterator<'_, K>>, Error> {
		debug!("iter:self.config={:?}", self.config)?;
		todo!()
	}
}

pub struct StaticHashtableBuilder {}

impl StaticHashtableBuilder {
	pub fn build<K, V>(
		config: StaticHashtableConfig,
	) -> Result<Box<dyn StaticHashtable<K, V>>, Error>
	where
		K: Serializable,
		V: Serializable,
	{
		Ok(Box::new(StaticHashImpl::new(config.into())?))
	}
}

pub struct StaticHashsetBuilder {}

impl StaticHashsetBuilder {
	pub fn build<K>(config: StaticHashsetConfig) -> Result<Box<dyn StaticHashset<K>>, Error>
	where
		K: Serializable,
	{
		Ok(Box::new(StaticHashImpl::new(config.into())?))
	}
}
