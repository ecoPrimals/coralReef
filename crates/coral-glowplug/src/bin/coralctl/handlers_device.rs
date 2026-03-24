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

pub(crate) fn rpc_reset(socket: &str, bdf: &str) {
    println!("resetting {bdf} via VFIO FLR...");
    let response = rpc_call(socket, "device.reset", json!({"bdf": bdf}));
    check_rpc_error(&response);
    println!("ok: {bdf} reset complete");
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
