// SPDX-License-Identifier: AGPL-3.0-only
//! Firmware management handlers — ember as sovereign firmware intermediary.
//!
//! These RPCs make ember the central authority for GPU firmware lifecycle:
//! inventory, loading, caching, and sovereign init orchestration. Firmware
//! blobs are treated as ingredients — loaded from `/lib/firmware/nvidia/`
//! and injected into hardware by the Rust driver.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;
use crate::isolation::{self, ForkResult};

use super::super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};
use super::{map_bar0_if_needed, preflight_gate, require_bdf, update_fault_counter};

/// `ember.firmware.inventory` — probe firmware availability for a held device.
///
/// Reads BOOT0 to determine chip identity, then checks `/lib/firmware/nvidia/{chip}/`
/// for each subsystem (ACR, GR, SEC2, PMU, GSP, NVDEC). Also probes VBIOS PROM
/// availability via fork-isolated BAR0 read.
///
/// Params: `{bdf}`
/// Result: `{bdf, chip, sm, acr, gr, sec2, pmu, gsp, nvdec, vbios_prom, compute_viable, blockers}`
pub(crate) fn firmware_inventory(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let Some(dev) = map.get_mut(bdf) else {
        write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: not held by ember"))
            .map_err(EmberIpcError::from)?;
        return Ok(());
    };

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_gate(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: {e}"))
            .map_err(EmberIpcError::from);
    }

    let bar0 = dev.bar0.as_ref().unwrap();
    let bar0_ptr = bar0.base_ptr() as usize;
    let bar0_size = bar0.size();
    let bdf_owned = bdf.to_string();
    drop(map);

    let fork_result = isolation::fork_isolated_mmio(
        &bdf_owned,
        std::time::Duration::from_secs(10),
        |pipe_fd| {
            let bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
            };

            let boot0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
            let sm = coral_driver::nv::identity::boot0_to_sm(boot0).unwrap_or(0);
            let chip = coral_driver::nv::identity::chip_name(sm);

            let mut inv = coral_driver::nv::identity::firmware_inventory(chip);

            // PMC_ENABLE (0x200): if engines are gated on, the GPU was
            // warm-cycled (nouveau/nvidia ran DEVINIT). On a cold boot
            // the register reads ~0 and VBIOS PROM access is unsafe —
            // the memory controller isn't initialized and BAR0 PROM
            // reads can hang the PCIe bus.
            let pmc_enable = bar0.read_u32(0x200).unwrap_or(0);
            let gpu_warm = pmc_enable.count_ones() >= 4;

            let vbios_available = if gpu_warm {
                let ok = coral_driver::vfio::channel::devinit::read_vbios_prom(&bar0).is_ok();
                if ok {
                    inv.vbios_prom = coral_driver::nv::identity::FwStatus::Present;
                }
                ok
            } else {
                false
            };

            let payload = serde_json::json!({
                "boot0": format!("{boot0:#010x}"),
                "chip": chip,
                "sm": sm,
                "acr": inv.acr.is_present(),
                "gr": inv.gr.is_present(),
                "sec2": inv.sec2.is_present(),
                "pmu": inv.pmu_available(),
                "gsp": inv.gsp.is_present(),
                "nvdec": inv.nvdec.is_present(),
                "vbios_prom": vbios_available,
                "gpu_warm": gpu_warm,
                "pmc_enable": format!("{pmc_enable:#010x}"),
                "compute_viable": inv.compute_viable(),
                "blockers": inv.compute_blockers(),
            });
            let bytes = serde_json::to_vec(&payload).unwrap_or_default();
            unsafe { libc::write(pipe_fd, bytes.as_ptr().cast(), bytes.len()); }
        },
    );

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    if let Some(dev) = map.get_mut(&bdf_owned) {
        match &fork_result {
            ForkResult::Ok(data) => {
                let boot0 = parse_boot0_from_json(data);
                let _ = update_fault_counter(dev, boot0);
            }
            ForkResult::Timeout | ForkResult::ChildFailed { .. } => {
                dev.mmio_fault_count += 1;
            }
            ForkResult::ForkFailed(_) | ForkResult::PipeFailed(_) => {}
        }
    }
    drop(map);

    handle_fork_result(stream, id, fork_result, &bdf_owned, "firmware inventory")
}

