// SPDX-License-Identifier: AGPL-3.0-only
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

fn ecosystem_socket(capability: &str) -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join(coralreef_core::config::ECOSYSTEM_NAMESPACE)
        .join(format!("{capability}.sock"))
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
    println!("║  compile (coralReef) → orchestrate (toadStool)          ║");
    println!("║                     → execute (barraCuda)               ║");
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

    // ── Layer 2: Orchestration (toadStool, when available) ───────────

    println!("Layer 2: Orchestration (toadStool)");
    println!();

    let toadstool_sock = ecosystem_socket("toadstool");
    let toadstool_available = toadstool_sock.exists();

    if toadstool_available {
        println!("  toadStool: AVAILABLE at {}", toadstool_sock.display());

        if let Some(resp) = jsonrpc_call(
            &toadstool_sock,
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
        println!("  toadStool: not found at {}", toadstool_sock.display());
        println!("  Operating in standalone mode — coralReef compiles independently.");
        println!("  Start toadStool to enable orchestrated compute dispatch.");
    }
    println!();

    // ── Layer 3: Execution (barraCuda or local dispatch) ─────────────

    println!("Layer 3: Execution");
    println!();

    let compute_sock = ecosystem_socket("compute");
    let barracuda_available = compute_sock.exists();

    if barracuda_available {
        println!("  barraCuda: AVAILABLE at {}", compute_sock.display());

        if let Some(resp) = jsonrpc_call(
            &compute_sock,
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
        println!("  barraCuda: not found at {}", compute_sock.display());
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
    println!("  coralReef  (compile)     — ALWAYS available (pure Rust compiler)");
    println!(
        "  toadStool  (orchestrate) — {}",
        if toadstool_available { "CONNECTED" } else { "not running (standalone OK)" }
    );
    println!(
        "  barraCuda  (execute)     — {}",
        if barracuda_available { "CONNECTED" } else { "not running (local dispatch OK)" }
    );
    println!();
    println!("  The compute triangle degrades gracefully.");
    println!("  Each layer adds capability. None is required.");
}
