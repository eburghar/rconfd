mod args;
mod checksum;
mod conf;
mod libc;
mod message;
mod s6;
mod secret;
mod subst;
mod task;

use crate::{
	args::Args,
	checksum::Checksums,
	conf::{config_files, parse_config, TemplateConfs},
	libc::User,
	message::{send_message, Message},
	s6::s6_ready,
	secret::{Backend, Secret, Secrets},
	subst::subst_var,
	task::delay_task,
};

use anyhow::{anyhow, Context, Result};
use async_std::{channel::bounded, stream::StreamExt};
use jrsonnet_evaluator::{
	trace::{CompactFormat, PathResolver},
	EvaluationState, FileImportResolver, ManifestFormat, Val,
};
use jrsonnet_interner::IStr;
use serde_json::{map, Value};
use std::{
	env,
	fs::{create_dir_all, File},
	io::{BufReader, Read, Write},
	os::unix::fs::PermissionsExt,
	path::PathBuf,
	process::Command,
	time::Duration,
};
use vaultk8s::client::VaultClient;

async fn main_loop(args: &Args) -> Result<()> {
	// Mutable variables defining the state inside the main loop
	// initialize vault client
	let mut client =
		async_std::task::block_on(VaultClient::new(&args.url, &args.token, &args.cacert))?;
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
		for (tmpl, conf) in conf {
			// move conf to dedicated hashmap
			confs.insert(tmpl.clone(), conf);

			let secrets_map = &confs.get(&tmpl).unwrap().secrets;
			if secrets_map.len() == 0 {
				// if no secrets generate template
				sender.send(Message::GenerateTemplate(tmpl.clone())).await?;
			} else {
				// otherwise fetch all the secrets defined in the template config
				for (path, _) in secrets_map.iter() {
					let secret = Secret::new(path)
						.with_context(|| format!("failed to parse secret path \"{}\"", path))?;
					if secret.backend == Backend::Vault {
						// ask the broker to login first
						sender.send(Message::Login(secret.args.to_owned())).await?;
					}
					// intialize secret to None
					secrets.insert(path.clone(), None);
					// ask the broker to get the secret initial value
					sender.send(Message::GetSecret(path.to_owned())).await?
				}
			}
		}
	}

	// actor loop
	while let Some(msg) = receiver.next().await {
		match msg {
			Message::Login(ref role) => {
				let role = subst_var(role)?;
				let auth = client
					.login(&role)
					.await
					.with_context(|| format!("failed to login to vault server {}", &args.url))?;
				// schedule a relogin login task at 2/3 of the lease_duration time
				if auth.renewable {
					let dur = auth.lease_duration * 2 / 3;
					log::debug!(
						"Successfuly logged in to {} with role {}. Log in again within {:?}",
						&client.url,
						&role,
						&dur
					);
					delay_task(send_message(sender.clone(), Message::Login(role)), dur);
				}
			}

			Message::GetSecret(ref path) => {
				let secret = Secret::new(path)
					.with_context(|| format!("failed to parse secret path \"{}\"", path))?;
				match secret.backend {
					Backend::Vault => {
						let secret_args = subst_var(secret.args)?;
						let secret_path = subst_var(secret.path)?;

						// fetch the secret
						let value = client
							.get_secret(&secret_args, &secret_path)
							.await
							.with_context(|| {
								format!("failed to get the secret \"{}\"", &secret_path)
							})?;

						// schedule the newew of the secret at 2/3 of the lease_duration time
						let renewable = value["renewable"].as_bool().unwrap_or(false);
						if renewable {
							let dur = Duration::from_secs(
								value["lease_duration"].as_u64().unwrap_or(0u64) * 2 / 3,
							);
							log::debug!("Successfuly get secret. Renew within {:?}", &dur);
							delay_task(
								send_message(sender.clone(), Message::GetSecret(path.to_owned())),
								dur,
							);
						}

						// replace secret value an regenerate template if necessary
						if secrets.replace(path, value) {
							confs.generate_templates(&secrets, path, &sender).await?;
						}
					}

					Backend::Env => {
						let secret_path = subst_var(secret.path)?;
						let value = match secret.args {
							"str" => {
								Value::String(env::var(&secret_path).unwrap_or("".to_owned()))
							},
							"js" => {
								serde_json::from_str(&env::var(&secret_path).unwrap_or("\"\"".to_owned()))
									.with_context(|| {
										format!("failed to parse \"{}\" variable content", &secret_path)
									})?
							},
							_ => Err(anyhow!("malformed secret \"{}\"\n    expected argument \"str\" or \"js\" found \"{}\"", path, secret.args))?
						};
						if secrets.replace(&path, value) {
							confs.generate_templates(&secrets, path, &sender).await?;
						}
					}

					Backend::File => {
						let secret_path = subst_var(secret.path)?;
						let mut file = File::open(&secret_path)
							.with_context(|| format!("failed to open \"{}\"", &secret_path))?;

						let value = match secret.args {
							"str" => {
								let mut buffer = String::new();
								file.read_to_string(&mut buffer).with_context(|| format!("failed to read \"{}\"", &secret_path))?;
								Value::String(buffer)
							},
							"js" => {
								let reader = BufReader::new(file);
								serde_json::from_reader(reader)
									.with_context(|| format!("failed to parse file \"{}\"", &secret_path))?
							},
							_ => Err(anyhow!("malformed secret \"{}\"\n    expected argument \"str\" or \"js\" found \"{}\"", path, secret.args))?
						};
						if secrets.replace(path, value) {
							confs.generate_templates(&secrets, path, &sender).await?;
						}
					}
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
					// add file import resolver
					let library_paths = if let Some(ref jpath) = args.jpath {
						jpath.split(",").map(|s| PathBuf::from(s.trim())).collect()
					} else {
						vec![]
					};
					state.set_import_resolver(Box::new(FileImportResolver { library_paths }));
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
	env_logger::Env::default().default_filter_or("rconfd=info");
	env_logger::init();
	async_std::task::block_on(main_loop(&args))?;
	Ok(())
}
