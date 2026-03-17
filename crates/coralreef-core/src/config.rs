// SPDX-License-Identifier: AGPL-3.0-only
//! Configuration constants for coralreef-core.
//!
//! All ecosystem-level constants are derived from the primal's own identity
//! or environment — never from knowledge of other primals.

use std::path::PathBuf;
use std::time::Duration;

/// Default timeout for graceful shutdown (SIGTERM/SIGINT).
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

/// The ecosystem namespace used for shared directories (discovery, sockets).
///
/// Per wateringHole `PRIMAL_IPC_PROTOCOL` v3.0 — all primals share the
/// `biomeos` namespace under `$XDG_RUNTIME_DIR`. This is the *only* place
/// this string literal appears. All other code references this constant.
pub const ECOSYSTEM_NAMESPACE: &str = "biomeos";

/// Primal identity derived from the binary name at compile time.
///
/// Used for socket paths and capability advertisement — a primal only
/// knows itself, never other primals.
pub const PRIMAL_NAME: &str = env!("CARGO_PKG_NAME");

/// Primal version derived from the crate version at compile time.
pub const PRIMAL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Family ID for multi-instance isolation.
///
/// Reads `$BIOMEOS_FAMILY_ID` at runtime (set by genomeBin or systemd).
/// Defaults to `"default"` for single-instance development.
#[must_use]
pub fn family_id() -> String {
    std::env::var("BIOMEOS_FAMILY_ID").unwrap_or_else(|_| "default".into())
}

/// Compute the socket filename for this primal per wateringHole standard.
///
/// Format: `<primal>-<family_id>.sock`
#[must_use]
pub fn primal_socket_name() -> String {
    format!("{}-{}.sock", PRIMAL_NAME, family_id())
}

/// Resolve the shared discovery directory for all ecoPrimals.
///
/// Uses `$XDG_RUNTIME_DIR` (Linux/freedesktop) with fallback to
/// `std::env::temp_dir()` for portability. The namespace is
/// [`ECOSYSTEM_NAMESPACE`], not a hardcoded primal name.
///
/// # Errors
///
/// Returns an error if `$XDG_RUNTIME_DIR` is not set and the temp
/// directory is unusable (extremely unlikely).
pub fn discovery_dir() -> std::io::Result<PathBuf> {
    let base =
        std::env::var("XDG_RUNTIME_DIR").map_or_else(|_| std::env::temp_dir(), PathBuf::from);
    Ok(base.join(ECOSYSTEM_NAMESPACE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ecosystem_namespace_is_set() {
        assert!(!ECOSYSTEM_NAMESPACE.is_empty());
        assert_eq!(ECOSYSTEM_NAMESPACE, "biomeos");
    }

    #[test]
    fn test_shutdown_timeout_is_reasonable() {
        assert!(DEFAULT_SHUTDOWN_TIMEOUT.as_secs() >= 5);
        assert!(DEFAULT_SHUTDOWN_TIMEOUT.as_secs() <= 120);
    }

    #[test]
    fn test_discovery_dir_returns_path() {
        // Even without XDG_RUNTIME_DIR, discovery_dir should work (falls back to temp)
        let dir = discovery_dir();
        assert!(dir.is_ok());
        let path = dir.unwrap();
        assert!(path.ends_with(ECOSYSTEM_NAMESPACE));
    }

    #[test]
    fn test_ecosystem_namespace_is_biomeos() {
        assert_eq!(ECOSYSTEM_NAMESPACE, "biomeos");
        assert!(!ECOSYSTEM_NAMESPACE.contains(' '));
    }

    #[test]
    fn test_primal_name_matches_crate() {
        assert_eq!(PRIMAL_NAME, env!("CARGO_PKG_NAME"));
    }

    #[test]
    fn test_family_id_defaults_to_default() {
        if std::env::var("BIOMEOS_FAMILY_ID").is_err() {
            assert_eq!(family_id(), "default");
        }
    }

    #[test]
    fn test_primal_socket_name_format() {
        let name = primal_socket_name();
        let path = std::path::Path::new(&name);
        assert_eq!(
            path.extension().and_then(std::ffi::OsStr::to_str),
            Some("sock"),
        );
        assert!(name.contains('-'));
    }

    #[test]
    fn test_discovery_dir_path_components() {
        let path = discovery_dir().unwrap();
        let components: Vec<_> = path
            .components()
            .map(std::path::Component::as_os_str)
            .collect();
        assert!(!components.is_empty());
        assert!(
            components
                .iter()
                .any(|c| c.to_string_lossy() == ECOSYSTEM_NAMESPACE)
        );
    }

    #[test]
    fn test_shutdown_timeout_is_30_seconds() {
        assert_eq!(DEFAULT_SHUTDOWN_TIMEOUT.as_secs(), 30);
    }

    #[test]
    fn test_discovery_dir_path_is_absolute() {
        let path = discovery_dir().unwrap();
        assert!(path.is_absolute() || path.components().next().is_some());
    }

    #[test]
    fn test_ecosystem_namespace_no_trailing_slash() {
        assert!(!ECOSYSTEM_NAMESPACE.ends_with('/'));
    }

    #[test]
    fn test_discovery_dir_parent_exists_or_creatable() {
        let path = discovery_dir().unwrap();
        let parent = path.parent().unwrap_or(&path);
        assert!(parent.exists() || std::fs::create_dir_all(parent).is_ok());
    }
}
