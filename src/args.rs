use argh::{FromArgs, TopLevelCommand};
use std::env;
use std::path::Path;

/// Generate files from jsonnet templates and eventually keep them in sync with secrets fetched from a
/// vault server using a jwt token to authenticate with.
#[derive(FromArgs)]
pub struct Args {
	/// directory containing the rconfd config files (/etc/rconfd)
	#[argh(option, short = 'd', default = "\"/etc/rconfd\".to_owned()")]
	pub dir: String,

	/// the vault url ($VAULT_URL or https://localhost:8200/v1)
	#[argh(option, short = 'u', default = "default_url()")]
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

	/// the JWT token taken from the given variable name or from the given string if it fails (take precedence over -t)
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

/// returns the default vault url if not defined on command line argument
/// VAULT_URL or localhost if undefined
fn default_url() -> String {
	env::var("VAULT_URL")
		.ok()
		.or_else(|| Some("https://localhost:8200/v1".to_owned()))
		.unwrap()
}

/// copy of argh::from_env to insert command name and version in help text
pub fn from_env<T: TopLevelCommand>() -> T {
	const NAME: &'static str = env!("CARGO_BIN_NAME");
	const VERSION: &'static str = env!("CARGO_PKG_VERSION");
	let args: Vec<String> = std::env::args().collect();
	let cmd = Path::new(&args[0])
		.file_name()
		.map_or(None, |s| s.to_str())
		.unwrap_or(&args[0]);
	let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
	T::from_args(&[cmd], &args_str[1..]).unwrap_or_else(|early_exit| {
		println!("{} {}\n", NAME, VERSION);
		println!("{}", early_exit.output);
		std::process::exit(match early_exit.status {
			Ok(()) => 0,
			Err(()) => 1,
		})
	})
}
