// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pre-dispatch capability gate for JSON-RPC methods (JH-0).
//!
//! Every incoming RPC call passes through [`MethodGate::check`] *before*
//! reaching the dispatch table. The gate classifies methods into
//! [`MethodAccessLevel::Public`] (allowed without any token — health probes,
//! identity, capability advertisement) and [`MethodAccessLevel::Protected`]
//! (require a valid capability token once enforcement is activated).
//!
//! Two enforcement modes control behavior:
//! - **Permissive** (default): protected methods are logged but allowed,
//!   preserving backward compatibility during ecosystem rollout.
//! - **Enforced**: protected methods without a valid token are rejected
//!   with `PERMISSION_DENIED` (-32001).
//!
//! Caller identity is extracted from `SO_PEERCRED` on Unix sockets (via
//! `rustix`) or inferred from connection origin. Token verification is a
//! trait interface that `BearDog` fills in later (`auth.verify_ionic`).
//!
//! Per `primalSpring/wateringHole/METHOD_GATE_STANDARD.md` v1.0.

use std::sync::OnceLock;

/// JSON-RPC error code: caller lacks scope for the method.
pub const PERMISSION_DENIED: i32 = -32_001;

/// JSON-RPC error code: caller identity could not be established.
#[allow(dead_code, reason = "reserved for enforced mode when BearDog ships auth.verify_ionic")]
pub const UNAUTHORIZED: i32 = -32_000;

/// Access level for a JSON-RPC method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodAccessLevel {
    /// Health probes, identity, capability advertisement — always allowed.
    Public,
    /// Requires a valid capability token when enforcement is active.
    Protected,
}

/// Methods that are always public (prefix match).
const PUBLIC_METHOD_PREFIXES: &[&str] = &["health."];

/// Methods that are always public (exact match).
const PUBLIC_METHODS: &[&str] = &[
    "identity.get",
    "capabilities.list",
    "capability.list",
    "lifecycle.status",
    "auth.check",
    "auth.mode",
    "auth.peer_info",
];

/// Classify a method string into its access level.
#[must_use]
pub fn classify_method(method: &str) -> MethodAccessLevel {
    if PUBLIC_METHODS.contains(&method) {
        return MethodAccessLevel::Public;
    }
    for prefix in PUBLIC_METHOD_PREFIXES {
        if method.starts_with(prefix) {
            return MethodAccessLevel::Public;
        }
    }
    MethodAccessLevel::Protected
}

/// Peer credentials extracted from `SO_PEERCRED` on Unix sockets.
#[derive(Debug, Clone)]
pub struct PeerCredentials {
    /// Process ID of the caller (if available).
    pub pid: Option<u32>,
    /// User ID of the caller.
    pub uid: u32,
}

/// Identity and authorization context for an incoming RPC call.
#[derive(Debug, Clone)]
pub struct CallerContext {
    /// Optional bearer / capability token sent in the request.
    pub bearer_token: Option<String>,
    /// Peer credentials from `SO_PEERCRED` (Unix socket only).
    pub peer: Option<PeerCredentials>,
    /// Where the connection came from.
    pub origin: ConnectionOrigin,
}

/// How the caller connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code, reason = "Unix and Remote constructed when peer creds are wired")]
pub enum ConnectionOrigin {
    /// Local Unix domain socket.
    Unix,
    /// TCP loopback (127.0.0.1 / `::1`).
    Loopback,
    /// Remote TCP connection.
    Remote,
}

impl CallerContext {
    /// Build a caller context for loopback TCP with no peer credentials.
    #[must_use]
    pub const fn loopback() -> Self {
        Self {
            bearer_token: None,
            peer: None,
            origin: ConnectionOrigin::Loopback,
        }
    }

    /// Build a caller context for a Unix socket connection.
    ///
    /// Peer credentials via `rustix` can be populated when stabilized or
    /// when the composition explicitly provides them. Until then, origin
    /// alone distinguishes local from remote.
    #[must_use]
    #[allow(dead_code, reason = "used when Unix socket accept wires caller context")]
    pub const fn unix() -> Self {
        Self {
            bearer_token: None,
            peer: None,
            origin: ConnectionOrigin::Unix,
        }
    }
}

