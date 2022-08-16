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

use crate::slabs::Slab;
use crate::{Serializable, StaticList};
use bmw_err::*;
use bmw_log::*;

info!();

struct StaticListImpl<'a> {
	x: &'a Vec<u8>,
}

impl<'a, V> StaticList<'a, V> for StaticListImpl<'a>
where
	V: Serializable,
{
	fn push(&mut self, _value: &V) -> Result<(), Error> {
		Ok(())
	}
	fn pop(&mut self) -> Result<Option<V>, Error> {
		todo!()
	}
	fn push_front(&mut self, _value: V) -> Result<(), Error> {
		todo!()
	}
	fn pop_front(&mut self) -> Result<Option<V>, Error> {
		todo!()
	}
	fn pop_raw(&mut self) -> Result<Slab, Error> {
		let slab_impl = Slab {
			id: 0,
			data: &self.x[0..10],
		};
		Ok(slab_impl)
	}
}

impl<'a> StaticListImpl<'a> {
	fn new(x: &'a Vec<u8>) -> Result<Self, Error> {
		Ok(Self { x })
	}
}

pub struct StaticListBuilder {}

impl<'a> StaticListBuilder {
	pub fn build<V>(v: &'a Vec<u8>) -> Result<impl StaticList<'a, V>, Error>
	where
		V: Serializable,
	{
		Ok(StaticListImpl::new(&v)?)
	}
}

#[cfg(test)]
mod test {
	use crate::static_list::StaticListBuilder;
	use crate::types::StaticList;
	use bmw_err::*;

	#[test]
	fn test_static_list() -> Result<(), Error> {
		/*
		let mut v = vec![];
		v.resize(100, 0u8);
		let static_list = StaticListImpl::new(&v)?;
		static_list.push(&1u8)?;
		let x = static_list.pop_raw();
			*/
		let mut v = vec![];
		v.resize(100, 0u8);
		let mut static_list = StaticListBuilder::build(&v)?;
		static_list.push(&1u8)?;
		let _x = static_list.pop_raw()?;
		Ok(())
	}
}
