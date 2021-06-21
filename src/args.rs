use argh::{FromArgs, TopLevelCommand};
use std::path::Path;

/// Generate config files from jsonnet templates and keep them in sync with secrets fetched from a
/// vault server with kubernetes authentication.
#[derive(FromArgs)]
pub struct Args {
	/// directory containing the rconfd config files
	#[argh(option, short = 'd')]
	pub dir: String,

	/// the vault url (https://localhost:8200)
	#[argh(option, short = 'u', default = "\"https://localhost:8200\".to_owned()")]
	pub url: String,

	/// , separated list of aditional path for jsonnet libraries
	#[argh(option, short = 'j')]
	pub jpath: Option<String>,

	/// path of the service account certificate	(/var/run/secrets/kubernetes.io/serviceaccount/ca.crt)
	#[argh(
		option,
		short = 'c',
		default = "\"/var/run/secrets/kubernetes.io/serviceaccount/ca.crt\".to_owned()"
	)]
	pub cacert: String,

	/// path of the kubernetes token (/var/run/secrets/kubernetes.io/serviceaccount/token)
	#[argh(
		option,
		short = 't',
		default = "\"/var/run/secrets/kubernetes.io/serviceaccount/token\".to_owned()"
	)]
	pub token: String,

	/// verbose mode
	#[argh(switch, short = 'V')]
	pub verbose: bool,

	/// s6 readiness file descriptor
	#[argh(option, short = 'r')]
	pub ready_fd: Option<i32>,

	/// daemon mode (no detach)
	#[argh(switch, short = 'D')]
	pub daemon: bool,
}

fn cmd<'a>(default: &'a String, path: &'a String) -> &'a str {
	Path::new(path)
		.file_name()
		.map(|s| s.to_str())
		.flatten()
		.unwrap_or(default.as_str())
}

/// copy of argh::from_env to insert command name and version
pub fn from_env<T: TopLevelCommand>() -> T {
	const NAME: &'static str = env!("CARGO_PKG_NAME");
	const VERSION: &'static str = env!("CARGO_PKG_VERSION");
	let strings: Vec<String> = std::env::args().collect();
	let cmd = cmd(&strings[0], &strings[0]);
	let strs: Vec<&str> = strings.iter().map(|s| s.as_str()).collect();
	T::from_args(&[cmd], &strs[1..]).unwrap_or_else(|early_exit| {
		println!("{} {}\n", NAME, VERSION);
		println!("{}", early_exit.output);
		std::process::exit(match early_exit.status {
			Ok(()) => 0,
			Err(()) => 1,
		})
	})
}
