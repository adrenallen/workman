//! Build and control-protocol identity shared by daemon clients.

use serde::{Deserialize, Serialize};

/// Protocol revision for the authenticated WebSocket control channel.
// WRU1 terminal frames distinguish human input from terminal protocol replies.
pub const CONTROL_PROTOCOL_VERSION: u32 = 4;

/// Cargo package version of this build.
pub const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Source revision embedded by `build.rs` (or `WORKMAN_BUILD_ID` for packaged builds).
pub const BUILD_ID: &str = env!("WORKMAN_BUILD_ID");

/// Identity returned by the `daemon.hello` handshake and daemon status.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonVersion {
    pub version: String,
    pub build_id: String,
    pub control_protocol_version: u32,
}

impl DaemonVersion {
    pub fn current() -> Self {
        Self {
            version: BUILD_VERSION.to_owned(),
            build_id: BUILD_ID.to_owned(),
            control_protocol_version: CONTROL_PROTOCOL_VERSION,
        }
    }

    pub fn matches_current_build(&self) -> bool {
        self.version == BUILD_VERSION
            && self.build_id == BUILD_ID
            && self.control_protocol_version == CONTROL_PROTOCOL_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_is_compatible_and_other_builds_are_not() {
        let current = DaemonVersion::current();
        assert!(current.matches_current_build());

        let mut different_version = current.clone();
        different_version.version.push_str("-other");
        assert!(!different_version.matches_current_build());

        let mut different_build = current.clone();
        different_build.build_id.push_str("-other");
        assert!(!different_build.matches_current_build());

        let mut different_protocol = current;
        different_protocol.control_protocol_version =
            different_protocol.control_protocol_version.wrapping_add(1);
        assert!(!different_protocol.matches_current_build());
    }
}
