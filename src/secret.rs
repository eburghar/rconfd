use serde_json::Value;
use std::{
	collections::HashMap,
	ops::{Deref, DerefMut},
};
use anyhow::{anyhow, Result};

pub struct Secrets(HashMap<String, Option<Value>>);

impl Deref for Secrets {
	type Target = HashMap<String, Option<Value>>;
	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl DerefMut for Secrets {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

impl Secrets {
	pub fn new() -> Self {
		Self(HashMap::<String, Option<Value>>::new())
	}

	/// Replace the secret at path if it has changed, and return if it has been replaced
	pub fn replace(&mut self, path: &str, value: Value) -> bool {
		let val = self.entry(path.to_owned()).or_insert(None);
		let prev_val = val.take();

		// if the secret has changed
		let res = prev_val.is_none() || prev_val.unwrap() != value;
		if res {
			// replace secret value
			*val = Some(value);
		}
		res
	}

}

/// The different types of supported backend
#[derive(Copy, Clone, PartialEq)]
pub enum Backend {
	/// Vault
	Vault,
	/// Environement
	Env,
	/// Filesystem
	File,
}

/// lookup list for backend
const BACKENDS: &'static [(&'static str, Backend)] = &[
	("vault", Backend::Vault),
	("env", Backend::Env),
	("file", Backend::File),
];

/// transform a backend str into its enum representation
pub fn backend_path(path: &str) -> Result<Backend> {
	BACKENDS
		.iter()
		.find_map(|(prefix, backend)| {
			if path.starts_with(*prefix) {
				Some(*backend)
			} else {
				None
			}
		})
		.ok_or_else(|| anyhow!("unknown backend \"{}\"", path))
}

/// Split a secret path into its 3 components: backend, args and path
pub struct Secret<'a> {
	pub backend: Backend,
	pub args: &'a str,
	pub path: &'a str
}

impl<'a> Secret<'a> {
	pub fn new(path: &'a str) -> Result<Self> {
		let mut it = path.split(":");
		let backend_str = it.next().ok_or_else(|| anyhow!("no backend"))?;
		let args = it.next().ok_or_else(|| anyhow!("no args"))?;
		let path = it.next().ok_or_else(|| anyhow!("no path"))?;
		if it.next().is_some() {
			anyhow!("extra ':' in path");
		}
		Ok(Self {
			backend: backend_path(backend_str)?,
			args,
			path
		})
	}
}
