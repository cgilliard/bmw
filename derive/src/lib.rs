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

extern crate proc_macro;
use bmw_err::{err, ErrKind, Error};
use bmw_log::*;
use proc_macro::TokenStream;
use proc_macro::TokenTree;
use proc_macro::TokenTree::{Group, Ident, Literal, Punct};

// Note about tarpaulin. Tarpaulin doesn't cover proc_macros so we disable it throughout this
// library.

info!();

struct MacroState {
	ret_read: String,
	ret_write: String,
	expect_name: bool,
	name: String,
	field_names: Vec<String>,
}

#[cfg(not(tarpaulin_include))]
impl MacroState {
	fn new() -> Self {
		Self {
			ret_read: "".to_string(),
			ret_write: "".to_string(),
			expect_name: false,
			name: "".to_string(),
			field_names: vec![],
		}
	}

	fn append_read(&mut self, s: &str) {
		self.ret_read = format!("{}{}", self.ret_read, s);
	}

	fn append_write(&mut self, s: &str) {
		self.ret_write = format!("{}{}", self.ret_write, s);
	}

	fn ret(&self) -> String {
		let mut field_name_return = "Ok(Self {".to_string();
		for x in &self.field_names {
			field_name_return = format!("{} {},", field_name_return, x);
		}
		field_name_return = format!("{} }})", field_name_return);

		let ret = format!("impl bmw_util::Serializable for {} {{ \n\
                    fn read<R>(reader: &mut R) -> Result<Self, bmw_err::Error> where R: bmw_util::Reader {{ {} {} }}\n\
                    fn write<W>(&self, writer: &mut W) -> Result<(), bmw_err::Error> where W: bmw_util::Writer {{ {} Ok(()) }}\n\
                    }}", self.name, self.ret_read, field_name_return, self.ret_write);
		let _ = debug!("ret='{}'", ret);
		ret
	}
}

#[proc_macro_derive(Serializable)]
#[cfg(not(tarpaulin_include))]
pub fn derive_serialize(strm: TokenStream) -> TokenStream {
	let mut state = MacroState::new();
	let _ = debug!("-----------------derive serialization----------------");
	match process_strm(strm, &mut state) {
		Ok(_) => state.ret().parse().unwrap(),
		Err(e) => {
			let _ = error!("parsing Serializable generated error: {}", e);
			"".parse().unwrap()
		}
	}
}

#[cfg(not(tarpaulin_include))]
fn process_strm(strm: TokenStream, state: &mut MacroState) -> Result<(), Error> {
	for tree in strm {
		process_token_tree(tree, state)?;
	}
	Ok(())
}

#[cfg(not(tarpaulin_include))]
fn process_token_tree(tree: TokenTree, state: &mut MacroState) -> Result<(), Error> {
	match tree {
		Ident(ident) => {
			let ident = ident.to_string();
			debug!("ident={}", ident)?;

			if state.expect_name {
				debug!("struct/enum name = {}", ident)?;
				state.name = ident.clone();
				state.expect_name = false;
			} else if ident != "pub" && ident != "struct" && ident != "enum" {
				let fmt = format!("error expected pub or struct. Found '{}'", ident);
				let e = err!(ErrKind::IllegalState, fmt);
				return Err(e);
			}

			if ident == "struct" || ident == "enum" {
				state.expect_name = true;
			}
		}
		Group(group) => {
			process_group(group, state)?;
		}
		Literal(literal) => {
			debug!("literal={}", literal)?;
		}
		Punct(punct) => {
			debug!("punct={}", punct)?;
		}
	}
	Ok(())
}

#[cfg(not(tarpaulin_include))]
fn process_group(group: proc_macro::Group, state: &mut MacroState) -> Result<(), Error> {
	debug!("group={}", group)?;

	let mut expect_name = true;
	let mut name = "".to_string();

	for item in group.stream() {
		match item {
			Ident(ident) => {
				debug!("groupident={}", ident)?;
				if expect_name {
					expect_name = false;
					name = ident.to_string();
				}
			}
			Group(_group) => {
				// we don't need to process the inner group because the read function
				// only requires the name
			}
			Literal(literal) => {
				debug!("groupliteral={}", literal)?;
			}
			Punct(punct) => {
				debug!("grouppunct={}", punct)?;
				if punct.to_string() == ",".to_string() {
					debug!("end a name")?;
					process_field(&name, &group, state)?;
					expect_name = true;
				}
			}
		}
	}

	// if there's no trailing comma.
	if !expect_name {
		process_field(&name, &group, state)?;
	}

	Ok(())
}

#[cfg(not(tarpaulin_include))]
fn process_field(
	name: &String,
	group: &proc_macro::Group,
	state: &mut MacroState,
) -> Result<(), Error> {
	if name.len() == 0 {
		let fmt = format!("expected name for this group: {:?}", group);
		let e = err!(ErrKind::IllegalState, fmt);
		return Err(e);
	}

	state.append_read(&format!("let {} = bmw_util::Serializable::read(reader)?; ", name)[..]);
	state.append_write(&format!("bmw_util::Serializable::write(&self.{}, writer)?; ", name)[..]);
	state.field_names.push(name.clone());

	Ok(())
}
