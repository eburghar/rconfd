use anyhow::{Context, Result};
use async_std::channel::Sender;
use serde::{Deserialize, Serialize};
use std::{
	collections::HashMap,
	fs::{self, File},
	ops::{Deref, DerefMut},
	path::{Path, PathBuf},
};

use crate::{message::Message, secret::Secrets};

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
#[derive(Debug, Serialize, Deserialize)]
pub struct Conf {
	/// path of the jsonnet template to use for config files manifestation
	pub tmpl: String,
	// configuration for template
	pub conf: TemplateConf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateConf {
	/// basedir for config files with relative path in jsonnet template
	pub dir: String,
	/// mode of resulting files
	pub mode: String,
	/// owner of resulting files
	pub user: String,
	/// secrets to inject in the jsonnet engine as "secrets" extVar
	pub secrets: HashMap<String, String>,
	/// command to spawn if some files have been modified
	pub cmd: String,
}

/// parse json to conf
pub fn parse_config(file: &Path) -> Result<Conf> {
	let reader = File::open(file).unwrap();
	Ok(serde_json::from_reader::<File, Conf>(reader)?)
}

/// list configfile from dir
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
