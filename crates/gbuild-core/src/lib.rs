//! Shared domain and service code for gbuild.

pub mod domain;
pub mod store;
pub mod terminal;

#[cfg(unix)]
pub mod pty;

pub use domain::*;
pub use store::{LATEST_SCHEMA_VERSION, Store, StoreError, StoreResult};

/// The user-facing project name.
pub const PROJECT_NAME: &str = "gbuild";