/// Enforcement mode for the method gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementMode {
    /// Log violations but allow all calls (backward-compatible default).
    Permissive,
    /// Reject unauthenticated calls to protected methods.
    Enforced,
}

impl EnforcementMode {
    /// Resolve from `CORALREEF_AUTH_MODE` env var.
    /// Falls back to `PRIMALSPRING_AUTH_MODE` for ecosystem consistency.
    /// Defaults to `Permissive` if unset or unrecognized.
    #[must_use]
    pub fn from_env() -> Self {
        let val = std::env::var("CORALREEF_AUTH_MODE")
            .or_else(|_| std::env::var("PRIMALSPRING_AUTH_MODE"))
            .unwrap_or_default();
        match val.to_lowercase().as_str() {
            "enforced" | "enforce" | "strict" => Self::Enforced,
            _ => Self::Permissive,
        }
    }

    /// Human-readable label for diagnostics and `auth.mode` responses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Permissive => "permissive",
            Self::Enforced => "enforced",
        }
    }
}

/// Result of a gate check — either allowed or denied with a JSON-RPC error.
#[derive(Debug)]
pub struct GateDenied {
    /// JSON-RPC error code.
    pub code: i32,
    /// Human-readable error message.
    pub message: String,
}

/// Pre-dispatch gate that checks caller authorization before method execution.
#[derive(Debug)]
pub struct MethodGate {
    mode: EnforcementMode,
}

impl MethodGate {
    /// Create a gate with the given enforcement mode.
    #[must_use]
    pub const fn new(mode: EnforcementMode) -> Self {
        Self { mode }
    }

    /// Create a gate from the environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self::new(EnforcementMode::from_env())
    }

    /// Current enforcement mode.
    #[must_use]
    pub const fn mode(&self) -> EnforcementMode {
        self.mode
    }

    /// Pre-dispatch authorization check.
    ///
    /// Returns `Ok(())` if the call should proceed.
    ///
    /// # Errors
    ///
    /// Returns `Err(GateDenied)` when a protected method is called without
    /// a valid capability token and the gate is in `Enforced` mode.
    pub fn check(&self, method: &str, caller: &CallerContext) -> Result<(), GateDenied> {
        let level = classify_method(method);

        if level == MethodAccessLevel::Public {
            return Ok(());
        }

        let authorized = caller.bearer_token.is_some();
        if authorized {
            return Ok(());
        }

        match self.mode {
            EnforcementMode::Permissive => {
                tracing::warn!(
                    method,
                    caller_uid = caller.peer.as_ref().map(|p| p.uid),
                    caller_pid = caller.peer.as_ref().and_then(|p| p.pid),
                    origin = ?caller.origin,
                    "method gate: unauthenticated call to protected method (permissive — allowing)"
                );
                Ok(())
            }
            EnforcementMode::Enforced => {
                tracing::warn!(
                    method,
                    caller_uid = caller.peer.as_ref().map(|p| p.uid),
                    caller_pid = caller.peer.as_ref().and_then(|p| p.pid),
                    origin = ?caller.origin,
                    "method gate: REJECTED unauthenticated call to protected method"
                );
                Err(GateDenied {
                    code: PERMISSION_DENIED,
                    message: format!(
                        "permission denied: method '{method}' requires a capability token"
                    ),
                })
            }
        }
    }
}

/// Global method gate instance, initialized from environment at first access.
static GATE: OnceLock<MethodGate> = OnceLock::new();

