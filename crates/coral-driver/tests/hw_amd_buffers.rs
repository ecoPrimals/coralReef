// SPDX-License-Identifier: AGPL-3.0-only
//! Layer 1 — AMD buffer operations: alloc, upload, readback, free.
//!
//! Run: `cargo test --test hw_amd_buffers -- --ignored`

use coral_driver::amd::AmdDevice;
use coral_driver::{ComputeDevice, MemoryDomain};

fn open_amd() -> AmdDevice {
    AmdDevice::open().expect("AmdDevice::open() failed — is amdgpu loaded?")
}

#[test]
#[ignore = "requires amdgpu hardware"]
fn alloc_gtt_succeeds() {
    let mut dev = open_amd();
    let buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc GTT");
    dev.free(buf).expect("free");
}

#[test]
#[ignore = "requires amdgpu hardware"]
fn alloc_vram_succeeds() {
    let mut dev = open_amd();
    let buf = dev.alloc(4096, MemoryDomain::Vram).expect("alloc VRAM");
    dev.free(buf).expect("free");
}

#[test]
#[ignore = "requires amdgpu hardware"]
fn upload_readback_roundtrip() {
    let mut dev = open_amd();
    let buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");

    let payload: Vec<u8> = (0..256).map(|i| (i & 0xFF) as u8).collect();
    dev.upload(buf, 0, &payload).expect("upload");

    let readback = dev.readback(buf, 0, 256).expect("readback");
    assert_eq!(
        readback, payload,
        "readback data does not match uploaded data"
    );

    dev.free(buf).expect("free");
}

#[test]
#[ignore = "requires amdgpu hardware"]
fn upload_readback_with_offset() {
    let mut dev = open_amd();
    let buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");

    let payload = b"coralReef sovereign GPU";
    dev.upload(buf, 128, payload).expect("upload at offset");

    let readback = dev
        .readback(buf, 128, payload.len())
        .expect("readback at offset");
    assert_eq!(&readback, payload);

    dev.free(buf).expect("free");
}

#[test]
#[ignore = "requires amdgpu hardware"]
fn alloc_multiple_free_reverse() {
    let mut dev = open_amd();
    let b1 = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc 1");
    let b2 = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc 2");
    let b3 = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc 3");

    dev.free(b3).expect("free 3");
    dev.free(b2).expect("free 2");
    dev.free(b1).expect("free 1");
}

#[test]
#[ignore = "requires amdgpu hardware"]
fn double_free_returns_error() {
    let mut dev = open_amd();
    let buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");
    dev.free(buf).expect("first free");

    let result = dev.free(buf);
    assert!(result.is_err(), "double free should return an error");
}
