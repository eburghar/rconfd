use crate::error::Error;

use anyhow::Result;
use std::{
	collections::HashMap,
	convert::TryFrom,
	ops::{Deref, DerefMut},
	fmt
};
use vaultk8s::secret::Secret;

/// new type to define new methods over HashMap
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
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
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

impl<'a> fmt::Display for Backend {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		for (s, b) in BACKENDS.iter() {
			if self == b {
				return write!(f, "{}", s);
			}
		}
		Ok(())
	}
}

/// Convert a backend text representation into its enum
impl<'a> TryFrom<&'a str> for Backend {
	type Error = Error;

	fn try_from(backend_str: &'a str) -> Result<Self, Self::Error> {
		BACKENDS
			.iter()
			.find_map(|(prefix, backend)| {
				if backend_str.starts_with(*prefix) {
					Some(*backend)
				} else {
					None
				}
			})
			.ok_or(Error::UnknowBackend(backend_str.to_owned()))
	}
}
/// Deserialize a secret path
pub struct SecretPath<'a> {
	pub backend: Backend,
	pub args: Vec<&'a str>,
	pub kwargs: Option<Vec<(&'a str, &'a str)>>,
	pub path: &'a str,
}

/// Serialize a SecretPath
impl<'a> fmt::Display for SecretPath<'a> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}:{}", self.backend, self.args.join(","))?;
		if let Some(ref kwargs) = self.kwargs {
			for (k, v) in kwargs.iter() {
				write!(f, ",{}={}", k, v)?;
			}
		}
		write!(f, ":{}", self.path)
	}
}
