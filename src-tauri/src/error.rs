//! One error type for every command, serialised to the frontend as a string.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum LauncherError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("network: {0}")]
    Http(#[from] reqwest::Error),

    #[error("config: {0}")]
    Config(String),

    #[error("{0}")]
    Message(String),
}

impl Serialize for LauncherError {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl From<String> for LauncherError {
    fn from(s: String) -> Self {
        LauncherError::Message(s)
    }
}

impl From<&str> for LauncherError {
    fn from(s: &str) -> Self {
        LauncherError::Message(s.to_string())
    }
}

pub type Result<T> = std::result::Result<T, LauncherError>;
