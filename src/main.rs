use bmw_err::Error;

fn main() -> Result<(), Error> {
	Ok(())
}

#[cfg(test)]
mod test {
	use crate::main;
	use bmw_err::Error;

	#[test]
	fn test_main() -> Result<(), Error> {
		assert!(main().is_ok());
		Ok(())
	}
}
