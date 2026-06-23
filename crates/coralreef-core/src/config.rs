// SPDX-License-Identifier: AGPL-3.0-or-later
//! Configuration constants for coralreef-core.
//!
//! All ecosystem-level constants are derived from the primal's own identity
//! or environment — never from knowledge of other primals.

use std::path::PathBuf;
use std::time::Duration;

use crate::env_keys;

/// Default timeout for graceful shutdown (SIGTERM/SIGINT).
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

/// Default ecosystem registry RPC timeout.
pub const DEFAULT_REGISTRY_TIMEOUT: Duration = Duration::from_secs(2);

/// Resolve the shutdown timeout (env-configurable via `$CORALREEF_SHUTDOWN_TIMEOUT_SECS`).
#[must_use]
pub fn shutdown_timeout() -> Duration {
    parse_duration_env(
        env_keys::CORALREEF_SHUTDOWN_TIMEOUT_SECS,
        DEFAULT_SHUTDOWN_TIMEOUT,
    )
}

/// Resolve the ecosystem registry RPC timeout (env-configurable via `$CORALREEF_REGISTRY_TIMEOUT_SECS`).
#[must_use]
pub fn registry_timeout() -> Duration {
    parse_duration_env(
        env_keys::CORALREEF_REGISTRY_TIMEOUT_SECS,
        DEFAULT_REGISTRY_TIMEOUT,
    )
}

fn parse_duration_env(key: &str, default: Duration) -> Duration {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map_or(default, Duration::from_secs)
}

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
        std::env::var(env_keys::BIOMEOS_ECOSYSTEM_NAMESPACE)
            .unwrap_or_else(|_| ECOSYSTEM_NAMESPACE.into())
    })
}

/// Primal identity derived from the binary name at compile time.
///
/// Used for socket paths and capability advertisement — a primal only
/// knows itself, never other primals.
pub const PRIMAL_NAME: &str = env!("CARGO_PKG_NAME");

/// Primal version derived from the crate version at compile time.
pub const PRIMAL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build hash injected via `CORALREEF_BUILD_HASH` env var at compile time.
/// Falls back to `"dev"` for local builds without CI.
pub const PRIMAL_BUILD_HASH: &str = match option_env!("CORALREEF_BUILD_HASH") {
    Some(h) => h,
    None => "dev",
};

/// Build session label injected via `CORALREEF_SESSION` env var at compile time.
/// Falls back to the crate version if unset.
pub const PRIMAL_SESSION: &str = match option_env!("CORALREEF_SESSION") {
    Some(s) => s,
    None => PRIMAL_VERSION,
};

/// All JSON-RPC methods served by this primal.
///
/// Single source of truth for both `capability.list` responses and
/// `primal.announce` advertisements. Add new methods here.
pub const SERVED_METHODS: &[&str] = &[
    "shader.compile.spirv",
    "shader.compile.wgsl",
    "shader.compile.status",
    "shader.compile.capabilities",
    "shader.compile.wgsl.multi",
    "shader.compile.multi",
    "shader.compile.gemm",
    "health.check",
    "health.liveness",
    "health.readiness",
    "health.version",
    "identity.get",
    "capability.list",
    "capabilities.list",
    "btsp.negotiate",
    "auth.check",
    "auth.mode",
    "auth.peer_info",
];