/// `ember.firmware.load` — load and validate firmware blobs for a held device.
///
/// Reads BOOT0 to determine chip, then loads ACR firmware set and GR firmware
/// blobs from `/lib/firmware/nvidia/{chip}/`. Reports parse statistics and
/// file counts. Loading is fork-isolated (filesystem I/O only, no MMIO beyond
/// BOOT0).
///
/// Params: `{bdf}`
/// Result: `{bdf, chip, acr_loaded, gr_loaded, acr_files, gr_bundles, gr_methods, error?}`
pub(crate) fn firmware_load(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let Some(dev) = map.get_mut(bdf) else {
        write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: not held by ember"))
            .map_err(EmberIpcError::from)?;
        return Ok(());
    };

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_gate(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: {e}"))
            .map_err(EmberIpcError::from);
    }

    let bar0 = dev.bar0.as_ref().unwrap();
    let bar0_ptr = bar0.base_ptr() as usize;
    let bar0_size = bar0.size();
    let bdf_owned = bdf.to_string();
    drop(map);

    let fork_result = isolation::fork_isolated_mmio(
        &bdf_owned,
        std::time::Duration::from_secs(15),
        |pipe_fd| {
            let bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
            };

            let boot0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
            let sm = coral_driver::nv::identity::boot0_to_sm(boot0).unwrap_or(0);
            let chip = coral_driver::nv::identity::chip_name(sm);

            let acr_result =
                coral_driver::nv::vfio_compute::acr_boot::AcrFirmwareSet::load(chip);
            let (acr_loaded, acr_error) = match &acr_result {
                Ok(_) => (true, None),
                Err(e) => (false, Some(format!("{e}"))),
            };

            let gr_result = coral_driver::gsp::GrFirmwareBlobs::parse(chip);
            let (gr_loaded, gr_bundles, gr_methods, gr_error) = match &gr_result {
                Ok(blobs) => (
                    true,
                    blobs.bundle_init.len(),
                    blobs.method_init.len(),
                    None,
                ),
                Err(e) => (false, 0, 0, Some(format!("{e}"))),
            };

            let payload = serde_json::json!({
                "boot0": format!("{boot0:#010x}"),
                "chip": chip,
                "sm": sm,
                "acr_loaded": acr_loaded,
                "acr_error": acr_error,
                "gr_loaded": gr_loaded,
                "gr_bundles": gr_bundles,
                "gr_methods": gr_methods,
                "gr_error": gr_error,
            });
            let bytes = serde_json::to_vec(&payload).unwrap_or_default();
            unsafe { libc::write(pipe_fd, bytes.as_ptr().cast(), bytes.len()); }
        },
    );

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    if let Some(dev) = map.get_mut(&bdf_owned) {
        match &fork_result {
            ForkResult::Ok(data) => {
                let boot0 = parse_boot0_from_json(data);
                let _ = update_fault_counter(dev, boot0);
            }
            ForkResult::Timeout | ForkResult::ChildFailed { .. } => {
                dev.mmio_fault_count += 1;
            }
            ForkResult::ForkFailed(_) | ForkResult::PipeFailed(_) => {}
        }
    }
    drop(map);

    handle_fork_result(stream, id, fork_result, &bdf_owned, "firmware load")
}

