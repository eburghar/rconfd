mod args;
mod checksum;
mod client;
mod conf;
mod libc;
mod message;
mod s6;
mod secret;

use anyhow::{Context, Result};
use async_std::{channel::bounded, stream::StreamExt, task};
use jrsonnet_evaluator::{
	trace::{CompactFormat, PathResolver},
	EvaluationState, FileImportResolver, ManifestFormat, Val,
};
use jrsonnet_interner::IStr;
use serde_json::{map, Value};
use std::{
	env,
	fs::{create_dir_all, File},
	io::{BufReader, Write},
	os::unix::fs::PermissionsExt,
	path::PathBuf,
	process::Command,
};

use crate::{
	args::Args,
	checksum::Checksums,
	client::{VaultClient, VaultClients},
	conf::{config_files, parse_config, TemplateConfs},
	libc::User,
	message::Message,
	s6::s6_ready,
	secret::{Backend, Secrets},
};

async fn main_loop(args: &Args) -> Result<()> {
	// Mutable variables defining the state inside the main loop
	// map role to vault client instance
	let mut clients = VaultClients::new();
	// map secret path to secret value
	let mut secrets = Secrets::new();
	// map template name to template conf
	let mut confs = TemplateConfs::new();
	// map path to checksums
	let mut checksums = Checksums::new();
	// before first generate
	let mut first_run = true;

	// initialise mpsc channel
	let (sender, mut receiver) = bounded::<Message>(10);

	// for each .json files in the conf directory
	let entries = config_files(&args.dir)?;
	for entry in entries.into_iter() {
		// parse config files
		let path = entry.as_path();
		let conf = parse_config(path).with_context(|| format!("config error: {:?}", path))?;
		// copy role and tmpl as they are used as owned key in hashmap
		let role = conf.conf.role.clone();
		let tmpl = conf.tmpl.clone();
		// initialize vault client
		let client = task::block_on(VaultClient::new(&args.url, &args.token, &args.cacert))?;

		// move conf to dedicated hashmap
		confs.insert(conf.tmpl, conf.conf);

		// first login
		sender.send(Message::Login(role.clone())).await?;
		clients.insert(role.clone(), client);

		// fetch all the secrets defined in the template config
		let secrets_it = confs.get(&tmpl).unwrap().secrets.iter();
		for (path, _) in secrets_it {
			// intialize secret to None
			secrets.insert(path.clone(), None);
			// ask the broker to get the initial value
			match secret::backend(path) {
				Backend::Vault => {
					sender
						.send(Message::GetSecret(role.clone(), path.to_owned()))
						.await?
				}
				Backend::Env => sender.send(Message::GetEnv(path.to_owned())).await?,
				Backend::File => sender.send(Message::GetFile(path.to_owned())).await?,
			}
		}
	}

	// actor loop
	while let Some(msg) = receiver.next().await {
		match msg {
			Message::Login(role) => {
				if let Some(client) = clients.get_mut(&role) {
					client.login(sender.clone(), role).await.with_context(|| {
						format!("failed to login to vault server {}", &args.url)
					})?;
				}
			}

			Message::GetSecret(role, path) => {
				if let Some(client) = clients.get_mut(&role) {
					// get the path without prefix
					let name = secret::backend_path(&path);
					// fetch the secret
					let value = client
						.get_secret(sender.clone(), role, name.to_owned())
						.await
						.with_context(|| format!("failed to get the secret \"{}\"", &path))?;
					if secrets.replace(&path, value) {
						confs.generate_templates(&secrets, &path, &sender).await?;
					}
				}
			}

			Message::GetEnv(path) => {
				let name = secret::backend_path(&path);
				let value = serde_json::from_str(&env::var(name).unwrap_or("\"\"".to_owned()))
					.with_context(|| format!("failed to parse variable {} content", name))?;
				if secrets.replace(&path, value) {
					confs.generate_templates(&secrets, &path, &sender).await?;
				}
			}

			Message::GetFile(path) => {
				let file_path = secret::backend_path(&path);
				let file = File::open(file_path)?;
				let reader = BufReader::new(file);
				let value = serde_json::from_reader(reader)
					.with_context(|| format!("failed to parse file \"{}\"", &path))?;
				if secrets.replace(&path, value) {
					confs.generate_templates(&secrets, &path, &sender).await?;
				}
			}

			Message::GenerateTemplate(tmpl) => {
				let conf = confs.get(&tmpl);
				if let Some(conf) = conf {
					// prepare the evaluation state
					let state = EvaluationState::default();
					state
						.with_stdlib()
						.set_manifest_format(ManifestFormat::ToString);
					// add library paths
					state.set_import_resolver(Box::new(FileImportResolver {
						library_paths: conf.paths.iter().map(|s| PathBuf::from(s)).collect(),
					}));
					// set trace format
					state.set_trace_format(Box::new(CompactFormat {
						resolver: PathResolver::Relative(PathBuf::from(&conf.dir)),
						padding: 4,
					}));
					// set trace depth
					state.set_max_trace(20);

					// add the template file
					let val = state
						.evaluate_file_raw(&PathBuf::from(tmpl))
						.map_err(|e| anyhow::Error::msg(state.stringify_err(&e)))
						.with_context(|| "template error")?;

					// add a map of (name, value) in "secrets" extVar
					let mut secrets_val = map::Map::new();
					for (path, val) in secrets.iter() {
						let val = val.as_ref().unwrap();
						// add only the secrets declared in the template config
						if let Some(name) = conf.secrets.get(path) {
							secrets_val.insert(name.clone(), val.clone());
						}
					}
					state.add_ext_var(
						IStr::from("secrets"),
						Val::from(&Value::Object(secrets_val)),
					);

					// parse file mode
					let mode = u32::from_str_radix(&conf.mode, 8);
					if mode.is_err() {
						log::error!("Unable to parse file mode: {}", conf.mode);
					}

					// get user
					let user = User::new(&conf.user);

					let mut changes = false;
					// generate files from template top keys
					for (file, data) in state
						.manifest_multi(val)
						.map_err(|e| anyhow::Error::msg(state.stringify_err(&e)))
						.with_context(|| "manifestation error")?
						.iter()
					{
						let mut path = PathBuf::from(&conf.dir);
						path.push(&file as &str);
						// dirname after joining conf.dir and file
						let mut dir = path.clone();
						dir.pop();
						create_dir_all(dir)?;

						// if path exists then it's not really first run
						if first_run && path.exists() {
							first_run = false;
						}

						// write file
						let mut file = File::create(&path)?;
						writeln!(file, "{}", data)
							.with_context(|| format!("failed to write {:?}", &path))?;
						log::info!("{} successfuly generated", path.to_str().expect("path"));
						// set file permissions
						if let Ok(mode) = mode {
							let mut perms = file.metadata()?.permissions();
							perms.set_mode(mode);
						}
						// set file group and owner
						if let Some(ref user) = user {
							user.chown(&path);
						}
						// save checksum and compare with previous one
						changes |= checksums.hash_file(&path).await.with_context(|| {
							format!("failed to calculate checksum of \"{:?}\"", &path)
						})?;
					}

					// signal s6 readiness that all config files have been generated
					s6_ready(args.ready_fd);

					// if checksums changed and not on first run, then launch cmd
					if changes && !first_run {
						let args: Vec<&str> = conf.cmd.split_whitespace().collect();
						let mut cmd = Command::new(&args[0]);
						if args.len() > 1 {
							cmd.args(&args[1..]);
						}
						log::info!("Configuration files changed. Executing \"{}\"", &conf.cmd);
						let res = cmd.output();
						if res.is_err() {
							log::error!("Failed to execute \"{}\"", &conf.cmd);
						}
					}

					// quit if not in daemon mode and templates have been generated
					if !args.daemon && checksums.iter().all(|(_, c)| c.is_some()) {
						break;
					}
					first_run = false;
				}
			}
		}
	}
	Ok(())
}

fn main() -> Result<()> {
	// parse command line arguments
	let args: Args = args::from_env();

	// initialize env_logger in info mode for rconfd by default
	env_logger::Builder::new()
		.parse_filters(&env::var(String::from("RUST_LOG")).unwrap_or(String::from("rconfd=info")))
		.init();
	task::block_on(main_loop(&args))?;
	Ok(())
}
