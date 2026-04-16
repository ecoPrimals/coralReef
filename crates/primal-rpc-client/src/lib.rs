// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Pure Rust JSON-RPC 2.0 client for ecoPrimals.
//!
//! Provides inter-primal and external communication with zero C dependencies.
//!
//! # Transport modes
//!
//! - **TCP line** — Newline-delimited JSON-RPC over TCP (wateringHole v3.1
//!   mandatory inter-primal framing). Preferred for composition graphs.
//! - **TCP HTTP** — JSON-RPC over HTTP/1.1 to a local or remote host (legacy).
//! - **Unix socket line** — Newline-delimited JSON-RPC over a Unix domain
//!   socket (ecosystem-standard local transport).
//! - **Unix socket HTTP** — JSON-RPC over HTTP/1.1 to a local Unix domain socket.
//! - **Delegated TLS** — HTTPS via the Tower Atomic pattern: plain HTTP to
//!   a local TLS edge proxy, which handles TLS 1.3 externally via ecosystem
//!   crypto delegation. Zero `ring`, zero `rustls`, zero C.
//!
//! # Example
//!
//! ```no_run
//! # async fn example() -> Result<(), primal_rpc_client::RpcError> {
//! use primal_rpc_client::RpcClient;
//! use std::net::SocketAddr;
//!
//! let addr: SocketAddr = "127.0.0.1:9090".parse().expect("valid socket addr");
//! let client = RpcClient::tcp(addr);
//!
//! let result: String = client.request("shader.compile.status", ()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! Unix socket and [`no_params`] for methods that take no arguments:
//!
//! ```no_run
//! # async fn example() -> Result<(), primal_rpc_client::RpcError> {
//! use primal_rpc_client::{no_params, RpcClient};
//!
//! let client = RpcClient::unix("/run/coralreef/primal.sock");
//! let _: String = client.request("gpu.health", no_params()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! Delegated TLS (HTTP to a local edge proxy) and notifications:
//!
//! ```no_run
//! # async fn example() -> Result<(), primal_rpc_client::RpcError> {
//! use primal_rpc_client::RpcClient;
//! use std::net::SocketAddr;
//!
//! let proxy: SocketAddr = "127.0.0.1:8443".parse().expect("valid proxy addr");
//! let client = RpcClient::delegated_tls_proxy(proxy, "reef.example.com");
//! client.notify("telemetry.heartbeat", serde_json::json!({ "ok": true })).await?;
//! # Ok(())
//! # }
//! ```

mod error;
mod transport;

pub use error::{RpcError, RpcErrorData};
pub use transport::Transport;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::sync::atomic::{AtomicU64, Ordering};

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// JSON-RPC 2.0 HTTP client.
///
/// Supports TCP, Unix socket, and delegated-TLS (local edge proxy) transports — all pure Rust.
#[derive(Debug, Clone)]
pub struct RpcClient {
    transport: Transport,
}

#[derive(Serialize)]
struct JsonRpcRequest<'a, P> {
    jsonrpc: &'static str,
    method: &'a str,
    params: P,
    id: u64,
}

#[derive(Deserialize)]
struct JsonRpcResponse<R> {
    #[serde(rename = "jsonrpc")]
    _jsonrpc: String,
    result: Option<R>,
    error: Option<RpcErrorData>,
    #[serde(rename = "id")]
    _id: u64,
}

impl RpcClient {
    /// Create a client that connects via TCP HTTP to the given address.
    ///
    /// ```
    /// use primal_rpc_client::RpcClient;
    /// use std::net::SocketAddr;
    ///
    /// let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    /// let _ = RpcClient::tcp(addr);
    /// ```
    #[must_use]
    pub const fn tcp(addr: std::net::SocketAddr) -> Self {
        Self {
            transport: Transport::Tcp(addr),
        }
    }

