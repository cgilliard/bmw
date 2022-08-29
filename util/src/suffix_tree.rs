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

use crate::types::{Dictionary, MatchImpl, Node, SuffixTreeImpl};
use crate::{Builder, List, Match, Pattern, Reader, Serializable, Stack, SuffixTree, Writer};
use bmw_err::{err, ErrKind, Error};

impl Default for Node {
	fn default() -> Self {
		Self {
			next: [u32::MAX; 257],
			pattern_id: usize::MAX,
			is_multi: false,
			is_term: false,
			is_start_only: false,
			is_multi_line: true,
		}
	}
}

impl Dictionary {
	fn new() -> Result<Self, Error> {
		Ok(Self {
			nodes: vec![Node::default()],
			next: 0,
		})
	}

	fn add(&mut self, pattern: Pattern) -> Result<(), Error> {
		if pattern.regex.len() == 0 {
			return Err(err!(
				ErrKind::IllegalArgument,
				"regex length must be greater than 0"
			));
		}

		let lower;
		let mut regex = if pattern.is_case_sensitive {
			pattern.regex.as_str().bytes().peekable()
		} else {
			lower = pattern.regex.to_lowercase();
			lower.as_str().bytes().peekable()
		};
		let mut cur_byte = regex.next().unwrap();
		let mut cur_node = &mut self.nodes[0];
		let mut is_start_only = false;

		if cur_byte == '^' as u8 {
			cur_byte = match regex.next() {
				Some(cur_byte) => {
					is_start_only = true;
					cur_byte
				}
				None => {
					return Err(err!(
						ErrKind::IllegalArgument,
						"Regex must be at least one byte long not including the ^ character"
					));
				}
			}
		}

		loop {
			let (check_index, is_multi) = if cur_byte == '.' as u8 {
				let peek = regex.peek();
				let is_multi = match peek {
					Some(peek) => {
						if *peek == '*' as u8 {
							regex.next();
							true
						} else {
							false
						}
					}
					_ => false,
				};
				(256usize, is_multi) // wild card is 256
			} else if cur_byte == '\\' as u8 {
				let next = regex.next();
				match next {
					Some(next) => {
						if next == '\\' as u8 {
							(cur_byte as usize, false)
						} else if next == '.' as u8 {
							(next as usize, false)
						} else {
							return Err(err!(
								ErrKind::IllegalArgument,
								&format!("Illegal escape character '{}'", next as char)[..]
							));
						}
					}
					None => {
						return Err(err!(
							ErrKind::IllegalArgument,
							"Illegal escape character at termination of string"
						));
					}
				}
			} else {
				(cur_byte as usize, false)
			};
			let index = match cur_node.next[check_index] {
				u32::MAX => {
					cur_node.next[check_index] = self.next + 1;
					self.next += 1;
					self.next
				}
				_ => cur_node.next[check_index],
			};

			if index >= self.nodes.len().try_into()? {
				self.nodes.push(Node::default());
			}
			cur_node = &mut self.nodes[index as usize];
			cur_node.is_multi = is_multi;
			cur_byte = match regex.next() {
				Some(cur_byte) => cur_byte,
				None => {
					cur_node.pattern_id = pattern.id;
					cur_node.is_term = pattern.is_termination_pattern;
					cur_node.is_start_only = is_start_only;
					cur_node.is_multi_line = pattern.is_multi_line;
					break;
				}
			};
		}

		Ok(())
	}
}

impl SuffixTree for SuffixTreeImpl {
	fn tmatch(&mut self, text: &[u8], matches: &mut [impl Match]) -> Result<usize, Error> {
		let match_count = 0;
		let max_wildcard_length = self.max_wildcard_length;
		let termination_length = self.termination_length;
		let dictionary = &self.dictionary_case_insensitive;
		loop {
			if self.branch_stack.pop().is_none() {
				break;
			}
		}
		let match_count = Self::tmatch_impl(
			text,
			matches,
			match_count,
			dictionary,
			false,
			max_wildcard_length,
			&mut self.branch_stack,
			termination_length,
		)?;
		let dictionary = &self.dictionary_case_sensitive;
		loop {
			if self.branch_stack.pop().is_none() {
				break;
			}
		}
		Self::tmatch_impl(
			text,
			matches,
			match_count,
			dictionary,
			true,
			max_wildcard_length,
			&mut self.branch_stack,
			termination_length,
		)
	}
}

