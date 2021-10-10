mod args;
mod backend;
mod checksum;
mod conf;
mod libc;
mod message;
mod result;
mod s6;
mod secrets;
mod subst;
mod task;

use anyhow::Context;
use async_std::{channel::unbounded, stream::StreamExt};
use jrsonnet_evaluator::{
	trace::{CompactFormat, PathResolver},
	EvaluationState, FileImportResolver, ManifestFormat, Val,
};
use jrsonnet_interner::IStr;
use serde_json::{Map, Value};
use std::{
	convert::TryFrom,
	env,
	fs::{create_dir_all, File},
	io::{BufReader, Read, Write},
	os::unix::fs::PermissionsExt,
	path::PathBuf,
	process::Command,
	time::Duration,
};
use vault_jwt::{
	client::VaultClient,
	secret::{Secret, SecretPath},
};

use crate::{
	args::Args,
	backend::Backend,
	checksum::Checksums,
	conf::{config_files, parse_config, HookType, TemplateConfs},
	libc::User,
	message::{send_message, Message},
	result::Error,
	s6::s6_ready,
	secrets::Secrets,
	task::delay_task,
};

async fn main_loop(args: &Args) -> anyhow::Result<()> {
	// variables defining the state inside the main loop
	// if token given as argument, get the value from an envar with given name, or just use the string if it fails
	let jwt = if let Some(jwt) = &args.token {
		env::var(jwt).ok().or_else(|| Some(jwt.to_owned())).unwrap()
	// otherwise read from a file
	} else {
		let mut jwt = String::new();
		File::open(&args.token_path)
			.with_context(|| format!("opening {}", args.token_path))?
			.read_to_string(&mut jwt)
			.with_context(|| format!("reading {}", args.token_path))?;
		jwt
	};
	// trim jwt on both ends
	let jwt = jwt.trim();
	// initialize a vault client
	let mut client = VaultClient::new(&args.url, &args.login_path, jwt, Some(&args.cacert))?;
	// map secret path to secret value
	let mut secrets = Secrets::new();
	// map template name to template conf
	let mut confs = TemplateConfs::new();
	// map path to checksums
	let mut checksums = Checksums::new();
	// before first generate
	let mut first_run = true;
	// number of generated templates
	let mut generated = 0;
	// current user
	let current_user = User::current();

	// initialise mpsc channel
	let (sender, mut receiver) = unbounded::<Message>();

	// for each .json files in the conf directory
	let mut entries = config_files(&args.dir)?;
	// sort entries by lexicographic order so we can influence order of config processing
	entries.sort_unstable();
	for entry in entries.into_iter() {
		// parse config files
		log::info!("Loading {:?}", entry);
		let path = entry.as_path();
		let conf = parse_config(path).with_context(|| format!("Parsing {:?}", path))?;
		for (tmpl, conf) in conf {
			log::info!("  Parsing {:?}", &tmpl);
			// move conf to dedicated hashmap
			confs.insert(tmpl.clone(), conf);

			let secrets_map = &confs.get(&tmpl).unwrap().secrets;
			if secrets_map.is_empty() {
				// if no secrets generate template straight away
				log::debug!("empty GenerateTemplate({})", tmpl);
				sender.send(Message::GenerateTemplate(tmpl.clone())).await?;
			} else {
				// otherwise fetch all the secrets defined in the template config
				for (path, _) in secrets_map.iter() {
					// if we didn't already ask to get the secret
					if secrets.get(path).is_none() {
						// parse the secret
						let secret = SecretPath::<Backend>::try_from(path.as_str())
							.with_context(|| format!("failed to parse \"{}\"", path))?;
						if secret.backend == Backend::Vault {
							// ask the broker to login first
							sender
								.send(Message::Login(secret.args[0].to_owned()))
								.await?;
						}
						// intialize secret to None
						secrets.insert(path.clone(), None);
						// ask the broker to get the secret initial value without triggering manifestation
						sender
							.send(Message::GetSecret(path.to_owned(), false))
							.await?
					}
				}
			}
		}
	}
	// trigger manifestation now we asked the broker to fetch all secrets
	confs.generate_all_templates(&secrets, &sender).await?;

	// actor loop
	while let Some(msg) = receiver.next().await {
		match msg {
			Message::Login(role) => {
				// log in if not already logged in with that role
				if !client.is_logged(&role) {
					log::debug!("  Login({})", &role);
					let auth = client
						.login_async(&role)
						.await
						.with_context(|| format!("failed to login vault server {}", &args.url))?;
					// schedule a relogin login task at 2/3 of the lease_duration time
					if let Some(renew_delay) = auth.renew_delay() {
						log::debug!(
							"  logged in {} with role {}. Log in again within {:?}",
							&client.url,
							&role,
							renew_delay
						);
						delay_task(
							send_message(sender.clone(), Message::Login(role)),
							renew_delay,
						);
					}
				}
			}

			Message::GetSecret(path, gen_tmpl) => {
				// parse the secret again ? (yes it's cheap and contains only reference from path)
				let secret_path = SecretPath::<Backend>::try_from(path.as_str())
					.with_context(|| format!("failed to parse \"{}\"", path))?;
				// get the secret if not already fetched or if it's not valid or it it needs to be renewed
				if secrets
					.get(secret_path.path)
					.filter(|o| {
						o.as_ref()
							.filter(|s| s.is_valid() && !s.to_renew())
							.is_some()
					})
					.is_none()
				{
					log::debug!("  GetSecret({}, {})", &path, gen_tmpl);
					let role = secret_path
						.args
						.get(0)
						.ok_or_else(|| Error::MissingRole(format!("{}", path)))?;
					let method = secret_path
						.args
						.get(1)
						.unwrap_or(&"get")
						.to_ascii_uppercase();
					match secret_path.backend {
						Backend::Vault => {
							// fetch the secret
							let secret = client
								.get_secret_async(
									role,
									&method,
									secret_path.full_path,
									secret_path.kwargs.as_ref(),
								)
								.await
								.with_context(|| {
									format!(
										"failed to get the secret \"{}\"",
										secret_path.full_path
									)
								})?;

							// schedule the newewal of the secret which can trigger template generation
							if let Some(renew_delay) = secret.renew_delay() {
								log::debug!("  Renew secret within {:?}", renew_delay);
								delay_task(
									send_message(
										sender.clone(),
										Message::GetSecret(path.clone(), true),
									),
									renew_delay,
								);
							}

							// replace secret value an regenerate template if necessary
							if secrets.replace(&path, secret) && gen_tmpl {
								confs
									.generate_templates(&secrets, &path, &sender)
									.await?;
							}
						}

						Backend::Env => {
							let value = match secret_path.args[0] {
								"str" => Value::String(
									env::var(secret_path.full_path).unwrap_or("".to_owned()),
								),
								"js" => serde_json::from_str(
									&env::var(secret_path.full_path).unwrap_or("\"\"".to_owned()),
								)
								.with_context(|| {
									format!(
										"failed to parse \"{}\" variable content",
										secret_path.full_path
									)
								})?,
								_ => Err(Error::ExpectedArg(
									"\"str\" or \"js\"".to_owned(),
									secret_path.to_string(),
								))?,
							};
							if secrets.replace(&path, Secret::new(value, None))
								&& gen_tmpl
							{
								confs
									.generate_templates(&secrets, &path, &sender)
									.await?;
							}
						}

						Backend::File => {
							let mut file =
								File::open(secret_path.full_path).with_context(|| {
									format!("failed to open \"{}\"", secret_path.full_path)
								})?;

							let value = match secret_path.args[0] {
								"str" => {
									let mut buffer = String::new();
									file.read_to_string(&mut buffer).with_context(|| {
										format!("failed to read \"{}\"", secret_path.full_path)
									})?;
									Value::String(buffer)
								}
								"js" => {
									let reader = BufReader::new(file);
									serde_json::from_reader(reader).with_context(|| {
										format!(
											"failed to parse file \"{}\"",
											secret_path.full_path
										)
									})?
								}
								_ => Err(Error::ExpectedArg(
									"\"str\" or \"js\"".to_owned(),
									secret_path.to_string(),
								))?,
							};
							if secrets.replace(&path, Secret::new(value, None))
								&& gen_tmpl
							{
								confs
									.generate_templates(&secrets, &path, &sender)
									.await?;
							}
						}

						Backend::Exe => {
							let args: Vec<&str> =
								secret_path.full_path.split_whitespace().collect();
							// enforce absolute exec path for security reason
							if !args[0].starts_with("/") {
								Err(Error::RelativePath(path.to_string(), args[0].to_owned()))?;
							}
							// use sudo to drop privilege if uid is 0 before executing
							let mut cmd = &mut Command::new(if current_user.uid == 0 {
								"/usr/bin/sudo"
							} else {
								args[0]
							});
							if current_user.uid == 0 {
								log::debug!(
									"    executing \"{}\" as nobody",
									secret_path.full_path
								);
								cmd = cmd.args(&["-u", "nobody", args[0]]);
							}
							if args.len() > 1 {
								cmd = cmd.args(&args[1..]);
							}
							let output = cmd.output().with_context(|| {
								format!("error executing \"{}\"", secret_path.full_path)
							})?;
							if !output.status.success() {
								Err(Error::CmdError(
									secret_path.full_path.to_owned(),
									output.status.code().unwrap_or(1),
									String::from_utf8_lossy(&output.stderr).to_string(),
								))?;
							}
							let value = match secret_path.args[0] {
								"str" => Value::String(
									String::from_utf8_lossy(&output.stdout).trim().to_owned(),
								),
								"js" => serde_json::from_str(
									&env::var(secret_path.full_path).unwrap_or("\"\"".to_owned()),
								)
								.with_context(|| {
									format!(
										"failed to parse \"{}\" variable content",
										secret_path.full_path
									)
								})?,
								_ => Err(Error::ExpectedArg(
									"\"str\" or \"js\"".to_owned(),
									secret_path.to_string(),
								))?,
							};
							// secret declared as static (default) have no lease, whereas dynamic are invalid as soon as fetched (0s lease)
							let dur = match secret_path.args.get(1) {
								Some(s) => match *s {
									"static" => None,
									"dynamic" => Some(Duration::from_secs(0)),
									_ => Err(Error::ExpectedArg(
										"\"static\" or \"dynamic\"".to_owned(),
										secret_path.to_string(),
									))?,
								},
								_ => None,
							};
							if secrets.replace(&path, Secret::new(value, dur))
								&& gen_tmpl
							{
								confs
									.generate_templates(&secrets, &path, &sender)
									.await?;
							}
						}
					}
				}
			}

			Message::GenerateTemplate(tmpl) => {
				log::info!(
					"Manifestations of {} ({}/{})",
					&tmpl,
					generated + 1,
					confs.len()
				);
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

					// inject secret_key: secret_value in "secrets" extVar
					let mut secrets_val = Map::with_capacity(secrets.len());
					for (path, secret) in secrets.iter() {
    					log::debug!("  path: {}", path);
						// all secrets should have been fetched at that point so unwrap should not panic, otherwise it's a relevant panic
						let secret = secret.as_ref().unwrap();
						// add only the secrets declared in the template config
						if let Some(name) = conf.secrets.get(path) {
							secrets_val.insert(name.clone(), secret.value.clone());
						}
					}
					state.add_ext_var(
						IStr::from("secrets"),
						Val::from(&Value::Object(secrets_val)),
					);

					// prepend args.dir if the template path is relative
					let tmpl_path = if tmpl.starts_with("/") {
						PathBuf::from(tmpl)
					} else {
						PathBuf::from(&args.dir).join(tmpl)
					};

					// add the template file
					let val = state
						.evaluate_file_raw(&PathBuf::from(tmpl_path))
						.map_err(|e| anyhow::Error::msg(state.stringify_err(&e)))
						.with_context(|| "template error")?;

					// parse file mode
					let mode = u32::from_str_radix(&conf.mode, 8);
					if mode.is_err() {
						log::error!("Unable to parse file mode: {}", conf.mode);
					}

					// get user
					let user = User::new(&conf.user);
					if let Some(ref user) = user {
						if &current_user != user && current_user.gid != 0 {
							log::warn!("user \"{}\" is different than rconfd user which is unprivileged user", conf.user)
						}
					}

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
						log::info!("  {} generated", path.to_str().expect("path"));
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

					// if checksums changed and not on first run, then trigger modified hook
					if changes && !first_run {
						conf.hooks.trigger(HookType::MODIFIED);
					}

					// increment generated counter
					generated += 1;
					// if all templates have been generated
					if generated == confs.len() {
						// reset generated
						generated = 0;
						// first_run complete
						first_run = false;
						// signal s6 readiness that all config files have been generated
						s6_ready(args.ready_fd);
						// trigger ready hook if defined
						conf.hooks.trigger(HookType::READY);
						// quit if not in daemon mode or no dynamic secrets used among templates
						if !args.daemon || !secrets.any_leased() {
							if args.daemon {
								log::info!("Exiting daemon mode: no leased secrets used");
							}
							break;
						}
					}
				}
			}
		}
	}
	Ok(())
}

fn main() -> anyhow::Result<()> {
	// parse command line arguments
	let args: Args = args::from_env();

	// initialize env_logger in info mode for rconfd by default
	env_logger::init_from_env(env_logger::Env::new().default_filter_or("rconfd=info"));
	async_std::task::block_on(main_loop(&args))?;
	Ok(())
}
