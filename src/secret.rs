use serde_json::Value;
use std::{
	collections::HashMap,
	ops::{Deref, DerefMut},
};

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

#[derive(Copy, Clone)]
pub enum Backend {
	Vault,
	Env,
	File,
}

/// hard-coded list of backend
const BACKENDS: &'static [(&'static str, Backend)] = &[
	("vault:", Backend::Vault),
	("env:", Backend::Env),
	("file:", Backend::File),
];

pub fn backend(path: &str) -> Backend {
	BACKENDS
		.iter()
		.find_map(|(prefix, backend)| {
			if path.starts_with(*prefix) {
				Some(*backend)
			} else {
				None
			}
		})
		.unwrap_or(Backend::Vault)
}

pub fn backend_path(path: &str) -> &str {
	BACKENDS
		.iter()
		.find_map(|(prefix, _)| {
			if path.starts_with(*prefix) {
				Some(&path[prefix.len()..])
			} else {
				None
			}
		})
		.unwrap_or(path)
}
