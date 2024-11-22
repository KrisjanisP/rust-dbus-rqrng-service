// src/error.rs

use std::fmt;

/// Custom `Error` type for handling RNG-related errors.
#[derive(Debug)]
pub enum Error {
    /// Indicates that no positive errno was set.
    ErrnoNotPositive,
    /// Represents any unexpected error.
    Unexpected,
    /// Captures OS-specific error codes.
    OsError(u32),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ErrnoNotPositive => write!(f, "No positive errno set"),
            Error::Unexpected => write!(f, "Unexpected error occurred"),
            Error::OsError(code) => write!(f, "OS error with code: {}", code),
        }
    }
}

impl std::error::Error for Error {}

impl From<u32> for Error {
    fn from(code: u32) -> Self {
        Error::OsError(code)
    }
}

impl Error {
    /// Maps the custom `Error` type to specific status codes.
    pub fn to_status_code(&self) -> u32 {
        match self {
            Error::ErrnoNotPositive => 2,
            Error::Unexpected => 3,
            Error::OsError(_) => 1,
            // Add more mappings as needed
        }
    }
}