impl SuffixTreeImpl {
	pub(crate) fn new(
		patterns: impl List<Pattern>,
		termination_length: usize,
		max_wildcard_length: usize,
	) -> Result<Self, Error> {
		let mut dictionary_case_insensitive = Dictionary::new()?;
		let mut dictionary_case_sensitive = Dictionary::new()?;

		let branch_stack = Builder::build_stack_box(patterns.size())?;

		for pattern in patterns.iter() {
			if pattern.is_case_sensitive {
				dictionary_case_sensitive.add(pattern)?;
			} else {
				dictionary_case_insensitive.add(pattern)?;
			}
		}

		Ok(Self {
			dictionary_case_insensitive,
			dictionary_case_sensitive,
			termination_length,
			max_wildcard_length,
			branch_stack,
		})
	}

	fn tmatch_impl(
		text: &[u8],
		matches: &mut [impl Match],
		mut match_count: usize,
		dictionary: &Dictionary,
		case_sensitive: bool,
		max_wildcard_length: usize,
		branch_stack: &mut Box<dyn Stack<(usize, usize)>>,
		termination_length: usize,
	) -> Result<usize, Error> {
		let mut itt = 0;
		let len = text.len();
		let mut cur_node = &dictionary.nodes[0];
		let mut start = 0;
		let mut multi_counter = 0;
		let mut is_branch = false;
		let mut has_newline = false;

		loop {
			if start >= len || start >= termination_length {
				break;
			}
			if is_branch {
				is_branch = false;
			} else {
				has_newline = false;
				itt = start;
			}

			let mut last_multi: Option<&Node> = None;
			loop {
				if itt >= len {
					break;
				}

				let byte = if case_sensitive {
					text[itt]
				} else {
					if text[itt] >= 'A' as u8 && text[itt] <= 'Z' as u8 {
						text[itt] + 32
					} else {
						text[itt]
					}
				};

				if byte == '\r' as u8 || byte == '\n' as u8 {
					has_newline = true;
				}

				if !cur_node.is_multi {
					multi_counter = 0;
				}

				match cur_node.next[byte as usize] {
					u32::MAX => {
						if cur_node.is_multi {
							last_multi = Some(cur_node);
							multi_counter += 1;
							if multi_counter >= max_wildcard_length {
								// wild card max length. break as no
								// match and continue
								break;
							}
							itt += 1;
							continue;
						}
						// check wildcard
						match cur_node.next[256] {
							u32::MAX => {
								if last_multi.is_some() {
									cur_node = last_multi.unwrap();
									continue;
								}
								break;
							}
							_ => cur_node = &dictionary.nodes[cur_node.next[256] as usize],
						}
					}
					_ => {
						match cur_node.next[256] {
							u32::MAX => {}
							_ => {
								// we have a branch here. Add it to the stack.
								branch_stack.push((itt, cur_node.next[256] as usize))?;
							}
						}
						cur_node = &dictionary.nodes[cur_node.next[byte as usize] as usize]
					}
				}

				match cur_node.pattern_id {
					usize::MAX => {}
					_ => {
						if !(cur_node.is_start_only && start != 0) {
							if match_count >= matches.len() {
								// too many matches return with the
								// first set of matches
								return Ok(match_count);
							}

							if !has_newline || cur_node.is_multi_line {
								matches[match_count].set_id(cur_node.pattern_id);
								matches[match_count].set_end(itt + 1);
								matches[match_count].set_start(start);
								last_multi = None;
								match_count += 1;
								if cur_node.is_term {
									return Ok(match_count);
								}
							}
						}
					}
				}

				itt += 1;
			}

			match branch_stack.pop() {
				Some(br) => {
					cur_node = &dictionary.nodes[br.1];
					itt = br.0;
					is_branch = true;
				}
				None => {
					start += 1;
					cur_node = &dictionary.nodes[0];
				}
			}
		}
		Ok(match_count)
	}
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
	fn set_start(&mut self, start: usize) {
		self.start = start;
	}
	fn set_end(&mut self, end: usize) {
		self.end = end;
	}
	fn set_id(&mut self, id: usize) {
		self.id = id;
	}
}

