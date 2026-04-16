// SPDX-License-Identifier: AGPL-3.0-or-later
//! Cross-primal end-to-end test: spawn the real `coralreef` binary in `server` mode,
//! exercise JSON-RPC over newline-delimited TCP via [`primal_rpc_client::RpcClient`],
//! SPIR-V compilation, optional tarpc, and graceful shutdown.
//!
//! Transport addresses are read from the discovery file written by the server (avoids relying
//! on piped `stderr`, which may be block-buffered when not attached to a TTY).
//!
//! Requires `--features e2e` (tarpc client types) and a built `coralreef` binary.
//!
//! Run:
//! `cargo build -p coralreef-core --bin coralreef && cargo test -p coralreef-core --test e2e_cross_primal --features e2e -- --ignored`
#![cfg(feature = "e2e")]

use std::net::SocketAddr;
use std::time::Duration;

use coralreef_core::config;
use coralreef_core::ipc::ShaderCompileTarpcClient;
use coralreef_core::service;
use primal_rpc_client::{RpcClient, no_params};
use serde_json::Value;
use tokio_serde::formats::Bincode;

/// Minimum SPIR-V for a trivial compute shader (WGSL → SPIR-V via naga), matching IPC unit tests.
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

/// Wait for the discovery file this server wrote (`pid` must match the spawned child).
async fn wait_for_discovery_tcp_addrs(child_pid: u32) -> Result<(SocketAddr, SocketAddr), String> {
    let path = config::discovery_dir()
        .map_err(|e| e.to_string())?
        .join(format!("{}.json", env!("CARGO_PKG_NAME")));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    while tokio::time::Instant::now() < deadline {
        match tokio::fs::read_to_string(&path).await {
            Ok(text) => {
                let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                let file_pid = v
                    .get("pid")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "discovery JSON missing pid".to_string())?;
                if file_pid != u64::from(child_pid) {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
                let jsonrpc_s = v
                    .pointer("/transports/jsonrpc/bind")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "discovery JSON missing transports.jsonrpc.bind".to_string())?;
                let tarpc_s = v
                    .pointer("/transports/tarpc/bind")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "discovery JSON missing transports.tarpc.bind".to_string())?;
                if tarpc_s.starts_with("unix://") {
                    return Err(format!(
                        "expected TCP tarpc bind in e2e test, got {tarpc_s:?}"
                    ));
                }
                let rpc_addr: SocketAddr = jsonrpc_s
                    .parse()
                    .map_err(|e| format!("parse jsonrpc bind {jsonrpc_s:?}: {e}"))?;
                let tarpc_addr: SocketAddr = tarpc_s
                    .parse()
                    .map_err(|e| format!("parse tarpc bind {tarpc_s:?}: {e}"))?;
                return Ok((rpc_addr, tarpc_addr));
            }
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
    Err(format!(
        "timed out waiting for discovery file at {}",
        path.display()
    ))
}

#[cfg(unix)]
fn send_sigterm(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status();
}

#[cfg(not(unix))]
fn send_sigterm(_pid: u32) {}

#[tokio::test]
#[ignore = "requires built coralreef binary (cargo build -p coralreef-core --bin coralreef)"]
async fn e2e_spawned_binary_jsonrpc_and_tarpc() {
    let bin = env!("CARGO_BIN_EXE_coralreef");
    let mut child = tokio::process::Command::new(bin)
        .args([
            "server",
            "--rpc-bind",
            "127.0.0.1:0",
            "--tarpc-bind",
            "127.0.0.1:0",
            "--log-level",
            "info",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn coralreef server");

    let pid = child.id().expect("running server child");

    let (rpc_addr, tarpc_addr) = match wait_for_discovery_tcp_addrs(pid).await {
        Ok(x) => x,
        Err(e) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            panic!("failed to discover server addresses: {e}");
        }
    };

    let jsonrpc = RpcClient::tcp_line(rpc_addr);
    let wgsl_req = service::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1)\nfn main() {}"),
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
        fp64_strategy: None,
        fma_policy: None,
    };

    let wgsl_outcome: Result<service::CompileResponse, _> =
        jsonrpc.request("shader.compile.wgsl", [wgsl_req]).await;

    match wgsl_outcome {
        Ok(resp) => {
            assert!(
                !resp.binary.is_empty(),
                "shader.compile.wgsl should return non-empty binary when compilation succeeds"
            );
            assert_eq!(resp.size, resp.binary.len());
        }
        Err(e) => {
            eprintln!("shader.compile.wgsl returned error (NVVM/driver may be unavailable): {e}");
        }
    }

    let spirv_req = service::CompileRequest {
        spirv_words: valid_spirv_minimal_compute(),
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
    };

    let spirv_outcome: Result<service::CompileResponse, _> =
        jsonrpc.request("shader.compile.spirv", [spirv_req]).await;

    match spirv_outcome {
        Ok(resp) => {
            assert!(
                !resp.binary.is_empty(),
                "shader.compile.spirv should return non-empty binary when compilation succeeds"
            );
            assert_eq!(resp.size, resp.binary.len());
        }
        Err(e) => {
            eprintln!("shader.compile.spirv returned error (NVVM/driver may be unavailable): {e}");
        }
    }

    // Verify health via NDJSON
    let _health: service::HealthResponse = jsonrpc
        .request("shader.compile.status", no_params())
        .await
        .expect("health check should succeed");

    let transport = tarpc::serde_transport::tcp::connect(tarpc_addr, Bincode::default)
        .await
        .expect("tarpc TCP connect to spawned server");
    let tarpc_client =
        ShaderCompileTarpcClient::new(tarpc::client::Config::default(), transport).spawn();

    let tarpc_wgsl_req = service::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1)\nfn main() {}"),
        arch: coral_reef::GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
        fp64_strategy: None,
        fma_policy: None,
    };

    let tarpc_wgsl = tarpc_client
        .wgsl(tarpc::context::current(), tarpc_wgsl_req)
        .await
        .expect("tarpc wgsl call must complete");

    match tarpc_wgsl {
        Ok(resp) => {
            assert!(!resp.binary.is_empty());
            assert_eq!(resp.size, resp.binary.len());
        }
        Err(e) => {
            eprintln!("tarpc wgsl returned error (NVVM/driver may be unavailable): {e}");
        }
    }

    send_sigterm(pid);

    match tokio::time::timeout(Duration::from_secs(20), child.wait()).await {
        Ok(Ok(status)) => {
            let code = status.code();
            assert!(
                code == Some(130) || code == Some(0) || code.is_none(),
                "unexpected exit after SIGTERM: {status:?}"
            );
        }
        Ok(Err(e)) => panic!("wait on coralreef child failed: {e}"),
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            panic!("coralreef server did not exit within timeout after SIGTERM");
        }
    }
}
