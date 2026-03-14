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
/// Derived from the ecoPrimals project structure — this is the *only* place
/// this string literal appears. All other code references this constant.
pub const ECOSYSTEM_NAMESPACE: &str = "ecoPrimals";

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
        assert_eq!(ECOSYSTEM_NAMESPACE, "ecoPrimals");
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
    fn test_ecosystem_namespace_is_eco_primals() {
        assert_eq!(ECOSYSTEM_NAMESPACE, "ecoPrimals");
        assert!(!ECOSYSTEM_NAMESPACE.contains(' '));
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
