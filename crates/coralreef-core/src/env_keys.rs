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

/// Test-only shutdown join timeout override (honored in test/debug builds).
pub const CORALREEF_TEST_SHUTDOWN_JOIN_TIMEOUT_MS: &str =
    "CORALREEF_TEST_SHUTDOWN_JOIN_TIMEOUT_MS";

/// coralReef-specific auth mode override.
pub const CORALREEF_AUTH_MODE: &str = "CORALREEF_AUTH_MODE";

/// Ecosystem-wide auth mode override.
pub const ECOSYSTEM_AUTH_MODE: &str = "ECOSYSTEM_AUTH_MODE";

/// Deprecated auth mode override from primalSpring naming.
pub const PRIMALSPRING_AUTH_MODE: &str = "PRIMALSPRING_AUTH_MODE";

/// Tier-1 crypto derivation input set by the composition launcher.
pub const FAMILY_SEED: &str = "FAMILY_SEED";

/// Security-domain provider socket path (preferred).
pub const BTSP_PROVIDER_SOCKET: &str = "BTSP_PROVIDER_SOCKET";

/// Deprecated alias for [`BTSP_PROVIDER_SOCKET`].
pub const BEARDOG_SOCKET: &str = "BEARDOG_SOCKET";

/// Songbird discovery socket path from the composition launcher.
pub const DISCOVERY_SOCKET: &str = "DISCOVERY_SOCKET";