/// `ember.sovereign.init` — staged sovereign init with per-stage fork isolation.
///
/// Each stage runs in its own fork-isolated child with a short timeout.
/// If any stage hangs (PCIe bus hang from BAR0 writes), ember kills just
/// that child and reports which stage failed — the system stays alive.
///
/// Stages:
///   0. Probe: read-only BOOT0 + PMC_ENABLE + DEVINIT check
///   1. HBM2 Training (cold only, auto-detected)
///   2. PMC + Engine Gating
///   3. Topology Discovery (read-only)
///   4. PFB + Memory Controller
///   5. Falcon Boot (SEC2 → ACR → FECS/GPCCS)
///   6. GR Engine Init
///   7. PFIFO Discovery
///
/// Params: `{bdf}`
/// Result: `{bdf, chip, stages: [{name, status, detail}], compute_ready, diagnostics}`
#[allow(unsafe_code)]
pub(crate) fn sovereign_init(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let Some(dev) = map.get_mut(bdf) else {
        write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: not held by ember"))
            .map_err(EmberIpcError::from)?;
        return Ok(());
    };

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_gate(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: {e}"))
            .map_err(EmberIpcError::from);
    }

    let bar0 = dev.bar0.as_ref().unwrap();
    let bar0_ptr = bar0.base_ptr() as usize;
    let bar0_size = bar0.size();
    let dma_backend = dev.device.dma_backend();
    let bdf_owned = bdf.to_string();
    drop(map);

    let mut completed_stages: Vec<serde_json::Value> = Vec::new();
    let mut sm: u32 = 70;
    let mut chip = String::from("gv100");
    let mut boot0_val: u32 = 0xFFFF_FFFF;
    let mut halted = false;

    // ── Stage 0: Read-only probe (3s timeout) ──────────────────────
    // Three warmth levels:
    //   - "hot":  PMC_ENABLE has many bits → engines alive (e.g. nvidia driver still loaded)
    //   - "warm": PMC_ENABLE has ANY bits → HBM2 trained, engines gated (post-nouveau teardown)
    //   - "cold": PMC_ENABLE == 0 → true cold boot, memory controller uninitialized
    // We only block on "cold". "Warm" is safe — the PMC stage will ungate engines.
    let probe = run_isolated_stage(
        &bdf_owned, bar0_ptr, bar0_size, "probe",
        std::time::Duration::from_secs(3),
        |bar0, pipe_fd| {
            let b0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
            let s = coral_driver::nv::identity::boot0_to_sm(b0).unwrap_or(70);
            let c = coral_driver::nv::identity::chip_name(s);
            let pmc = bar0.read_u32(0x200).unwrap_or(0);
            let devinit = bar0.read_u32(0x0002_240C).unwrap_or(0);
            // "warm enough" = any PMC bit set (post-nouveau teardown leaves ≥1 bit)
            // OR DEVINIT status bit set. Truly cold reads PMC as 0x00000000.
            let warm = pmc != 0 || (devinit & 2) != 0;
            let payload = serde_json::json!({
                "boot0": format!("{b0:#010x}"),
                "sm": s, "chip": c,
                "pmc_enable": format!("{pmc:#010x}"),
                "pmc_bits": pmc.count_ones(),
                "gpu_warm": warm,
                "devinit_done": (devinit & 2) != 0,
            });
            pipe_json(pipe_fd, &payload);
        },
    );

    match probe {
        StageOutcome::Ok(data) => {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&data) {
                let warm = v["gpu_warm"].as_bool().unwrap_or(false);
                sm = v["sm"].as_u64().unwrap_or(70) as u32;
                chip = v["chip"].as_str().unwrap_or("gv100").to_string();
                boot0_val = v["boot0"].as_str()
                    .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0xFFFF_FFFF);
                completed_stages.push(serde_json::json!({
                    "name": "probe", "status": "ok", "detail": v,
                }));
                if !warm {
                    completed_stages.push(serde_json::json!({
                        "name": "probe", "status": "blocked",
                        "detail": "GPU is truly cold (PMC_ENABLE=0x0). modprobe nouveau to warm-cycle HBM2.",
                    }));
                    halted = true;
                }
            }
        }
        StageOutcome::Timeout => {
            completed_stages.push(serde_json::json!({
                "name": "probe", "status": "timeout",
                "detail": "Read-only probe timed out — GPU may be in D-state",
            }));
            halted = true;
        }
        StageOutcome::Failed(msg) => {
            completed_stages.push(serde_json::json!({
                "name": "probe", "status": "failed", "detail": msg,
            }));
            halted = true;
        }
    }

    // Stages 1–7: each gets its own fork with a short timeout.
    // We stop at the first stage that hangs.
    struct StageDef {
        name: &'static str,
        timeout_secs: u64,
        method: &'static str,
    }
    let stages = [
        StageDef { name: "hbm2", timeout_secs: 10, method: "init_hbm2" },
        StageDef { name: "pmc", timeout_secs: 5, method: "init_pmc" },
        StageDef { name: "topology", timeout_secs: 5, method: "init_topology" },
        StageDef { name: "pfb", timeout_secs: 5, method: "init_pfb" },
        StageDef { name: "pri_ring_reset", timeout_secs: 5, method: "reset_pri_ring" },
        StageDef { name: "falcons", timeout_secs: 15, method: "init_falcons" },
        StageDef { name: "gr", timeout_secs: 10, method: "init_gr" },
        StageDef { name: "pfifo", timeout_secs: 5, method: "init_pfifo_discovery" },
    ];

    let sm_cap = sm;
    let bdf_cap = bdf_owned.clone();
    let dma_cap = dma_backend.clone();

    for stage_def in &stages {
        if halted { break; }

        let method = stage_def.method;
        let sm_c = sm_cap;
        let bdf_c = bdf_cap.clone();
        let dma_c = dma_cap.clone();

        let outcome = run_isolated_stage(
            &bdf_owned, bar0_ptr, bar0_size, stage_def.name,
            std::time::Duration::from_secs(stage_def.timeout_secs),
            move |bar0, pipe_fd| {
                let pipeline =
                    coral_driver::nv::vfio_compute::sovereign_init::SovereignInit::new(bar0, sm_c)
                        .with_bdf(&bdf_c)
                        .with_dma_backend(dma_c);

                let result = match method {
                    "init_hbm2" => pipeline.init_hbm2(),
                    "init_pmc" => pipeline.init_pmc(),
                    "reset_pri_ring" => pipeline.reset_pri_ring(),
                    "init_topology" => {
                        let (r, topo) = pipeline.init_topology();
                        if let Some(t) = &topo {
                            let payload = serde_json::json!({
                                "stage": r.stage, "ok": r.ok(),
                                "writes_applied": r.writes_applied,
                                "writes_failed": r.writes_failed,
                                "duration_us": r.duration_us,
                                "topology": {
                                    "gpc": t.gpc_count, "sm": t.sm_count,
                                    "fbp": t.fbp_count, "pbdma": t.pbdma_count,
                                },
                            });
                            pipe_json(pipe_fd, &payload);
                            return;
                        }
                        r
                    }
                    "init_pfb" => pipeline.init_pfb(),
                    "init_falcons" => pipeline.init_falcons(),
                    "init_gr" => pipeline.init_gr(),
                    "init_pfifo_discovery" => pipeline.init_pfifo_discovery(),
                    _ => unreachable!(),
                };

                let payload = serde_json::json!({
                    "stage": result.stage, "ok": result.ok(),
                    "writes_applied": result.writes_applied,
                    "writes_failed": result.writes_failed,
                    "duration_us": result.duration_us,
                });
                pipe_json(pipe_fd, &payload);
            },
        );

        match outcome {
            StageOutcome::Ok(data) => {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&data) {
                    let ok = v["ok"].as_bool().unwrap_or(false);
                    completed_stages.push(serde_json::json!({
                        "name": stage_def.name,
                        "status": if ok { "ok" } else { "failed" },
                        "detail": v,
                    }));
                } else {
                    completed_stages.push(serde_json::json!({
                        "name": stage_def.name, "status": "ok",
                        "detail": String::from_utf8_lossy(&data).to_string(),
                    }));
                }
            }
            StageOutcome::Timeout => {
                tracing::error!(
                    bdf = %bdf_owned, stage = stage_def.name,
                    "sovereign init stage TIMED OUT — child sacrificed, stopping pipeline"
                );
                completed_stages.push(serde_json::json!({
                    "name": stage_def.name, "status": "timeout",
                    "detail": format!(
                        "Stage timed out after {}s — fork child killed. \
                         GPU may need warm cycle or the stage is incompatible.",
                        stage_def.timeout_secs
                    ),
                }));
                halted = true;
            }
            StageOutcome::Failed(msg) => {
                completed_stages.push(serde_json::json!({
                    "name": stage_def.name, "status": "crashed",
                    "detail": msg,
                }));
                halted = true;
            }
        }
    }

    // Build final result
    let all_ok = completed_stages.iter().all(|s| {
        s["status"].as_str() == Some("ok")
    });
    let last_completed = completed_stages.last()
        .and_then(|s| s["name"].as_str())
        .unwrap_or("none");

    let payload = serde_json::json!({
        "bdf": bdf_owned,
        "boot0": format!("{boot0_val:#010x}"),
        "chip": chip,
        "sm": sm,
        "staged": true,
        "stages": completed_stages,
        "all_ok": all_ok,
        "halted_at": if halted { Some(last_completed) } else { None },
        "compute_ready": all_ok && !halted,
    });

    // Update device health
    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    if let Some(dev) = map.get_mut(&bdf_owned) {
        if halted {
            dev.mmio_fault_count += 1;
            if completed_stages.iter().any(|s| s["status"].as_str() == Some("timeout")) {
                dev.health = crate::hold::DeviceHealth::Faulted;
            }
        } else if all_ok {
            dev.health = crate::hold::DeviceHealth::Active;
            dev.experiment_dirty = true;
        }
        let _ = update_fault_counter(dev, boot0_val);
    }
    drop(map);

    write_jsonrpc_ok(stream, id, payload).map_err(EmberIpcError::from)
}

