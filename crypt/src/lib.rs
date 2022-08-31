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

use bmw_err::*;
use bmw_log::*;

info!();

pub fn test() -> Result<(), Error> {
	Ok(())
}

#[cfg(test)]
mod test {
	use bmw_derive::Serializable;
	use bmw_err::*;
	use bmw_log::*;
	use bmw_util::*;

	#[derive(Serializable, Debug, Clone, PartialEq)]
	enum TestEnum {
		A(String),
		B(Vec<u8>),
		C(Option<u16>),
	}

	#[derive(Serializable, Debug, Clone)]
	struct TestStruct {
		id: u128,
		a: u16,
		v: Box<dyn SortableList<String>>,
		w: Array<u32>,
		x: Vec<u8>,
		y: Option<String>,
		z: [u8; 8],
	}

	info!();

	#[test]
	fn test_ser() -> Result<(), Error> {
		let ok = "ok".to_string();
		let ok2 = "ok".to_string();
		let ok3 = "notok".to_string();
		{
			let mut x: Array<String> = Builder::build_array(10)?;
			x[0] = ok.clone();
			assert_eq!(x[0], ok2);
			assert_ne!(x[0], ok3);
		}
		let s = TestStruct {
			id: 1234,
			a: 2,
			v: array_list_box!(10)?,
			w: array!(10)?,
			x: vec![0, 1, 2],
			y: Some("test".to_string()),
			z: [0u8; 8],
		};

		let mut hashtable = hashtable!()?;
		hashtable.insert(&1, &s)?;
		assert_eq!(hashtable.get(&1)?.unwrap().id, s.id);

		let t = TestEnum::A("str123".to_string());
		let mut hashtable = hashtable!()?;
		hashtable.insert(&1, &t)?;
		assert_eq!(hashtable.get(&1)?.unwrap(), t);
		Ok(())
	}
}
