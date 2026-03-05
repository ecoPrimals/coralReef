// SPDX-License-Identifier: AGPL-3.0-only
//! IPC transports — JSON-RPC 2.0 and tarpc servers.
//!
//! Follows wateringHole `UNIVERSAL_IPC_STANDARD_V3.md`:
//! - JSON-RPC 2.0 as primary protocol (TCP/HTTP — external, debuggable)
//! - tarpc as optional high-performance channel (TCP or Unix socket — internal)
//! - Semantic method names: `compiler.compile`, `compiler.health`
//!
//! ## Platform-agnostic transport (ecoBin compliance)
//!
//! On Unix platforms, tarpc defaults to Unix domain sockets for lower overhead
//! on local primal-to-primal communication. JSON-RPC stays TCP-based (HTTP).
//! On non-Unix platforms, both protocols use TCP loopback.

use std::fmt;
use std::net::SocketAddr;

use futures::StreamExt;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::server::{Server, ServerHandle};
use jsonrpsee::types::ErrorObjectOwned;
use tokio::sync::watch;

use crate::service;

/// Errors from IPC server operations.
#[derive(Debug, thiserror::Error)]
pub(crate) enum IpcError {
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
pub(crate) enum BoundAddr {
    /// TCP socket address (host:port).
    Tcp(SocketAddr),
    /// Unix domain socket path (Unix platforms only).
    #[cfg(unix)]
    Unix(std::path::PathBuf),
}

impl BoundAddr {
    /// Protocol name for capability advertisement.
    pub(crate) fn protocol(&self) -> &'static str {
        match self {
            BoundAddr::Tcp(_) => "tcp",
            #[cfg(unix)]
            BoundAddr::Unix(_) => "unix",
        }
    }
}

impl fmt::Display for BoundAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoundAddr::Tcp(addr) => write!(f, "{addr}"),
            #[cfg(unix)]
            BoundAddr::Unix(path) => write!(f, "unix://{}", path.display()),
        }
    }
}

/// TCP loopback with OS-assigned port.
pub(crate) const DEFAULT_TCP_BIND: &str = "127.0.0.1:0";

/// Platform-aware default bind address for tarpc.
///
/// On Unix: returns a path for a Unix domain socket under `$XDG_RUNTIME_DIR`
/// (or `/tmp` as fallback), namespaced by the primal binary name.
/// On non-Unix: returns TCP loopback with OS-assigned port.
pub(crate) fn default_tarpc_bind() -> String {
    #[cfg(unix)]
    {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_owned());
        format!("unix://{runtime_dir}/{}/tarpc.sock", env!("CARGO_PKG_NAME"))
    }
    #[cfg(not(unix))]
    {
        DEFAULT_TCP_BIND.to_owned()
    }
}

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

/// Start the JSON-RPC 2.0 server (always TCP — HTTP transport).
///
/// Returns the bound address and server handle for graceful shutdown.
/// The server runs in a background task until [`ServerHandle::stop`] is called.
///
/// # Errors
///
/// Returns an error if the server fails to bind.
pub async fn start_jsonrpc_server(bind: &str) -> Result<(SocketAddr, ServerHandle), IpcError> {
    let addr: SocketAddr = bind.parse()?;
    let server = Server::builder()
        .build(addr)
        .await
        .map_err(IpcError::JsonRpc)?;
    let bound = server.local_addr().map_err(IpcError::JsonRpc)?;
    let handle = server.start(RpcImpl.into_rpc());
    let handle_for_task = handle.clone();

    tokio::spawn(async move {
        handle_for_task.stopped().await;
    });

    tracing::info!(%bound, "JSON-RPC server listening");
    Ok((bound, handle))
}

// ---------------------------------------------------------------------------
// tarpc — high-performance binary protocol (TCP or Unix socket)
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

/// Start a tarpc server over TCP.
///
/// Returns the bound address and join handle for graceful shutdown.
///
/// # Errors
///
/// Returns an error if the server fails to bind.
pub async fn start_tarpc_tcp_server(
    bind: &str,
    shutdown_rx: watch::Receiver<()>,
) -> Result<(BoundAddr, tokio::task::JoinHandle<()>), IpcError> {
    use tarpc::server::{self, Channel};
    use tokio_serde::formats::Json;

    let addr: SocketAddr = bind.parse()?;
    let listener = tarpc::serde_transport::tcp::listen(&addr, Json::default).await?;
    let bound = BoundAddr::Tcp(listener.local_addr());

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

    tracing::info!(%bound, "tarpc server listening (tcp)");
    Ok((bound, handle))
}

/// Start a tarpc server over a Unix domain socket.
///
/// Creates the socket file at `path`, removing any stale socket first.
/// Returns the bound path and join handle for graceful shutdown.
///
/// # Errors
///
/// Returns an error if the socket cannot be created.
#[cfg(unix)]
#[allow(clippy::unused_async)]
pub async fn start_tarpc_unix_server(
    path: &std::path::Path,
    shutdown_rx: watch::Receiver<()>,
) -> Result<(BoundAddr, tokio::task::JoinHandle<()>), IpcError> {
    use tarpc::server::{self, Channel};
    use tokio::net::UnixListener;
    use tokio_serde::formats::Json;
    use tokio_util::codec::length_delimited::Builder as LengthDelimitedBuilder;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(IpcError::Tarpc)?;
    }
    let _ = std::fs::remove_file(path);

    let listener = UnixListener::bind(path).map_err(IpcError::Tarpc)?;
    let bound = BoundAddr::Unix(path.to_path_buf());

    let handle = tokio::spawn(async move {
        let mut shutdown_rx = shutdown_rx;
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _addr)) => {
                            let framed = LengthDelimitedBuilder::new().new_framed(stream);
                            let transport = tarpc::serde_transport::new(
                                framed,
                                Json::default(),
                            );
                            tokio::spawn(
                                server::BaseChannel::with_defaults(transport)
                                    .execute(TarpcServer.serve())
                                    .for_each(|response| async move {
                                        tokio::spawn(response);
                                    }),
                            );
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "tarpc unix: failed to accept connection");
                        }
                    }
                }
                _ = shutdown_rx.changed() => break,
            }
        }
    });

    tracing::info!(%bound, "tarpc server listening (unix)");
    Ok((bound, handle))
}