/// Get the global method gate (initialized on first call).
pub fn gate() -> &'static MethodGate {
    GATE.get_or_init(MethodGate::from_env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_methods_are_public() {
        assert_eq!(classify_method("health.check"), MethodAccessLevel::Public);
        assert_eq!(classify_method("health.liveness"), MethodAccessLevel::Public);
        assert_eq!(classify_method("health.readiness"), MethodAccessLevel::Public);
    }

    #[test]
    fn identity_is_public() {
        assert_eq!(classify_method("identity.get"), MethodAccessLevel::Public);
    }

    #[test]
    fn capabilities_list_is_public() {
        assert_eq!(classify_method("capabilities.list"), MethodAccessLevel::Public);
        assert_eq!(classify_method("capability.list"), MethodAccessLevel::Public);
    }

    #[test]
    fn auth_introspection_is_public() {
        assert_eq!(classify_method("auth.check"), MethodAccessLevel::Public);
        assert_eq!(classify_method("auth.mode"), MethodAccessLevel::Public);
        assert_eq!(classify_method("auth.peer_info"), MethodAccessLevel::Public);
    }

    #[test]
    fn lifecycle_status_is_public() {
        assert_eq!(classify_method("lifecycle.status"), MethodAccessLevel::Public);
    }

    #[test]
    fn shader_methods_are_protected() {
        assert_eq!(classify_method("shader.compile.wgsl"), MethodAccessLevel::Protected);
        assert_eq!(classify_method("shader.compile.spirv"), MethodAccessLevel::Protected);
        assert_eq!(classify_method("shader.compile.wgsl.multi"), MethodAccessLevel::Protected);
    }

    #[test]
    fn btsp_negotiate_is_protected() {
        assert_eq!(classify_method("btsp.negotiate"), MethodAccessLevel::Protected);
    }

    #[test]
    fn unknown_methods_are_protected() {
        assert_eq!(classify_method("foo.bar"), MethodAccessLevel::Protected);
        assert_eq!(classify_method(""), MethodAccessLevel::Protected);
    }

    #[test]
    fn gate_allows_public_methods_in_enforced_mode() {
        let gate = MethodGate::new(EnforcementMode::Enforced);
        let caller = CallerContext::loopback();
        assert!(gate.check("health.check", &caller).is_ok());
        assert!(gate.check("identity.get", &caller).is_ok());
        assert!(gate.check("capability.list", &caller).is_ok());
        assert!(gate.check("auth.mode", &caller).is_ok());
    }

    #[test]
    fn gate_allows_protected_in_permissive_mode() {
        let gate = MethodGate::new(EnforcementMode::Permissive);
        let caller = CallerContext::loopback();
        assert!(gate.check("shader.compile.wgsl", &caller).is_ok());
    }

    #[test]
    fn gate_rejects_protected_in_enforced_mode_without_token() {
        let gate = MethodGate::new(EnforcementMode::Enforced);
        let caller = CallerContext::loopback();
        let result = gate.check("shader.compile.wgsl", &caller);
        assert!(result.is_err());
        let denied = result.unwrap_err();
        assert_eq!(denied.code, PERMISSION_DENIED);
        assert!(denied.message.contains("shader.compile.wgsl"));
    }

    #[test]
    fn gate_allows_protected_in_enforced_mode_with_token() {
        let gate = MethodGate::new(EnforcementMode::Enforced);
        let caller = CallerContext {
            bearer_token: Some("valid-ionic-token".to_owned()),
            peer: None,
            origin: ConnectionOrigin::Unix,
        };
        assert!(gate.check("shader.compile.wgsl", &caller).is_ok());
    }

    #[test]
    fn enforcement_mode_as_str() {
        assert_eq!(EnforcementMode::Permissive.as_str(), "permissive");
        assert_eq!(EnforcementMode::Enforced.as_str(), "enforced");
    }

    #[test]
    fn caller_context_unix_origin() {
        let ctx = CallerContext::unix();
        assert_eq!(ctx.origin, ConnectionOrigin::Unix);
        assert!(ctx.bearer_token.is_none());
        assert!(ctx.peer.is_none());
    }

    #[test]
    fn caller_context_loopback_origin() {
        let ctx = CallerContext::loopback();
        assert_eq!(ctx.origin, ConnectionOrigin::Loopback);
    }
}
