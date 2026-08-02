//! The crate's [`Error`] type, returned by the pipeline and every stage.

use std::fmt;

/// Errors produced by the framework stages and the pipeline driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The input image had zero width or height.
    EmptyImage,
    /// Transparency keying was requested but no unused key color could be found.
    NoKeyColor,
    /// A requested feature is recognized but not yet implemented.
    Unsupported(String),
    /// The run was aborted via a [`crate::progress::CancelToken`].
    Cancelled,
    /// Any other failure, carrying a human-readable message.
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::EmptyImage => write!(f, "input image is empty"),
            Error::NoKeyColor => {
                write!(f, "unable to find an unused color in image to use as key")
            }
            Error::Unsupported(what) => write!(f, "unsupported: {what}"),
            Error::Cancelled => write!(f, "conversion cancelled"),
            Error::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(msg: String) -> Self {
        Error::Other(msg)
    }
}

impl From<&str> for Error {
    fn from(msg: &str) -> Self {
        Error::Other(msg.to_string())
    }
}
