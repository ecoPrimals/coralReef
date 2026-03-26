// SPDX-License-Identifier: AGPL-3.0-only
//! RPC handlers: device lifecycle, compute, dispatch, and system health.

use crate::rpc::{check_rpc_error, rpc_call};

use base64::Engine;
use serde_json::json;

pub(crate) fn rpc_status(socket: &str) {
    let response = rpc_call(socket, "device.list", json!({}));
    check_rpc_error(&response);

    let result = match response.get("result") {
        Some(r) => r,
        None => {
            eprintln!("error: no result in response");
            std::process::exit(1);
        }
    };

    let devices = if result.is_array() {
        result.as_array()
    } else {
        result.get("devices").and_then(|d| d.as_array())
    };

    match devices {
        Some(devs) if !devs.is_empty() => {
            println!(
                "{:<16} {:<22} {:<6} {:<6} NAME",
                "BDF", "PERSONALITY", "POWER", "VRAM",
            );
            println!("{}", "-".repeat(70));
            for dev in devs {
                let bdf = dev.get("bdf").and_then(|v| v.as_str()).unwrap_or("?");
                let personality = dev
                    .get("personality")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let power = dev.get("power").and_then(|v| v.as_str()).unwrap_or("?");
                let vram = if dev
                    .get("vram_alive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    "ok"
                } else {
                    "-"
                };
                let name = dev.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let protected = dev
                    .get("protected")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let suffix = if protected {
                    format!("{name} [PROTECTED]")
                } else {
                    name.to_string()
                };
                println!("{bdf:<16} {personality:<22} {power:<6} {vram:<6} {suffix}");
            }
        }
        _ => {
            println!("no devices managed");
        }
    }
}

pub(crate) fn rpc_swap(socket: &str, bdf: &str, target: &str, trace: bool) {
    if trace {
        println!("swapping {bdf} -> {target} (mmiotrace capture enabled)...");
    } else {
        println!("swapping {bdf} -> {target}...");
    }

    let mut params = json!({
        "bdf": bdf,
        "target": target,
    });
    if trace {
        params["trace"] = json!(true);
    }

    let response = rpc_call(socket, "device.swap", params);
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let personality = result
            .get("personality")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let vram = result
            .get("vram_alive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if trace {
            println!("ok: {bdf} now on {personality} (vram_alive={vram}, trace captured)");
        } else {
            println!("ok: {bdf} now on {personality} (vram_alive={vram})");
        }
    }
}

pub(crate) fn rpc_reset(socket: &str, bdf: &str, method: &str) {
    match method {
        "flr" => {
            println!("resetting {bdf} via VFIO FLR...");
            let response = rpc_call(socket, "device.reset", json!({"bdf": bdf}));
            check_rpc_error(&response);
            println!("ok: {bdf} FLR reset complete");
        }
        "sbr" | "bridge-sbr" | "remove-rescan" | "auto" => {
            let label = match method {
                "auto" => "auto-detect",
                "bridge-sbr" => "bridge SBR",
                "remove-rescan" => "PCI remove+rescan",
                _ => "device SBR",
            };
            println!("resetting {bdf} via {label}...");
            let response = rpc_call(
                socket,
                "device.reset",
                json!({"bdf": bdf, "method": method}),
            );
            check_rpc_error(&response);
            let actual_method = response
                .get("result")
                .and_then(|r| r.get("method"))
                .and_then(|v| v.as_str())
                .unwrap_or(method);
            println!("ok: {bdf} reset complete (method={actual_method})");
        }
        other => {
            eprintln!("error: unknown reset method '{other}' (use: auto, flr, sbr, bridge-sbr, remove-rescan)");
            std::process::exit(1);
        }
    }
}

