#[cfg(feature = "nom")]
use nom::error::ErrorKind;
use std::fmt;

/// CustomError enum for clear error messages
#[derive(Debug, PartialEq)]
pub enum Error {
	UnknowBackend(String),
	NoBackend,
	NoArgs(String),
	NoPath(String),
	ExtraData(String),
	#[cfg(feature = "nom")]
	Nom(String, ErrorKind),
	#[cfg(feature = "nom")]
	Incomplete
}

impl std::error::Error for Error {}

/// Proper display of errors
impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Error::UnknowBackend(backend) => write!(f, "unknown backend \"{}\"", backend),
			Error::NoBackend => write!(f, "missing the backend argument"),
			Error::NoArgs(s) => {
				write!(
					f,
					"missing a \":\" to separate backend and arguments somewhere in \"{}\"",
					s
				)
			}
			Error::NoPath(s) => write!(
				f,
				"missing a \":\" to separate arguments and path somewhere in \"{}\"",
				s
			),
			Error::ExtraData(s) => write!(f, "extra data after path \"{}\"", s),
			#[cfg(feature = "nom")]
			Error::Nom(s, kind) => write!(
				f,
				"error with {} somewhere in \"{}\"",
				kind.description(),
				s
			),
			#[cfg(feature = "nom")]
			Error::Incomplete => write!(f, "incomplete data")
		}
	}
}

#[cfg(feature = "nom")]
impl From<nom::Err<Error>> for Error {
	fn from(e: nom::Err<Error>) -> Self {
		match e {
			nom::Err::Error(e) | nom::Err::Failure(e) => {
				e
			},
			nom::Err::Incomplete(_) => Error::Incomplete
		}
	}
}
