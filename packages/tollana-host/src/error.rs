use std::fmt;
use tollana_core::{HostInterfaceError, IdentityError, SnapshotError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostError {
    pub message: String,
}

impl HostError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for HostError {}

impl From<HostInterfaceError> for HostError {
    fn from(e: HostInterfaceError) -> Self {
        Self::new(e.to_string())
    }
}

impl From<IdentityError> for HostError {
    fn from(e: IdentityError) -> Self {
        Self::new(e.message)
    }
}

impl From<SnapshotError> for HostError {
    fn from(e: SnapshotError) -> Self {
        Self::new(e.message)
    }
}

impl From<serde_json::Error> for HostError {
    fn from(e: serde_json::Error) -> Self {
        Self::new(e.to_string())
    }
}
