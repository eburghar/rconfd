use crate::{
	error::Error,
	secret::{Backend, SecretPath},
};

use std::convert::TryFrom;

/// Simple secret path parser
impl<'a> TryFrom<&'a String> for SecretPath<'a> {
	type Error = Error;

	fn try_from(path: &'a String) -> Result<Self, Self::Error> {
		// split all path components
		let mut it = path.split(":");
		let backend_str = it.next().ok_or(Error::NoBackend)?;
		let backend = Backend::try_from(backend_str)?;
		let args_ = it.next().ok_or(Error::NoArgs(backend_str.to_owned()))?;
		let path = it.next().ok_or(Error::NoPath(args_.to_owned()))?;
		if it.next().is_some() {
			Err(Error::ExtraData(path.to_owned()))?;
		}
		// split simple and keyword arguments in separate lists
		let mut args = Vec::new();
		let mut kwargs = Vec::new();
		for arg in args_.split(",") {
			if let Some(pos) = arg.find('=') {
				kwargs.push((&arg[..pos], &arg[pos + 1..]));
			} else {
				args.push(arg);
			}
		}

		Ok(Self {
			backend,
			args,
			kwargs: if kwargs.is_empty() {
				None
			} else {
				Some(kwargs)
			},
			path,
		})
	}
}
