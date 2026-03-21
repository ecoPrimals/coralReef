// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Pure Rust JSON-RPC 2.0 HTTP client for ecoPrimals.
//!
//! Provides inter-primal and external communication with zero C dependencies.
//!
//! # Transport modes
//!
//! - **TCP** — Direct JSON-RPC over HTTP/1.1 to a local or remote host.
//! - **Unix socket** — JSON-RPC over HTTP/1.1 to a local Unix domain socket
//!   (the standard ecoPrimals primal-to-primal transport).
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
//! let addr: SocketAddr = "127.0.0.1:9090".parse().unwrap();
//! let client = RpcClient::tcp(addr);
//!
//! let result: String = client.request("shader.compile.status", ()).await?;
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
    /// Create a client that connects via TCP to the given address.
    #[must_use]
    pub const fn tcp(addr: std::net::SocketAddr) -> Self {
        Self {
            transport: Transport::Tcp(addr),
        }
    }

    /// Create a client that connects via Unix domain socket.
    #[must_use]
    pub fn unix(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            transport: Transport::Unix(path.into()),
        }
    }

    /// Create a client that routes HTTPS through a local edge proxy (delegated TLS).
    ///
    /// `proxy_addr` is the local HTTP listen address of the TLS edge. `target_host`
    /// is the upstream hostname the edge uses for the TLS 1.3 connection.
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
#[must_use]
pub const fn no_params() -> [(); 0] {
    []
}

#[cfg(test)]
mod tests;
