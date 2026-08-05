//! Shared domain and service code for workman.

pub mod attention;
pub mod domain;
pub mod locks;
#[cfg(unix)]
mod output_spill;
/// Revision-guarded project scratchpads.
pub mod scratchpads;
pub mod store;
pub mod terminal;
pub mod todos;
/// Authenticated release checks and verified atomic self-updates.
pub mod update;

#[cfg(unix)]
pub mod pty;

pub use domain::*;
pub use locks::*;
pub use scratchpads::*;
pub use store::{LATEST_SCHEMA_VERSION, Store, StoreError, StoreResult};
pub use todos::*;
pub use update::*;

/// The user-facing project name.
pub const PROJECT_NAME: &str = "workman";