pub(crate) fn rpc_warm_fecs(socket: &str, bdf: &str, settle_secs: u64) {
    println!("=== Warm FECS via nouveau round-trip ===");
    println!("step 1: swapping {bdf} -> nouveau (loads ACR → FECS firmware)...");

    let resp1 = rpc_call(
        socket,
        "device.swap",
        json!({"bdf": bdf, "target": "nouveau"}),
    );
    check_rpc_error(&resp1);

    let personality = resp1
        .get("result")
        .and_then(|r| r.get("personality"))
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    println!("  now on {personality}");

    println!("step 2: waiting {settle_secs}s for nouveau GR init...");
    std::thread::sleep(std::time::Duration::from_secs(settle_secs));

    println!(
        "step 3: swapping {bdf} -> vfio (Ember disables reset_method to preserve FECS IMEM)..."
    );
    let resp2 = rpc_call(socket, "device.swap", json!({"bdf": bdf, "target": "vfio"}));
    check_rpc_error(&resp2);

    let personality = resp2
        .get("result")
        .and_then(|r| r.get("personality"))
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let vram = resp2
        .get("result")
        .and_then(|r| r.get("vram_alive"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    println!("  now on {personality} (vram_alive={vram})");
    println!("=== warm-fecs complete — run vfio_dispatch_warm_handoff test ===");
}

pub(crate) fn rpc_compute_info(socket: &str, bdf: &str) {
    let response = rpc_call(socket, "device.compute_info", json!({"bdf": bdf}));
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let chip = result.get("chip").and_then(|v| v.as_str()).unwrap_or("?");
        let role = result.get("role").and_then(|v| v.as_str()).unwrap_or("?");
        let protected = result
            .get("protected")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let render = result
            .get("render_node")
            .and_then(|v| v.as_str())
            .unwrap_or("none");

        println!(
            "{bdf}  {chip}  role={role}{}",
            if protected { " [PROTECTED]" } else { "" }
        );
        println!("  Render Node: {render}");

        if let Some(c) = result.get("compute") {
            if let Some(err) = c.get("error") {
                println!("  Compute: unavailable ({})", err.as_str().unwrap_or("?"));
            } else {
                let name = c.get("gpu_name").and_then(|v| v.as_str()).unwrap_or("?");
                let mem_total = c
                    .get("memory_total_mib")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let mem_free = c
                    .get("memory_free_mib")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let mem_used = c
                    .get("memory_used_mib")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let temp = c.get("temperature_c").and_then(|v| v.as_u64()).unwrap_or(0);
                let power = c
                    .get("power_draw_w")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let power_limit = c
                    .get("power_limit_w")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let sm = c.get("clock_sm_mhz").and_then(|v| v.as_u64()).unwrap_or(0);
                let mem_clk = c.get("clock_mem_mhz").and_then(|v| v.as_u64()).unwrap_or(0);
                let cc = c.get("compute_cap").and_then(|v| v.as_str()).unwrap_or("?");
                let pcie = c.get("pcie_width").and_then(|v| v.as_u64()).unwrap_or(0);

                println!("  GPU:         {name}");
                println!("  Compute Cap: {cc}");
                println!(
                    "  Memory:      {mem_used:.0} / {mem_total:.0} MiB ({mem_free:.0} MiB free)"
                );
                println!("  Temperature: {temp}C");
                println!("  Power:       {power:.1}W / {power_limit:.0}W");
                println!("  Clocks:      SM {sm} MHz, Mem {mem_clk} MHz");
                println!("  PCIe Width:  x{pcie}");
            }
        }
    }
}

pub(crate) fn rpc_get_quota(socket: &str, bdf: &str) {
    let response = rpc_call(socket, "device.quota", json!({"bdf": bdf}));
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let role = result.get("role").and_then(|v| v.as_str()).unwrap_or("?");
        let protected = result
            .get("protected")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        println!(
            "{bdf}  role={role}{}",
            if protected { " [PROTECTED]" } else { "" }
        );

        if let Some(q) = result.get("quota") {
            let pl = q.get("power_limit_w").and_then(|v| v.as_u64());
            let vb = q.get("vram_budget_mib").and_then(|v| v.as_u64());
            let cm = q
                .get("compute_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            let cp = q
                .get("compute_priority")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            println!("  Quota:");
            println!(
                "    Power Limit:  {}",
                pl.map_or("default".to_string(), |w| format!("{w}W"))
            );
            println!(
                "    VRAM Budget:  {}",
                vb.map_or("unlimited".to_string(), |m| format!("{m} MiB"))
            );
            println!("    Compute Mode: {cm}");
            println!("    Priority:     {cp}");
        }

        if let Some(c) = result.get("current").filter(|c| c.get("error").is_none()) {
            let mem_used = c
                .get("memory_used_mib")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let mem_total = c
                .get("memory_total_mib")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let power = c
                .get("power_draw_w")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let power_limit = c
                .get("power_limit_w")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            println!("  Current:");
            println!("    Memory:       {mem_used:.0} / {mem_total:.0} MiB");
            println!("    Power:        {power:.1}W / {power_limit:.0}W");
        }
    }
}

pub(crate) fn rpc_set_quota(
    socket: &str,
    bdf: &str,
    power_limit: Option<u32>,
    compute_mode: Option<&str>,
    vram_budget: Option<u32>,
) {
    let mut params = json!({"bdf": bdf});
    if let Some(pl) = power_limit {
        params["power_limit_w"] = json!(pl);
    }
    if let Some(cm) = compute_mode {
        params["compute_mode"] = json!(cm);
    }
    if let Some(vb) = vram_budget {
        params["vram_budget_mib"] = json!(vb);
    }

    let response = rpc_call(socket, "device.set_quota", params);
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        println!("Quota updated for {bdf}");
        if let Some(q) = result.get("quota") {
            let pl = q.get("power_limit_w").and_then(|v| v.as_u64());
            let vb = q.get("vram_budget_mib").and_then(|v| v.as_u64());
            let cm = q
                .get("compute_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            println!(
                "  Power Limit:  {}",
                pl.map_or("default".to_string(), |w| format!("{w}W"))
            );
            println!(
                "  VRAM Budget:  {}",
                vb.map_or("unlimited".to_string(), |m| format!("{m} MiB"))
            );
            println!("  Compute Mode: {cm}");
        }
        if let Some(applied) = result.get("applied") {
            for (key, val) in applied.as_object().into_iter().flatten() {
                let ok = val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
                let msg = val.get("message").and_then(|v| v.as_str()).unwrap_or("");
                let status = if ok { "OK" } else { "FAILED" };
                println!("  {key}: [{status}] {msg}");
            }
        }
    }
}

