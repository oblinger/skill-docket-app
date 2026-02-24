use std::fmt;
use std::path::PathBuf;

use crate::skill::types::SkillParseError;

// ---------------------------------------------------------------------------
// Library errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum LibraryError {
    /// A source path was expected to exist but doesn't.
    SourceNotFound(PathBuf),
    /// No skill with the given name was found in the resolved library.
    SkillNotFound(String),
    /// A skill file could not be parsed.
    ParseError {
        skill: String,
        error: SkillParseError,
    },
    /// Filesystem I/O error.
    IoError(std::io::Error),
    /// Configuration/settings error.
    SettingsError(String),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LibraryError::SourceNotFound(path) => {
                write!(f, "library source not found: {}", path.display())
            }
            LibraryError::SkillNotFound(name) => {
                write!(f, "skill not found: {}", name)
            }
            LibraryError::ParseError { skill, error } => {
                write!(f, "failed to parse skill '{}': {}", skill, error)
            }
            LibraryError::IoError(e) => write!(f, "I/O error: {}", e),
            LibraryError::SettingsError(msg) => {
                write!(f, "settings error: {}", msg)
            }
        }
    }
}

impl std::error::Error for LibraryError {}

impl From<std::io::Error> for LibraryError {
    fn from(e: std::io::Error) -> Self {
        LibraryError::IoError(e)
    }
}
