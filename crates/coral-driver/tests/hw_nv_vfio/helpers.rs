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
                    let total_ms = result
                        .get("total_ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    eprintln!(
                        "glowplug: warm handoff complete (fecs_running={fecs_running}, {total_ms}ms)"
                    );
                    if !fecs_running {
                        eprintln!(
                            "glowplug: WARNING — FECS never seen running during poll window"
                        );
                    }
                }
                Err(e) => {
                    eprintln!("glowplug: warm_handoff RPC failed: {e}");
                    eprintln!("glowplug: falling back — assuming warm handoff was done externally (coralctl warm-fecs)");
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
    NvVfioComputeDevice::open_warm(&bdf, fds, sm, 0)
        .expect("NvVfioComputeDevice::open_warm()")
}
