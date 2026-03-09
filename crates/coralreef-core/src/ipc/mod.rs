// SPDX-License-Identifier: AGPL-3.0-only
//! IPC transports — JSON-RPC 2.0 and tarpc servers.
//!
//! Follows wateringHole `UNIVERSAL_IPC_STANDARD_V3.md`:
//! - JSON-RPC 2.0 as primary protocol (TCP/HTTP — external, debuggable)
//! - tarpc as optional high-performance channel (TCP or Unix socket — internal)
//! - Semantic method names: `shader.compile.{spirv,wgsl,status,capabilities}`
//!
//! ## Platform-agnostic transport (ecoBin compliance)
//!
//! On Unix platforms, tarpc defaults to Unix domain sockets for lower overhead
//! on local primal-to-primal communication. JSON-RPC stays TCP-based (HTTP).
//! On non-Unix platforms, both protocols use TCP loopback.

use std::fmt;
use std::net::SocketAddr;

mod jsonrpc;
pub use jsonrpc::start_jsonrpc_server;

mod tarpc_transport;
pub use tarpc_transport::start_tarpc_server;
#[cfg(all(test, unix))]
pub use tarpc_transport::start_tarpc_unix_server;
#[cfg(test)]
pub use tarpc_transport::{ShaderCompileTarpcClient, start_tarpc_tcp_server};

/// Errors from IPC server operations.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
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
pub enum BoundAddr {
    /// TCP socket address (host:port).
    Tcp(SocketAddr),
    /// Unix domain socket path (Unix platforms only).
    #[cfg(unix)]
    Unix(std::path::PathBuf),
}

impl BoundAddr {
    /// Protocol name for capability advertisement.
    pub(crate) const fn protocol(&self) -> &'static str {
        match self {
            Self::Tcp(_) => "tcp",
            #[cfg(unix)]
            Self::Unix(_) => "unix",
        }
    }
}

impl fmt::Display for BoundAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp(addr) => write!(f, "{addr}"),
            #[cfg(unix)]
            Self::Unix(path) => write!(f, "unix://{}", path.display()),
        }
    }
}

/// TCP loopback with OS-assigned port (fallback when `CORALREEF_TCP_BIND` is unset).
const FALLBACK_TCP_BIND: &str = "127.0.0.1:0";

/// Resolve the TCP bind address for JSON-RPC.
///
/// Checks `$CORALREEF_TCP_BIND` first for deployment configuration,
/// then falls back to loopback with OS-assigned port.
pub fn default_tcp_bind() -> String {
    std::env::var("CORALREEF_TCP_BIND").unwrap_or_else(|_| FALLBACK_TCP_BIND.to_owned())
}

/// Platform-aware default bind address for tarpc.
///
/// On Unix: returns a path for a Unix domain socket under `$XDG_RUNTIME_DIR`
/// (or `std::env::temp_dir()` as fallback — no hardcoded paths per ecoBin),
/// namespaced by the primal binary name.
/// On non-Unix: returns TCP loopback with OS-assigned port.
pub fn default_tarpc_bind() -> String {
    #[cfg(unix)]
    {
        let dir = coralreef_core::config::discovery_dir().unwrap_or_else(|_| {
            std::env::temp_dir().join(coralreef_core::config::ECOSYSTEM_NAMESPACE)
        });
        let sock = dir.join(format!("{}-tarpc.sock", env!("CARGO_PKG_NAME")));
        format!("unix://{}", sock.display())
    }
    #[cfg(not(unix))]
    {
        default_tcp_bind()
    }
}

#[cfg(test)]
mod tests {
    // All panic!("expected TCP address") below are test-only assertions:
    // start_tarpc_tcp_server returns BoundAddr::Tcp by design.
    use super::*;
    use crate::service;
    use tokio::sync::watch;

    fn test_shutdown_channel() -> (watch::Sender<()>, watch::Receiver<()>) {
        watch::channel(())
    }

