// SPDX-License-Identifier: AGPL-3.0-only
//! Warm FECS handlers: nouveau and nvidia proprietary round-trip flows.

use crate::rpc::{check_rpc_error, rpc_call};
use serde_json::json;

pub(crate) fn rpc_warm_fecs(
    socket: &str,
    bdf: &str,
    settle_secs: u64,
    poll_fecs: bool,
    keepalive: bool,
) {
    println!("=== Warm FECS via nouveau round-trip ===");

    // Livepatch must be DISABLED before nouveau loads so gk104_runl_commit
    // (and other functions) run normally during init. If it's enabled and
    // nouveau loads, the NOP would prevent runlist submission and break init.
    let lp_enabled = "/sys/kernel/livepatch/livepatch_nvkm_mc_reset/enabled";
    if std::path::Path::new(lp_enabled).exists() {
        let cur = std::fs::read_to_string(lp_enabled).unwrap_or_default();
        if cur.trim() == "1" {
            println!("step 0: disabling livepatch before nouveau load...");
            sysfs_write_privileged(lp_enabled, "0");
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

    // Keepalive: spawn a GPU workload to prevent FECS from entering idle-halt.
    // This opens a DRM render node and runs continuous GPU work so FECS stays
    // in its scheduling loop during the swap window.
    let mut keepalive_child: Option<std::process::Child> = None;
    if keepalive {
        println!("step 1b: spawning keepalive workload to prevent FECS idle-halt...");

        // Find the nouveau render node for this BDF
        let render_node = find_nouveau_render_node(bdf);
        if let Some(ref node) = render_node {
            println!("  render node: {node}");
            // Use a simple OpenGL loop via __NV_PRIME_RENDER_OFFLOAD or direct DRM_FILE
            // The simplest approach: use `dd` to hold the DRM fd open, which keeps
            // a DRM master/client active — nouveau keeps the channel alive.
            // Better approach: use `vulkaninfo` in a loop or `glxgears` offscreen.
            //
            // Most portable: open the render node and do periodic ioctls.
            // Simplest for now: hold fd open with a cat that blocks on read.
            match std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("exec 3<{node}; while true; do sleep 0.1; done"))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(child) => {
                    println!("  keepalive PID: {}", child.id());
                    keepalive_child = Some(child);
                }
                Err(e) => println!("  WARNING: keepalive spawn failed: {e}"),
            }
        } else {
            println!("  WARNING: no nouveau render node found for {bdf}");
        }

        // Give the keepalive process time to open the DRM fd
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    if poll_fecs {
        // Timing attack: wait minimum 2s for nouveau GR init, then poll
        // FECS CPUCTL via BAR0 sysfs. Swap the instant we see FECS running
        // (CPUCTL bit4=0, not halted) to catch it before idle-halt.
        const MIN_INIT_SECS: u64 = 2;
        const POLL_INTERVAL_MS: u64 = 50;
        const MAX_POLL_SECS: u64 = 30;

        println!("step 2: waiting {MIN_INIT_SECS}s minimum init, then polling FECS CPUCTL...");
        std::thread::sleep(std::time::Duration::from_secs(MIN_INIT_SECS));

        // Enable livepatch BEFORE polling — we want the NOPs active so that
        // if nouveau's idle path tries to drain the runlist, it's blocked.
        if std::path::Path::new(lp_enabled).exists() {
            println!("step 2a: enabling livepatch (freezing runlist for warm handoff)...");
            sysfs_write_privileged(lp_enabled, "1");
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        let bar0_path = format!("/sys/bus/pci/devices/{bdf}/resource0");
        let fecs_cpuctl_offset: u64 = 0x409100;
        let start = std::time::Instant::now();
        let mut last_cpuctl = 0xDEAD_DEADu32;
        let mut caught_running = false;

        println!("step 2b: polling FECS CPUCTL at {bar0_path} +{fecs_cpuctl_offset:#x}...");

        while start.elapsed().as_secs() < MAX_POLL_SECS {
            if let Ok(cpuctl) = read_bar0_u32(&bar0_path, fecs_cpuctl_offset) {
                let halted = cpuctl & 0x10 != 0;
                let stopped = cpuctl & 0x20 != 0;
                let dead = cpuctl == 0xDEAD_DEAD || cpuctl & 0xBADF_0000 == 0xBADF_0000;

                if cpuctl != last_cpuctl {
                    println!(
                        "  FECS CPUCTL={cpuctl:#010x} halted={halted} stopped={stopped} dead={dead} @ {:.1}s",
                        start.elapsed().as_secs_f64()
                    );
                    last_cpuctl = cpuctl;
                }

                if !dead && !halted && !stopped {
                    println!(
                        "  >>> FECS RUNNING at {:.3}s — triggering swap!",
                        start.elapsed().as_secs_f64()
                    );
                    caught_running = true;
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS));
        }

        if !caught_running {
            println!(
                "  WARNING: FECS never seen running after {:.1}s — swapping anyway (halted state)",
                start.elapsed().as_secs_f64()
            );
        }
    } else {
        println!("step 2: waiting {settle_secs}s for nouveau GR init...");
        std::thread::sleep(std::time::Duration::from_secs(settle_secs));

        // Enable livepatch AFTER init, BEFORE teardown — NOPs freeze the
        // runlist, prevent falcon halts, and skip engine resets so FECS
        // stays alive in its context-switch-ready HALT state.
        if std::path::Path::new(lp_enabled).exists() {
            println!("step 2b: enabling livepatch (freezing runlist for warm handoff)...");
            sysfs_write_privileged(lp_enabled, "1");
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
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

    // Kill keepalive process now that we're on vfio
    if let Some(mut child) = keepalive_child {
        println!("  killing keepalive PID {}...", child.id());
        let _ = child.kill();
        let _ = child.wait();
    }

    println!("=== warm-fecs complete — run vfio_dispatch_warm_handoff test ===");
}

/// Warm FECS via nvidia proprietary driver round-trip.
///
/// Unlike nouveau, nvidia RM has full control over FECS lifecycle and may
/// leave it in a state that's more amenable to warm handoff. Also captures
/// BAR0 register snapshots for diff analysis.
pub(crate) fn rpc_warm_fecs_nvidia(socket: &str, bdf: &str, settle_secs: u64) {
    println!("=== Warm FECS via nvidia proprietary round-trip ===");

    // Capture pre-swap FECS state via BAR0 sysfs
    let bar0_path = format!("/sys/bus/pci/devices/{bdf}/resource0");
    println!("step 0: capturing pre-swap FECS state...");
    let pre_fecs = read_bar0_u32(&bar0_path, 0x409100).unwrap_or(0xDEAD);
    let pre_sctl = read_bar0_u32(&bar0_path, 0x409240).unwrap_or(0xDEAD);
    let pre_pmc = read_bar0_u32(&bar0_path, 0x200).unwrap_or(0xDEAD);
    println!("  FECS CPUCTL={pre_fecs:#010x}  SCTL={pre_sctl:#010x}  PMC={pre_pmc:#010x}");

    println!("step 1: swapping {bdf} -> nvidia...");
    let resp1 = rpc_call(
        socket,
        "device.swap",
        json!({"bdf": bdf, "target": "nvidia"}),
    );
    check_rpc_error(&resp1);

    let personality = resp1
        .get("result")
        .and_then(|r| r.get("personality"))
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    println!("  now on {personality}");

    println!("step 2: waiting {settle_secs}s for nvidia RM init...");
    std::thread::sleep(std::time::Duration::from_secs(settle_secs));

    // Capture FECS state under nvidia
    println!("step 2b: capturing FECS state under nvidia RM...");
    let nvidia_fecs = read_bar0_u32(&bar0_path, 0x409100).unwrap_or(0xDEAD);
    let nvidia_sctl = read_bar0_u32(&bar0_path, 0x409240).unwrap_or(0xDEAD);
    let nvidia_pmc = read_bar0_u32(&bar0_path, 0x200).unwrap_or(0xDEAD);
    let nvidia_pc = read_bar0_u32(&bar0_path, 0x409030).unwrap_or(0xDEAD);
    let nvidia_sec2 = read_bar0_u32(&bar0_path, 0x87100).unwrap_or(0xDEAD);
    println!("  FECS CPUCTL={nvidia_fecs:#010x}  SCTL={nvidia_sctl:#010x}  PC={nvidia_pc:#010x}");
    println!("  PMC={nvidia_pmc:#010x}  SEC2={nvidia_sec2:#010x}");

    let nvidia_halted = nvidia_fecs & 0x10 != 0;
    let nvidia_stopped = nvidia_fecs & 0x20 != 0;
    let nvidia_running = !nvidia_halted && !nvidia_stopped && nvidia_fecs != 0xDEAD_DEAD;
    println!("  FECS: halted={nvidia_halted} stopped={nvidia_stopped} running={nvidia_running}");

    println!("step 3: swapping {bdf} -> vfio...");
    let resp2 = rpc_call(socket, "device.swap", json!({"bdf": bdf, "target": "vfio"}));
    check_rpc_error(&resp2);

    let personality = resp2
        .get("result")
        .and_then(|r| r.get("personality"))
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    println!("  now on {personality}");

    // Capture post-swap FECS state
    println!("step 4: capturing post-swap FECS state...");
    let post_fecs = read_bar0_u32(&bar0_path, 0x409100).unwrap_or(0xDEAD);
    let post_sctl = read_bar0_u32(&bar0_path, 0x409240).unwrap_or(0xDEAD);
    let post_pmc = read_bar0_u32(&bar0_path, 0x200).unwrap_or(0xDEAD);
    let post_pc = read_bar0_u32(&bar0_path, 0x409030).unwrap_or(0xDEAD);
    let post_sec2 = read_bar0_u32(&bar0_path, 0x87100).unwrap_or(0xDEAD);
    println!("  FECS CPUCTL={post_fecs:#010x}  SCTL={post_sctl:#010x}  PC={post_pc:#010x}");
    println!("  PMC={post_pmc:#010x}  SEC2={post_sec2:#010x}");

    let post_halted = post_fecs & 0x10 != 0;
    let post_stopped = post_fecs & 0x20 != 0;
    let post_alive = post_fecs != 0xDEAD_DEAD && post_fecs & 0xBADF_0000 != 0xBADF_0000;
    println!("  FECS: halted={post_halted} stopped={post_stopped} alive={post_alive}");

    // Summary
    println!("\n=== nvidia warm-fecs summary ===");
    println!(
        "  under nvidia:  CPUCTL={nvidia_fecs:#010x} SCTL={nvidia_sctl:#010x} running={nvidia_running}"
    );
    println!("  after swap:    CPUCTL={post_fecs:#010x} SCTL={post_sctl:#010x} alive={post_alive}");
    if post_alive && !post_halted {
        println!("  >>> FECS survived the swap and is NOT halted — test dispatch!");
    } else if post_alive {
        println!("  FECS survived but halted — same HS+ lockdown as nouveau path");
    } else {
        println!("  FECS did not survive — nvidia unbind destroyed falcon state");
    }
    println!("=== warm-fecs-nvidia complete ===");
}

/// Read a u32 from BAR0 via sysfs resource0 file seek+read.
///
/// Uses standard file I/O (no mmap/unsafe) to read a 4-byte register
/// from the PCI BAR0 sysfs resource. The kernel translates the file
/// read into a PCI MMIO access.
fn read_bar0_u32(resource_path: &str, offset: u64) -> Result<u32, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file =
        std::fs::File::open(resource_path).map_err(|e| format!("open {resource_path}: {e}"))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("seek to {offset:#x}: {e}"))?;
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf)
        .map_err(|e| format!("read at {offset:#x}: {e}"))?;
    Ok(u32::from_le_bytes(buf))
}

/// Find the DRM render node for a PCI device by walking sysfs.
fn find_nouveau_render_node(bdf: &str) -> Option<String> {
    let drm_dir = format!("/sys/bus/pci/devices/{bdf}/drm");
    let entries = std::fs::read_dir(&drm_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("renderD") {
            return Some(format!("/dev/dri/{name}"));
        }
    }
    // Fallback: check for card* nodes
    let entries = std::fs::read_dir(&drm_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("card") {
            return Some(format!("/dev/dri/{name}"));
        }
    }
    None
}

/// Write to a privileged sysfs path via `sudo -n coralreef-sysfs-write`.
///
/// Override the helper path with `$CORALREEF_SYSFS_WRITE` when set and non-empty.
/// Falls back to direct write if the helper is not installed.
fn sysfs_write_privileged(path: &str, value: &str) {
    let helper = std::env::var("CORALREEF_SYSFS_WRITE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/bin/coralreef-sysfs-write".to_string());

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
