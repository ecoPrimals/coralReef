// SPDX-License-Identifier: AGPL-3.0-only

use crate::ember_client;
use coral_driver::nv::NvVfioComputeDevice;

pub fn init_tracing() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_test_writer()
            .try_init()
            .ok();
    });
}

pub fn vfio_bdf() -> String {
    std::env::var("CORALREEF_VFIO_BDF")
        .expect("set CORALREEF_VFIO_BDF=0000:XX:XX.X to run VFIO tests")
}

/// SM hint: 0 = auto-detect from BOOT0 (preferred), nonzero = validate.
pub fn vfio_sm() -> u32 {
    std::env::var("CORALREEF_VFIO_SM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Open VFIO device — primary path: get fds from ember via SCM_RIGHTS.
/// Fallback: open /dev/vfio/* directly (only works without ember).
///
/// SM and compute class are auto-detected from BOOT0 by default.
/// Set `CORALREEF_VFIO_SM` to a nonzero value to validate instead.
pub fn open_vfio() -> NvVfioComputeDevice {
    init_tracing();
    let bdf = vfio_bdf();
    let sm = vfio_sm();

    match ember_client::request_fds(&bdf) {
        Ok(fds) => {
            eprintln!("ember: received VFIO fds for {bdf}");
            NvVfioComputeDevice::open_from_fds(&bdf, fds, sm, 0)
                .expect("NvVfioComputeDevice::open_from_fds()")
        }
        Err(e) => {
            eprintln!("ember unavailable ({e}), opening VFIO directly");
            NvVfioComputeDevice::open(&bdf, sm, 0)
                .expect("NvVfioComputeDevice::open() — is GPU bound to vfio-pci?")
        }
    }
}

/// K80 BDF from env var `CORALREEF_K80_BDF` (falls back to `CORALREEF_VFIO_BDF`).
pub fn k80_bdf() -> String {
    std::env::var("CORALREEF_K80_BDF")
        .or_else(|_| std::env::var("CORALREEF_VFIO_BDF"))
        .expect("set CORALREEF_K80_BDF or CORALREEF_VFIO_BDF to target a K80 die")
}

/// Open K80 via ember fds with direct-open fallback.
/// SM and compute class are auto-detected from BOOT0 (SM 37 = GK210).
pub fn open_k80() -> NvVfioComputeDevice {
    init_tracing();
    let bdf = k80_bdf();

    match ember_client::request_fds(&bdf) {
        Ok(fds) => {
            eprintln!("ember: received VFIO fds for {bdf} (K80)");
            NvVfioComputeDevice::open_from_fds(&bdf, fds, 0, 0)
                .expect("open_from_fds — is FECS booted on K80?")
        }
        Err(e) => {
            eprintln!("ember unavailable ({e}), trying direct open for K80");
            NvVfioComputeDevice::open(&bdf, 0, 0)
                .expect("direct K80 open — need root or vfio group perms")
        }
    }
}

/// Read a BAR0 register through ember's `mmio.read` RPC (no sudo needed).
/// Falls back to None if ember is not available.
pub fn ember_mmio_read(bdf: &str, offset: u32) -> Option<u32> {
    match ember_client::mmio_read(bdf, offset) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("ember.mmio.read({bdf}, {offset:#x}): {e}");
            None
        }
    }
}

/// Bulk BAR0 read via glowplug (returns offset→value map).
/// Useful for register-level diagnostics without sysfs root access.
pub fn glowplug_bar0_range(bdf: &str, offset: u64, count: u64) -> Option<serde_json::Value> {
    match crate::glowplug_client::GlowPlugClient::connect() {
        Ok(mut gp) => match gp.read_bar0_range(bdf, offset, count) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("glowplug.read_bar0_range: {e}");
                None
            }
        },
        Err(e) => {
            eprintln!("glowplug: not available ({e})");
            None
        }
    }
}

