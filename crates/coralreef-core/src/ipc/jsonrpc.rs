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
/// Method names follow `shader.compile.*` and `health.*` — aligned with
/// capability advertisement and wateringHole `SEMANTIC_METHOD_NAMING_STANDARD`.
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

    /// `shader.compile.capabilities` — structured capability report including
    /// supported architectures and f64 transcendental lowering capabilities.
    #[method(name = "shader.compile.capabilities")]
    async fn shader_compile_capabilities(
        &self,
    ) -> Result<service::CompileCapabilitiesResponse, ErrorObjectOwned>;

    /// `shader.compile.wgsl.multi` — compile WGSL to multiple GPU targets at once.
    #[method(name = "shader.compile.wgsl.multi")]
    async fn shader_compile_wgsl_multi(
        &self,
        request: service::MultiDeviceCompileRequest,
    ) -> Result<service::MultiDeviceCompileResponse, ErrorObjectOwned>;

    /// `health.check` — full health probe per wateringHole standard.
    #[method(name = "health.check")]
    async fn health_check(&self) -> Result<service::HealthCheckResponse, ErrorObjectOwned>;

    /// `health.liveness` — lightweight alive probe.
    #[method(name = "health.liveness")]
    async fn health_liveness(&self) -> Result<service::LivenessResponse, ErrorObjectOwned>;

    /// `health.readiness` — ready to accept compilation requests.
    #[method(name = "health.readiness")]
    async fn health_readiness(&self) -> Result<service::ReadinessResponse, ErrorObjectOwned>;

    /// `identity.get` — primal self-description for capability-based discovery.
    #[method(name = "identity.get")]
    async fn identity_get(&self) -> Result<service::IdentityGetResponse, ErrorObjectOwned>;

    /// `capabilities.list` — list capability IDs this primal provides.
    #[method(name = "capabilities.list")]
    async fn capabilities_list(&self) -> Result<Vec<String>, ErrorObjectOwned>;

    /// `shader.compile.cpu` — compile/validate WGSL for CPU execution.
    #[method(name = "shader.compile.cpu")]
    async fn shader_compile_cpu(
        &self,
        request: coral_reef_cpu::CompileCpuRequest,
    ) -> Result<service::CompileResponse, ErrorObjectOwned>;

    /// `shader.execute.cpu` — execute WGSL on the CPU interpreter.
    #[method(name = "shader.execute.cpu")]
    async fn shader_execute_cpu(
        &self,
        request: coral_reef_cpu::ExecuteCpuRequest,
    ) -> Result<coral_reef_cpu::ExecuteCpuResponse, ErrorObjectOwned>;

    /// `shader.validate` — execute on CPU and compare against expected values.
    #[method(name = "shader.validate")]
    async fn shader_validate(
        &self,
        request: coral_reef_cpu::ValidateRequest,
    ) -> Result<coral_reef_cpu::ValidateResponse, ErrorObjectOwned>;
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

    async fn shader_compile_capabilities(
        &self,
    ) -> Result<service::CompileCapabilitiesResponse, ErrorObjectOwned> {
        Ok(service::handle_compile_capabilities())
    }

    async fn shader_compile_wgsl_multi(
        &self,
        request: service::MultiDeviceCompileRequest,
    ) -> Result<service::MultiDeviceCompileResponse, ErrorObjectOwned> {
        service::handle_compile_wgsl_multi(request).map_err(|e| compile_error_to_rpc(&e))
    }

    async fn health_check(&self) -> Result<service::HealthCheckResponse, ErrorObjectOwned> {
        Ok(service::handle_health_check())
    }

    async fn health_liveness(&self) -> Result<service::LivenessResponse, ErrorObjectOwned> {
        Ok(service::handle_health_liveness())
    }

    async fn health_readiness(&self) -> Result<service::ReadinessResponse, ErrorObjectOwned> {
        Ok(service::handle_health_readiness())
    }

    async fn identity_get(&self) -> Result<service::IdentityGetResponse, ErrorObjectOwned> {
        Ok(service::handle_identity_get())
    }

    async fn capabilities_list(&self) -> Result<Vec<String>, ErrorObjectOwned> {
        let desc = crate::capability::self_description();
        Ok(desc.provides.iter().map(|c| c.id.to_string()).collect())
    }

    async fn shader_compile_cpu(
        &self,
        request: coral_reef_cpu::CompileCpuRequest,
    ) -> Result<service::CompileResponse, ErrorObjectOwned> {
        service::handle_compile_cpu(&request).map_err(|e| compile_error_to_rpc(&e))
    }

    async fn shader_execute_cpu(
        &self,
        request: coral_reef_cpu::ExecuteCpuRequest,
    ) -> Result<coral_reef_cpu::ExecuteCpuResponse, ErrorObjectOwned> {
        service::handle_execute_cpu(&request).map_err(|e| compile_error_to_rpc(&e))
    }

    async fn shader_validate(
        &self,
        request: coral_reef_cpu::ValidateRequest,
    ) -> Result<coral_reef_cpu::ValidateResponse, ErrorObjectOwned> {
        service::handle_validate(&request).map_err(|e| compile_error_to_rpc(&e))
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
