// SPDX-License-Identifier: AGPL-3.0-or-later
//! AMD GPU stress tests — large buffers, concurrent dispatches, rapid cycles.
//!
//! Run: `cargo test --test hw_amd_stress -- --ignored`

use coral_driver::amd::AmdDevice;
use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};
use coral_reef::CompileOptions;
use coral_reef::gpu_arch::{AmdArch, GpuTarget};

fn open_amd() -> AmdDevice {
    AmdDevice::open().expect("AmdDevice::open() failed — is amdgpu loaded?")
}

fn compile_for_rdna2(wgsl: &str) -> coral_reef::backend::CompiledBinary {
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        fma_policy: coral_reef::FmaPolicy::Fused,
        ..Default::default()
    };
    coral_reef::compile_wgsl_full(wgsl, &opts).expect("compile")
}

const TRIVIAL_SHADER: &str = r"
@compute @workgroup_size(1)
fn main() {}
";

const WRITE_42_SHADER: &str = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    out[0] = 42u;
}
";

/// Allocate a 4 MB GTT buffer, upload and readback to verify large transfers.
#[test]
#[ignore = "requires amdgpu hardware"]
fn large_buffer_4mb_roundtrip() {
    let mut dev = open_amd();
    let size: u64 = 4 * 1024 * 1024;
    let buf = dev.alloc(size, MemoryDomain::Gtt).expect("alloc 4MB");

    #[expect(
        clippy::cast_possible_truncation,
        reason = "test payload with modular byte pattern"
    )]
    let payload: Vec<u8> = (0..size as usize).map(|i| (i % 251) as u8).collect();
    dev.upload(buf, 0, &payload).expect("upload 4MB");

    let readback = dev.readback(buf, 0, payload.len()).expect("readback 4MB");
    assert_eq!(readback, payload, "4MB roundtrip mismatch");

    dev.free(buf).expect("free");
}

/// Allocate a 64 MB VRAM buffer to test larger GPU-local allocations.
#[test]
#[ignore = "requires amdgpu hardware"]
fn large_buffer_64mb_vram_alloc() {
    let mut dev = open_amd();
    let size: u64 = 64 * 1024 * 1024;
    let buf = dev
        .alloc(size, MemoryDomain::Vram)
        .expect("alloc 64MB VRAM");
    dev.free(buf).expect("free");
}

/// Multiple sequential dispatches with sync between each.
#[test]
#[ignore = "requires amdgpu hardware"]
fn sequential_dispatches_10x() {
    let mut dev = open_amd();
    let compiled = compile_for_rdna2(WRITE_42_SHADER);

    let out_buf: BufferHandle = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 32,
        local_mem_bytes: None,
    };

    for i in 0..10 {
        dev.upload(out_buf, 0, &[0u8; 4096])
            .unwrap_or_else(|e| panic!("zero buf iter {i}: {e}"));
        dev.dispatch(&compiled.binary, &[out_buf], DispatchDims::linear(1), &info)
            .unwrap_or_else(|e| panic!("dispatch iter {i}: {e}"));
        dev.sync().unwrap_or_else(|e| panic!("sync iter {i}: {e}"));

        let readback = dev.readback(out_buf, 0, 4).expect("readback");
        let value = u32::from_le_bytes(readback[..4].try_into().unwrap());
        assert_eq!(value, 42, "iter {i}: expected 42, got {value}");
    }

    dev.free(out_buf).expect("free");
}

/// Rapid alloc/free cycle to stress GEM handle management.
#[test]
#[ignore = "requires amdgpu hardware"]
fn rapid_alloc_free_100x() {
    let mut dev = open_amd();
    for i in 0..100 {
        let buf = dev
            .alloc(4096, MemoryDomain::Gtt)
            .unwrap_or_else(|e| panic!("alloc iter {i}: {e}"));
        dev.free(buf)
            .unwrap_or_else(|e| panic!("free iter {i}: {e}"));
    }
}

/// Allocate many buffers simultaneously, then free all.
#[test]
#[ignore = "requires amdgpu hardware"]
fn many_concurrent_buffers() {
    let mut dev = open_amd();
    let count = 64;
    let mut handles = Vec::with_capacity(count);

    for _ in 0..count {
        let buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");
        handles.push(buf);
    }

    for (i, buf) in handles.into_iter().enumerate() {
        dev.free(buf)
            .unwrap_or_else(|e| panic!("free buffer {i}: {e}"));
    }
}

/// Dispatch with no buffers multiple times — stresses the PM4 path without memory bindings.
#[test]
#[ignore = "requires amdgpu hardware"]
fn dispatch_no_buffers_20x() {
    let mut dev = open_amd();
    let compiled = compile_for_rdna2(TRIVIAL_SHADER);

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 32,
        local_mem_bytes: None,
    };

    for i in 0..20 {
        dev.dispatch(&compiled.binary, &[], DispatchDims::linear(1), &info)
            .unwrap_or_else(|e| panic!("dispatch iter {i}: {e}"));
        dev.sync().unwrap_or_else(|e| panic!("sync iter {i}: {e}"));
    }
}

/// Sync without any prior dispatch — should be a no-op, not panic.
#[test]
#[ignore = "requires amdgpu hardware"]
fn sync_without_dispatch_is_noop() {
    let mut dev = open_amd();
    dev.sync().expect("sync without dispatch should succeed");
}

/// Mixed VRAM and GTT allocations interleaved with dispatches.
#[test]
#[ignore = "requires amdgpu hardware"]
fn mixed_domain_alloc_with_dispatch() {
    let mut dev = open_amd();
    let compiled = compile_for_rdna2(TRIVIAL_SHADER);

    let vram_buf = dev.alloc(4096, MemoryDomain::Vram).expect("alloc VRAM");
    let gtt_buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc GTT");

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 32,
        local_mem_bytes: None,
    };

    dev.dispatch(&compiled.binary, &[], DispatchDims::linear(1), &info)
        .expect("dispatch");
    dev.sync().expect("sync");

    dev.free(gtt_buf).expect("free GTT");
    dev.free(vram_buf).expect("free VRAM");
}
