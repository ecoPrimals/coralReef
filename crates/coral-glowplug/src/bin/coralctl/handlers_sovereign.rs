// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign GPU compute handlers — route through ember's MMIO gateway.
//!
//! All operations here connect to the **ember** socket (not glowplug).
//! Ember holds the VFIO device, maps BAR0, and runs operations inside
//! fork-isolated children with PCIe armor. The user process never
//! touches /dev/vfio/* or maps BAR0 directly.

use crate::rpc::{check_rpc_error, rpc_call};

/// Resolve the ember socket for a specific BDF.
///
/// Fleet mode: each device gets its own ember instance at
/// `/run/coralreef/fleet/ember-{slug}.sock`. Falls back to the
/// global ember socket if the fleet socket doesn't exist.
fn ember_socket_for(bdf: &str) -> String {
    let fleet = coral_ember::ember_instance_socket_path(bdf);
    if std::path::Path::new(&fleet).exists() {
        fleet
    } else {
        coral_ember::ember_socket_path()
    }
}

/// `ember.firmware.inventory` — probe firmware availability.
pub(crate) fn rpc_firmware_inventory(bdf: &str) {
    let socket = ember_socket_for(bdf);
    println!("Probing firmware inventory for {bdf} via ember...");

    let response = rpc_call(
        &socket,
        "ember.firmware.inventory",
        serde_json::json!({"bdf": bdf}),
    );
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let chip = result["chip"].as_str().unwrap_or("unknown");
        let sm = result["sm"].as_u64().unwrap_or(0);
        let gpu_warm = result["gpu_warm"].as_bool().unwrap_or(false);
        let pmc_enable = result["pmc_enable"].as_str().unwrap_or("?");

        println!("\n  GPU:   {bdf}  chip={chip}  sm={sm}");
        println!("  State: {} (PMC_ENABLE={pmc_enable})",
            if gpu_warm { "WARM (DEVINIT ran)" } else { "COLD (no DEVINIT)" });
        println!("  ─────────────────────────────────────");

        if !gpu_warm {
            println!("  WARNING: GPU is cold — VBIOS PROM read skipped (would hang bus)");
            println!("  To warm: modprobe nouveau, wait 5s, unbind nouveau, bind vfio-pci");
            println!();
        }

        for subsystem in &["acr", "gr", "sec2", "pmu", "gsp", "nvdec"] {
            let present = result[subsystem].as_bool().unwrap_or(false);
            let mark = if present { "OK" } else { "MISSING" };
            println!("  {subsystem:8} {mark}");
        }

        let vbios = result["vbios_prom"].as_bool().unwrap_or(false);
        let vbios_label = if vbios {
            "OK (PROM)"
        } else if gpu_warm {
            "NOT READABLE"
        } else {
            "SKIPPED (cold GPU)"
        };
        println!("  vbios    {vbios_label}");

        let viable = result["compute_viable"].as_bool().unwrap_or(false);
        println!("\n  Compute viable: {viable}");

        if let Some(blockers) = result["blockers"].as_array() {
            if !blockers.is_empty() {
                println!("  Blockers:");
                for b in blockers {
                    if let Some(s) = b.as_str() {
                        println!("    - {s}");
                    }
                }
            }
        }
    }
}

/// `ember.firmware.load` — load and validate firmware blobs.
pub(crate) fn rpc_firmware_load(bdf: &str) {
    let socket = ember_socket_for(bdf);
    println!("Loading firmware blobs for {bdf} via ember...");

    let response = rpc_call(
        &socket,
        "ember.firmware.load",
        serde_json::json!({"bdf": bdf}),
    );
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
    }
}

