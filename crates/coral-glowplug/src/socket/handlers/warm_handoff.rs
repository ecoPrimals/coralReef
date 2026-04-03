// SPDX-License-Identifier: AGPL-3.0-only
//! Warm handoff: swap to a kernel driver, settle, poll FECS, swap back to vfio.

use std::sync::Arc;

use super::validate_bdf;

/// Find the DRM render node for a PCI device by BDF.
///
/// Scans `/sys/class/drm/renderD*/device/uevent` for the matching BDF
/// and returns the `/dev/dri/renderDNNN` path.
fn find_render_node(bdf: &str) -> Option<String> {
    let bdf_upper = bdf.to_uppercase();
    for entry in std::fs::read_dir("/sys/class/drm").ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("renderD") {
            continue;
        }
        let uevent_path = entry.path().join("device/uevent");
        if let Ok(uevent) = std::fs::read_to_string(&uevent_path) {
            if uevent.to_uppercase().contains(&bdf_upper) {
                return Some(format!("/dev/dri/{name}"));
            }
        }
    }
    None
}

/// Spawn a GPU workload process that forces FECS into an active running state.
///
/// For nvidia: creates a CUDA context which forces RM to start the GR engine.
/// For nouveau: headless EGL+GL rendering loop on the device's render node.
///
/// Returns the child process handle so it can be killed after FECS is frozen.
fn spawn_gpu_keepalive(bdf: &str, driver: &str) -> Option<std::process::Child> {
    let cmd = match driver {
        "nvidia" => {
            // python3 one-liner: create a CUDA context (forces GR/FECS active),
            // then sleep to keep it alive. Falls back to nvidia-smi if python3 unavailable.
            let script = "import ctypes,time\n\
                          try:\n  lib=ctypes.CDLL('libcuda.so.1')\n  lib.cuInit(0)\n\
                            ctx=ctypes.c_void_p()\n  lib.cuCtxCreate(ctypes.byref(ctx),0,0)\n\
                            time.sleep(120)\n\
                          except: time.sleep(120)\n";
            std::process::Command::new("python3")
                .arg("-c")
                .arg(script)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()
        }
        "nouveau" => {
            // glmark2-drm renders directly via DRM KMS, bypassing X11/Wayland.
            // It submits real GPU draw calls through nouveau, forcing FECS
            // into active scheduling mode.
            let render_node = find_render_node(bdf);
            tracing::info!(
                bdf,
                render_node = render_node.as_deref().unwrap_or("none"),
                "warm_handoff: keepalive using glmark2-drm"
            );
            if let Some(ref node) = render_node {
                std::process::Command::new("glmark2-drm")
                    .arg("--off-screen")
                    .env("MESA_LOADER_DRIVER_OVERRIDE", "nouveau")
                    .env("DRM_DEVICE", node.as_str())
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .ok()
            } else {
                tracing::warn!(bdf, "warm_handoff: no render node found for keepalive");
                None
            }
        }
        _ => None,
    };

    if cmd.is_some() {
        tracing::info!(bdf, driver, "warm_handoff: keepalive workload spawned");
    } else {
        tracing::warn!(bdf, driver, "warm_handoff: failed to spawn keepalive workload");
    }
    cmd
}

