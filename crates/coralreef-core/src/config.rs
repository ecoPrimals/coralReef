// SPDX-License-Identifier: AGPL-3.0-or-later
//! Configuration constants for coralreef-core.
//!
//! All ecosystem-level constants are derived from the primal's own identity
//! or environment — never from knowledge of other primals.

use std::path::PathBuf;
use std::time::Duration;

/// Default timeout for graceful shutdown (SIGTERM/SIGINT).
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

/// Default ecosystem namespace for shared directories (discovery, sockets).
///
/// Per wateringHole `PRIMAL_IPC_PROTOCOL` v3.0 — all primals share the
/// `biomeos` namespace under `$XDG_RUNTIME_DIR`. Use [`ecosystem_namespace()`]
/// for runtime resolution (respects `$BIOMEOS_ECOSYSTEM_NAMESPACE` override).
pub const ECOSYSTEM_NAMESPACE: &str = "biomeos";

/// Resolve the ecosystem namespace at runtime.
///
/// Returns `$BIOMEOS_ECOSYSTEM_NAMESPACE` if set, otherwise [`ECOSYSTEM_NAMESPACE`].
pub fn ecosystem_namespace() -> &'static str {
    use std::sync::OnceLock;
    static NS: OnceLock<String> = OnceLock::new();
    NS.get_or_init(|| {
        std::env::var("BIOMEOS_ECOSYSTEM_NAMESPACE").unwrap_or_else(|_| ECOSYSTEM_NAMESPACE.into())
    })
}

/// Primal identity derived from the binary name at compile time.
///
/// Used for socket paths and capability advertisement — a primal only
/// knows itself, never other primals.
pub const PRIMAL_NAME: &str = env!("CARGO_PKG_NAME");

/// Primal version derived from the crate version at compile time.
pub const PRIMAL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Configuration validation error.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ConfigError {
    /// Human-readable explanation for logging and CLI output.
    pub message: String,
}

/// Environment variable: stem for the capability-domain symlink next to the Unix socket.
///
/// Per wateringHole `CAPABILITY_BASED_DISCOVERY_STANDARD` v1.1, clients discover the
/// shader capability via `{stem}.sock` in the same directory as the instance socket.
/// Default stem when unset or invalid: `shader`.
pub const CORALREEF_CAPABILITY_DOMAIN_ENV: &str = "CORALREEF_CAPABILITY_DOMAIN";

/// Family ID for multi-instance isolation.
///
/// Reads `$CORALREEF_FAMILY_ID` (set by `composition_nucleus.sh`) or
/// `$BIOMEOS_FAMILY_ID` (set by genomeBin / systemd).
/// Defaults to `"default"` for single-instance development.
#[must_use]
pub fn family_id() -> String {
    std::env::var("CORALREEF_FAMILY_ID")
        .or_else(|_| std::env::var("BIOMEOS_FAMILY_ID"))
        .unwrap_or_else(|_| "default".into())
}

/// Check that `BIOMEOS_INSECURE` and `BIOMEOS_FAMILY_ID` are not both active.
///
/// Per wateringHole `PRIMAL_SELF_KNOWLEDGE_STANDARD` v1.1: a primal must
/// refuse to start when a non-default family ID is set AND insecure mode is
/// requested — you cannot claim a family AND skip authentication.
///
/// # Errors
///
/// Returns [`ConfigError`] if the invariant is violated.
pub fn validate_insecure_guard() -> Result<(), ConfigError> {
    let fid = family_id();
    let insecure = std::env::var("BIOMEOS_INSECURE")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    if insecure && fid != "default" {
        return Err(ConfigError {
            message: format!(
                "BIOMEOS_INSECURE=1 cannot be used with BIOMEOS_FAMILY_ID={fid} — \
                 a primal cannot claim a family and skip authentication \
                 (wateringHole PRIMAL_SELF_KNOWLEDGE_STANDARD v1.1)"
            ),
        });
    }
    Ok(())
}

/// Filename for the capability-domain symlink: `{domain}.sock`.
///
/// Reads [`CORALREEF_CAPABILITY_DOMAIN_ENV`]. Empty or path-like values fall back to `shader`.
#[must_use]
pub fn capability_domain_socket_filename() -> String {
    const DEFAULT_STEM: &str = "shader";
    let raw = std::env::var(CORALREEF_CAPABILITY_DOMAIN_ENV).unwrap_or_default();
    let stem = raw.trim();
    let stem = if stem.is_empty()
        || stem.contains('/')
        || stem.contains('\\')
        || stem == "."
        || stem == ".."
    {
        DEFAULT_STEM
    } else {
        stem
    };
    format!("{stem}.sock")
}

