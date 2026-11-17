//! Failures of a command, with the stable code and exit status scripts rely on.

use std::process::ExitCode;

use serde::Serialize;
use sheaf_core::{Error, NoteRef};

/// Documented in `help --json` and `docs/cli-schema/error.schema.json`.
pub const EXIT_CODES: [(u8, &str); 5] = [
    (0, "ok"),
    (1, "error"),
    (2, "usage"),
    (3, "not found"),
    (4, "ambiguous reference"),
];

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Core(#[from] Error),

    #[error("{0}")]
    Usage(String),

    #[error("{0}")]
    AppUnavailable(String),

    #[error("{0}")]
    ServiceUnavailable(String),
}

impl CliError {
    pub fn code(&self) -> &'static str {
        match self {
            CliError::Core(err) => err.code(),
            CliError::Usage(_) => "usage",
            CliError::AppUnavailable(_) => "app_unavailable",
            CliError::ServiceUnavailable(_) => "service_unavailable",
        }
    }

    pub fn exit_code(&self) -> ExitCode {
        ExitCode::from(match self {
            CliError::Usage(_) => 2,
            CliError::Core(Error::NotFound(_)) => 3,
            CliError::Core(Error::Ambiguous { .. }) => 4,
            _ => 1,
        })
    }

    pub fn candidates(&self) -> Option<&[NoteRef]> {
        match self {
            CliError::Core(Error::Ambiguous { candidates, .. }) => Some(candidates),
            _ => None,
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        CliError::Core(Error::Io(err))
    }
}

pub type Result<T = (), E = CliError> = std::result::Result<T, E>;

/// The body of the JSON error document.
#[derive(Serialize)]
pub struct ErrorBody<'a> {
    pub code: &'a str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidates: Option<&'a [NoteRef]>,
}

impl<'a> From<&'a CliError> for ErrorBody<'a> {
    fn from(err: &'a CliError) -> Self {
        ErrorBody {
            code: err.code(),
            message: err.to_string(),
            candidates: err.candidates(),
        }
    }
}