/// Capture SEC2 falcon state while it's still alive (non-PRI-faulted).
///
/// Reads SEC2 control registers and the first 256 bytes of DMEM via PIO.
/// This must be called during the narrow window where nouveau has SEC2
/// running for ACR boot — before it disables the engine.
fn capture_sec2_live(
    ember: &coral_glowplug::ember::EmberClient,
    bdf: &str,
) -> serde_json::Value {
    const SEC2_BASE: u32 = 0x0084_0000;
    let sec2_cpuctl = ember.mmio_read(bdf, SEC2_BASE + 0x100).unwrap_or(0);
    let sec2_sctl = ember.mmio_read(bdf, SEC2_BASE + 0x240).unwrap_or(0);
    let sec2_bind_inst = ember.mmio_read(bdf, SEC2_BASE + 0x090).unwrap_or(0);
    let sec2_bootvec = ember.mmio_read(bdf, SEC2_BASE + 0x104).unwrap_or(0);
    let pramin_window = ember.mmio_read(bdf, 0x1700).unwrap_or(0);
    let pmc_enable = ember.mmio_read(bdf, 0x200).unwrap_or(0);

    // Read first 256 bytes (64 u32s) of SEC2 DMEM via PIO.
    // DMEMC = SEC2_BASE + 0x1C0, DMEMD = SEC2_BASE + 0x1C4.
    // BIT(25) = read mode for GM200+ PIO protocol.
    let mut sec2_dmem = Vec::with_capacity(64);
    for word_idx in 0..64u32 {
        let addr = word_idx * 4;
        let _ = ember.mmio_write(bdf, SEC2_BASE + 0x1C0, addr | (1 << 25));
        let val = ember.mmio_read(bdf, SEC2_BASE + 0x1C4).unwrap_or(0xDEAD_DEAD);
        sec2_dmem.push(format!("{val:#010x}"));
    }

    tracing::info!(
        bdf,
        sec2_cpuctl = format_args!("{sec2_cpuctl:#010x}"),
        sec2_sctl = format_args!("{sec2_sctl:#010x}"),
        sec2_bind_inst = format_args!("{sec2_bind_inst:#010x}"),
        sec2_bootvec = format_args!("{sec2_bootvec:#010x}"),
        pramin_window = format_args!("{pramin_window:#010x}"),
        pmc_enable = format_args!("{pmc_enable:#010x}"),
        "SEC2 LIVE snapshot captured"
    );

    serde_json::json!({
        "captured_live": true,
        "sec2_cpuctl": format!("{sec2_cpuctl:#010x}"),
        "sec2_sctl": format!("{sec2_sctl:#010x}"),
        "sec2_bind_inst": format!("{sec2_bind_inst:#010x}"),
        "sec2_bootvec": format!("{sec2_bootvec:#010x}"),
        "pramin_window": format!("{pramin_window:#010x}"),
        "pmc_enable": format!("{pmc_enable:#010x}"),
        "sec2_dmem_0x000_0x0FF": sec2_dmem,
    })
}

