// SPDX-License-Identifier: AGPL-3.0-only
//! tarpc — high-performance binary protocol (TCP or Unix socket).

use futures::StreamExt;
use tokio::sync::watch;

use crate::service;

use super::{BoundAddr, IpcError};

/// tarpc service definition.
///
/// Method names are bare (`compile`, `compile_wgsl`, `health`) —
/// the trait name `CoralReefTarpc` already provides the namespace.
/// JSON-RPC equivalents use the `compiler.{method}` dotted form.
#[tarpc::service]
pub trait CoralReefTarpc {
    /// Compile SPIR-V to native GPU binary.
    async fn compile(request: service::CompileRequest) -> Result<service::CompileResponse, String>;

    /// Compile WGSL source to native GPU binary.
    async fn compile_wgsl(
        request: service::CompileWgslRequest,
    ) -> Result<service::CompileResponse, String>;

    /// Health check.
    async fn health() -> service::HealthResponse;
}

/// tarpc server implementation.
#[derive(Clone)]
struct TarpcServer;

impl CoralReefTarpc for TarpcServer {
    async fn compile(
        self,
        _ctx: tarpc::context::Context,
        request: service::CompileRequest,
    ) -> Result<service::CompileResponse, String> {
        service::handle_compile(&request).map_err(|e| e.to_string())
    }

    async fn compile_wgsl(
        self,
        _ctx: tarpc::context::Context,
        request: service::CompileWgslRequest,
    ) -> Result<service::CompileResponse, String> {
        service::handle_compile_wgsl(&request).map_err(|e| e.to_string())
    }

    async fn health(self, _ctx: tarpc::context::Context) -> service::HealthResponse {
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

    let addr: std::net::SocketAddr = bind.parse()?;
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