/// Outcome of a single fork-isolated stage.
enum StageOutcome {
    Ok(Vec<u8>),
    Timeout,
    Failed(String),
}

/// Run a single init stage in a fork-isolated child.
#[allow(unsafe_code)]
fn run_isolated_stage(
    bdf: &str,
    bar0_ptr: usize,
    bar0_size: usize,
    stage_name: &str,
    timeout: std::time::Duration,
    body: impl FnOnce(&coral_driver::vfio::device::MappedBar, i32),
) -> StageOutcome {
    tracing::info!(bdf, stage = stage_name, timeout_ms = timeout.as_millis() as u64, "sovereign stage: starting");
    let result = isolation::fork_isolated_mmio(
        bdf,
        timeout,
        |pipe_fd| {
            let bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
            };
            body(&bar0, pipe_fd);
            std::mem::forget(bar0);
        },
    );
    match result {
        ForkResult::Ok(data) => {
            tracing::info!(bdf, stage = stage_name, bytes = data.len(), "sovereign stage: completed");
            StageOutcome::Ok(data)
        }
        ForkResult::Timeout => {
            tracing::error!(bdf, stage = stage_name, "sovereign stage: TIMEOUT — child killed");
            StageOutcome::Timeout
        }
        ForkResult::ChildFailed { status } => {
            tracing::error!(bdf, stage = stage_name, status, "sovereign stage: child crashed");
            StageOutcome::Failed(format!("child exited with status {status}"))
        }
        ForkResult::ForkFailed(e) | ForkResult::PipeFailed(e) => {
            tracing::error!(bdf, stage = stage_name, error = %e, "sovereign stage: fork/pipe failed");
            StageOutcome::Failed(format!("fork error: {e}"))
        }
    }
}

