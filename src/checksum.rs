use anyhow::Result;
use async_std::{
	fs::read,
	path::{Path, PathBuf},
};
use sha1::{Digest, Sha1};
use std::{
	collections::HashMap,
	ops::{Deref, DerefMut},
};

pub struct Checksums(HashMap<PathBuf, Option<Digest>>);

impl Deref for Checksums {
	type Target = HashMap<PathBuf, Option<Digest>>;
	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl DerefMut for Checksums {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

impl Checksums {
	pub fn new() -> Self {
		Self(HashMap::<PathBuf, Option<Digest>>::new())
	}

	/// add file digest identifies by path in the hashmap and return true if the value is new or has changed
	pub async fn hash_file<T>(&mut self, path: T) -> Result<bool>
	where
		T: AsRef<Path>,
	{
		let path = path.as_ref();
		let digest = self.entry(path.to_owned()).or_insert(None);
		let prev_digest = digest.clone();
		let mut hasher = Sha1::default();
		let content = read(path).await?;
		hasher.update(&content);
		*digest = Some(hasher.digest());
		Ok(prev_digest != *digest)
	}
}
