// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC 2.0 server — semantic method names per wateringHole standard.
//!
//! Method namespace: `shader.compile.*` — aligned with capability
//! advertisement (`shader.compile`) per `SEMANTIC_METHOD_NAMING_STANDARD`.

use std::net::SocketAddr;

use coral_reef::CompileError;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::server::{Server, ServerHandle};
use jsonrpsee::types::ErrorObjectOwned;

use crate::service;

use super::IpcError;

/// Application-defined JSON-RPC error codes (server error range: -32000..-32099).
///
/// Per wateringHole `PRIMAL_IPC_PROTOCOL` v2.0.
const JSONRPC_INVALID_INPUT: i32 = -32001;
const JSONRPC_NOT_IMPLEMENTED: i32 = -32002;
const JSONRPC_UNSUPPORTED_ARCH: i32 = -32003;
const JSONRPC_INTERNAL_COMPILE: i32 = -32000;

fn compile_error_to_rpc(e: &CompileError) -> ErrorObjectOwned {
    let code = match e {
        CompileError::InvalidInput(_) => JSONRPC_INVALID_INPUT,
        CompileError::NotImplemented(_) => JSONRPC_NOT_IMPLEMENTED,
        CompileError::UnsupportedArch(_) => JSONRPC_UNSUPPORTED_ARCH,
        _ => JSONRPC_INTERNAL_COMPILE,
    };
    ErrorObjectOwned::owned(code, e.to_string(), None::<()>)
}

/// JSON-RPC 2.0 API definition.
///
/// Method names follow `shader.compile.*` — aligned with capability
/// advertisement and wateringHole `SEMANTIC_METHOD_NAMING_STANDARD`.
#[rpc(server)]
trait CoralReefRpc {
    /// `shader.compile.spirv` — compile SPIR-V to native GPU binary.
    #[method(name = "shader.compile.spirv")]
    async fn shader_compile_spirv(
        &self,
        request: service::CompileRequest,
    ) -> Result<service::CompileResponse, ErrorObjectOwned>;

    /// `shader.compile.wgsl` — compile WGSL source to native GPU binary.
    #[method(name = "shader.compile.wgsl")]
    async fn shader_compile_wgsl(
        &self,
        request: service::CompileWgslRequest,
    ) -> Result<service::CompileResponse, ErrorObjectOwned>;

    /// `shader.compile.status` — health/status check.
    #[method(name = "shader.compile.status")]
    async fn shader_compile_status(&self) -> Result<service::HealthResponse, ErrorObjectOwned>;

    /// `shader.compile.capabilities` — list supported GPU architectures.
    #[method(name = "shader.compile.capabilities")]
    async fn shader_compile_capabilities(&self) -> Result<Vec<String>, ErrorObjectOwned>;
}

struct RpcImpl;

#[async_trait]
impl CoralReefRpcServer for RpcImpl {
    async fn shader_compile_spirv(
        &self,
        request: service::CompileRequest,
    ) -> Result<service::CompileResponse, ErrorObjectOwned> {
        // handle_compile converts spirv_words to Bytes at the boundary (JSON-RPC wire format unchanged).
        service::handle_compile(&request).map_err(|e| compile_error_to_rpc(&e))
    }

    async fn shader_compile_wgsl(
        &self,
        request: service::CompileWgslRequest,
    ) -> Result<service::CompileResponse, ErrorObjectOwned> {
        service::handle_compile_wgsl(&request).map_err(|e| compile_error_to_rpc(&e))
    }

    async fn shader_compile_status(&self) -> Result<service::HealthResponse, ErrorObjectOwned> {
        Ok(service::handle_health())
    }

    async fn shader_compile_capabilities(&self) -> Result<Vec<String>, ErrorObjectOwned> {
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
