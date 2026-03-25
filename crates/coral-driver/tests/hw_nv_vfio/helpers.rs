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