/// Start a tarpc server, automatically selecting transport from the bind string.
///
/// - `unix:///path/to/socket` → Unix domain socket (Unix platforms only)
/// - `host:port` → TCP
///
/// # Errors
///
/// Returns an error if the server fails to bind.
pub async fn start_tarpc_server(
    bind: &str,
    shutdown_rx: watch::Receiver<()>,
) -> Result<(BoundAddr, tokio::task::JoinHandle<()>), IpcError> {
    #[cfg(unix)]
    if let Some(path) = bind.strip_prefix("unix://") {
        return start_tarpc_unix_server(std::path::Path::new(path), shutdown_rx).await;
    }
    start_tarpc_tcp_server(bind, shutdown_rx).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_shutdown_channel() -> (watch::Sender<()>, watch::Receiver<()>) {
        watch::channel(())
    }

    #[tokio::test]
    async fn test_jsonrpc_server_starts() {
        let (addr, _handle) = start_jsonrpc_server(DEFAULT_TCP_BIND).await.unwrap();
        assert_ne!(addr.port(), 0);
    }

    #[tokio::test]
    async fn test_tarpc_tcp_server_starts() {
        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_tcp_server(DEFAULT_TCP_BIND, rx).await.unwrap();
        assert!(matches!(addr, BoundAddr::Tcp(_)));
    }

    #[tokio::test]
    async fn test_tarpc_server_auto_tcp() {
        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_server(DEFAULT_TCP_BIND, rx).await.unwrap();
        assert!(matches!(addr, BoundAddr::Tcp(_)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_tarpc_unix_server_starts() {
        let dir = std::env::temp_dir().join("coralreef-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("test-{}.sock", std::process::id()));

        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_unix_server(&path, rx).await.unwrap();
        assert!(matches!(addr, BoundAddr::Unix(_)));

        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_tarpc_server_auto_unix() {
        let dir = std::env::temp_dir().join("coralreef-test");
        let _ = std::fs::create_dir_all(&dir);
        let sock_path = dir.join(format!("auto-{}.sock", std::process::id()));
        let bind = format!("unix://{}", sock_path.display());

        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_server(&bind, rx).await.unwrap();
        assert!(matches!(addr, BoundAddr::Unix(_)));

        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn test_jsonrpc_health_endpoint() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (addr, _handle) = start_jsonrpc_server(DEFAULT_TCP_BIND).await.unwrap();
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

        let (addr, _handle) = start_jsonrpc_server(DEFAULT_TCP_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let archs: Vec<String> = client
            .request("compiler.supported_archs", jsonrpsee::rpc_params![])
            .await
            .unwrap();

        let default_arch = coral_reef::GpuArch::default().to_string();
        assert!(archs.contains(&default_arch));
    }

    #[tokio::test]
    async fn test_jsonrpc_compile_empty_spirv() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (addr, _handle) = start_jsonrpc_server(DEFAULT_TCP_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let req = service::CompileRequest {
            spirv_words: vec![],
            arch: coral_reef::GpuArch::default().to_string(),
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
        let (addr, _handle) = start_tarpc_tcp_server(DEFAULT_TCP_BIND, rx).await.unwrap();
        let BoundAddr::Tcp(tcp_addr) = addr else {
            panic!("expected TCP address");
        };

        let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Json::default)
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
        let (addr, _handle) = start_tarpc_tcp_server(DEFAULT_TCP_BIND, rx).await.unwrap();
        let BoundAddr::Tcp(tcp_addr) = addr else {
            panic!("expected TCP address");
        };

        let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Json::default)
            .await
            .unwrap();
        let client = CoralNakTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

        let req = service::CompileRequest {
            spirv_words: vec![],
            arch: coral_reef::GpuArch::default().to_string(),
            opt_level: 2,
            fp64_software: true,
        };

        let result = client
            .compiler_compile(tarpc::context::current(), req)
            .await
            .unwrap();

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_bound_addr_display() {
        let tcp = BoundAddr::Tcp("127.0.0.1:8080".parse().unwrap());
        assert_eq!(tcp.to_string(), "127.0.0.1:8080");
        assert_eq!(tcp.protocol(), "tcp");

        #[cfg(unix)]
        {
            let unix = BoundAddr::Unix(std::path::PathBuf::from("/tmp/test.sock"));
            assert_eq!(unix.to_string(), "unix:///tmp/test.sock");
            assert_eq!(unix.protocol(), "unix");
        }
    }

    #[tokio::test]
    async fn test_default_tarpc_bind() {
        let bind = default_tarpc_bind();
        #[cfg(unix)]
        assert!(
            bind.starts_with("unix://"),
            "Unix should default to unix socket"
        );
        #[cfg(not(unix))]
        assert_eq!(bind, DEFAULT_TCP_BIND);
    }
}