    /// Create a client using newline-delimited JSON-RPC over TCP.
    ///
    /// This is the wateringHole v3.1 mandatory inter-primal wire framing.
    ///
    /// ```
    /// use primal_rpc_client::RpcClient;
    /// use std::net::SocketAddr;
    ///
    /// let addr: SocketAddr = "127.0.0.1:0".parse().expect("parse");
    /// let _ = RpcClient::tcp_line(addr);
    /// ```
    #[must_use]
    pub const fn tcp_line(addr: std::net::SocketAddr) -> Self {
        Self {
            transport: Transport::TcpLine(addr),
        }
    }

    /// Create a client that connects via Unix domain socket (HTTP framing).
    ///
    /// ```
    /// use primal_rpc_client::RpcClient;
    ///
    /// let _ = RpcClient::unix("/run/coralreef/primal.sock");
    /// ```
    #[must_use]
    pub fn unix(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            transport: Transport::Unix(path.into()),
        }
    }

    /// Create a client using newline-delimited JSON-RPC over a Unix domain socket.
    ///
    /// ```
    /// use primal_rpc_client::RpcClient;
    ///
    /// let _ = RpcClient::unix_line("/run/coralreef/primal.sock");
    /// ```
    #[must_use]
    pub fn unix_line(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            transport: Transport::UnixLine(path.into()),
        }
    }

    /// Create a client that routes HTTPS through a local edge proxy (delegated TLS).
    ///
    /// `proxy_addr` is the local HTTP listen address of the TLS edge. `target_host`
    /// is the upstream hostname the edge uses for the TLS 1.3 connection.
    ///
    /// ```
    /// use primal_rpc_client::RpcClient;
    /// use std::net::SocketAddr;
    ///
    /// let proxy: SocketAddr = "127.0.0.1:8443".parse().expect("parse");
    /// let _ = RpcClient::delegated_tls_proxy(proxy, "upstream.local");
    /// ```
    #[must_use]
    pub fn delegated_tls_proxy(
        proxy_addr: std::net::SocketAddr,
        target_host: impl Into<String>,
    ) -> Self {
        Self {
            transport: Transport::DelegatedTlsProxy {
                proxy_addr,
                target_host: target_host.into(),
            },
        }
    }

    /// Send a JSON-RPC 2.0 request and await the response.
    ///
    /// # Errors
    ///
    /// Returns [`RpcError`] on I/O failures, HTTP parse errors, or JSON-RPC
    /// error responses from the server.
    pub async fn request<P, R>(&self, method: &str, params: P) -> Result<R, RpcError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let rpc_req = JsonRpcRequest {
            jsonrpc: "2.0",
            method,
            params,
            id,
        };
        let body = serde_json::to_vec(&rpc_req)?;

        let response_bytes = self.transport.roundtrip(&body).await?;

        let rpc_resp: JsonRpcResponse<R> = serde_json::from_slice(&response_bytes)?;

        if let Some(err) = rpc_resp.error {
            return Err(RpcError::Server(err));
        }

        rpc_resp.result.ok_or(RpcError::EmptyResponse)
    }

    /// Send a JSON-RPC 2.0 notification (no response expected).
    ///
    /// # Errors
    ///
    /// Returns [`RpcError`] on I/O or serialization failures.
    pub async fn notify<P: Serialize>(&self, method: &str, params: P) -> Result<(), RpcError> {
        #[derive(Serialize)]
        struct Notification<'a, P> {
            jsonrpc: &'static str,
            method: &'a str,
            params: P,
        }

        let notif = Notification {
            jsonrpc: "2.0",
            method,
            params,
        };
        let body = serde_json::to_vec(&notif)?;

        let _ = self.transport.roundtrip(&body).await?;
        Ok(())
    }
}

/// Convenience: empty params for methods that take none.
///
/// ```
/// use primal_rpc_client::no_params;
///
/// let params = no_params();
/// assert_eq!(params.len(), 0);
/// ```
#[must_use]
pub const fn no_params() -> [(); 0] {
    []
}

#[cfg(test)]
mod tests;