fn parse_triple(s: &str) -> [u32; 3] {
    let parts: Vec<u32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    [
        parts.first().copied().unwrap_or(1),
        parts.get(1).copied().unwrap_or(1),
        parts.get(2).copied().unwrap_or(1),
    ]
}

#[expect(clippy::too_many_arguments)]
pub(crate) fn rpc_dispatch(
    socket: &str,
    bdf: &str,
    shader_path: &str,
    input_paths: &[String],
    output_sizes: &[u64],
    workgroups: &str,
    threads: &str,
    output_dir: Option<&str>,
) {
    use base64::engine::general_purpose::STANDARD;
    let b64 = STANDARD;

    let shader_bytes = std::fs::read(shader_path).unwrap_or_else(|e| {
        eprintln!("error: cannot read shader {shader_path}: {e}");
        std::process::exit(1);
    });
    let shader_b64 = b64.encode(&shader_bytes);

    let inputs_b64: Vec<String> = input_paths
        .iter()
        .map(|p| {
            let data = std::fs::read(p).unwrap_or_else(|e| {
                eprintln!("error: cannot read input {p}: {e}");
                std::process::exit(1);
            });
            b64.encode(&data)
        })
        .collect();

    let dims = parse_triple(workgroups);
    let wg = parse_triple(threads);

    let params = json!({
        "bdf": bdf,
        "shader": shader_b64,
        "inputs": inputs_b64,
        "output_sizes": output_sizes,
        "dims": dims,
        "workgroup": wg,
    });

    eprintln!(
        "dispatching on {bdf}: shader={shader_path} inputs={} outputs={} grid={}x{}x{} block={}x{}x{}",
        input_paths.len(),
        output_sizes.len(),
        dims[0],
        dims[1],
        dims[2],
        wg[0],
        wg[1],
        wg[2],
    );

    let response = rpc_call(socket, "device.dispatch", params);
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let outputs: Vec<serde_json::Value> = result
            .get("outputs")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        eprintln!("dispatch complete: {} output buffer(s)", outputs.len());

        for (i, out) in outputs.iter().enumerate() {
            if let Some(encoded) = out.as_str() {
                let data = b64.decode(encoded).unwrap_or_else(|e| {
                    eprintln!("error: base64 decode output {i}: {e}");
                    std::process::exit(1);
                });
                eprintln!("  output[{i}]: {} bytes", data.len());

                if let Some(dir) = output_dir {
                    let path = format!("{dir}/output_{i}.bin");
                    std::fs::write(&path, &data).unwrap_or_else(|e| {
                        eprintln!("error: write {path}: {e}");
                        std::process::exit(1);
                    });
                    eprintln!("  written to {path}");
                }
            }
        }
    }
}

