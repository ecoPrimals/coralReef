// SPDX-License-Identifier: AGPL-3.0-or-later
//! tarpc — high-performance binary protocol (bincode over TCP or Unix socket).
//!
//! All wateringHole health triad methods are implemented: `health_check`,
//! `health_liveness`, `health_readiness`, plus `identity_get` and
//! `capability_list`. The tarpc endpoint listens on a `-tarpc.sock` suffixed
//! socket (via `resolve_uds_binds`), while the main socket speaks JSON-RPC.
//! GAP-04 (tarpc health) is resolved.

use futures::StreamExt;
use tokio::sync::watch;

use crate::service::{self, TarpcCompileError};

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
    ) -> Result<service::CompileResponse, TarpcCompileError>;

    /// Compile WGSL source to native GPU binary (`shader.compile.wgsl`).
    async fn wgsl(
        request: service::CompileWgslRequest,
    ) -> Result<service::CompileResponse, TarpcCompileError>;

    /// Health/status check (`shader.compile.status`).
    async fn status() -> service::HealthResponse;

    /// List supported GPU architectures (`shader.compile.capabilities`).
    async fn capabilities() -> Vec<String>;

    /// Compile WGSL to multiple GPU targets (`shader.compile.wgsl.multi`).
    async fn wgsl_multi(
        request: service::MultiDeviceCompileRequest,
    ) -> Result<service::MultiDeviceCompileResponse, TarpcCompileError>;

    /// Full health probe (`health.check`).
    async fn health_check() -> service::HealthCheckResponse;

    /// Lightweight alive probe (`health.liveness`).
    async fn health_liveness() -> service::LivenessResponse;

    /// Ready to accept work (`health.readiness`).
    async fn health_readiness() -> service::ReadinessResponse;

    /// Build identity for upgrade verification (`health.version`).
    async fn health_version() -> service::VersionResponse;

    /// Self-description for ecosystem discovery (`identity.get`).
    async fn identity_get() -> service::IdentityGetResponse;

    /// Batch compile mixed-input shaders (`shader.compile.multi`).
    async fn compile_multi(
        request: service::BatchCompileRequest,
    ) -> Result<service::BatchCompileResponse, TarpcCompileError>;

    /// Compile HMMA GEMM kernel for tensor-core dispatch (`shader.compile.gemm`).
    async fn gemm(
        request: service::GemmCompileRequest,
    ) -> Result<service::CompileResponse, TarpcCompileError>;

    /// Wire Standard L2 capability/method inventory (`capability.list`).
    async fn capability_list() -> service::CapabilityListResponse;
}

/// tarpc server implementation.
#[derive(Clone)]
struct TarpcServer;

impl ShaderCompileTarpc for TarpcServer {
    async fn spirv(
        self,
        _ctx: tarpc::context::Context,
        request: service::CompileSpirvRequestTarpc,
    ) -> Result<service::CompileResponse, TarpcCompileError> {
        let deadline = super::newline_jsonrpc::compile_timeout();
        let task = tokio::task::spawn_blocking(move || {
            service::handle_compile_spirv(
                &request.spirv,
                request.arch,
                request.opt_level,
                request.fp64_software,
            )
            .map_err(service::TarpcCompileError::from_error)
        });
        match tokio::time::timeout(deadline, task).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => Err(TarpcCompileError {
                message: format!("compile task panicked: {e}"),
            }),
            Err(_elapsed) => Err(TarpcCompileError {
                message: format!("shader compilation exceeded {deadline:?} deadline"),
            }),
        }
    }

    async fn wgsl(
        self,
        _ctx: tarpc::context::Context,
        request: service::CompileWgslRequest,
    ) -> Result<service::CompileResponse, TarpcCompileError> {
        let deadline = super::newline_jsonrpc::compile_timeout();
        let task = tokio::task::spawn_blocking(move || {
            service::handle_compile_wgsl(&request).map_err(service::TarpcCompileError::from_error)
        });
        match tokio::time::timeout(deadline, task).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => Err(TarpcCompileError {
                message: format!("compile task panicked: {e}"),
            }),
            Err(_elapsed) => Err(TarpcCompileError {
                message: format!("shader compilation exceeded {deadline:?} deadline"),
            }),
        }
    }

    async fn status(self, _ctx: tarpc::context::Context) -> service::HealthResponse {
        service::handle_health()
    }

    async fn capabilities(self, _ctx: tarpc::context::Context) -> Vec<String> {
        service::handle_health().supported_archs
    }

    async fn wgsl_multi(
        self,
        _ctx: tarpc::context::Context,
        request: service::MultiDeviceCompileRequest,
    ) -> Result<service::MultiDeviceCompileResponse, TarpcCompileError> {
        let deadline = super::newline_jsonrpc::compile_timeout();
        let task = tokio::task::spawn_blocking(move || {
            service::handle_compile_wgsl_multi(request)
                .map_err(service::TarpcCompileError::from_error)
        });
        match tokio::time::timeout(deadline, task).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => Err(TarpcCompileError {
                message: format!("compile task panicked: {e}"),
            }),
            Err(_elapsed) => Err(TarpcCompileError {
                message: format!("shader compilation exceeded {deadline:?} deadline"),
            }),
        }
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

    async fn health_version(self, _ctx: tarpc::context::Context) -> service::VersionResponse {
        service::handle_health_version()
    }

    async fn identity_get(self, _ctx: tarpc::context::Context) -> service::IdentityGetResponse {
        service::handle_identity_get()
    }

    async fn compile_multi(
        self,
        _ctx: tarpc::context::Context,
        request: service::BatchCompileRequest,
    ) -> Result<service::BatchCompileResponse, TarpcCompileError> {
        let deadline = super::newline_jsonrpc::compile_timeout();
        let task = tokio::task::spawn_blocking(move || {
            service::handle_compile_multi(request).map_err(service::TarpcCompileError::from_error)
        });
        match tokio::time::timeout(deadline, task).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => Err(TarpcCompileError {
                message: format!("batch compile task panicked: {e}"),
            }),
            Err(_elapsed) => Err(TarpcCompileError {
                message: format!("batch compilation exceeded {deadline:?} deadline"),
            }),
        }
    }

    async fn gemm(
        self,
        _ctx: tarpc::context::Context,
        request: service::GemmCompileRequest,
    ) -> Result<service::CompileResponse, TarpcCompileError> {
        let deadline = super::newline_jsonrpc::compile_timeout();
        let task = tokio::task::spawn_blocking(move || {
            service::handle_compile_gemm(&request).map_err(service::TarpcCompileError::from_error)
        });
        match tokio::time::timeout(deadline, task).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => Err(TarpcCompileError {
                message: format!("gemm compile task panicked: {e}"),
            }),
            Err(_elapsed) => Err(TarpcCompileError {
                message: format!("gemm compilation exceeded {deadline:?} deadline"),
            }),
        }
    }

    async fn capability_list(
        self,
        _ctx: tarpc::context::Context,
    ) -> service::CapabilityListResponse {
        service::handle_capability_list()
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
    let cleanup_path = path.to_path_buf();

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
        let _ = std::fs::remove_file(&cleanup_path);
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