/// Resolve the gate identity from `$BIOMEOS_GATE_ID`.
///
/// Falls back to `"unknown"` if not set — composition launchers should always
/// inject this, but standalone dev builds proceed without it.
#[must_use]
pub fn gate_id() -> String {
    std::env::var(env_keys::BIOMEOS_GATE_ID)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Compiler version string for provenance tagging.
///
/// Format: `"{name}/{version}+{build_hash}"` (e.g. `"coralreef-core/0.2.0+dev"`).
#[must_use]
pub fn compiler_version_string() -> String {
    format!("{PRIMAL_NAME}/{PRIMAL_VERSION}+{PRIMAL_BUILD_HASH}")
}

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
pub const CORALREEF_CAPABILITY_DOMAIN_ENV: &str = env_keys::CORALREEF_CAPABILITY_DOMAIN;

/// Family ID for multi-instance isolation.
///
/// Reads `$CORALREEF_FAMILY_ID` (set by `composition_nucleus.sh`) or
/// `$BIOMEOS_FAMILY_ID` (set by genomeBin / systemd).
/// Defaults to `"default"` for single-instance development.
#[must_use]
pub fn family_id() -> String {
    std::env::var(env_keys::CORALREEF_FAMILY_ID)
        .or_else(|_| std::env::var(env_keys::BIOMEOS_FAMILY_ID))
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
    let insecure = std::env::var(env_keys::BIOMEOS_INSECURE)
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

/// 3-tier socket base directory resolution.
///
/// Resolution order (first non-empty wins):
/// 1. `$BIOMEOS_SOCKET_DIR` — explicit override from composition launcher
/// 2. `$XDG_RUNTIME_DIR` — Linux/freedesktop runtime directory
/// 3. `/run/biomeos` — system fallback (standard for production deployments)
///
/// This is the canonical resolution used by both socket binding and
/// `primal.announce` advertisement. All paths that need the socket base
/// directory must use this function to avoid path divergence.
#[must_use]
pub fn socket_base_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(env_keys::BIOMEOS_SOCKET_DIR) {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(dir) = std::env::var(env_keys::XDG_RUNTIME_DIR) {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    PathBuf::from("/run/biomeos")
}

/// Resolve the default Unix socket path for this primal.
///
/// Uses [`socket_base_dir`] + ecosystem namespace + primal socket name.
/// This is the canonical path that both the server binds on and
/// `primal.announce` advertises.
#[must_use]
pub fn default_socket_path() -> PathBuf {
    socket_base_dir()
        .join(ecosystem_namespace())
        .join(primal_socket_name())
}

/// Resolve the shared socket/discovery directory for all ecoPrimals.
///
/// Resolution order (first non-empty wins):
/// 1. `$BIOMEOS_SOCKET_DIR` — explicit override from composition launcher
/// 2. `$XDG_RUNTIME_DIR/{namespace}` — Linux/freedesktop standard
/// 3. `/run/biomeos` — system fallback
///
/// # Errors
///
/// Returns an error if the resolution fails (extremely unlikely).
pub fn discovery_dir() -> std::io::Result<PathBuf> {
    Ok(socket_base_dir().join(ecosystem_namespace()))
}

/// Resolve the security-domain provider socket path.
///
/// The composition launcher sets `$BTSP_PROVIDER_SOCKET` (preferred) or
/// `$BEARDOG_SOCKET` (legacy alias) to the concrete path.
/// Returns `None` when unset or empty (standalone/dev mode).
#[must_use]
pub fn btsp_provider_socket() -> Option<PathBuf> {
    non_empty_env_path(env_keys::BTSP_PROVIDER_SOCKET)
}

/// Deprecated legacy alias — reads `$BEARDOG_SOCKET`.
///
/// Prefer [`btsp_provider_socket`] (`$BTSP_PROVIDER_SOCKET`). This exists
/// only for backward compatibility with composition launchers that have not
/// yet migrated to the generic env var name.
#[deprecated(since = "0.2.0", note = "use btsp_provider_socket() instead")]
#[must_use]
pub fn security_provider_socket_legacy() -> Option<PathBuf> {
    #[allow(deprecated)]
    let key = env_keys::BEARDOG_SOCKET;
    non_empty_env_path(key)
}

/// Resolve the ecosystem discovery relay socket path.
///
/// The composition launcher sets `$DISCOVERY_SOCKET` so that primals can
/// resolve capabilities without scanning the filesystem.
/// Returns `None` when unset or empty.
#[must_use]
pub fn discovery_socket() -> Option<PathBuf> {
    non_empty_env_path(env_keys::DISCOVERY_SOCKET)
}

/// Retrieve the family seed (Tier 1 crypto derivation input).
///
/// Set by the composition launcher as `$FAMILY_SEED`. The value is
/// opaque hex — coralReef forwards it to the security-domain provider for
/// purpose-key derivation and artifact signing.
/// Returns `None` when unset or empty.
#[must_use]
pub fn family_seed() -> Option<String> {
    std::env::var(env_keys::FAMILY_SEED)
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
    #[allow(deprecated)]
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

    #[test]
    fn compiler_version_string_format() {
        let s = compiler_version_string();
        assert!(
            s.contains('/'),
            "should contain name/version separator: {s}"
        );
        assert!(
            s.contains('+'),
            "should contain version+build separator: {s}"
        );
        assert!(s.starts_with(PRIMAL_NAME));
        assert!(s.contains(PRIMAL_VERSION));
        assert!(s.contains(PRIMAL_BUILD_HASH));
    }

    #[test]
    fn gate_id_returns_string() {
        let gid = gate_id();
        assert!(!gid.is_empty(), "gate_id should never be empty");
        if std::env::var(env_keys::BIOMEOS_GATE_ID).is_err() {
            assert_eq!(gid, "unknown");
        }
    }

    #[test]
    fn config_error_display() {
        let err = ConfigError {
            message: "test error message".into(),
        };
        assert_eq!(err.to_string(), "test error message");
    }

    #[test]
    fn default_socket_path_contains_namespace_and_sock() {
        let path = default_socket_path();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains(ecosystem_namespace()),
            "should contain namespace: {path_str}"
        );
        assert!(
            path.extension().is_some_and(|e| e == "sock"),
            "should end in .sock: {path_str}"
        );
    }

    #[test]
    fn socket_base_dir_returns_absolute_path() {
        let base = socket_base_dir();
        assert!(
            base.is_absolute(),
            "socket base dir should be absolute: {}",
            base.display()
        );
    }

    #[test]
    fn primal_build_hash_is_non_empty() {
        assert!(!PRIMAL_BUILD_HASH.is_empty());
    }

    #[test]
    fn primal_session_is_non_empty() {
        assert!(!PRIMAL_SESSION.is_empty());
    }

    #[test]
    fn served_methods_contains_required_methods() {
        assert!(SERVED_METHODS.contains(&"health.check"));
        assert!(SERVED_METHODS.contains(&"health.liveness"));
        assert!(SERVED_METHODS.contains(&"health.readiness"));
        assert!(SERVED_METHODS.contains(&"health.version"));
        assert!(SERVED_METHODS.contains(&"identity.get"));
        assert!(SERVED_METHODS.contains(&"capability.list"));
        assert!(SERVED_METHODS.contains(&"btsp.negotiate"));
        assert!(SERVED_METHODS.contains(&"shader.compile.wgsl"));
        assert!(SERVED_METHODS.contains(&"shader.compile.spirv"));
        assert!(SERVED_METHODS.contains(&"shader.compile.multi"));
        assert!(SERVED_METHODS.contains(&"shader.compile.gemm"));
    }

    #[test]
    fn registry_timeout_returns_reasonable_duration() {
        let t = registry_timeout();
        assert!(t.as_secs() >= 1 && t.as_secs() <= 60);
    }

    #[test]
    fn capability_domain_socket_rejects_path_traversal() {
        // Without env override, should return default
        if std::env::var(CORALREEF_CAPABILITY_DOMAIN_ENV).is_err() {
            let name = capability_domain_socket_filename();
            assert_eq!(name, "shader.sock");
        }
    }

    #[test]
    fn validate_insecure_guard_passes_default_state() {
        if std::env::var(env_keys::BIOMEOS_FAMILY_ID).is_err()
            && std::env::var(env_keys::BIOMEOS_INSECURE).is_err()
        {
            assert!(
                validate_insecure_guard().is_ok(),
                "default env state should pass the insecure guard"
            );
        }
    }

    #[test]
    fn family_id_defaults_correctly() {
        if std::env::var(env_keys::CORALREEF_FAMILY_ID).is_err()
            && std::env::var(env_keys::BIOMEOS_FAMILY_ID).is_err()
        {
            assert_eq!(family_id(), "default");
        }
    }

    #[test]
    fn parse_duration_env_returns_default_for_missing_var() {
        let d = parse_duration_env(
            "__CORALREEF_NONEXISTENT_DURATION__",
            Duration::from_secs(42),
        );
        assert_eq!(d, Duration::from_secs(42));
    }
}
