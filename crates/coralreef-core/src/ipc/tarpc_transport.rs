// SPDX-License-Identifier: AGPL-3.0-only
//! tarpc — high-performance binary protocol (bincode over TCP or Unix socket).

use futures::StreamExt;
use tokio::sync::watch;

use crate::service;

use super::{BoundAddr, IpcError};

/// tarpc service definition.
///
/// Method names align with `shader.compile.*` JSON-RPC endpoints.
/// The trait name `ShaderCompileTarpc` provides the namespace;
/// methods use bare names per tarpc convention.
///
/// SPIR-V input uses `Bytes` for zero-copy IPC — clients can send raw bytes
/// without parsing into words first.
#[tarpc::service]
pub trait ShaderCompileTarpc {
    /// Compile SPIR-V to native GPU binary (`shader.compile.spirv`).
    /// Uses `Bytes` for zero-copy SPIR-V input.
    async fn spirv(
        request: service::CompileSpirvRequestTarpc,
    ) -> Result<service::CompileResponse, String>;

    /// Compile WGSL source to native GPU binary (`shader.compile.wgsl`).
    async fn wgsl(request: service::CompileWgslRequest)
    -> Result<service::CompileResponse, String>;

    /// Health/status check (`shader.compile.status`).
    async fn status() -> service::HealthResponse;

    /// Full capabilities response (`shader.compile.capabilities`).
    async fn capabilities() -> service::CompileCapabilitiesResponse;

    /// Compile WGSL to multiple GPU targets (`shader.compile.wgsl.multi`).
    async fn wgsl_multi(
        request: service::MultiDeviceCompileRequest,
    ) -> Result<service::MultiDeviceCompileResponse, String>;

    /// Full health probe (`health.check`).
    async fn health_check() -> service::HealthCheckResponse;

    /// Lightweight alive probe (`health.liveness`).
    async fn health_liveness() -> service::LivenessResponse;

    /// Ready to accept work (`health.readiness`).
    async fn health_readiness() -> service::ReadinessResponse;

    /// Compile/validate WGSL for CPU execution (`shader.compile.cpu`).
    async fn compile_cpu(
        request: coral_reef_cpu::CompileCpuRequest,
    ) -> Result<service::CompileResponse, String>;

    /// Execute WGSL on the CPU interpreter (`shader.execute.cpu`).
    async fn execute_cpu(
        request: coral_reef_cpu::ExecuteCpuRequest,
    ) -> Result<coral_reef_cpu::ExecuteCpuResponse, String>;

    /// Execute on CPU and compare against expected (`shader.validate`).
    async fn validate(
        request: coral_reef_cpu::ValidateRequest,
    ) -> Result<coral_reef_cpu::ValidateResponse, String>;
}

/// tarpc server implementation.
#[derive(Clone)]
struct TarpcServer;

impl ShaderCompileTarpc for TarpcServer {
    async fn spirv(
        self,
        _ctx: tarpc::context::Context,
        request: service::CompileSpirvRequestTarpc,
    ) -> Result<service::CompileResponse, String> {
        service::handle_compile_spirv(
            &request.spirv,
            request.arch,
            request.opt_level,
            request.fp64_software,
        )
        .map_err(|e| e.to_string())
    }

    async fn wgsl(
        self,
        _ctx: tarpc::context::Context,
        request: service::CompileWgslRequest,
    ) -> Result<service::CompileResponse, String> {
        service::handle_compile_wgsl(&request).map_err(|e| e.to_string())
    }

    async fn status(self, _ctx: tarpc::context::Context) -> service::HealthResponse {
        service::handle_health()
    }

    async fn capabilities(
        self,
        _ctx: tarpc::context::Context,
    ) -> service::CompileCapabilitiesResponse {
        service::handle_compile_capabilities()
    }

    async fn wgsl_multi(
        self,
        _ctx: tarpc::context::Context,
        request: service::MultiDeviceCompileRequest,
    ) -> Result<service::MultiDeviceCompileResponse, String> {
        service::handle_compile_wgsl_multi(request).map_err(|e| e.to_string())
    }

    async fn health_check(self, _ctx: tarpc::context::Context) -> service::HealthCheckResponse {
        service::handle_health_check()
    }

    async fn health_liveness(self, _ctx: tarpc::context::Context) -> service::LivenessResponse {
        service::handle_health_liveness()
    }

    async fn health_readiness(self, _ctx: tarpc::context::Context) -> service::ReadinessResponse {
        service::handle_health_readiness()
    }

    async fn compile_cpu(
        self,
        _ctx: tarpc::context::Context,
        request: coral_reef_cpu::CompileCpuRequest,
    ) -> Result<service::CompileResponse, String> {
        service::handle_compile_cpu(&request).map_err(|e| e.to_string())
    }

    async fn execute_cpu(
        self,
        _ctx: tarpc::context::Context,
        request: coral_reef_cpu::ExecuteCpuRequest,
    ) -> Result<coral_reef_cpu::ExecuteCpuResponse, String> {
        service::handle_execute_cpu(&request).map_err(|e| e.to_string())
    }

    async fn validate(
        self,
        _ctx: tarpc::context::Context,
        request: coral_reef_cpu::ValidateRequest,
    ) -> Result<coral_reef_cpu::ValidateResponse, String> {
        service::handle_validate(&request).map_err(|e| e.to_string())
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
    use tokio_serde::formats::Bincode;

    let addr: std::net::SocketAddr = bind.parse()?;
    let listener = tarpc::serde_transport::tcp::listen(&addr, Bincode::default).await?;
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
pub fn start_tarpc_unix_server(
    path: &std::path::Path,
    shutdown_rx: watch::Receiver<()>,
) -> Result<(BoundAddr, tokio::task::JoinHandle<()>), IpcError> {
    use tarpc::server::{self, Channel};
    use tokio::net::UnixListener;
    use tokio_serde::formats::Bincode;
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
                                Bincode::default(),
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
        return start_tarpc_unix_server(std::path::Path::new(path), shutdown_rx);
    }
    start_tarpc_tcp_server(bind, shutdown_rx).await
}
