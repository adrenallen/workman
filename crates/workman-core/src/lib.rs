//! Shared domain and service code for workman.

pub mod agent_notifications;
pub mod attention;
pub mod domain;
pub mod locks;
pub mod notifications;
#[cfg(unix)]
mod output_spill;
/// Revision-guarded project scratchpads.
pub mod scratchpads;
pub mod store;
pub mod terminal;
pub mod todo_claims;
pub mod todos;
/// Authenticated release checks and verified atomic self-updates.
pub mod update;

#[cfg(unix)]
pub mod pty;

pub use agent_notifications::*;
pub use domain::*;
pub use locks::*;
pub use notifications::*;
pub use scratchpads::*;
pub use store::{LATEST_SCHEMA_VERSION, Store, StoreError, StoreResult};
pub use todo_claims::*;
pub use todos::*;
pub use update::*;

/// The user-facing project name.
pub const PROJECT_NAME: &str = "workman";