/// Write a JSON value to the fork pipe.
#[allow(unsafe_code)]
fn pipe_json(pipe_fd: i32, val: &serde_json::Value) {
    let bytes = serde_json::to_vec(val).unwrap_or_default();
    unsafe { libc::write(pipe_fd, bytes.as_ptr().cast(), bytes.len()); }
}

// ── Shared helpers ──────────────────────────────────────────────────

fn parse_boot0_from_json(data: &[u8]) -> u32 {
    let val: serde_json::Value = match serde_json::from_slice(data) {
        Ok(v) => v,
        Err(_) => return 0xFFFF_FFFF,
    };
    val.get("boot0")
        .and_then(|b| b.as_str())
        .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0xFFFF_FFFF)
}

fn handle_fork_result(
    stream: &mut impl Write,
    id: serde_json::Value,
    fork_result: ForkResult,
    bdf: &str,
    op_name: &str,
) -> Result<(), EmberIpcError> {
    match fork_result {
        ForkResult::Ok(data) => match serde_json::from_slice::<serde_json::Value>(&data) {
            Ok(mut result) => {
                result
                    .as_object_mut()
                    .map(|o| o.insert("bdf".into(), serde_json::Value::String(bdf.to_string())));
                write_jsonrpc_ok(stream, id, result).map_err(EmberIpcError::from)
            }
            Err(e) => write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("{bdf}: {op_name} parse error: {e}"),
            )
            .map_err(EmberIpcError::from),
        },
        ForkResult::Timeout => write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf}: {op_name} timed out"),
        )
        .map_err(EmberIpcError::from),
        ForkResult::ChildFailed { status } => write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf}: {op_name} fork crashed (exit {status})"),
        )
        .map_err(EmberIpcError::from),
        ForkResult::ForkFailed(e) | ForkResult::PipeFailed(e) => {
            write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: fork failed: {e}"))
                .map_err(EmberIpcError::from)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_inventory_missing_bdf_returns_error() {
        let held = Arc::new(RwLock::new(HashMap::<String, HeldDevice>::new()));
        let params = serde_json::json!({});
        let mut buf = Vec::new();

        let result = firmware_inventory(&mut buf, &held, serde_json::json!(1), &params);
        assert!(result.is_err() || {
            let s = String::from_utf8_lossy(&buf);
            s.contains("missing") || s.contains("bdf")
        });
    }

    #[test]
    fn firmware_load_missing_bdf_returns_error() {
        let held = Arc::new(RwLock::new(HashMap::<String, HeldDevice>::new()));
        let params = serde_json::json!({});
        let mut buf = Vec::new();

        let result = firmware_load(&mut buf, &held, serde_json::json!(1), &params);
        assert!(result.is_err() || {
            let s = String::from_utf8_lossy(&buf);
            s.contains("missing") || s.contains("bdf")
        });
    }

    #[test]
    fn sovereign_init_missing_bdf_returns_error() {
        let held = Arc::new(RwLock::new(HashMap::<String, HeldDevice>::new()));
        let params = serde_json::json!({});
        let mut buf = Vec::new();

        let result = sovereign_init(&mut buf, &held, serde_json::json!(1), &params);
        assert!(result.is_err() || {
            let s = String::from_utf8_lossy(&buf);
            s.contains("missing") || s.contains("bdf")
        });
    }

    #[test]
    fn firmware_inventory_device_not_held() {
        let held = Arc::new(RwLock::new(HashMap::<String, HeldDevice>::new()));
        let params = serde_json::json!({"bdf": "0000:65:00.0"});
        let mut buf = Vec::new();

        let _ = firmware_inventory(&mut buf, &held, serde_json::json!(1), &params);
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("not held"), "expected 'not held' error, got: {s}");
    }
}