pub(crate) fn handle_warm_handoff(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();
    let driver = params
        .get("driver")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("nouveau");
    let settle_ms = params
        .get("settle_ms")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(2000);
    let poll_fecs = params
        .get("poll_fecs")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let poll_timeout_ms = params
        .get("poll_timeout_ms")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(30_000);
    let keepalive = params
        .get("keepalive")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let enable_trace = params
        .get("trace")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let allow_cold = params
        .get("allow_cold")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let slot = devices
        .iter_mut()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf.as_str()),
        })
        .map_err(RpcError::from)?;
    if slot.is_busy() {
        return Err(RpcError::device_error(format!(
            "device {bdf} is busy — cannot perform warm handoff"
        )));
    }

    let ember = coral_glowplug::ember::EmberClient::connect().ok_or_else(|| {
        RpcError::device_error("ember not available — warm handoff requires ember")
    })?;

    let handoff_start = std::time::Instant::now();

    // Step 1: Disable livepatch if targeting nouveau (unfreeze teardown paths)
    if driver == "nouveau" {
        tracing::info!(bdf = %bdf, "warm_handoff: disabling livepatch");
        if let Err(e) = ember.livepatch_disable() {
            tracing::warn!(bdf = %bdf, error = %e, "warm_handoff: livepatch disable failed (non-fatal)");
        }
    }

    // Step 2: Capture pre-swap FECS state
    let pre_fecs = ember.fecs_state(&bdf).ok();

    // Step 3: Swap to target driver via ember (use cold path if requested)
    tracing::info!(bdf = %bdf, driver, trace = enable_trace, allow_cold, "warm_handoff: swapping to driver");
    if allow_cold {
        slot.swap_cold_traced(driver, enable_trace)
    } else {
        slot.swap_traced(driver, enable_trace)
    }
    .map_err(|e| RpcError::device_error(format!("swap to {driver}: {e}")))?;

    // Step 4: Settle — but poll SEC2 during the wait (Exp 139).
    //
    // SEC2 runs only briefly during nouveau init (for ACR bootstrap).
    // If we sleep the full settle period, SEC2 may be disabled by the
    // time we start polling. Instead, use active polling with 50ms
    // intervals and capture SEC2 DMEM the instant it's alive.
    let mut sec2_live_snapshot: Option<serde_json::Value> = None;
    {
        let settle_start = std::time::Instant::now();
        let settle_dur = std::time::Duration::from_millis(settle_ms);
        while settle_start.elapsed() < settle_dur {
            if sec2_live_snapshot.is_none() {
                if let Ok(sec2_cpuctl) = ember.mmio_read(&bdf, 0x0084_0100) {
                    let is_pri_fault = (sec2_cpuctl & 0xBADF_0000) == 0xBADF_0000;
                    if !is_pri_fault {
                        tracing::info!(
                            bdf = %bdf,
                            sec2_cpuctl = format_args!("{sec2_cpuctl:#010x}"),
                            elapsed_ms = settle_start.elapsed().as_millis(),
                            "warm_handoff: SEC2 ALIVE during settle — capturing DMEM"
                        );
                        sec2_live_snapshot = Some(capture_sec2_live(&ember, &bdf));
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    // Step 4b: Spawn GPU keepalive workload if requested.
    // This forces FECS into an active running state (not idle-halted) so
    // the poll loop can detect it and STOP_CTXSW can freeze it.
    let mut keepalive_child = if keepalive {
        spawn_gpu_keepalive(&bdf, driver)
    } else {
        None
    };

    // Step 5: Enable livepatch (freeze teardown paths)
    if driver == "nouveau" {
        tracing::info!(bdf = %bdf, "warm_handoff: enabling livepatch");
        if let Err(e) = ember.livepatch_enable() {
            tracing::warn!(bdf = %bdf, error = %e, "warm_handoff: livepatch enable failed");
        }
    }

    // Step 6: Poll FECS + SEC2 in parallel.
    //
    // FECS tiers:
    //   Tier 1 — actively running: halted=false, stopped=false, pri_fault=false
    //   Tier 2 — initialized idle-halted: halted=true, stopped=false, cpuctl!=0
    //
    // SEC2 capture: nouveau boots SEC2 briefly for ACR (to bootstrap FECS/GPCCS),
    // then disables it. We poll SEC2_CPUCTL alongside FECS and capture DMEM the
    // moment SEC2 is alive (non-PRI-fault). This is the only window to read the
    // CMDQ/MSGQ ring buffers that SEC2 uses for ACR commands. (Exp 139)
    let mut fecs_ever_running = false;
    let mut fecs_initialized = false;
    let mut poll_count = 0u32;
    let mut last_fecs_during_poll = None;
    if poll_fecs {
        let poll_start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(poll_timeout_ms);
        while poll_start.elapsed() < timeout {
            poll_count += 1;

            // SEC2 opportunistic capture: check if SEC2 is alive this tick
            if sec2_live_snapshot.is_none() {
                if let Ok(sec2_cpuctl) = ember.mmio_read(&bdf, 0x0084_0100) {
                    let is_pri_fault = (sec2_cpuctl & 0xBADF_0000) == 0xBADF_0000;
                    if !is_pri_fault {
                        tracing::info!(
                            bdf = %bdf,
                            sec2_cpuctl = format_args!("{sec2_cpuctl:#010x}"),
                            poll_count,
                            elapsed_ms = poll_start.elapsed().as_millis(),
                            "warm_handoff: SEC2 ALIVE — capturing DMEM snapshot"
                        );
                        sec2_live_snapshot = Some(capture_sec2_live(&ember, &bdf));
                    }
                }
            }

            if let Ok(state) = ember.fecs_state(&bdf) {
                let pri_fault = state
                    .get("pri_fault")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let halted = state
                    .get("halted")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let stopped = state
                    .get("stopped")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let cpuctl_str = state
                    .get("cpuctl")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0x00000000")
                    .to_owned();
                let cpuctl_nonzero = cpuctl_str != "0x00000000";

                last_fecs_during_poll = Some(state);

                // Tier 1: actively running
                if !pri_fault && !halted && !stopped {
                    fecs_ever_running = true;
                    fecs_initialized = true;
                    tracing::info!(
                        bdf = %bdf,
                        poll_count,
                        elapsed_ms = poll_start.elapsed().as_millis(),
                        "warm_handoff: FECS detected running (tier 1)"
                    );
                    if !keepalive {
                        break;
                    }
                }

                // Tier 2: initialized but idle-halted
                if !pri_fault && halted && !stopped && cpuctl_nonzero && !fecs_initialized {
                    fecs_initialized = true;
                    tracing::info!(
                        bdf = %bdf,
                        poll_count,
                        cpuctl = %cpuctl_str,
                        "warm_handoff: FECS detected initialized (tier 2, idle-halted)"
                    );
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if !fecs_ever_running && !fecs_initialized {
            tracing::warn!(
                bdf = %bdf,
                poll_count,
                timeout_ms = poll_timeout_ms,
                "warm_handoff: FECS never seen running or initialized during poll window"
            );
        }
        if sec2_live_snapshot.is_none() {
            tracing::warn!(
                bdf = %bdf,
                poll_count,
                "warm_handoff: SEC2 was never seen alive during poll window — \
                 ACR boot may have completed before polling started"
            );
        }
    }

    // Determine if we should proceed with FECS freeze and PFIFO capture.
    // Either tier is sufficient — the FECS method interface works on
    // initialized falcons even when idle-halted.
    let fecs_usable = fecs_ever_running || fecs_initialized;

    // Step 6b: Freeze FECS scheduling via STOP_CTXSW before driver teardown.
    //
    // FECS method interface: write data to 0x409500, method to 0x409504,
    // poll 0x409804 for completion. Method 0x01 = STOP_CTXSW.
    // This keeps FECS alive but stops it from scheduling channels — so it
    // won't idle-halt when the driver frees its channels during teardown.
    let mut fecs_frozen = false;
    if fecs_usable {
        tracing::info!(bdf = %bdf, fecs_ever_running, fecs_initialized, "warm_handoff: freezing FECS scheduling (STOP_CTXSW)");

        // Clear status registers
        let _ = ember.mmio_write(&bdf, 0x0040_9804, 0); // MTHD_STATUS2
        let _ = ember.mmio_write(&bdf, 0x0040_9800, 0); // MTHD_STATUS
        // Write method data=0, method=0x01 (STOP_CTXSW)
        let _ = ember.mmio_write(&bdf, 0x0040_9500, 0); // MTHD_DATA
        let _ = ember.mmio_write(&bdf, 0x0040_9504, 0x01); // MTHD_CMD

        // Poll for completion (status2 == 0x01)
        let freeze_start = std::time::Instant::now();
        let freeze_timeout = std::time::Duration::from_millis(2000);
        while freeze_start.elapsed() < freeze_timeout {
            std::thread::sleep(std::time::Duration::from_millis(5));
            if let Ok(status2) = ember.mmio_read(&bdf, 0x0040_9804) {
                if status2 == 0x01 {
                    fecs_frozen = true;
                    tracing::info!(
                        bdf = %bdf,
                        elapsed_ms = freeze_start.elapsed().as_millis(),
                        "warm_handoff: FECS scheduling frozen (STOP_CTXSW success)"
                    );
                    break;
                }
                if status2 == 0x02 {
                    tracing::warn!(bdf = %bdf, "warm_handoff: STOP_CTXSW error (status2=0x02)");
                    break;
                }
            }
        }
        if !fecs_frozen {
            tracing::warn!(bdf = %bdf, "warm_handoff: STOP_CTXSW did not complete — proceeding anyway");
        }
    }

    // Kill keepalive workload before swap-back
    if let Some(ref mut child) = keepalive_child {
        tracing::info!(bdf = %bdf, pid = child.id(), "warm_handoff: killing keepalive workload");
        let _ = child.kill();
        let _ = child.wait();
    }

    // Step 6c: Capture PFIFO snapshot while the driver is still bound.
    let pfifo_snapshot = if fecs_usable {
        let pmc_enable = ember.mmio_read(&bdf, 0x0000_0200).unwrap_or(0);
        let pbdma_map = ember.mmio_read(&bdf, 0x0000_2004).unwrap_or(0);
        let pfifo_sched_en = ember.mmio_read(&bdf, 0x0000_2504).unwrap_or(0);
        let runlist_base = ember.mmio_read(&bdf, 0x0000_2270).unwrap_or(0);
        let runlist_submit = ember.mmio_read(&bdf, 0x0000_2274).unwrap_or(0);

        tracing::info!(
            bdf = %bdf,
            pmc_enable = format_args!("{pmc_enable:#010x}"),
            pbdma_map = format_args!("{pbdma_map:#010x}"),
            pfifo_sched_en = format_args!("{pfifo_sched_en:#010x}"),
            runlist_base = format_args!("{runlist_base:#010x}"),
            runlist_submit = format_args!("{runlist_submit:#010x}"),
            "warm_handoff: PFIFO snapshot captured"
        );

        Some(serde_json::json!({
            "pmc_enable": format!("{pmc_enable:#010x}"),
            "pbdma_map": format!("{pbdma_map:#010x}"),
            "pfifo_sched_en": format!("{pfifo_sched_en:#010x}"),
            "runlist_base": format!("{runlist_base:#010x}"),
            "runlist_submit": format!("{runlist_submit:#010x}"),
        }))
    } else {
        None
    };

    // Step 6d: Capture SEC2 falcon state while driver is still bound (Exp 138).
    //
    // SEC2 manages ACR (Authenticated Code Runner) which bootstraps
    // FECS/GPCCS on Volta+. Capturing its DMEM (CMDQ/MSGQ ring buffers)
    // and DMA context (BIND_INST) while the driver is alive gives us the
    // data needed to reconstruct sovereign ACR dispatch after swap-back.
    let sec2_snapshot = if fecs_usable {
        const SEC2_BASE: u32 = 0x0084_0000;
        let sec2_cpuctl = ember.mmio_read(&bdf, SEC2_BASE + 0x100).unwrap_or(0);
        let sec2_sctl = ember.mmio_read(&bdf, SEC2_BASE + 0x240).unwrap_or(0);
        let sec2_bind_inst = ember.mmio_read(&bdf, SEC2_BASE + 0x090).unwrap_or(0);
        let sec2_bootvec = ember.mmio_read(&bdf, SEC2_BASE + 0x104).unwrap_or(0);
        let pramin_window = ember.mmio_read(&bdf, 0x1700).unwrap_or(0);

        // Read first 256 bytes of SEC2 DMEM via PIO (DMEMC/DMEMD ports).
        // DMEMC = SEC2_BASE + 0x1C0, DMEMD = SEC2_BASE + 0x1C4.
        // BIT(25) = read mode for GM200+ PIO protocol.
        let mut sec2_dmem = Vec::with_capacity(64);
        for word_idx in 0..64u32 {
            let addr = word_idx * 4;
            // Set DMEMC: address | BIT(25) for read mode
            let _ = ember.mmio_write(&bdf, SEC2_BASE + 0x1C0, addr | (1 << 25));
            let val = ember.mmio_read(&bdf, SEC2_BASE + 0x1C4).unwrap_or(0xDEAD_DEAD);
            sec2_dmem.push(format!("{val:#010x}"));
        }

        tracing::info!(
            bdf = %bdf,
            sec2_cpuctl = format_args!("{sec2_cpuctl:#010x}"),
            sec2_sctl = format_args!("{sec2_sctl:#010x}"),
            sec2_bind_inst = format_args!("{sec2_bind_inst:#010x}"),
            sec2_bootvec = format_args!("{sec2_bootvec:#010x}"),
            pramin_window = format_args!("{pramin_window:#010x}"),
            "warm_handoff: SEC2 snapshot captured"
        );

        Some(serde_json::json!({
            "sec2_cpuctl": format!("{sec2_cpuctl:#010x}"),
            "sec2_sctl": format!("{sec2_sctl:#010x}"),
            "sec2_bind_inst": format!("{sec2_bind_inst:#010x}"),
            "sec2_bootvec": format!("{sec2_bootvec:#010x}"),
            "pramin_window": format!("{pramin_window:#010x}"),
            "sec2_dmem_0x000_0x0FF": sec2_dmem,
        }))
    } else {
        None
    };

    // Step 7: Swap back to vfio-pci
    tracing::info!(bdf = %bdf, "warm_handoff: swapping back to vfio-pci");
    slot.swap_traced("vfio", false)
        .map_err(|e| RpcError::device_error(format!("swap back to vfio: {e}")))?;

    // Step 8: Capture post-swap FECS state
    let post_fecs = ember.fecs_state(&bdf).ok();

    let total_ms = handoff_start.elapsed().as_millis() as u64;
    tracing::info!(
        bdf = %bdf,
        total_ms,
        fecs_ever_running,
        fecs_initialized,
        fecs_frozen,
        poll_count,
        "warm_handoff complete"
    );

    Ok(serde_json::json!({
        "bdf": bdf,
        "driver": driver,
        "total_ms": total_ms,
        "settle_ms": settle_ms,
        "poll_fecs": poll_fecs,
        "poll_count": poll_count,
        "fecs_ever_running": fecs_ever_running,
        "fecs_initialized": fecs_initialized,
        "fecs_frozen": fecs_frozen,
        "pre_fecs": pre_fecs,
        "post_fecs": post_fecs,
        "last_fecs_during_poll": last_fecs_during_poll,
        "pfifo_snapshot": pfifo_snapshot,
        "sec2_snapshot": sec2_snapshot,
        "sec2_live_snapshot": sec2_live_snapshot,
        "personality": slot.personality.to_string(),
        "vram_alive": slot.health.vram_alive,
    }))
}
