#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error(transparent)]
	Vault(#[from] vault_jwt::error::Error),
	#[error("command \"{0}\" filed with code {1}:\n{2}")]
	Cmd(String, i32, String),
	#[error("missing role in {0}")]
	MissingRole(String),
	#[error(transparent)]
	Parse(#[from] serde_json::error::Error),
	#[error("Expected argument {0} on {1}")]
	ExpectedArg(String, String),
	#[error("No matching }} found")]
	RightBrace,
	#[error("in \"{0}\", command \"{1}\" should be absolute and start with /")]
	RelativePath(String, String),
	#[error("{1}: {0}")]
	UnknownVar(String, #[source] std::env::VarError),
}

pub type Result<T> = std::result::Result<T, Error>;