impl Pattern {
	pub(crate) fn new(
		regex: &str,
		is_case_sensitive: bool,
		is_termination_pattern: bool,
		is_multi_line: bool,
		id: usize,
	) -> Self {
		Self {
			regex: regex.to_string(),
			is_termination_pattern,
			is_case_sensitive,
			is_multi_line,
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
		let is_multi_line = match reader.read_u8()? {
			0 => false,
			_ => true,
		};
		let id = reader.read_usize()?;
		Ok(Self {
			regex,
			is_case_sensitive,
			is_termination_pattern,
			is_multi_line,
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
		match self.is_multi_line {
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
	use crate::{list, Builder, Match, SuffixTree};
	use bmw_err::*;
	use bmw_log::*;

	info!();

	#[test]
	fn test_suffix_tree1() -> Result<(), Error> {
		let mut suffix_tree = Builder::build_suffix_tree(
			list![
				Builder::build_pattern("p1", false, false, false, 0),
				Builder::build_pattern("p2", false, false, false, 1),
				Builder::build_pattern("p3", true, false, false, 2)
			],
			1_000,
			100,
		)?;

		let mut matches = [Builder::build_match_default(); 10];
		let count = suffix_tree.tmatch(b"p1p2", &mut matches)?;
		info!("count={}", count)?;
		assert_eq!(count, 2);
		assert_eq!(matches[0].id(), 0);
		assert_eq!(matches[0].start(), 0);
		assert_eq!(matches[0].end(), 2);
		assert_eq!(matches[1].id(), 1);
		assert_eq!(matches[1].start(), 2);
		assert_eq!(matches[1].end(), 4);

		Ok(())
	}

	#[test]
	fn test_suffix_tree_wildcard() -> Result<(), Error> {
		let mut suffix_tree = Builder::build_suffix_tree(
			list![
				Builder::build_pattern("p1.*abc", false, false, false, 0),
				Builder::build_pattern("p2", false, false, false, 1),
				Builder::build_pattern("p3", true, false, false, 2)
			],
			37,
			10,
		)?;

		let mut matches = [Builder::build_match_default(); 10];
		let count = suffix_tree.tmatch(b"p1xyz123abcp2", &mut matches)?;
		assert_eq!(count, 2);
		assert_eq!(matches[0].id(), 0);
		assert_eq!(matches[0].start(), 0);
		assert_eq!(matches[0].end(), 11);
		assert_eq!(matches[1].id(), 1);
		assert_eq!(matches[1].start(), 11);
		assert_eq!(matches[1].end(), 13);
		for i in 0..count {
			info!("match[{}]={:?}", i, matches[i])?;
		}

		// try a wildcard that is too long
		let count = suffix_tree.tmatch(b"p1xyzxxxxxxxxxxxxxxxxxxxxxxxx123abcp2", &mut matches)?;
		assert_eq!(count, 1);
		assert_eq!(matches[0].id(), 1);
		assert_eq!(matches[0].start(), 35);
		assert_eq!(matches[0].end(), 37);
		for i in 0..count {
			info!("match[{}]={:?}", i, matches[i])?;
		}

		// test termination
		let count =
			suffix_tree.tmatch(b"p1xyzxxxxxxxxxxxxxxxxxxxxxxxxxxx123abcp2", &mut matches)?;
		assert_eq!(count, 0);

		Ok(())
	}

	#[test]
	fn test_case_sensitivity() -> Result<(), Error> {
		let mut matches = [Builder::build_match_default(); 10];
		let pattern1 = Builder::build_pattern("AaAaA", true, false, false, 0);
		let pattern2 = Builder::build_pattern("AaAaA", false, false, false, 0);

		let mut suffix_tree = Builder::build_suffix_tree(list![pattern1], 100, 100)?;

		assert_eq!(suffix_tree.tmatch(b"AAAAA", &mut matches)?, 0);

		let mut suffix_tree = Builder::build_suffix_tree(list![pattern2], 100, 100)?;

		assert_eq!(suffix_tree.tmatch(b"AAAAA", &mut matches)?, 1);

		Ok(())
	}

	#[test]
	fn test_error_conditions() -> Result<(), Error> {
		assert!(Builder::build_suffix_tree(
			list![Builder::build_pattern("", false, false, false, 0)],
			36,
			36
		)
		.is_err());
		Ok(())
	}
}
