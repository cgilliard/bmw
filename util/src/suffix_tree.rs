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

use crate::{List, Match, Pattern, Reader, Serializable, SuffixTree, Writer};
use bmw_err::Error;

struct Dictionary {}

#[allow(dead_code)]
pub(crate) struct SuffixTreeImpl {
	dictionary: Dictionary,
}

impl SuffixTreeImpl {
	pub(crate) fn new(_patterns: impl List<Pattern>) -> Result<Self, Error> {
		Ok(Self {
			dictionary: Dictionary {},
		})
	}

	pub fn _add(&mut self, _pattern: Pattern) -> Result<(), Error> {
		Ok(())
	}
}

impl SuffixTree for SuffixTreeImpl {
	fn run_matches(
		&mut self,
		_text: &[u8],
		_matches: &mut [Box<dyn Match>],
	) -> Result<usize, Error> {
		Ok(0)
	}
}

pub(crate) struct MatchImpl {
	start: usize,
	end: usize,
	id: usize,
}

impl MatchImpl {
	pub(crate) fn new(start: usize, end: usize, id: usize) -> Self {
		Self { start, end, id }
	}
}

impl Match for MatchImpl {
	fn start(&self) -> usize {
		self.start
	}
	fn end(&self) -> usize {
		self.end
	}
	fn id(&self) -> usize {
		self.id
	}
	fn set_start(&mut self, start: usize) -> Result<(), Error> {
		self.start = start;
		Ok(())
	}
	fn set_end(&mut self, end: usize) -> Result<(), Error> {
		self.end = end;
		Ok(())
	}
	fn set_id(&mut self, id: usize) -> Result<(), Error> {
		self.id = id;
		Ok(())
	}
}

impl Pattern {
	pub(crate) fn new(
		regex: &str,
		is_case_sensitive: bool,
		is_termination_pattern: bool,
		id: usize,
	) -> Self {
		Self {
			regex: regex.to_string(),
			is_termination_pattern,
			is_case_sensitive,
			id,
		}
	}
	pub fn regex(&self) -> &String {
		&self.regex
	}
	pub fn is_case_sensitive(&self) -> bool {
		self.is_case_sensitive
	}
	pub fn is_termination_pattern(&self) -> bool {
		self.is_termination_pattern
	}
	pub fn id(&self) -> usize {
		self.id
	}
}

impl Serializable for Pattern {
	fn read<R: Reader>(reader: &mut R) -> Result<Self, Error> {
		let regex = String::read(reader)?;
		let is_case_sensitive = match reader.read_u8()? {
			0 => false,
			_ => true,
		};
		let is_termination_pattern = match reader.read_u8()? {
			0 => false,
			_ => true,
		};
		let id = reader.read_usize()?;
		Ok(Self {
			regex,
			is_case_sensitive,
			is_termination_pattern,
			id,
		})
	}
	fn write<W: Writer>(&self, writer: &mut W) -> Result<(), Error> {
		String::write(&self.regex, writer)?;
		match self.is_case_sensitive {
			false => writer.write_u8(0)?,
			true => writer.write_u8(1)?,
		}
		match self.is_termination_pattern {
			false => writer.write_u8(0)?,
			true => writer.write_u8(1)?,
		}
		writer.write_usize(self.id)?;
		Ok(())
	}
}

#[cfg(test)]
mod test {
	use crate as bmw_util;
	use crate::{list, Builder};
	use bmw_err::*;

	#[test]
	fn test_suffix_tree() -> Result<(), Error> {
		let _suffix_tree = Builder::build_suffix_tree(list![
			Builder::build_pattern("p1", false, false, 0),
			Builder::build_pattern("p2", false, false, 1),
			Builder::build_pattern("p3", true, false, 2)
		])?;
		Ok(())
	}
}