pub(crate) fn rpc_health(socket: &str) {
    let response = rpc_call(socket, "health.check", json!({}));
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let alive = result
            .get("alive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let device_count = result
            .get("device_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let healthy_count = result
            .get("healthy_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let status = if alive && healthy_count == device_count {
            "HEALTHY"
        } else if alive {
            "DEGRADED"
        } else {
            "DOWN"
        };
        println!("system: {status}  ({healthy_count}/{device_count} devices healthy)");

        if !alive {
            println!("  daemon reports not alive");
        }
    }
}

/// Resolve the ember socket path for direct journal access.
fn ember_socket() -> String {
    std::env::var("CORALREEF_EMBER_SOCKET")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/run/coralreef/ember.sock".to_string())
}

pub(crate) fn rpc_journal_query(
    _glowplug_socket: &str,
    bdf: Option<String>,
    kind: Option<String>,
    personality: Option<String>,
    limit: usize,
) {
    let mut params = json!({});
    if let Some(ref b) = bdf {
        params["bdf"] = json!(b);
    }
    if let Some(ref k) = kind {
        params["kind"] = json!(k);
    }
    if let Some(ref p) = personality {
        params["personality"] = json!(p);
    }
    params["limit"] = json!(limit);

    let response = rpc_call(&ember_socket(), "ember.journal.query", params);
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let entries = result
            .get("entries")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if entries.is_empty() {
            println!("No journal entries found.");
            return;
        }

        println!("{} journal entries:", entries.len());
        println!("{}", "-".repeat(80));

        for entry in &entries {
            let kind = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
            let bdf = entry.get("bdf").and_then(|v| v.as_str()).unwrap_or("?");
            let ts = entry
                .get("timestamp_epoch_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            match kind {
                "Swap" => {
                    let to = entry
                        .get("to_personality")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let from = entry
                        .get("from_personality")
                        .and_then(|v| v.as_str())
                        .unwrap_or("none");
                    let total_ms = entry
                        .get("timing")
                        .and_then(|t| t.get("total_ms"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let trace = entry
                        .get("trace_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    print!("[{ts}] SWAP {bdf}: {from} → {to} ({total_ms}ms)");
                    if !trace.is_empty() {
                        print!(" trace={trace}");
                    }
                    println!();
                }
                "Reset" => {
                    let method = entry
                        .get("method")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let success = entry
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let dur = entry
                        .get("duration_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let status = if success { "OK" } else { "FAIL" };
                    println!("[{ts}] RESET {bdf}: {method} {status} ({dur}ms)");
                }
                "BootAttempt" => {
                    let strategy = entry
                        .get("strategy")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let success = entry
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let sec2 = entry
                        .get("sec2_exci")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let status = if success { "OK" } else { "FAIL" };
                    println!(
                        "[{ts}] BOOT {bdf}: {strategy} {status} (sec2_exci=0x{sec2:08x})"
                    );
                }
                _ => {
                    println!("[{ts}] {kind} {bdf}");
                }
            }
        }
    }
}

pub(crate) fn rpc_journal_stats(_glowplug_socket: &str, bdf: Option<String>) {
    let params = match bdf {
        Some(ref b) => json!({"bdf": b}),
        None => json!({}),
    };

    let response = rpc_call(&ember_socket(), "ember.journal.stats", params);
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let total_swaps = result
            .get("total_swaps")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let total_resets = result
            .get("total_resets")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let total_boots = result
            .get("total_boot_attempts")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        println!("Journal Statistics");
        println!("{}", "=".repeat(60));
        println!(
            "Total: {} swaps, {} resets, {} boot attempts",
            total_swaps, total_resets, total_boots
        );

        if let Some(personalities) = result
            .get("personality_stats")
            .and_then(|v| v.as_array())
        {
            if !personalities.is_empty() {
                println!("\nPersonality Swap Timing:");
                println!(
                    "  {:<16} {:>6} {:>10} {:>10} {:>10}",
                    "PERSONALITY", "COUNT", "AVG_TOTAL", "AVG_BIND", "AVG_UNBIND"
                );
                for p in personalities {
                    let name = p
                        .get("personality")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let count = p.get("swap_count").and_then(|v| v.as_u64()).unwrap_or(0);
                    let avg_total = p.get("avg_total_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                    let avg_bind = p.get("avg_bind_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                    let avg_unbind = p
                        .get("avg_unbind_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    println!(
                        "  {:<16} {:>6} {:>8}ms {:>8}ms {:>8}ms",
                        name, count, avg_total, avg_bind, avg_unbind
                    );
                }
            }
        }

        if let Some(resets) = result
            .get("reset_method_stats")
            .and_then(|v| v.as_array())
        {
            if !resets.is_empty() {
                println!("\nReset Method Stats:");
                println!(
                    "  {:<16} {:>8} {:>8} {:>10} {:>10}",
                    "METHOD", "ATTEMPTS", "SUCCESS", "RATE", "AVG_MS"
                );
                for r in resets {
                    let method = r.get("method").and_then(|v| v.as_str()).unwrap_or("?");
                    let attempts = r.get("attempts").and_then(|v| v.as_u64()).unwrap_or(0);
                    let successes = r.get("successes").and_then(|v| v.as_u64()).unwrap_or(0);
                    let rate = r
                        .get("success_rate")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let avg_ms = r
                        .get("avg_duration_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    println!(
                        "  {:<16} {:>8} {:>8} {:>9.0}% {:>8}ms",
                        method,
                        attempts,
                        successes,
                        rate * 100.0,
                        avg_ms
                    );
                }
            }
        }
    }
}

/// Default personalities to sweep when none specified.
const DEFAULT_SWEEP_PERSONALITIES: &[&str] = &[
    "nouveau",
    "amdgpu",
    "nvidia-open",
    "xe",
    "i915",
];

struct SweepResult {
    bdf: String,
    personality: String,
    _iteration: u32,
    success: bool,
    total_ms: u64,
    bind_ms: u64,
    unbind_ms: u64,
    trace_path: Option<String>,
    insights: usize,
    error: Option<String>,
}

fn sweep_single_card(
    socket: &str,
    bdf: &str,
    targets: &[&str],
    return_to: &str,
    trace: bool,
    repeat: u32,
) -> Vec<SweepResult> {
    let total_ops = targets.len() as u32 * repeat;
    let mut results: Vec<SweepResult> = Vec::new();
    let mut step = 0u32;

    for target in targets {
        for iter in 0..repeat {
            step += 1;
            let iter_label = if repeat > 1 {
                format!("{target} (iter {}/{})", iter + 1, repeat)
            } else {
                target.to_string()
            };
            println!("\n[{step}/{total_ops}] {bdf} -> {iter_label}");

            let swap_resp = rpc_call(
                socket,
                "device.swap",
                json!({
                    "bdf": bdf,
                    "target": target,
                    "trace": trace,
                }),
            );

            if let Some(error) = swap_resp.get("error") {
                let msg = error
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                println!("  FAILED: {msg}");
                results.push(SweepResult {
                    bdf: bdf.to_string(),
                    personality: target.to_string(),
                    _iteration: iter,
                    success: false,
                    total_ms: 0,
                    bind_ms: 0,
                    unbind_ms: 0,
                    trace_path: None,
                    insights: 0,
                    error: Some(msg.to_string()),
                });
            } else if let Some(result) = swap_resp.get("result") {
                let obs = result.get("observation").and_then(|v| {
                    if v.is_null() {
                        None
                    } else {
                        Some(v)
                    }
                });
                let total_ms = obs
                    .and_then(|o| o.get("total_ms"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let bind_ms = obs
                    .and_then(|o| o.get("bind_ms"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let unbind_ms = obs
                    .and_then(|o| o.get("unbind_ms"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let trace_path = obs
                    .and_then(|o| o.get("trace_path"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let insights = result
                    .get("insights")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let vram = result
                    .get("vram_alive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                println!("  OK: {iter_label} ({total_ms}ms, bind={bind_ms}ms, vram={vram})");
                if let Some(ref tp) = trace_path {
                    println!("  Trace: {tp}");
                }
                if insights > 0 {
                    println!("  Observer insights: {insights}");
                }

                results.push(SweepResult {
                    bdf: bdf.to_string(),
                    personality: target.to_string(),
                    _iteration: iter,
                    success: true,
                    total_ms,
                    bind_ms,
                    unbind_ms,
                    trace_path,
                    insights,
                    error: None,
                });
            }

            if *target != return_to {
                print!("  Returning to {return_to}...");
                let ret_resp = rpc_call(
                    socket,
                    "device.swap",
                    json!({
                        "bdf": bdf,
                        "target": return_to,
                    }),
                );
                if ret_resp.get("error").is_some() {
                    println!(" FAILED (experiment may be in inconsistent state)");
                    return results;
                }
                println!(" ok");
            }
        }
    }

    results
}

/// Compute per-personality aggregates (avg, min, max, stddev) from successful results.
struct PersonalityAggregate {
    personality: String,
    bdf: String,
    count: u32,
    avg_total_ms: u64,
    min_total_ms: u64,
    max_total_ms: u64,
    stddev_total_ms: f64,
    avg_bind_ms: u64,
    avg_unbind_ms: u64,
    fail_count: u32,
}

fn aggregate_results(results: &[SweepResult]) -> Vec<PersonalityAggregate> {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(String, String), Vec<&SweepResult>> = BTreeMap::new();
    for r in results {
        groups
            .entry((r.bdf.clone(), r.personality.clone()))
            .or_default()
            .push(r);
    }

    groups
        .into_iter()
        .map(|((bdf, personality), runs)| {
            let ok_runs: Vec<_> = runs.iter().filter(|r| r.success).collect();
            let fail_count = runs.len() as u32 - ok_runs.len() as u32;
            let count = ok_runs.len() as u32;
            if count == 0 {
                return PersonalityAggregate {
                    personality,
                    bdf,
                    count: 0,
                    avg_total_ms: 0,
                    min_total_ms: 0,
                    max_total_ms: 0,
                    stddev_total_ms: 0.0,
                    avg_bind_ms: 0,
                    avg_unbind_ms: 0,
                    fail_count,
                };
            }
            let totals: Vec<u64> = ok_runs.iter().map(|r| r.total_ms).collect();
            let sum: u64 = totals.iter().sum();
            let avg = sum / count as u64;
            let min = *totals.iter().min().unwrap_or(&0);
            let max = *totals.iter().max().unwrap_or(&0);
            let mean_f = sum as f64 / count as f64;
            let variance = totals
                .iter()
                .map(|&t| {
                    let d = t as f64 - mean_f;
                    d * d
                })
                .sum::<f64>()
                / count as f64;
            let stddev = variance.sqrt();

            let bind_sum: u64 = ok_runs.iter().map(|r| r.bind_ms).sum();
            let unbind_sum: u64 = ok_runs.iter().map(|r| r.unbind_ms).sum();

            PersonalityAggregate {
                personality,
                bdf,
                count,
                avg_total_ms: avg,
                min_total_ms: min,
                max_total_ms: max,
                stddev_total_ms: stddev,
                avg_bind_ms: bind_sum / count as u64,
                avg_unbind_ms: unbind_sum / count as u64,
                fail_count,
            }
        })
        .collect()
}

fn print_results_table(results: &[SweepResult], repeat: u32) {
    if repeat > 1 {
        let aggs = aggregate_results(results);
        println!(
            "{:<14} {:<16} {:>5} {:>5} {:>10} {:>10} {:>10} {:>8} {:>10}",
            "BDF", "PERSONALITY", "OK", "FAIL", "AVG_MS", "MIN_MS", "MAX_MS", "STDDEV", "BIND_MS"
        );
        println!("{}", "-".repeat(100));
        for a in &aggs {
            println!(
                "{:<14} {:<16} {:>5} {:>5} {:>8}ms {:>8}ms {:>8}ms {:>7.1} {:>8}ms",
                a.bdf, a.personality, a.count, a.fail_count,
                a.avg_total_ms, a.min_total_ms, a.max_total_ms,
                a.stddev_total_ms, a.avg_bind_ms,
            );
        }
    } else {
        println!(
            "{:<14} {:<16} {:>6} {:>10} {:>10} {:>10} {:>8} {}",
            "BDF", "PERSONALITY", "STATUS", "TOTAL_MS", "BIND_MS", "UNBIND_MS", "INSIGHTS", "TRACE/ERROR"
        );
        println!("{}", "-".repeat(100));
        for r in results {
            let status = if r.success { "OK" } else { "FAIL" };
            let trail = if let Some(ref e) = r.error {
                e.clone()
            } else if let Some(ref t) = r.trace_path {
                t.clone()
            } else {
                String::new()
            };
            println!(
                "{:<14} {:<16} {:>6} {:>8}ms {:>8}ms {:>8}ms {:>8} {}",
                r.bdf, r.personality, status, r.total_ms, r.bind_ms, r.unbind_ms, r.insights, trail
            );
        }
    }
}

fn print_cross_card_comparison(results: &[SweepResult]) {
    use std::collections::BTreeMap;
    let bdfs: Vec<String> = {
        let mut seen = Vec::new();
        for r in results {
            if !seen.contains(&r.bdf) {
                seen.push(r.bdf.clone());
            }
        }
        seen
    };
    if bdfs.len() < 2 {
        return;
    }

    let aggs = aggregate_results(results);
    let mut by_personality: BTreeMap<String, Vec<&PersonalityAggregate>> = BTreeMap::new();
    for a in &aggs {
        by_personality.entry(a.personality.clone()).or_default().push(a);
    }

    println!("\n{}", "=".repeat(100));
    println!("Cross-Card Comparison");
    println!("{}", "=".repeat(100));

    for (personality, card_aggs) in &by_personality {
        if card_aggs.len() < 2 {
            continue;
        }
        println!("\n  {personality}:");
        for a in card_aggs {
            println!(
                "    {:<14}  avg={:>7}ms  bind={:>7}ms  unbind={:>7}ms  (n={})",
                a.bdf, a.avg_total_ms, a.avg_bind_ms, a.avg_unbind_ms, a.count,
            );
        }
        let ok_aggs: Vec<&&PersonalityAggregate> = card_aggs.iter().filter(|a| a.count > 0).collect();
        if ok_aggs.len() >= 2 {
            let totals: Vec<u64> = ok_aggs.iter().map(|a| a.avg_total_ms).collect();
            let min_t = *totals.iter().min().unwrap();
            let max_t = *totals.iter().max().unwrap();
            let delta = max_t.saturating_sub(min_t);
            let pct = if min_t > 0 {
                delta as f64 / min_t as f64 * 100.0
            } else {
                0.0
            };
            println!("    variance: {delta}ms ({pct:.1}%)");
        }
    }
}

fn print_journal_summary(bdf: &str) {
    let ember = ember_socket();
    let params = json!({"bdf": bdf});
    let response = rpc_call(&ember, "ember.journal.stats", params);

    if response.get("error").is_some() {
        println!("  (journal stats unavailable for {bdf})");
        return;
    }

    if let Some(result) = response.get("result") {
        let total_swaps = result.get("total_swaps").and_then(|v| v.as_u64()).unwrap_or(0);
        let total_resets = result.get("total_resets").and_then(|v| v.as_u64()).unwrap_or(0);
        let total_boots = result.get("total_boot_attempts").and_then(|v| v.as_u64()).unwrap_or(0);

        println!("  {bdf}: {total_swaps} swaps, {total_resets} resets, {total_boots} boot attempts");

        if let Some(personalities) = result.get("personality_stats").and_then(|v| v.as_array()) {
            for p in personalities {
                let name = p.get("personality").and_then(|v| v.as_str()).unwrap_or("?");
                let count = p.get("swap_count").and_then(|v| v.as_u64()).unwrap_or(0);
                let avg_total = p.get("avg_total_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                let avg_bind = p.get("avg_bind_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                println!("    {name:<16} n={count:<4} avg_total={avg_total}ms  avg_bind={avg_bind}ms");
            }
        }
    }
}

#[expect(clippy::too_many_arguments)]
pub(crate) fn rpc_experiment_sweep(
    socket: &str,
    bdf_arg: &str,
    personalities: Option<&str>,
    return_to: &str,
    trace: bool,
    repeat: u32,
) {
    let bdfs: Vec<&str> = bdf_arg.split(',').map(|s| s.trim()).collect();
    let targets: Vec<&str> = match personalities {
        Some(p) => p.split(',').map(|s| s.trim()).collect(),
        None => DEFAULT_SWEEP_PERSONALITIES.to_vec(),
    };
    let repeat = repeat.max(1);

    println!("Experiment Sweep");
    println!("Cards: {}", bdfs.join(", "));
    println!("Personalities: {}", targets.join(", "));
    println!("Repeat: {repeat}x  |  Return-to: {return_to}  |  Trace: {trace}");
    println!("{}", "=".repeat(100));

    let mut all_results: Vec<SweepResult> = Vec::new();

    for bdf in &bdfs {
        if bdfs.len() > 1 {
            println!("\n>>> Card: {bdf}");
        }
        let card_results = sweep_single_card(socket, bdf, &targets, return_to, trace, repeat);
        all_results.extend(card_results);
    }

    // Per-card results table
    println!("\n{}", "=".repeat(100));
    println!("Experiment Results");
    println!("{}", "=".repeat(100));
    print_results_table(&all_results, repeat);

    let ok_count = all_results.iter().filter(|r| r.success).count();
    let fail_count = all_results.len() - ok_count;
    println!("{}", "-".repeat(100));
    println!(
        "Summary: {ok_count} succeeded, {fail_count} failed out of {} operations",
        all_results.len()
    );

    // Cross-card comparison (only when multiple BDFs)
    if bdfs.len() > 1 {
        print_cross_card_comparison(&all_results);
    }

    // Auto journal summary
    println!("\n{}", "=".repeat(100));
    println!("Journal Summary (all-time aggregates from ember)");
    println!("{}", "-".repeat(100));
    for bdf in &bdfs {
        print_journal_summary(bdf);
    }
}