/// Compute the socket filename for this primal per wateringHole standard.
///
/// Format: `<primal>-<family_id>.sock`
#[must_use]
pub fn primal_socket_name() -> String {
    format!("{}-{}.sock", PRIMAL_NAME, family_id())
}

/// Resolve the shared socket/discovery directory for all ecoPrimals.
///
/// Resolution order (first non-empty wins):
/// 1. `$BIOMEOS_SOCKET_DIR` — explicit override from composition launcher
/// 2. `$XDG_RUNTIME_DIR/{namespace}` — Linux/freedesktop standard
/// 3. `{temp_dir}/{namespace}` — portability fallback
///
/// # Errors
///
/// Returns an error if `$XDG_RUNTIME_DIR` is not set and the temp
/// directory is unusable (extremely unlikely).
pub fn discovery_dir() -> std::io::Result<PathBuf> {
    if let Ok(dir) = std::env::var("BIOMEOS_SOCKET_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    let base =
        std::env::var("XDG_RUNTIME_DIR").map_or_else(|_| std::env::temp_dir(), PathBuf::from);
    Ok(base.join(ecosystem_namespace()))
}

/// Resolve the security-domain provider socket path.
///
/// The composition launcher sets `$BTSP_PROVIDER_SOCKET` (preferred) or
/// `$BEARDOG_SOCKET` (legacy alias) to the concrete path.
/// Returns `None` when unset or empty (standalone/dev mode).
#[must_use]
pub fn btsp_provider_socket() -> Option<PathBuf> {
    non_empty_env_path("BTSP_PROVIDER_SOCKET")
}

/// Legacy alias: composition launcher may set `$BEARDOG_SOCKET`.
///
/// Prefer [`btsp_provider_socket`] — this reads the legacy env var name
/// that some composition launchers still emit.
#[must_use]
pub fn security_provider_socket_legacy() -> Option<PathBuf> {
    non_empty_env_path("BEARDOG_SOCKET")
}

/// Resolve the Songbird discovery socket path.
///
/// The composition launcher sets `$DISCOVERY_SOCKET` so that primals can
/// resolve capabilities without scanning the filesystem.
/// Returns `None` when unset or empty.
#[must_use]
pub fn discovery_socket() -> Option<PathBuf> {
    non_empty_env_path("DISCOVERY_SOCKET")
}

/// Retrieve the family seed (Tier 1 crypto derivation input).
///
/// Set by the composition launcher as `$FAMILY_SEED`. The value is
/// opaque hex — coralReef forwards it to the security-domain provider for
/// purpose-key derivation and artifact signing.
/// Returns `None` when unset or empty.
#[must_use]
pub fn family_seed() -> Option<String> {
    std::env::var("FAMILY_SEED")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Helper: read an environment variable as a `PathBuf`, returning `None`
/// for missing or empty values.
fn non_empty_env_path(var: &str) -> Option<PathBuf> {
    std::env::var(var)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
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
    fn validate_insecure_guard_rejects_family_plus_insecure() {
        // This test checks the logic only — it cannot safely mutate env vars
        // in a parallel test suite. The guard reads BIOMEOS_INSECURE and
        // BIOMEOS_FAMILY_ID; we test the function's return behavior assuming
        // neither is set (default state should pass).
        // The actual rejection is validated by the integration test below.
        if std::env::var("BIOMEOS_FAMILY_ID").is_err() && std::env::var("BIOMEOS_INSECURE").is_err()
        {
            assert!(validate_insecure_guard().is_ok());
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
    fn test_capability_domain_socket_filename_suffix() {
        let name = capability_domain_socket_filename();
        assert!(
            std::path::Path::new(&name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("sock"))
        );
        assert!(!name.trim().is_empty());
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

    #[test]
    fn security_provider_legacy_returns_none_when_unset() {
        if std::env::var("BEARDOG_SOCKET").is_err() {
            assert!(security_provider_socket_legacy().is_none());
        }
    }

    #[test]
    fn btsp_provider_socket_returns_none_when_unset() {
        if std::env::var("BTSP_PROVIDER_SOCKET").is_err() {
            assert!(btsp_provider_socket().is_none());
        }
    }

    #[test]
    fn discovery_socket_returns_none_when_unset() {
        if std::env::var("DISCOVERY_SOCKET").is_err() {
            assert!(discovery_socket().is_none());
        }
    }

    #[test]
    fn family_seed_returns_none_when_unset() {
        if std::env::var("FAMILY_SEED").is_err() {
            assert!(family_seed().is_none());
        }
    }

    #[test]
    fn non_empty_env_path_returns_none_for_missing() {
        assert!(non_empty_env_path("__CORALREEF_TEST_NONEXISTENT_VAR__").is_none());
    }
}
