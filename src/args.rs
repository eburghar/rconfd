use argh::{FromArgs, TopLevelCommand};
use std::path::Path;

/// Generate files from jsonnet templates and eventually keep them in sync with secrets fetched from a
/// vault server using a jwt token to authenticate with.
#[derive(FromArgs)]
pub struct Args {
	/// directory containing the rconfd config files (/etc/rconfd)
	#[argh(option, short = 'd', default = "\"/etc/rconfd\".to_owned()")]
	pub dir: String,

	/// the vault url (https://localhost:8200)
	#[argh(
		option,
		short = 'u',
		default = "\"https://localhost:8200/v1\".to_owned()"
	)]
	pub url: String,

	/// the login path (/auth/kubernetes/login)
	#[argh(option, short = 'l', default = "\"/auth/kubernetes/login\".to_owned()")]
	pub login_path: String,

	/// , separated list of aditional path for jsonnet libraries
	#[argh(option, short = 'j')]
	pub jpath: Option<String>,

	/// path of vault CA certificate (/var/run/secrets/kubernetes.io/serviceaccount/ca.crt)
	#[argh(
		option,
		short = 'c',
		default = "\"/var/run/secrets/kubernetes.io/serviceaccount/ca.crt\".to_owned()"
	)]
	pub cacert: String,

	/// the JWT token string (take precedence over -t)
	#[argh(option, short = 'T')]
	pub token: Option<String>,

	/// path of the JWT token (/var/run/secrets/kubernetes.io/serviceaccount/token)
	#[argh(
		option,
		short = 't',
		default = "\"/var/run/secrets/kubernetes.io/serviceaccount/token\".to_owned()"
	)]
	pub token_path: String,

	/// verbose mode
	#[argh(switch, short = 'v')]
	pub verbose: bool,

	/// s6 readiness file descriptor
	#[argh(option, short = 'r')]
	pub ready_fd: Option<i32>,

	/// daemon mode (stays in the foreground)
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
