// SPDX-License-Identifier: AGPL-3.0-or-later
//! Layer 0 — AMD device probe: can we open the DRM render node?
//!
//! `AmdDevice::open()` already verifies the kernel driver is `amdgpu`,
//! so a successful open proves both DRM access and driver identity.
//!
//! Run: `cargo test --test hw_amd_probe -- --ignored`

use coral_driver::amd::AmdDevice;

#[test]
#[ignore = "requires amdgpu hardware"]
fn amd_device_opens_successfully() {
    let device = AmdDevice::open().expect("AmdDevice::open() failed — is amdgpu loaded?");
    drop(device);
}

#[test]
#[ignore = "requires amdgpu hardware"]
fn amd_device_opens_twice() {
    let d1 = AmdDevice::open().expect("first open");
    let d2 = AmdDevice::open().expect("second open");
    drop(d1);
    drop(d2);
}
