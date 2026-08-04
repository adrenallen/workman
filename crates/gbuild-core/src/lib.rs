//! Shared domain and service code for gbuild.

pub mod attention;
pub mod domain;
pub mod locks;
/// Revision-guarded project scratchpads.
pub mod scratchpads;
pub mod store;
pub mod terminal;
pub mod todos;

#[cfg(unix)]
pub mod pty;

pub use domain::*;
pub use locks::*;
pub use scratchpads::*;
pub use store::{LATEST_SCHEMA_VERSION, Store, StoreError, StoreResult};
pub use todos::*;

/// The user-facing project name.
pub const PROJECT_NAME: &str = "gbuild";
