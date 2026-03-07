// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC 2.0 server — semantic method names per wateringHole standard.

use std::net::SocketAddr;

use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::server::{Server, ServerHandle};
use jsonrpsee::types::ErrorObjectOwned;

use crate::service;

use super::IpcError;

/// JSON-RPC 2.0 API definition.
///
/// Method names follow `domain.operation` format.
#[rpc(server)]
trait CoralReefRpc {
    /// `compiler.compile` — compile SPIR-V to native GPU binary.
    #[method(name = "compiler.compile")]
    async fn compiler_compile(
        &self,
        request: service::CompileRequest,
    ) -> Result<service::CompileResponse, ErrorObjectOwned>;

    /// `compiler.compile_wgsl` — compile WGSL source to native GPU binary.
    #[method(name = "compiler.compile_wgsl")]
    async fn compiler_compile_wgsl(
        &self,
        request: service::CompileWgslRequest,
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
impl CoralReefRpcServer for RpcImpl {
    async fn compiler_compile(
        &self,
        request: service::CompileRequest,
    ) -> Result<service::CompileResponse, ErrorObjectOwned> {
        service::handle_compile(&request)
            .map_err(|e| ErrorObjectOwned::owned(-32000, e.to_string(), None::<()>))
    }

    async fn compiler_compile_wgsl(
        &self,
        request: service::CompileWgslRequest,
    ) -> Result<service::CompileResponse, ErrorObjectOwned> {
        service::handle_compile_wgsl(&request)
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