/// `ember.sovereign.init` — staged sovereign init pipeline.
///
/// Each init stage runs in its own fork-isolated child with a short
/// timeout. If a stage hangs (PCIe bus hang from BAR0 writes), ember
/// kills that child and reports which stage failed — the system stays
/// alive. Response contains a `stages` array with per-stage results.
pub(crate) fn rpc_sovereign_init(bdf: &str) {
    let socket = ember_socket_for(bdf);
    println!("================================================================");
    println!("  SOVEREIGN INIT — Staged Pure Rust GPU Pipeline (via ember)");
    println!("================================================================");
    println!("  BDF:      {bdf}");
    println!("  Route:    coralctl -> ember -> per-stage fork_isolated");
    println!("  Safety:   PCIe armor, AER masked, per-stage sacrifice on hang");
    println!("  Driver:   NONE (pure Rust + VFIO + firmware blobs)");
    println!("================================================================\n");

    let response = rpc_call(
        &socket,
        "ember.sovereign.init",
        serde_json::json!({"bdf": bdf}),
    );
    check_rpc_error(&response);

    let Some(result) = response.get("result") else { return };

    // Cold GPU error (from probe stage)
    if let Some(err) = result["error"].as_str() {
        if err == "cold_gpu" {
            let msg = result["message"].as_str().unwrap_or("GPU is cold");
            let pmc = result["pmc_enable"].as_str().unwrap_or("?");
            println!("  BLOCKED: GPU is cold (PMC_ENABLE={pmc})");
            println!("  {msg}");
            println!("\n  Sovereign init requires a warm GPU (DEVINIT must have run).");
            println!("  Fix: modprobe nouveau -> wait 5s -> unbind -> bind vfio-pci -> retry");
            return;
        }
    }

    let chip = result["chip"].as_str().unwrap_or("?");
    let sm = result["sm"].as_u64().unwrap_or(0);
    let all_ok = result["all_ok"].as_bool().unwrap_or(false);
    let compute = result["compute_ready"].as_bool().unwrap_or(false);
    let halted = result["halted_at"].as_str();

    println!("  Chip:       {chip} (SM {sm})");
    println!("  All OK:     {all_ok}");
    if let Some(h) = halted {
        println!("  Halted at:  {h}");
    }
    println!();

    // Per-stage results
    if let Some(stages) = result["stages"].as_array() {
        println!("  ──── Stages ────────────────────────────────────");
        for stage in stages {
            let name = stage["name"].as_str().unwrap_or("?");
            let status = stage["status"].as_str().unwrap_or("?");
            let mark = match status {
                "ok" => "  OK ",
                "failed" => "FAIL ",
                "timeout" => " HANG",
                "blocked" => "BLOCK",
                "crashed" => "CRASH",
                _ => "  ?  ",
            };
            println!("  [{mark}] {name}");

            // Print useful detail for each status
            if let Some(detail) = stage.get("detail") {
                if let Some(s) = detail.as_str() {
                    println!("         {s}");
                } else if detail.is_object() {
                    // Topology extraction
                    if let Some(topo) = detail.get("topology") {
                        if topo.is_object() {
                            let gpc = topo["gpc"].as_u64().unwrap_or(0);
                            let sm_count = topo["sm"].as_u64().unwrap_or(0);
                            let fbp = topo["fbp"].as_u64().unwrap_or(0);
                            let pbdma = topo["pbdma"].as_u64().unwrap_or(0);
                            println!("         GPC:{gpc} SM:{sm_count} FBP:{fbp} PBDMA:{pbdma}");
                        }
                    }
                    // Write counts
                    let applied = detail["writes_applied"].as_u64();
                    let failed = detail["writes_failed"].as_u64();
                    let dur = detail["duration_us"].as_u64();
                    if let (Some(a), Some(f)) = (applied, failed) {
                        let dur_str = dur.map(|d| format!(" ({d}us)")).unwrap_or_default();
                        println!("         writes: {a} applied, {f} failed{dur_str}");
                    }
                    // PMC/warm info from probe
                    if let Some(pmc) = detail["pmc_enable"].as_str() {
                        let warm = detail["gpu_warm"].as_bool().unwrap_or(false);
                        let devinit = detail["devinit_done"].as_bool().unwrap_or(false);
                        let bits = detail["pmc_bits"].as_u64().unwrap_or(0);
                        println!(
                            "         PMC_ENABLE={pmc} ({bits} bits) warm={warm} devinit={devinit}"
                        );
                    }
                }
            }
        }
        println!("  ────────────────────────────────────────────────");
    }

    println!();
    println!("  ════════════════════════════════════════════");
    if compute {
        println!("  COMPUTE READY -- sovereign pipeline succeeded");
    } else if let Some(h) = halted {
        println!("  NOT COMPUTE READY -- halted at stage: {h}");
    } else {
        println!("  NOT COMPUTE READY");
    }
    println!("  ════════════════════════════════════════════");
}
