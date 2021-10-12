use crate::{message::Message, secrets::Secrets, subst::subst_envar};

use anyhow::{Context, Result};
use async_std::channel::Sender;
use serde::{de, Deserialize, Deserializer};
use std::{
	collections::HashMap,
	fmt,
	fs::{self, File},
	ops::{Deref, DerefMut},
	path::{Path, PathBuf},
	process::Command,
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

	/// (re)generate templates that have a declared secret at path
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
				// fetch dynamic (exe) secrets before generating the template (dynamic secrets are always invalid)
				for (path, secret) in secrets.iter().filter_map(|(path, val)| {
					if conf.secrets.get(path).is_some() {
						Some((path, val))
					} else {
						None
					}
				}) {
					// fetch the secret without triggering template manifestation once again
					if secret.is_none() || !secret.as_ref().unwrap().is_valid() {
						sender
							.send(Message::GetSecret(path.to_owned(), false))
							.await?;
					}
				}
				// ask the broker to regenerate the template
				sender.send(Message::GenerateTemplate(tmpl.clone())).await?;
			}
		}
		Ok(())
	}

	/// (re)generate all templates that have all secrets defined
	pub async fn generate_all_templates(
		&self,
		secrets: &Secrets,
		sender: &Sender<Message>,
	) -> Result<()> {
		for (tmpl, conf) in self.iter() {
			if conf
				.secrets
				.iter()
				.filter_map(|(path, _)| Some(secrets.get(path).is_some()))
				.all(|o| o)
			{
				sender.send(Message::GenerateTemplate(tmpl.clone())).await?;
			} else {
				log::warn!("skipping template \"{}\" due to undefined secrets", tmpl);
			}
		}
		Ok(())
	}
}

/// Define a template job
type Conf = HashMap<String, TemplateConf>;

#[derive(Debug, Deserialize)]
pub struct Hooks {
	/// executed whenever some files have been modified
	pub modified: Option<String>,
	/// executed right after the first manifestation
	pub ready: Option<String>,
}

pub enum HookType {
	MODIFIED,
	READY,
}

impl fmt::Display for HookType {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			HookType::MODIFIED => write!(f, "modified"),
			HookType::READY => write!(f, "ready"),
		}
	}
}

impl Hooks {
	pub fn trigger(&self, hook_type: HookType) {
		let hook = match hook_type {
			HookType::MODIFIED => &self.modified,
			HookType::READY => &self.ready,
		};
		if let Some(ref cmd_str) = hook {
			let args: Vec<&str> = cmd_str.split_whitespace().collect();
			if args.len() > 0 {
				// enforce absolute exec path for security reason
				if args[0].starts_with("/") {
					let mut cmd = Command::new(&args[0]);
					if args.len() > 1 {
						cmd.args(&args[1..]);
					}
					log::info!("  hook {} trigerred. Executing \"{}\"", hook_type, cmd_str);
					let res = cmd.output();
					if res.is_err() {
						log::error!("Executing \"{}\"", cmd_str);
					}
				} else {
					log::error!(
						"cmd \"{}\" must be absolute and start with / to be executed",
						cmd_str
					);
				}
			}
		}
	}
}

#[derive(Debug, Deserialize)]
pub struct TemplateConf {
	/// basedir for config files with relative path in jsonnet template
	#[serde(deserialize_with = "string_envar")]
	pub dir: String,
	/// mode of resulting files
	pub mode: String,
	/// owner of resulting files
	pub user: String,
	/// secrets to inject in the jsonnet engine as "secrets" extVar
	#[serde(deserialize_with = "key_envar")]
	pub secrets: HashMap<String, String>,
	/// hooks to execute commands on events
	pub hooks: Hooks,
}

/// Substitute environement variables in a string
pub fn string_envar<'a, D>(deserializer: D) -> Result<String, D::Error>
where
	D: Deserializer<'a>,
{
	let s = String::deserialize(deserializer)?;
	// try to substiture all variable in path
	Ok(subst_envar(&s).map_err(de::Error::custom)?)
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
		.with_context(|| format!("Browsing config dir {}", dir))?
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
