// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::helpers::open_vfio;
use coral_driver::{ComputeDevice, MemoryDomain};

#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_open_and_bar0_read() {
    let _dev = open_vfio();
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_alloc_and_free() {
    let mut dev = open_vfio();
    let handle = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");
    dev.free(handle).expect("free");
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_upload_and_readback() {
    let mut dev = open_vfio();
    let handle = dev.alloc(256, MemoryDomain::Gtt).expect("alloc");
    let data: Vec<u8> = (0..256).map(|i| (i & 0xFF) as u8).collect();
    dev.upload(handle, 0, &data).expect("upload");
    let result = dev.readback(handle, 0, 256).expect("readback");
    assert_eq!(result, data);
    dev.free(handle).expect("free");
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_multiple_buffers() {
    let mut dev = open_vfio();
    let handles: Vec<_> = (0..4)
        .map(|_| dev.alloc(4096, MemoryDomain::Gtt).expect("alloc"))
        .collect();
    for h in handles {
        dev.free(h).expect("free");
    }
}