/// Orchestrate a full warm handoff via glowplug, then open in warm mode.
///
/// 1. Connects to glowplug and calls `device.warm_handoff` — this swaps to
///    nouveau (FECS boots), waits, then swaps back to `vfio-pci`.
/// 2. Requests fresh VFIO fds from ember via SCM_RIGHTS.
/// 3. Opens the device with `open_warm` to preserve falcon state.
///
/// No `sudo` required — ember/glowplug run as root and handle all
/// privileged operations (livepatch, driver swap, VFIO fd management).
pub fn open_vfio_warm() -> NvVfioComputeDevice {
    init_tracing();
    let bdf = vfio_bdf();
    let sm = vfio_sm();

    // Step 1: trigger warm handoff through glowplug (nouveau → fecs boot → vfio-pci)
    match crate::glowplug_client::GlowPlugClient::connect() {
        Ok(mut gp) => {
            eprintln!("glowplug: orchestrating warm handoff for {bdf}...");
            match gp.warm_handoff(&bdf, "nouveau", 2000, true, 15000) {
                Ok(result) => {
                    let fecs_running = result
                        .get("fecs_ever_running")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let total_ms = result.get("total_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                    eprintln!(
                        "glowplug: warm handoff complete (fecs_running={fecs_running}, {total_ms}ms)"
                    );
                    if !fecs_running {
                        eprintln!("glowplug: WARNING — FECS never seen running during poll window");
                    }
                }
                Err(e) => {
                    eprintln!("glowplug: warm_handoff RPC failed: {e}");
                    eprintln!(
                        "glowplug: falling back — assuming warm handoff was done externally (coralctl warm-fecs)"
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("glowplug: not available ({e})");
            eprintln!("glowplug: assuming warm handoff was done externally (coralctl warm-fecs)");
        }
    }

    // Step 2: get VFIO fds from ember
    let fds = match ember_client::request_fds(&bdf) {
        Ok(fds) => {
            eprintln!("ember: received VFIO fds for {bdf} (WARM MODE)");
            fds
        }
        Err(e) => {
            panic!("warm handoff requires ember for VFIO fds (ember unavailable: {e})");
        }
    };

    // Step 3: open in warm mode (preserves falcon state from nouveau)
    NvVfioComputeDevice::open_warm(&bdf, fds, sm, 0).expect("NvVfioComputeDevice::open_warm()")
}

/// Orchestrate a full warm handoff with FECS freeze context (Exp 132).
///
/// Like `open_vfio_warm()` but uses `open_warm_with_context` to pass the
/// handoff result's FECS freeze status and PFIFO snapshot to the device
/// open, enabling the hybrid PFIFO init that rebuilds scheduler state.
pub fn open_vfio_warm_with_context() -> NvVfioComputeDevice {
    use coral_driver::nv::vfio_compute::{PfifoSnapshot, WarmHandoffContext};

    init_tracing();
    let bdf = vfio_bdf();
    let sm = vfio_sm();

    let mut ctx = WarmHandoffContext::default();

    match crate::glowplug_client::GlowPlugClient::connect() {
        Ok(mut gp) => {
            eprintln!("glowplug: orchestrating warm handoff (with context) for {bdf}...");
            match gp.warm_handoff(&bdf, "nouveau", 2000, true, 15000) {
                Ok(result) => {
                    ctx.fecs_alive = result
                        .get("fecs_ever_running")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    ctx.fecs_frozen = result
                        .get("fecs_frozen")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    if let Some(snap) = result.get("pfifo_snapshot") {
                        let parse_hex = |key: &str| -> u32 {
                            snap.get(key)
                                .and_then(|v| v.as_str())
                                .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                                .unwrap_or(0)
                        };
                        ctx.pfifo_snapshot = Some(PfifoSnapshot {
                            pmc_enable: parse_hex("pmc_enable"),
                            pbdma_map: parse_hex("pbdma_map"),
                            pfifo_sched_en: parse_hex("pfifo_sched_en"),
                            runlist_base: parse_hex("runlist_base"),
                            runlist_submit: parse_hex("runlist_submit"),
                        });
                    }

                    let total_ms = result.get("total_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                    eprintln!(
                        "glowplug: warm handoff complete (fecs_alive={}, fecs_frozen={}, {total_ms}ms)",
                        ctx.fecs_alive, ctx.fecs_frozen
                    );
                }
                Err(e) => {
                    eprintln!("glowplug: warm_handoff RPC failed: {e}");
                    eprintln!("glowplug: falling back to default context");
                }
            }
        }
        Err(e) => {
            eprintln!("glowplug: not available ({e}), using default context");
        }
    }

    let fds = match ember_client::request_fds(&bdf) {
        Ok(fds) => {
            eprintln!("ember: received VFIO fds for {bdf} (WARM+CONTEXT)");
            fds
        }
        Err(e) => {
            panic!("warm handoff with context requires ember for VFIO fds ({e})");
        }
    };

    NvVfioComputeDevice::open_warm_with_context(&bdf, fds, sm, 0, &ctx)
        .expect("NvVfioComputeDevice::open_warm_with_context()")
}

/// Post-warm-handoff falcon state snapshot for regression testing.
#[derive(Debug)]
pub struct WarmHandoffReport {
    pub bdf: String,
    pub handoff_result: serde_json::Value,
    pub fecs_running: bool,
    pub total_ms: u64,
    pub fecs_state: Option<serde_json::Value>,
}

/// Like `open_vfio_warm()` but returns both the device and a diagnostic report
/// capturing handoff timing and FECS state — for automated regression detection.
pub fn open_vfio_warm_diagnostic() -> (NvVfioComputeDevice, WarmHandoffReport) {
    init_tracing();
    let bdf = vfio_bdf();
    let sm = vfio_sm();

    let mut report = WarmHandoffReport {
        bdf: bdf.clone(),
        handoff_result: serde_json::Value::Null,
        fecs_running: false,
        total_ms: 0,
        fecs_state: None,
    };

    match crate::glowplug_client::GlowPlugClient::connect() {
        Ok(mut gp) => {
            eprintln!("glowplug: orchestrating warm handoff (diagnostic) for {bdf}...");
            match gp.warm_handoff(&bdf, "nouveau", 2000, true, 15000) {
                Ok(result) => {
                    report.fecs_running = result
                        .get("fecs_ever_running")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    report.total_ms = result.get("total_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                    report.handoff_result = result;
                    eprintln!(
                        "glowplug: warm handoff complete (fecs_running={}, {}ms)",
                        report.fecs_running, report.total_ms
                    );
                }
                Err(e) => {
                    eprintln!("glowplug: warm_handoff RPC failed: {e}");
                }
            }
        }
        Err(e) => {
            eprintln!("glowplug: not available for diagnostic warm handoff ({e})");
        }
    }

    // Snapshot FECS state via ember
    match ember_client::fecs_state(&bdf) {
        Ok(state) => {
            eprintln!("ember: FECS state captured post-handoff");
            report.fecs_state = Some(state);
        }
        Err(e) => {
            eprintln!("ember: fecs_state unavailable ({e})");
        }
    }

    let fds = ember_client::request_fds(&bdf)
        .unwrap_or_else(|e| panic!("warm handoff diagnostic requires ember for VFIO fds ({e})"));
    eprintln!("ember: received VFIO fds for {bdf} (WARM DIAGNOSTIC)");

    let dev =
        NvVfioComputeDevice::open_warm(&bdf, fds, sm, 0).expect("NvVfioComputeDevice::open_warm()");

    (dev, report)
}

/// RAII guard that enables livepatch on creation and disables on drop.
///
/// Used in warm handoff tests to ensure the kernel livepatch module is
/// active during the nouveau → vfio-pci swap, then cleaned up afterwards.
pub struct LivepatchGuard {
    enabled: bool,
}

impl LivepatchGuard {
    /// Enable livepatch via ember. Returns Err if ember is unreachable.
    pub fn enable() -> Result<Self, String> {
        let result = ember_client::livepatch_enable()?;
        let was_noop = result
            .get("was_noop")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if was_noop {
            eprintln!("livepatch: already enabled (noop)");
        } else {
            eprintln!("livepatch: enabled via ember");
        }
        Ok(Self { enabled: true })
    }

    /// Query current livepatch status through ember.
    pub fn status(&self) -> Result<serde_json::Value, String> {
        ember_client::livepatch_status()
    }
}

impl Drop for LivepatchGuard {
    fn drop(&mut self) {
        if self.enabled {
            match ember_client::livepatch_disable() {
                Ok(_) => eprintln!("livepatch: disabled on guard drop"),
                Err(e) => eprintln!("livepatch: failed to disable on drop: {e}"),
            }
        }
    }
}

/// Result of a VBIOS recipe replay.
#[derive(Debug)]
pub struct RecipeReplayResult {
    pub total_writes: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
}

/// Replay the GK210 VBIOS init recipe through glowplug.write_register.
///
/// Set `CORALREEF_K80_BIOS_RECIPE` to override the default recipe path.
/// Default: `../springs/hotSpring/data/k80/nvidia470-vm-captures/gk210_full_bios_recipe.json`
/// relative to the coralReef repo root (i.e. sibling repo layout).
///
/// `dangerous_regions` controls whether to allow writes to known-dangerous
/// registers (PMC, PBUS, etc.). Set `true` for full replay, `false` for safe-only.
pub fn replay_k80_bios_recipe(
    bdf: &str,
    dangerous_regions: bool,
) -> Result<RecipeReplayResult, String> {
    let recipe_path = std::env::var("CORALREEF_K80_BIOS_RECIPE").unwrap_or_else(|_| {
        let manifest =
            std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        format!(
            "{manifest}/../../../springs/hotSpring/data/k80/nvidia470-vm-captures/gk210_full_bios_recipe.json"
        )
    });

    let raw = std::fs::read_to_string(&recipe_path)
        .map_err(|e| format!("read recipe {recipe_path}: {e}"))?;
    let recipe: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse recipe: {e}"))?;

    let writes = recipe
        .get("writes")
        .and_then(|v| v.as_array())
        .ok_or("recipe missing 'writes' array")?;

    let mut gp = crate::glowplug_client::GlowPlugClient::connect()
        .map_err(|e| format!("glowplug connect: {e}"))?;

    let mut result = RecipeReplayResult {
        total_writes: writes.len(),
        succeeded: 0,
        failed: 0,
        skipped: 0,
    };

    let dangerous = ["PMC", "PBUS", "PPCI", "PRING", "PDISP"];

    for entry in writes {
        let offset = entry
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(u64::MAX);
        let value = entry.get("value").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let region = entry.get("region").and_then(|v| v.as_str()).unwrap_or("");

        if offset == u64::MAX {
            result.skipped += 1;
            continue;
        }

        if !dangerous_regions && dangerous.contains(&region) {
            result.skipped += 1;
            continue;
        }

        match gp.write_register(bdf, offset, value, dangerous_regions) {
            Ok(_) => result.succeeded += 1,
            Err(e) => {
                eprintln!("  replay: write {offset:#x}={value:#x} ({region}): {e}");
                result.failed += 1;
            }
        }
    }

    eprintln!(
        "recipe replay: {}/{} ok, {} failed, {} skipped",
        result.succeeded, result.total_writes, result.failed, result.skipped
    );

    Ok(result)
}
