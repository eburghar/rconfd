#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error(transparent)]
	VaultError(#[from] vault_jwt::error::Error),
	#[error("getting token {0}")]
	TokenError(#[from] std::io::Error),
	#[error("command \"{0}\" filed with code {1}:\n{2}")]
	CmdError(String, i32, String),
	#[error("missing role in {0}")]
	MissingRole(String),
	#[error(transparent)]
	ParseError(#[from] serde_json::error::Error),
	#[error("Expected argument {0} on {1}")]
	ExpectedArg(String, String),
	#[error("No matching }} found")]
	RightBrace,
	#[error("in \"{0}\", command \"{1}\" should be absolute and start with /")]
	RelativePath(String, String),
	#[error(transparent)]
	UnknownVar(#[from] std::env::VarError),
	#[error("json pointer \"{0}\" returns no result")]
	Pointer(String),
}

pub type Result<T> = std::result::Result<T, Error>;
