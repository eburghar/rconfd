use std::{
	collections::HashMap,
	ops::{Deref, DerefMut},
};
use vault_jwt::secret::Secret;

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
	pub fn any_leased(&self) -> bool {
		self.iter()
			.any(|(_, secret)| secret.as_ref().filter(|s| s.has_lease()).is_some())
	}
}
