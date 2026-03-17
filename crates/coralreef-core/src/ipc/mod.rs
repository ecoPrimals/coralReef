// SPDX-License-Identifier: AGPL-3.0-only
//! IPC transports — JSON-RPC 2.0 and tarpc servers.
//!
//! Follows wateringHole `UNIVERSAL_IPC_STANDARD_V3.md`:
//! - JSON-RPC 2.0 as primary protocol (TCP/HTTP — external, debuggable)
//! - JSON-RPC 2.0 over Unix socket (newline-delimited — toadStool compatible)
//! - tarpc as optional high-performance channel (TCP or Unix socket — internal)
//! - Semantic method names: `shader.compile.{spirv,wgsl,status,capabilities}`
//!
//! ## Platform-agnostic transport (ecoBin compliance)
//!
//! On Unix platforms, tarpc and JSON-RPC both support Unix domain sockets
//! for local primal-to-primal communication. JSON-RPC also serves over TCP
//! (HTTP) for external access. On non-Unix platforms, all protocols use TCP.

use std::fmt;
use std::net::SocketAddr;

use crate::config;

pub mod error;

mod jsonrpc;
pub use jsonrpc::start_jsonrpc_server;

mod tarpc_transport;
pub use tarpc_transport::start_tarpc_server;
#[cfg(all(test, unix))]
pub use tarpc_transport::start_tarpc_unix_server;
#[cfg(any(test, feature = "e2e"))]
#[allow(unused_imports)]
pub use tarpc_transport::{ShaderCompileTarpcClient, start_tarpc_tcp_server};

#[cfg(unix)]
mod unix_jsonrpc;
#[cfg(all(test, unix))]
pub use unix_jsonrpc::unix_socket_path_for_base;
#[cfg(unix)]
pub use unix_jsonrpc::{default_unix_socket_path, start_unix_jsonrpc_server};
#[cfg(all(test, unix))]
pub(crate) use unix_jsonrpc::{dispatch, make_response};

/// Errors from IPC server operations.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Failed to parse bind address.
    #[error("invalid bind address: {0}")]
    InvalidAddress(#[from] std::net::AddrParseError),

    /// JSON-RPC server failed to bind or start.
    #[error("JSON-RPC server error: {0}")]
    JsonRpc(#[source] std::io::Error),

    /// tarpc listener failed to bind.
    #[error("tarpc server error: {0}")]
    Tarpc(#[from] std::io::Error),
}

/// Transport-agnostic bound address reported by servers.
#[derive(Debug, Clone)]
pub enum BoundAddr {
    /// TCP socket address (host:port).
    Tcp(SocketAddr),
    /// Unix domain socket path (Unix platforms only).
    #[cfg(unix)]
    Unix(std::path::PathBuf),
}

impl BoundAddr {
    /// Protocol name for capability advertisement.
    #[must_use]
    pub const fn protocol(&self) -> &'static str {
        match self {
            Self::Tcp(_) => "tcp",
            #[cfg(unix)]
            Self::Unix(_) => "unix",
        }
    }
}

impl fmt::Display for BoundAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp(addr) => write!(f, "{addr}"),
            #[cfg(unix)]
            Self::Unix(path) => write!(f, "unix://{}", path.display()),
        }
    }
}

/// TCP loopback with OS-assigned port (fallback when `CORALREEF_TCP_BIND` is unset).
pub const FALLBACK_TCP_BIND: &str = "127.0.0.1:0";

/// Resolve the TCP bind address for JSON-RPC.
///
/// Checks `$CORALREEF_TCP_BIND` first for deployment configuration,
/// then falls back to loopback with OS-assigned port.
#[must_use]
pub fn default_tcp_bind() -> String {
    std::env::var("CORALREEF_TCP_BIND").unwrap_or_else(|_| FALLBACK_TCP_BIND.to_owned())
}

/// Platform-aware default bind address for tarpc.
///
/// On Unix: returns a path for a Unix domain socket under `$XDG_RUNTIME_DIR`
/// (or `std::env::temp_dir()` as fallback — no hardcoded paths per ecoBin),
/// namespaced by the primal identity and family ID.
/// On non-Unix: returns TCP loopback with OS-assigned port.
#[must_use]
pub fn default_tarpc_bind() -> String {
    #[cfg(unix)]
    {
        let dir = config::discovery_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join(config::ECOSYSTEM_NAMESPACE));
        let sock = dir.join(format!(
            "{}-{}-tarpc.sock",
            config::PRIMAL_NAME,
            config::family_id(),
        ));
        format!("unix://{}", sock.display())
    }
    #[cfg(not(unix))]
    {
        default_tcp_bind()
    }
}

#[cfg(test)]
mod test_helpers;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_chaos;
#[cfg(test)]
mod tests_fault;
#[cfg(test)]
mod tests_jsonrpc;
#[cfg(test)]
mod tests_tarpc;
#[cfg(test)]
mod tests_unix;
#[cfg(test)]
mod tests_unix_edge;
