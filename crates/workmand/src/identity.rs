//! Stable and side-by-side development runtime identities.

use std::{env, ffi::OsStr, path::Path};

pub const STABLE_APPLICATION_NAME: &str = "workman";
pub const DEV_APPLICATION_NAME: &str = "workman-dev";
pub const STABLE_APP_BUNDLE_NAME: &str = "Workman.app";
pub const DEV_APP_BUNDLE_NAME: &str = "Workman Dev.app";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RuntimeIdentity {
    #[default]
    Stable,
    Dev,
}

impl RuntimeIdentity {
    pub fn current() -> Self {
        env::current_exe()
            .ok()
            .map_or(Self::Stable, |path| identity_from_executable(&path))
    }

    pub const fn is_dev(self) -> bool {
        matches!(self, Self::Dev)
    }

    pub const fn application_name(self) -> &'static str {
        match self {
            Self::Stable => STABLE_APPLICATION_NAME,
            Self::Dev => DEV_APPLICATION_NAME,
        }
    }

    pub const fn cli_binary_name(self) -> &'static str {
        match self {
            Self::Stable => "wrk",
            Self::Dev => "wrk-dev",
        }
    }

    pub const fn daemon_binary_name(self) -> &'static str {
        match self {
            Self::Stable => "workmand",
            Self::Dev => "workmand-dev",
        }
    }

    pub const fn app_bundle_name(self) -> &'static str {
        match self {
            Self::Stable => STABLE_APP_BUNDLE_NAME,
            Self::Dev => DEV_APP_BUNDLE_NAME,
        }
    }

    pub const fn app_bundle_identifier(self) -> &'static str {
        match self {
            Self::Stable => "com.workman.desktop",
            Self::Dev => "com.workman.dev",
        }
    }

    pub const fn mcp_server_name(self) -> &'static str {
        match self {
            Self::Stable => "workman",
            Self::Dev => "workman-dev",
        }
    }

    pub const fn mcp_authorization_env(self) -> &'static str {
        match self {
            Self::Stable => "WORKMAN_MCP_AUTHORIZATION",
            Self::Dev => "WORKMAN_DEV_MCP_AUTHORIZATION",
        }
    }
}

pub fn identity_from_executable(executable: &Path) -> RuntimeIdentity {
    let dev_binary = executable.file_stem().is_some_and(|name| {
        matches!(
            name.to_str(),
            Some("wrk-dev" | "workmand-dev" | "workman-desktop-dev")
        )
    });
    let dev_bundle = executable
        .ancestors()
        .any(|path| path.file_name() == Some(OsStr::new(DEV_APP_BUNDLE_NAME)));
    if dev_binary || dev_bundle {
        RuntimeIdentity::Dev
    } else {
        RuntimeIdentity::Stable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_identity_is_derived_from_binary_or_bundle_name() {
        assert_eq!(
            identity_from_executable(Path::new("/opt/workman/bin/wrk")),
            RuntimeIdentity::Stable
        );
        assert_eq!(
            identity_from_executable(Path::new("/opt/workman-dev/bin/wrk-dev")),
            RuntimeIdentity::Dev
        );
        assert_eq!(
            identity_from_executable(Path::new(
                "/Users/test/Applications/Workman Dev.app/Contents/MacOS/workman-desktop"
            )),
            RuntimeIdentity::Dev
        );
        assert_eq!(
            identity_from_executable(Path::new(
                "/Applications/Workman.app/Contents/MacOS/workman-desktop"
            )),
            RuntimeIdentity::Stable
        );
    }

    #[test]
    fn identities_have_non_overlapping_install_names() {
        let stable = RuntimeIdentity::Stable;
        let dev = RuntimeIdentity::Dev;
        assert_ne!(stable.application_name(), dev.application_name());
        assert_ne!(stable.cli_binary_name(), dev.cli_binary_name());
        assert_ne!(stable.daemon_binary_name(), dev.daemon_binary_name());
        assert_ne!(stable.app_bundle_name(), dev.app_bundle_name());
        assert_ne!(stable.app_bundle_identifier(), dev.app_bundle_identifier());
        assert_ne!(stable.mcp_server_name(), dev.mcp_server_name());
        assert_ne!(stable.mcp_authorization_env(), dev.mcp_authorization_env());
    }
}