    /// Generate valid SPIR-V for a minimal compute shader via naga (WGSL → SPIR-V).
    fn valid_spirv_minimal_compute() -> Vec<u32> {
        let wgsl = "@compute @workgroup_size(1) fn main() {}";
        let module = naga::front::wgsl::parse_str(wgsl).expect("WGSL should parse");
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::default(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("module should validate");
        naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
            .expect("SPIR-V write should succeed")
    }

    #[tokio::test]
    async fn test_jsonrpc_server_starts() {
        let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
        assert_ne!(addr.port(), 0);
    }

    #[tokio::test]
    async fn test_tarpc_tcp_server_starts() {
        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
        assert!(matches!(addr, BoundAddr::Tcp(_)));
    }

    #[tokio::test]
    async fn test_tarpc_server_auto_tcp() {
        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_server(FALLBACK_TCP_BIND, rx).await.unwrap();
        assert!(matches!(addr, BoundAddr::Tcp(_)));
    }

    #[tokio::test]
    async fn test_tarpc_tcp_invalid_bind_address() {
        let (_tx, rx) = test_shutdown_channel();
        let result = start_tarpc_tcp_server("not-a-valid-address", rx).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("invalid")
                || err.to_string().to_lowercase().contains("address"),
            "invalid bind should produce address parse error: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_tarpc_server_invalid_bind_returns_error() {
        let (_tx, rx) = test_shutdown_channel();
        let result = start_tarpc_server("garbage:not-valid", rx).await;
        assert!(result.is_err());
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
    async fn test_tarpc_unix_server_invalid_path() {
        let (_tx, rx) = test_shutdown_channel();
        // Binding to a directory path fails (UnixListener expects a file path)
        let path = std::env::temp_dir();
        let result = start_tarpc_unix_server(&path, rx).await;
        assert!(result.is_err());
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

        let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let response: service::HealthResponse = client
            .request("shader.compile.status", jsonrpsee::rpc_params![])
            .await
            .unwrap();

        assert_eq!(response.name, env!("CARGO_PKG_NAME"));
        assert!(!response.supported_archs.is_empty());
    }

    #[tokio::test]
    async fn test_jsonrpc_supported_archs_endpoint() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let archs: Vec<String> = client
            .request("shader.compile.capabilities", jsonrpsee::rpc_params![])
            .await
            .unwrap();

        let default_arch = coral_reef::GpuArch::default().to_string();
        assert!(archs.contains(&default_arch));
    }

    #[tokio::test]
    async fn test_jsonrpc_compile_empty_spirv() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let req = service::CompileRequest {
            spirv_words: vec![],
            arch: coral_reef::GpuArch::default().to_string(),
            opt_level: 2,
            fp64_software: true,
        };

        let result: Result<service::CompileResponse, _> =
            client.request("shader.compile.spirv", [req]).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_tarpc_health_endpoint() {
        use tokio_serde::formats::Bincode;

        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
        let BoundAddr::Tcp(tcp_addr) = addr else {
            panic!("expected TCP address");
        };

        let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
            .await
            .unwrap();
        let client =
            ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

        let response = client.status(tarpc::context::current()).await.unwrap();

        assert_eq!(response.name, env!("CARGO_PKG_NAME"));
    }

    #[tokio::test]
    async fn test_tarpc_compile_empty_spirv() {
        use bytes::Bytes;
        use tokio_serde::formats::Bincode;

        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
        let BoundAddr::Tcp(tcp_addr) = addr else {
            panic!("expected TCP address");
        };

        let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
            .await
            .unwrap();
        let client =
            ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

        let req = service::CompileSpirvRequestTarpc {
            spirv: Bytes::new(),
            arch: coral_reef::GpuArch::default().to_string(),
            opt_level: 2,
            fp64_software: true,
        };

        let result = client.spirv(tarpc::context::current(), req).await.unwrap();

        assert!(result.is_err());
    }

    // ---------------------------------------------------------------------------
    // JSON-RPC E2E compile tests
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test_jsonrpc_compile_valid_shader() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let spirv = valid_spirv_minimal_compute();
        let req = service::CompileRequest {
            spirv_words: spirv,
            arch: coral_reef::GpuArch::default().to_string(),
            opt_level: 2,
            fp64_software: true,
        };

        let response: Result<service::CompileResponse, _> =
            client.request("shader.compile.spirv", [req]).await;

        match response {
            Ok(resp) => {
                assert!(
                    !resp.binary.is_empty(),
                    "response should contain non-empty binary"
                );
                assert_eq!(resp.size, resp.binary.len());
            }
            Err(e) => {
                let msg = format!("{e:?}");
                assert!(
                    msg.contains("not implemented") || msg.contains("-32000"),
                    "IPC should propagate compile errors: {msg}"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_jsonrpc_compile_wgsl_shader() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let req = service::CompileWgslRequest {
            wgsl_source: "@compute @workgroup_size(1)\nfn main() {}".to_owned(),
            arch: "sm_70".to_owned(),
            opt_level: 2,
            fp64_software: true,
        };

        let response: Result<service::CompileResponse, _> =
            client.request("shader.compile.wgsl", [req]).await;

        match response {
            Ok(resp) => {
                assert!(
                    !resp.binary.is_empty(),
                    "WGSL compile should produce non-empty binary"
                );
                assert_eq!(resp.size, resp.binary.len());
            }
            Err(e) => {
                let msg = format!("{e:?}");
                assert!(
                    msg.contains("-32000"),
                    "IPC should propagate compile errors: {msg}"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_jsonrpc_compile_error_propagation() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let req_bad_arch = service::CompileRequest {
            spirv_words: valid_spirv_minimal_compute(),
            arch: "sm_99".to_string(),
            opt_level: 2,
            fp64_software: true,
        };
        let err: Result<service::CompileResponse, _> =
            client.request("shader.compile.spirv", [req_bad_arch]).await;
        assert!(err.is_err(), "invalid arch should return JSON-RPC error");
        let err_msg = format!("{:?}", err.unwrap_err());
        assert!(
            err_msg.contains("-32000")
                || err_msg.contains("sm_99")
                || err_msg.contains("UnsupportedArch"),
            "error should indicate compile failure: {err_msg}"
        );

        let req_bad_spirv = service::CompileRequest {
            spirv_words: vec![0xDEAD_BEEF, 0x0001_0000, 0, 0, 0],
            arch: coral_reef::GpuArch::default().to_string(),
            opt_level: 2,
            fp64_software: true,
        };
        let err2: Result<service::CompileResponse, _> = client
            .request("shader.compile.spirv", [req_bad_spirv])
            .await;
        assert!(err2.is_err(), "bad SPIR-V should return JSON-RPC error");
    }

    // ---------------------------------------------------------------------------
    // tarpc E2E compile tests
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test_tarpc_compile_valid_shader() {
        use bytes::Bytes;
        use tokio_serde::formats::Bincode;

        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
        let BoundAddr::Tcp(tcp_addr) = addr else {
            panic!("expected TCP address");
        };

        let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
            .await
            .unwrap();
        let client =
            ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

        let spirv_words = valid_spirv_minimal_compute();
        let spirv_bytes: Vec<u8> = spirv_words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let req = service::CompileSpirvRequestTarpc {
            spirv: Bytes::from(spirv_bytes),
            arch: coral_reef::GpuArch::default().to_string(),
            opt_level: 2,
            fp64_software: true,
        };

        let response = client.spirv(tarpc::context::current(), req).await.unwrap();

        match response {
            Ok(resp) => {
                assert!(
                    !resp.binary.is_empty(),
                    "response should contain non-empty binary"
                );
                assert_eq!(resp.size, resp.binary.len());
            }
            Err(msg) => {
                assert!(
                    msg.contains("not implemented") || msg.contains("NotImplemented"),
                    "IPC should propagate compile errors: {msg}"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_tarpc_compile_error_propagation() {
        use bytes::Bytes;
        use tokio_serde::formats::Bincode;

        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
        let BoundAddr::Tcp(tcp_addr) = addr else {
            panic!("expected TCP address");
        };

        let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
            .await
            .unwrap();
        let client =
            ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

        let spirv_bytes: Vec<u8> = valid_spirv_minimal_compute()
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let req_bad_arch = service::CompileSpirvRequestTarpc {
            spirv: Bytes::from(spirv_bytes),
            arch: "sm_99".to_string(),
            opt_level: 2,
            fp64_software: true,
        };
        let result = client
            .spirv(tarpc::context::current(), req_bad_arch)
            .await
            .unwrap();
        assert!(result.is_err(), "invalid arch should return Err");

        let bad_spirv_words = [0xDEAD_BEEF_u32, 0x0001_0000, 0, 0, 0];
        let bad_spirv_bytes: Vec<u8> = bad_spirv_words
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let req_bad_spirv = service::CompileSpirvRequestTarpc {
            spirv: Bytes::from(bad_spirv_bytes),
            arch: coral_reef::GpuArch::default().to_string(),
            opt_level: 2,
            fp64_software: true,
        };
        let result2 = client
            .spirv(tarpc::context::current(), req_bad_spirv)
            .await
            .unwrap();
        assert!(result2.is_err(), "bad SPIR-V should return Err");
    }

    // ---------------------------------------------------------------------------
    // Cross-protocol consistency
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test_cross_protocol_health_consistency() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;
        use tokio_serde::formats::Bincode;

        let (rpc_addr, _rpc_handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
        let (_tx, rx) = test_shutdown_channel();
        let (tarpc_addr, _tarpc_handle) =
            start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();

        let url = format!("http://{rpc_addr}");
        let rpc_client = HttpClientBuilder::default().build(&url).unwrap();
        let jsonrpc_health: service::HealthResponse = rpc_client
            .request("shader.compile.status", jsonrpsee::rpc_params![])
            .await
            .unwrap();

        let BoundAddr::Tcp(tcp_addr) = tarpc_addr else {
            panic!("expected TCP address");
        };
        let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
            .await
            .unwrap();
        let tarpc_client =
            ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();
        let tarpc_health = tarpc_client
            .status(tarpc::context::current())
            .await
            .unwrap();

        assert_eq!(jsonrpc_health.name, tarpc_health.name);
        assert_eq!(jsonrpc_health.version, tarpc_health.version);
        assert_eq!(jsonrpc_health.status, tarpc_health.status);
        assert_eq!(jsonrpc_health.supported_archs, tarpc_health.supported_archs);
    }

    // ---------------------------------------------------------------------------
    // Shutdown tests
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test_graceful_shutdown() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (shutdown_tx, shutdown_rx) = test_shutdown_channel();
        let (rpc_addr, rpc_handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
        let (_tarpc_addr, tarpc_handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, shutdown_rx)
            .await
            .unwrap();

        let url = format!("http://{rpc_addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();
        let _health: service::HealthResponse = client
            .request("shader.compile.status", jsonrpsee::rpc_params![])
            .await
            .unwrap();

        let _ = shutdown_tx.send(());
        let _ = rpc_handle.stop();

        let shutdown_timeout = std::time::Duration::from_secs(5);
        let rpc_stopped = rpc_handle.clone().stopped();
        let shutdown_result = tokio::time::timeout(shutdown_timeout, async move {
            rpc_stopped.await;
            tarpc_handle.await.ok();
        })
        .await;

        assert!(
            shutdown_result.is_ok(),
            "servers should shut down cleanly within timeout"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_graceful_shutdown_unix() {
        let dir = std::env::temp_dir().join("coralreef-test");
        let _ = std::fs::create_dir_all(&dir);
        let sock_path = dir.join(format!("shutdown-{}.sock", std::process::id()));

        let (shutdown_tx, shutdown_rx) = test_shutdown_channel();
        let (_addr, handle) = start_tarpc_unix_server(&sock_path, shutdown_rx)
            .await
            .unwrap();

        let _ = shutdown_tx.send(());

        let shutdown_timeout = std::time::Duration::from_secs(3);
        let shutdown_result = tokio::time::timeout(shutdown_timeout, handle).await;

        assert!(
            shutdown_result.is_ok(),
            "unix tarpc server should shut down cleanly on signal"
        );

        let _ = std::fs::remove_file(&sock_path);
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
        assert_eq!(bind, FALLBACK_TCP_BIND);
    }

    // ---------------------------------------------------------------------------
    // tarpc capabilities endpoint
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test_tarpc_capabilities() {
        use tokio_serde::formats::Bincode;

        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
        let BoundAddr::Tcp(tcp_addr) = addr else {
            panic!("expected TCP address");
        };

        let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
            .await
            .unwrap();
        let client =
            ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

        let caps = client
            .capabilities(tarpc::context::current())
            .await
            .unwrap();
        assert!(!caps.is_empty(), "capabilities must list at least one arch");
        assert!(
            caps.iter().any(|a| a == "sm_70"),
            "must include sm_70 baseline"
        );
    }

    // ---------------------------------------------------------------------------
    // tarpc WGSL compile endpoint
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test_tarpc_compile_wgsl() {
        use tokio_serde::formats::Bincode;

        let (_tx, rx) = test_shutdown_channel();
        let (addr, _handle) = start_tarpc_tcp_server(FALLBACK_TCP_BIND, rx).await.unwrap();
        let BoundAddr::Tcp(tcp_addr) = addr else {
            panic!("expected TCP address");
        };

        let transport = tarpc::serde_transport::tcp::connect(tcp_addr, Bincode::default)
            .await
            .unwrap();
        let client =
            ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

        let req = service::CompileWgslRequest {
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".to_string(),
            arch: coral_reef::GpuArch::default().to_string(),
            opt_level: 2,
            fp64_software: true,
        };
        let result = client.wgsl(tarpc::context::current(), req).await.unwrap();
        assert!(result.is_ok(), "WGSL compile should succeed");
        let resp = result.unwrap();
        assert!(!resp.binary.is_empty());
    }

    // ---------------------------------------------------------------------------
    // JSON-RPC differentiated error codes
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test_jsonrpc_error_code_invalid_input() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let req = service::CompileRequest {
            spirv_words: vec![0xDEAD_BEEF],
            arch: coral_reef::GpuArch::default().to_string(),
            opt_level: 2,
            fp64_software: true,
        };

        let result: Result<service::CompileResponse, _> =
            client.request("shader.compile.spirv", [req]).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("-32001") || err_str.contains("invalid input"),
            "bad SPIR-V should produce error code -32001: got {err_str}"
        );
    }

    #[tokio::test]
    async fn test_jsonrpc_capabilities_endpoint() {
        use jsonrpsee::core::client::ClientT;
        use jsonrpsee::http_client::HttpClientBuilder;

        let (addr, _handle) = start_jsonrpc_server(FALLBACK_TCP_BIND).await.unwrap();
        let url = format!("http://{addr}");
        let client = HttpClientBuilder::default().build(&url).unwrap();

        let archs: Vec<String> = client
            .request("shader.compile.capabilities", jsonrpsee::rpc_params![])
            .await
            .unwrap();
        assert!(!archs.is_empty());
        assert!(archs.iter().any(|a| a == "sm_70"));
    }
}
