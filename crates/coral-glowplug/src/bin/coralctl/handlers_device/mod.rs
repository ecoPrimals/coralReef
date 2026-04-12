// SPDX-License-Identifier: AGPL-3.0-only
//! RPC handlers: device lifecycle, compute, dispatch, and system health.

use crate::rpc::{check_rpc_error, rpc_call};

use base64::Engine;
use serde_json::json;
use std::path::PathBuf;

fn livepatch_enabled_path() -> PathBuf {
    std::env::var("CORALREEF_LIVEPATCH_ENABLED_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/sys/kernel/livepatch/livepatch_nvkm_mc_reset/enabled"))
}

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
            eprintln!(
                "error: unknown reset method '{other}' (use: auto, flr, sbr, bridge-sbr, remove-rescan)"
            );
            std::process::exit(1);
        }
    }
}

pub(crate) fn rpc_warm_fecs(socket: &str, bdf: &str, settle_secs: u64) {
    println!("=== Warm FECS via nouveau round-trip ===");

    // Livepatch must be DISABLED before nouveau loads so gk104_runl_commit
    // (and other functions) run normally during init. If it's enabled and
    // nouveau loads, the NOP would prevent runlist submission and break init.
    let lp_enabled = livepatch_enabled_path();
    let lp_enabled_str = lp_enabled
        .to_str()
        .expect("CORALREEF_LIVEPATCH_ENABLED_PATH must be UTF-8");
    if lp_enabled.exists() {
        let cur = std::fs::read_to_string(&lp_enabled).unwrap_or_default();
        if cur.trim() == "1" {
            println!("step 0: disabling livepatch before nouveau load...");
            sysfs_write_privileged(lp_enabled_str, "0");
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

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

    // Enable livepatch AFTER init, BEFORE teardown — NOPs freeze the
    // runlist, prevent falcon halts, and skip engine resets so FECS
    // stays alive in its context-switch-ready HALT state.
    if lp_enabled.exists() {
        println!("step 2b: enabling livepatch (freezing runlist for warm handoff)...");
        sysfs_write_privileged(lp_enabled_str, "1");
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

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
/// Uses the canonical wateringHole formula matching `coral-ember`'s server side.
pub(crate) fn ember_socket() -> String {
    coral_ember::ember_socket_path()
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
                    let method = entry.get("method").and_then(|v| v.as_str()).unwrap_or("?");
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
                    let sec2 = entry.get("sec2_exci").and_then(|v| v.as_u64()).unwrap_or(0);
                    let status = if success { "OK" } else { "FAIL" };
                    println!("[{ts}] BOOT {bdf}: {strategy} {status} (sec2_exci=0x{sec2:08x})");
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

        if let Some(personalities) = result.get("personality_stats").and_then(|v| v.as_array())
            && !personalities.is_empty()
        {
            println!("\nPersonality Swap Timing:");
            println!(
                "  {:<16} {:>6} {:>10} {:>10} {:>10}",
                "PERSONALITY", "COUNT", "AVG_TOTAL", "AVG_BIND", "AVG_UNBIND"
            );
            for p in personalities {
                let name = p.get("personality").and_then(|v| v.as_str()).unwrap_or("?");
                let count = p.get("swap_count").and_then(|v| v.as_u64()).unwrap_or(0);
                let avg_total = p.get("avg_total_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                let avg_bind = p.get("avg_bind_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                let avg_unbind = p.get("avg_unbind_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                println!(
                    "  {:<16} {:>6} {:>8}ms {:>8}ms {:>8}ms",
                    name, count, avg_total, avg_bind, avg_unbind
                );
            }
        }

        if let Some(resets) = result.get("reset_method_stats").and_then(|v| v.as_array())
            && !resets.is_empty()
        {
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

const DEFAULT_CORALREEF_SYSFS_WRITE: &str = "/usr/local/bin/coralreef-sysfs-write";

fn coralreef_sysfs_write_path() -> String {
    std::env::var("CORALREEF_SYSFS_WRITE_PATH")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_CORALREEF_SYSFS_WRITE.to_string())
}

/// Write to a privileged sysfs path via `sudo -n coralreef-sysfs-write`.
/// Falls back to direct write if the helper is not installed.
fn sysfs_write_privileged(path: &str, value: &str) {
    let helper = coralreef_sysfs_write_path();
    let status = std::process::Command::new("sudo")
        .args(["-n", &helper, path, value])
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("warning: coralreef-sysfs-write {path} exited with {s}, trying direct write");
            let _ = std::fs::write(path, value);
        }
        Err(_) => {
            let _ = std::fs::write(path, value);
        }
    }
}

mod sweep;

pub(crate) use sweep::rpc_experiment_sweep;
