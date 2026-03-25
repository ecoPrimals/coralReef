// SPDX-License-Identifier: AGPL-3.0-only
//! Integration tests for [`coral_ember::handle_swap_device`] (non-hardware paths).

use std::collections::HashMap;
use std::sync::Mutex;

use coral_ember::{HeldDevice, handle_swap_device, verify_drm_isolation_with_paths};

static SWAP_TEST_LOCK: Mutex<()> = Mutex::new(());

const NONEXISTENT_BDF: &str = "9999:99:99.9";

#[test]
fn handle_swap_unbound_without_sysfs_device_succeeds() {
    let _guard = SWAP_TEST_LOCK.lock().expect("swap lock");
    let mut held: HashMap<String, HeldDevice> = HashMap::new();
    let obs = handle_swap_device(NONEXISTENT_BDF, "unbound", &mut held, false).expect("unbound");
    assert_eq!(obs.to_personality, "unbound");
}

#[test]
fn handle_swap_unknown_target_errors() {
    let _guard = SWAP_TEST_LOCK.lock().expect("swap lock");
    let mut held: HashMap<String, HeldDevice> = HashMap::new();
    let err =
        handle_swap_device(NONEXISTENT_BDF, "not-a-real-driver", &mut held, false).unwrap_err();
    assert!(err.contains("unknown target driver"));
}

#[test]
fn verify_drm_isolation_ok_with_temp_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let xorg = dir.path().join("xorg.conf");
    let udev = dir.path().join("udev.rules");
    let bdf = "0000:03:00.0";
    std::fs::write(&xorg, "Option \"AutoAddGPU\" \"false\"\n").expect("xorg");
    std::fs::write(
        &udev,
        format!("KERNEL==\"card*\", ATTR{{address}}==\"{bdf}\""),
    )
    .expect("udev");
    verify_drm_isolation_with_paths(
        bdf,
        xorg.to_str().expect("utf8"),
        udev.to_str().expect("utf8"),
    )
    .expect("isolation ok");
}

#[test]
fn verify_drm_isolation_fails_when_xorg_missing_autoaddgpu_false() {
    let dir = tempfile::tempdir().expect("tempdir");
    let xorg = dir.path().join("xorg.conf");
    let udev = dir.path().join("udev.rules");
    let bdf = "0000:03:00.0";
    std::fs::write(&xorg, "Section \"Device\"\nEndSection\n").expect("xorg");
    std::fs::write(
        &udev,
        format!("KERNEL==\"card*\", ATTR{{address}}==\"{bdf}\""),
    )
    .expect("udev");
    let err = verify_drm_isolation_with_paths(
        bdf,
        xorg.to_str().expect("utf8"),
        udev.to_str().expect("utf8"),
    )
    .expect_err("expected drm isolation failure");
    assert!(err.contains("AutoAddGPU"));
}

#[test]
fn verify_drm_isolation_fails_when_udev_missing_bdf_token() {
    let dir = tempfile::tempdir().expect("tempdir");
    let xorg = dir.path().join("xorg.conf");
    let udev = dir.path().join("udev.rules");
    let bdf = "0000:03:00.0";
    std::fs::write(&xorg, "Option \"AutoAddGPU\" \"false\"\n").expect("xorg");
    std::fs::write(&udev, "ATTR{address}==\"0000:04:00.0\"").expect("udev");
    let err = verify_drm_isolation_with_paths(
        bdf,
        xorg.to_str().expect("utf8"),
        udev.to_str().expect("utf8"),
    )
    .expect_err("expected udev mismatch");
    assert!(err.contains("does not cover BDF"));
}

#[test]
fn handle_swap_non_drm_native_target_skips_drm_gate() {
    let _guard = SWAP_TEST_LOCK.lock().expect("swap lock");
    let mut held: HashMap<String, HeldDevice> = HashMap::new();
    let err =
        handle_swap_device(NONEXISTENT_BDF, "akida-pcie", &mut held, false).expect_err("swap");
    assert!(
        !err.contains("DRM isolation"),
        "akida-pcie should not hit DRM isolation: {err}"
    );
    assert!(
        err.contains("preflight") || err.contains("swap_device"),
        "expected preflight or swap error for nonexistent device: {err}"
    );
}

#[test]
fn handle_swap_drm_targets_hit_isolation_or_sysfs() {
    let _guard = SWAP_TEST_LOCK.lock().expect("swap lock");
    for target in ["amdgpu", "nouveau", "nvidia", "xe", "i915"] {
        let mut held: HashMap<String, HeldDevice> = HashMap::new();
        let err = handle_swap_device(NONEXISTENT_BDF, target, &mut held, false).expect_err(target);
        assert!(
            err.contains("BLOCKED")
                || err.contains("swap_device")
                || err.contains("bind")
                || err.contains("preflight"),
            "{target}: unexpected error: {err}"
        );
    }
}

#[test]
fn handle_swap_vfio_alias_targets_fail_consistently() {
    let _guard = SWAP_TEST_LOCK.lock().expect("swap lock");
    let mut held_vfio = HashMap::new();
    let mut held_hyphen = HashMap::new();
    let e1 = handle_swap_device(NONEXISTENT_BDF, "vfio", &mut held_vfio, false);
    let e2 = handle_swap_device(NONEXISTENT_BDF, "vfio-pci", &mut held_hyphen, false);
    assert!(e1.is_err());
    assert!(e2.is_err());
}
