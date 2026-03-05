// SPDX-License-Identifier: AGPL-3.0-only
//! IPC transports — JSON-RPC 2.0 and tarpc servers.
//!
//! Follows wateringHole `UNIVERSAL_IPC_STANDARD_V3.md`:
//! - JSON-RPC 2.0 as primary protocol
//! - tarpc as optional high-performance channel
//! - Semantic method names: `compiler.compile`, `compiler.health`

use std::net::SocketAddr;

use futures::StreamExt;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::server::{Server, ServerHandle};
use jsonrpsee::types::ErrorObjectOwned;
use tokio::sync::watch;

use crate::service;

/// Loopback with OS-assigned port — zero-knowledge default binding.
pub(crate) const DEFAULT_BIND: &str = "127.0.0.1:0";

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 — semantic method names per wateringHole standard
// ---------------------------------------------------------------------------

/// JSON-RPC 2.0 API definition.
///
/// Method names follow `domain.operation` format.
#[rpc(server)]
trait CoralNakRpc {
    /// `compiler.compile` — compile SPIR-V to native GPU binary.
    #[method(name = "compiler.compile")]
    async fn compiler_compile(
        &self,
        request: service::CompileRequest,
    ) -> Result<service::CompileResponse, ErrorObjectOwned>;

    /// `compiler.health` — health check.
    #[method(name = "compiler.health")]
    async fn compiler_health(&self) -> Result<service::HealthResponse, ErrorObjectOwned>;

    /// `compiler.supported_archs` — list supported GPU architectures.
    #[method(name = "compiler.supported_archs")]
    async fn compiler_supported_archs(&self) -> Result<Vec<String>, ErrorObjectOwned>;
}

struct RpcImpl;

#[async_trait]
impl CoralNakRpcServer for RpcImpl {
    async fn compiler_compile(
        &self,
        request: service::CompileRequest,
    ) -> Result<service::CompileResponse, ErrorObjectOwned> {
        service::handle_compile(&request)
            .map_err(|e| ErrorObjectOwned::owned(-32000, e.to_string(), None::<()>))
    }

    async fn compiler_health(&self) -> Result<service::HealthResponse, ErrorObjectOwned> {
        Ok(service::handle_health())
    }

    async fn compiler_supported_archs(&self) -> Result<Vec<String>, ErrorObjectOwned> {
        let health = service::handle_health();
        Ok(health.supported_archs)
    }
}

/// Start the JSON-RPC 2.0 server.
///
/// Returns the bound address and server handle for graceful shutdown.
/// The server runs in a background task until [`ServerHandle::stop`] is called.
///
/// # Errors
///
/// Returns an error if the server fails to bind.
pub async fn start_jsonrpc_server(
    bind: &str,
) -> Result<(SocketAddr, ServerHandle), Box<dyn std::error::Error>> {
    let addr: SocketAddr = bind.parse()?;
    let server = Server::builder().build(addr).await?;
    let bound = server.local_addr()?;
    let handle = server.start(RpcImpl.into_rpc());
    let handle_for_task = handle.clone();

    tokio::spawn(async move {
        handle_for_task.stopped().await;
    });

    tracing::info!(%bound, "JSON-RPC server listening");
    Ok((bound, handle))
}

// ---------------------------------------------------------------------------
// tarpc — high-performance binary protocol
// ---------------------------------------------------------------------------

/// tarpc service definition (mirrors JSON-RPC methods).
#[tarpc::service]
pub trait CoralNakTarpc {
    /// Compile SPIR-V to native GPU binary.
    async fn compiler_compile(
        request: service::CompileRequest,
    ) -> Result<service::CompileResponse, String>;

    /// Health check.
    async fn compiler_health() -> service::HealthResponse;
}

/// tarpc server implementation.
#[derive(Clone)]
struct TarpcServer;

impl CoralNakTarpc for TarpcServer {
    async fn compiler_compile(
        self,
        _ctx: tarpc::context::Context,
        request: service::CompileRequest,
    ) -> Result<service::CompileResponse, String> {
        service::handle_compile(&request).map_err(|e| e.to_string())
    }

    async fn compiler_health(self, _ctx: tarpc::context::Context) -> service::HealthResponse {
        service::handle_health()
    }
}

