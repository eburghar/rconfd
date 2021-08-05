use crate::{
	message::Message,
	secret::Secrets,
	subst::subst_envar
};

use anyhow::{Context, Result};
use async_std::channel::Sender;
use serde::{Deserialize, Deserializer};
use std::{
	collections::HashMap,
	fs::{self, File},
	ops::{Deref, DerefMut},
	path::{Path, PathBuf},
};

pub struct TemplateConfs(HashMap<String, TemplateConf>);

impl Deref for TemplateConfs {
	type Target = HashMap<String, TemplateConf>;
	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl DerefMut for TemplateConfs {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

impl TemplateConfs {
	pub fn new() -> Self {
		Self(HashMap::<String, TemplateConf>::new())
	}

	/// generate all templates if conditions are met
	pub async fn generate_templates(
		&self,
		secrets: &Secrets,
		path: &str,
		sender: &Sender<Message>,
	) -> Result<()> {
		for (tmpl, conf) in self.iter() {
			// if the secret is among the template declared secrets
			// and all template secrets are defined
			if conf.secrets.get(path).is_some()
				&& secrets
					.iter()
					.filter_map(|(path, val)| {
						if conf.secrets.get(path).is_some() {
							Some(val)
						} else {
							None
						}
					})
					.all(|o| o.is_some())
			{
				// ask the broker to regenerate the template
				sender.send(Message::GenerateTemplate(tmpl.clone())).await?;
			}
		}
		Ok(())
	}
}

/// Define a template job
type Conf = HashMap<String, TemplateConf>;

#[derive(Debug, Deserialize)]
pub struct TemplateConf {
	/// basedir for config files with relative path in jsonnet template
	#[serde(deserialize_with="string_envar")]
	pub dir: String,
	/// mode of resulting files
	pub mode: String,
	/// owner of resulting files
	pub user: String,
	/// secrets to inject in the jsonnet engine as "secrets" extVar
	#[serde(deserialize_with = "key_envar")]
	pub secrets: HashMap<String, String>,
	/// command to spawn if some files have been modified
	pub cmd: Option<String>,
}

/// Substitute environement variables in a string
pub fn string_envar<'a, D>(deserializer: D) -> Result<String, D::Error>
where
	D: Deserializer<'a>,
{
	let s = String::deserialize(deserializer)?;
	// try to substiture all variable in path
	if let Ok(s) = subst_envar(&s) {
		Ok(s)
	// return the original string (TODO: show error at deserialization)
	} else {
		Ok(s)
	}
}

/// Substitute environement variables in the keys (path) of secrets hashmaps before serializing
fn key_envar<'a, D>(deserializer: D) -> Result<HashMap<String, String>, D::Error>
where
	D: Deserializer<'a>,
{
	// new type to be able to define a specific deserialize_with function to apply upon
	#[derive(Deserialize, PartialEq, Eq, Hash)]
	struct Wrapper(#[serde(deserialize_with = "string_envar")] String);

	let v = HashMap::<Wrapper, String>::deserialize(deserializer)?;
	Ok(v.into_iter().map(|(Wrapper(k), v)| (k, v)).collect())
}

/// parse json to conf
pub fn parse_config(file: &Path) -> Result<Conf> {
	let reader = File::open(file).unwrap();
	Ok(serde_json::from_reader::<File, Conf>(reader)?)
}

/// Return the list of config files names inside dir
/// TODO: use generics to return iterator
pub fn config_files(dir: &String) -> Result<Vec<PathBuf>> {
	fs::read_dir(dir)
		.with_context(|| format!("can't browse confid dir {}", dir))?
		.map(|r| r.map_err(|e| anyhow::Error::from(e)).map(|d| d.path()))
		.filter(|r| r.is_ok() && is_conffile(r.as_deref().unwrap()))
		.collect()
}

/// must be a regular file and have .json extension
fn is_conffile<T>(path: T) -> bool
where
	T: AsRef<Path>,
{
	let path = path.as_ref();
	path.is_file()
		&& if let Some(ext) = path.extension() {
			ext == "json"
		} else {
			false
		}
}
