use anyhow::{anyhow, Result};
use std::{
	collections::HashMap,
	ops::{Deref, DerefMut},
};
use vaultk8s::secret::Secret;

pub struct Secrets(HashMap<String, Option<Secret>>);

impl Deref for Secrets {
	type Target = HashMap<String, Option<Secret>>;
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
		Self(HashMap::<String, Option<Secret>>::new())
	}

	/// Replace the secret at path if it has changed, and return true if it has been replaced
	pub fn replace(&mut self, path: &str, secret: Secret) -> bool {
		let val = self.entry(path.to_owned()).or_insert(None);
		let prev_val = val.take();

		// if the secret has changed
		let res = prev_val.is_none() || prev_val.unwrap() != secret;
		// replace/restore (after take) the secret value
		*val = Some(secret);
		res
	}

	/// Tell if the secrets map contains at least a leased secret
	pub fn has_lease(&self) -> bool {
		self.iter()
			.any(|(_, secret)| secret.as_ref().filter(|s| s.has_lease()).is_some())
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
	pub path: &'a str,
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
			path,
		})
	}
}