/// Start the tarpc server with JSON codec over TCP.
///
/// Returns the bound address and join handle for graceful shutdown.
/// When `shutdown_rx` is notified (sender sends), the server stops accepting new
/// connections and the join handle completes after in-flight requests finish.
///
/// # Errors
///
/// Returns an error if the server fails to bind.
pub async fn start_tarpc_server(
    bind: &str,
    shutdown_rx: watch::Receiver<()>,
) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), Box<dyn std::error::Error>> {
    use tarpc::server::{self, Channel};
    use tokio_serde::formats::Json;

    let addr: SocketAddr = bind.parse()?;
    let listener = tarpc::serde_transport::tcp::listen(&addr, Json::default).await?;
    let bound = listener.local_addr();

    let handle = tokio::spawn(async move {
        let mut shutdown_rx = shutdown_rx;
        listener
            .take_until(async move {
                let _ = shutdown_rx.changed().await;
            })
            .filter_map(|r| futures::future::ready(r.ok()))
            .map(server::BaseChannel::with_defaults)
            .for_each(|channel| async move {
                tokio::spawn(channel.execute(TarpcServer.serve()).for_each(
                    |response| async move {
                        tokio::spawn(response);
                    },
                ));
            })
            .await;
    });

    tracing::info!(%bound, "tarpc server listening");
    Ok((bound, handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns (sender, receiver). Caller must hold the sender for the test duration
    /// so the server does not receive a shutdown signal.
    fn test_shutdown_channel() -> (watch::Sender<()>, watch::Receiver<()>) {
        watch::channel(())
    }

    #[tokio::test]
    async fn test_jsonrpc_server_starts() {
        let (addr, _handle) = start_jsonrpc_server(DEFAULT_BIND).await.unwrap();
        assert_ne!(addr.port(), 0);
    }

    #[tokio::test]
    async fn test_tarpc_server_starts() {
        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_server(DEFAULT_BIND, rx).await.unwrap();
        assert_ne!(addr.port(), 0);
    }

    #[tokio::test]
    async fn test_jsonrpc_health_endpoint() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (addr, _handle) = start_jsonrpc_server(DEFAULT_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let response: service::HealthResponse = client
            .request("compiler.health", jsonrpsee::rpc_params![])
            .await
            .unwrap();

        assert_eq!(response.name, env!("CARGO_PKG_NAME"));
        assert!(!response.supported_archs.is_empty());
    }

    #[tokio::test]
    async fn test_jsonrpc_supported_archs_endpoint() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (addr, _handle) = start_jsonrpc_server(DEFAULT_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let archs: Vec<String> = client
            .request("compiler.supported_archs", jsonrpsee::rpc_params![])
            .await
            .unwrap();

        let default_arch = coral_nak::GpuArch::default().to_string();
        assert!(archs.contains(&default_arch));
    }

    #[tokio::test]
    async fn test_jsonrpc_compile_empty_spirv() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (addr, _handle) = start_jsonrpc_server(DEFAULT_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let req = service::CompileRequest {
            spirv_words: vec![],
            arch: coral_nak::GpuArch::default().to_string(),
            opt_level: 2,
            fp64_software: true,
        };

        let result: Result<service::CompileResponse, _> =
            client.request("compiler.compile", [req]).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_tarpc_health_endpoint() {
        use tokio_serde::formats::Json;

        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_server(DEFAULT_BIND, rx).await.unwrap();

        let transport = tarpc::serde_transport::tcp::connect(addr, Json::default)
            .await
            .unwrap();
        let client = CoralNakTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

        let response = client
            .compiler_health(tarpc::context::current())
            .await
            .unwrap();

        assert_eq!(response.name, env!("CARGO_PKG_NAME"));
    }

    #[tokio::test]
    async fn test_tarpc_compile_empty_spirv() {
        use tokio_serde::formats::Json;

        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_server(DEFAULT_BIND, rx).await.unwrap();

        let transport = tarpc::serde_transport::tcp::connect(addr, Json::default)
            .await
            .unwrap();
        let client = CoralNakTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

        let req = service::CompileRequest {
            spirv_words: vec![],
            arch: coral_nak::GpuArch::default().to_string(),
            opt_level: 2,
            fp64_software: true,
        };

        let result = client
            .compiler_compile(tarpc::context::current(), req)
            .await
            .unwrap();

        assert!(result.is_err());
    }
}
