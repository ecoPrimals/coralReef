// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]
use coral_gpu::{AmdArch, GpuContext, GpuTarget, NvArch};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

const VECADD_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> result: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    result[gid.x] = a[gid.x] + b[gid.x];
}
"#;

const STORE_42_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(1)
fn main() {
    output[0] = 42u;
}
"#;

/// Scan the ecosystem discovery directory for a provider advertising `capability_id`,
/// returning its endpoint (socket path) if found. No hardcoded primal names.
fn discover_provider(capability_id: &str) -> Option<PathBuf> {
    let dir = coralreef_core::config::discovery_dir().ok()?;
    let entries = std::fs::read_dir(&dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let data = std::fs::read_to_string(&path).ok()?;
        let val: serde_json::Value = serde_json::from_str(&data).ok()?;
        let has_capability = val
            .get("provides")
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter().any(|c| {
                    c.as_str() == Some(capability_id)
                        || c.get("id").and_then(|i| i.as_str()) == Some(capability_id)
                })
            })
            .unwrap_or(false)
            || val
                .get("capabilities")
                .and_then(|p| p.as_array())
                .map(|arr| arr.iter().any(|c| c.as_str() == Some(capability_id)))
                .unwrap_or(false);

        if has_capability {
            if let Some(endpoint) = val.get("endpoint").and_then(|e| e.as_str()) {
                return Some(PathBuf::from(endpoint));
            }
        }
    }
    None
}

fn jsonrpc_call(socket: &PathBuf, method: &str, params: serde_json::Value) -> Option<serde_json::Value> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });

    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;

    let payload = serde_json::to_vec(&request).ok()?;
    stream.write_all(&payload).ok()?;
    stream.flush().ok()?;

    let mut response = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
        if serde_json::from_slice::<serde_json::Value>(&response).is_ok() {
            break;
        }
    }

    serde_json::from_slice(&response).ok()
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║          coralReef — Full Compute Triangle              ║");
    println!("║  compile (shader.compile) → orchestrate (gpu.orchestrate)║");
    println!("║                          → execute (gpu.dispatch)       ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();

    // ── Layer 1: Compilation (always available) ──────────────────────

    println!("Layer 1: Shader Compilation (coralReef)");
    println!();

    let targets = [
        ("NVIDIA SM86 (RTX 3090)", GpuTarget::Nvidia(NvArch::Sm86)),
        ("AMD RDNA2 (RX 6950 XT)", GpuTarget::Amd(AmdArch::Rdna2)),
    ];

    for (name, target) in &targets {
        let ctx = GpuContext::new(*target).expect("compiler init");

        let is_nv = target.as_nvidia().is_some();
        let shader_name = if is_nv { "vecadd (SM86)" } else { "store_42 (RDNA2)" };
        let shader_src = if is_nv { VECADD_SHADER } else { STORE_42_SHADER };

        match ctx.compile_wgsl(shader_src) {
            Ok(kernel) => {
                println!("  {name}:");
                println!("    Shader:       {shader_name}");
                println!("    Binary:       {} bytes", kernel.binary.len());
                println!("    GPRs:         {}", kernel.gpr_count);
                println!("    Instructions: {}", kernel.instr_count);
            }
            Err(e) => {
                println!("  {name}:");
                println!("    Shader:       {shader_name}");
                println!("    Status:       compile limitation — {e}");
                println!("    (known RDNA2 limitation for global_invocation_id)");
            }
        }
        println!();
    }

    // ── Layer 2: Orchestration (discovered by "gpu.orchestrate" capability) ──

    println!("Layer 2: Orchestration (capability: gpu.orchestrate)");
    println!();

    let orchestrator = discover_provider("gpu.orchestrate");
    let orchestrator_available = orchestrator.is_some();

    if let Some(ref sock) = orchestrator {
        println!("  Orchestrator: AVAILABLE at {}", sock.display());

        if let Some(resp) = jsonrpc_call(
            sock,
            "science.gpu.capabilities",
            serde_json::json!({}),
        ) {
            println!("  GPU capabilities:");
            if let Some(result) = resp.get("result") {
                let pretty = serde_json::to_string_pretty(result).unwrap_or_default();
                for line in pretty.lines().take(10) {
                    println!("    {line}");
                }
                let total_lines = pretty.lines().count();
                if total_lines > 10 {
                    println!("    ... ({} more lines)", total_lines - 10);
                }
            }
        }
    } else {
        println!("  No gpu.orchestrate provider discovered.");
        println!("  Operating in standalone mode — coralReef compiles independently.");
    }
    println!();

    // ── Layer 3: Execution (discovered by "gpu.dispatch" capability) ─────────

    println!("Layer 3: Execution (capability: gpu.dispatch)");
    println!();

    let executor = discover_provider("gpu.dispatch");
    let executor_available = executor.is_some();

    if let Some(ref sock) = executor {
        println!("  GPU dispatch provider: AVAILABLE at {}", sock.display());

        if let Some(resp) = jsonrpc_call(
            sock,
            "compute.submit",
            serde_json::json!({
                "shader_wgsl": STORE_42_SHADER,
                "buffers": [{"size": 256, "domain": "vram"}],
                "dispatch": [1, 1, 1],
            }),
        ) {
            println!("  Compute result:");
            if let Some(result) = resp.get("result") {
                println!("    {result}");
            } else if let Some(error) = resp.get("error") {
                println!("    Error: {error}");
            }
        }
    } else {
        println!("  No gpu.dispatch provider discovered.");
        println!();

        #[cfg(target_os = "linux")]
        {
            println!("  Attempting local sovereign dispatch (coralReef direct)...");
            match GpuContext::auto() {
                Ok(mut ctx) => {
                    let kernel = ctx.compile_wgsl(STORE_42_SHADER).expect("compilation");
                    match ctx.alloc(256) {
                        Ok(buf) => {
                            match ctx.dispatch(&kernel, &[buf], [1, 1, 1]) {
                                Ok(()) => {
                                    ctx.sync().expect("sync");
                                    match ctx.readback(buf, 4) {
                                        Ok(data) => {
                                            let v = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                                            println!("    Local dispatch result: output[0] = {v}");
                                            if v == 42 {
                                                println!("    PASS — sovereign pipeline working end-to-end.");
                                            }
                                        }
                                        Err(e) => println!("    Readback: {e}"),
                                    }
                                    let _ = ctx.free(buf);
                                }
                                Err(e) => println!("    Dispatch: {e}"),
                            }
                        }
                        Err(e) => println!("    Alloc: {e}"),
                    }
                }
                Err(e) => {
                    println!("    No local GPU: {e}");
                    println!("    coralReef can compile shaders (Layer 1) without hardware.");
                }
            }
        }

        #[cfg(not(target_os = "linux"))]
        println!("  Local dispatch requires Linux. Compile-only mode.");
    }

    println!();
    println!("Summary:");
    println!();
    println!("  shader.compile   (coralReef)       — ALWAYS available (pure Rust compiler)");
    println!(
        "  gpu.orchestrate  (capability scan) — {}",
        if orchestrator_available { "CONNECTED" } else { "not discovered (standalone OK)" }
    );
    println!(
        "  gpu.dispatch     (capability scan) — {}",
        if executor_available { "CONNECTED" } else { "not discovered (local dispatch OK)" }
    );
    println!();
    println!("  The compute triangle degrades gracefully.");
    println!("  Each layer adds capability. None is required.");
}
