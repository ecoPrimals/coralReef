// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2025-2026 ecoPrimals Collective
//! Environment variable names used by coralReef and shared ecoPrimals composition.
//!
//! Centralizes literal env var keys so production code reads them via named
//! constants instead of scattered string literals.

/// Override for the shared ecosystem namespace directory name.
pub const BIOMEOS_ECOSYSTEM_NAMESPACE: &str = "BIOMEOS_ECOSYSTEM_NAMESPACE";

/// Full JSON-RPC bind string for the ecosystem registry primal.
pub const BIOMEOS_ECOSYSTEM_REGISTRY: &str = "BIOMEOS_ECOSYSTEM_REGISTRY";

/// Family ID set by genomeBin / systemd for multi-instance isolation.
pub const BIOMEOS_FAMILY_ID: &str = "BIOMEOS_FAMILY_ID";

/// When set to `1` or `true`, disables authentication (dev-only).
pub const BIOMEOS_INSECURE: &str = "BIOMEOS_INSECURE";

/// Explicit override for the shared socket/discovery directory.
pub const BIOMEOS_SOCKET_DIR: &str = "BIOMEOS_SOCKET_DIR";

/// XDG Base Directory runtime path (typically `/run/user/{uid}`).
pub const XDG_RUNTIME_DIR: &str = "XDG_RUNTIME_DIR";

/// Family ID set by `composition_nucleus.sh` for multi-instance isolation.
pub const CORALREEF_FAMILY_ID: &str = "CORALREEF_FAMILY_ID";

/// Stem for the capability-domain symlink next to the Unix socket.
pub const CORALREEF_CAPABILITY_DOMAIN: &str = "CORALREEF_CAPABILITY_DOMAIN";

/// Interval between `ipc.heartbeat` calls to the ecosystem registry (seconds).
pub const CORALREEF_HEARTBEAT_SECS: &str = "CORALREEF_HEARTBEAT_SECS";

/// TCP bind address for JSON-RPC when Unix sockets are unavailable.
pub const CORALREEF_TCP_BIND: &str = "CORALREEF_TCP_BIND";

/// Compile deadline for CPU-heavy shader compilation over IPC (seconds).
pub const CORALREEF_COMPILE_TIMEOUT_SECS: &str = "CORALREEF_COMPILE_TIMEOUT_SECS";

/// Graceful shutdown timeout (seconds). Default: 30.
pub const CORALREEF_SHUTDOWN_TIMEOUT_SECS: &str = "CORALREEF_SHUTDOWN_TIMEOUT_SECS";

/// Ecosystem registry JSON-RPC timeout (seconds). Default: 2.
pub const CORALREEF_REGISTRY_TIMEOUT_SECS: &str = "CORALREEF_REGISTRY_TIMEOUT_SECS";

/// Test-only shutdown join timeout override (honored in test/debug builds).
pub const CORALREEF_TEST_SHUTDOWN_JOIN_TIMEOUT_MS: &str = "CORALREEF_TEST_SHUTDOWN_JOIN_TIMEOUT_MS";

/// coralReef-specific auth mode override.
pub const CORALREEF_AUTH_MODE: &str = "CORALREEF_AUTH_MODE";

/// Ecosystem-wide auth mode override.
pub const ECOSYSTEM_AUTH_MODE: &str = "ECOSYSTEM_AUTH_MODE";

/// Deprecated auth mode override (legacy ecosystem naming).
///
/// **Removal target: v0.3.0.** Migrate to [`ECOSYSTEM_AUTH_MODE`] or
/// [`CORALREEF_AUTH_MODE`]. Composition launchers should stop setting this
/// after all gates reach v0.2.x parity.
#[deprecated(
    since = "0.2.0",
    note = "use ECOSYSTEM_AUTH_MODE or CORALREEF_AUTH_MODE"
)]
pub const PRIMALSPRING_AUTH_MODE: &str = "PRIMALSPRING_AUTH_MODE";

/// Tier-1 crypto derivation input set by the composition launcher.
pub const FAMILY_SEED: &str = "FAMILY_SEED";

/// Security-domain provider socket path (preferred).
pub const BTSP_PROVIDER_SOCKET: &str = "BTSP_PROVIDER_SOCKET";

/// Composition launcher alias for the security-domain provider socket.
pub const BEARDOG_SOCKET: &str = "BEARDOG_SOCKET";

/// Tier-1 crypto derivation input set by the composition launcher (BTSP-specific alias).
pub const BTSP_FAMILY_SEED: &str = "BTSP_FAMILY_SEED";

/// Ecosystem discovery relay socket path from the composition launcher.
pub const DISCOVERY_SOCKET: &str = "DISCOVERY_SOCKET";

/// Gate identity for provenance tagging (e.g. `"strandGate"`, `"biomeGate"`).
pub const BIOMEOS_GATE_ID: &str = "BIOMEOS_GATE_ID";

/// Transport endpoint injection (JSON string).
///
/// When set, the launcher or Tower Atomic decides our transport — we don't self-bind.
/// Format: `{"transport":"uds","path":"/run/biomeos/coralreef.sock"}`
///       | `{"transport":"tcp","host":"127.0.0.1","port":9200}`
pub const TRANSPORT_ENDPOINT: &str = "TRANSPORT_ENDPOINT";
