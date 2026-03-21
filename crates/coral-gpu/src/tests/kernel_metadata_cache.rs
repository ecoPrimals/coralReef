// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! [`crate::CompiledKernel`] / [`crate::KernelCacheEntry`] metadata, serde, and cache roundtrips.

use bytes::Bytes;

use crate::{CompiledKernel, GpuContext, KernelCacheEntry};
use coral_reef::{AmdArch, GpuTarget, NvArch};

#[test]
fn compiled_kernel_metadata_from_wgsl() {
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm70)).unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(32, 2, 1) fn main() { }")
        .unwrap();
    assert_eq!(kernel.workgroup, [32, 2, 1]);
    assert!(kernel.instr_count > 0);
    assert!(!kernel.binary.is_empty());
    assert_eq!(kernel.target, GpuTarget::Nvidia(NvArch::Sm70));
}

#[test]
fn compiled_kernel_source_hash_nonzero() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    assert_ne!(kernel.source_hash, 0);
}

#[test]
fn compiled_kernel_metadata_fields_populated() {
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm70)).unwrap();
    let kernel = ctx
        .compile_wgsl(
            "@compute @workgroup_size(64) fn main() {
            var x: array<f32, 256>;
            x[0u] = 1.0;
            workgroupBarrier();
        }",
        )
        .unwrap();
    assert_eq!(kernel.workgroup, [64, 1, 1]);
    assert!(!kernel.binary.is_empty());
    // shared_mem_bytes and barrier_count may be 0 if compiler optimizes
    assert!(kernel.gpr_count >= 4 || kernel.instr_count > 0);
}

#[test]
fn compiled_kernel_debug_format() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    let debug = format!("{kernel:?}");
    assert!(debug.contains("CompiledKernel"));
    assert!(debug.contains("binary"));
}

#[test]
fn cache_entry_roundtrip_preserves_binary() {
    let kernel = CompiledKernel {
        binary: Bytes::from(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        source_hash: 0x1234_5678_9ABC_DEF0,
        target: GpuTarget::Nvidia(NvArch::Sm86),
        gpr_count: 32,
        instr_count: 100,
        shared_mem_bytes: 4096,
        barrier_count: 2,
        workgroup: [64, 1, 1],
    };
    let entry = kernel.to_cache_entry();
    assert_eq!(&entry.binary[..], &[0xDE, 0xAD, 0xBE, 0xEF]);
    assert_eq!(entry.source_hash, 0x1234_5678_9ABC_DEF0);
    assert_eq!(entry.gpr_count, 32);
    assert_eq!(entry.instr_count, 100);
    assert_eq!(entry.shared_mem_bytes, 4096);
    assert_eq!(entry.barrier_count, 2);
    assert_eq!(entry.workgroup, [64, 1, 1]);
    assert_eq!(entry.target_id, "nvidia:sm86");

    let restored = CompiledKernel::from_cache_entry(&entry, GpuTarget::Nvidia(NvArch::Sm86));
    assert_eq!(&restored.binary[..], &[0xDE, 0xAD, 0xBE, 0xEF]);
    assert_eq!(restored.source_hash, kernel.source_hash);
    assert_eq!(restored.gpr_count, kernel.gpr_count);
}

#[test]
fn cache_entry_serde_roundtrip() {
    let entry = KernelCacheEntry {
        binary: Bytes::from(vec![1, 2, 3, 4, 5]),
        target_id: "amd:rdna2".to_string(),
        gpr_count: 16,
        instr_count: 50,
        shared_mem_bytes: 0,
        barrier_count: 0,
        workgroup: [32, 2, 1],
        source_hash: 42,
    };
    let json = serde_json::to_string(&entry).expect("serialize");
    let deserialized: KernelCacheEntry = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(&deserialized.binary[..], &[1, 2, 3, 4, 5]);
    assert_eq!(deserialized.target_id, "amd:rdna2");
    assert_eq!(deserialized.workgroup, [32, 2, 1]);
}

#[test]
fn cache_entry_zero_copy_clone() {
    let entry = KernelCacheEntry {
        binary: Bytes::from(vec![0xFF; 1024]),
        target_id: "nvidia:sm70".to_string(),
        gpr_count: 8,
        instr_count: 10,
        shared_mem_bytes: 256,
        barrier_count: 1,
        workgroup: [1, 1, 1],
        source_hash: 99,
    };
    let cloned = entry.clone();
    assert_eq!(entry.binary.as_ptr(), cloned.binary.as_ptr());
}

#[test]
fn kernel_cache_entry_debug_format() {
    let entry = KernelCacheEntry {
        binary: Bytes::from_static(b"x"),
        target_id: "nvidia:sm86".to_string(),
        gpr_count: 1,
        instr_count: 2,
        shared_mem_bytes: 0,
        barrier_count: 0,
        workgroup: [1, 1, 1],
        source_hash: 3,
    };
    let s = format!("{entry:?}");
    assert!(s.contains("KernelCacheEntry"));
    assert!(s.contains("sm86"));
}

#[test]
fn cache_entry_target_id_amd_rdna3() {
    let kernel = CompiledKernel {
        binary: Bytes::from_static(b"\0"),
        source_hash: 1,
        target: GpuTarget::Amd(AmdArch::Rdna3),
        gpr_count: 4,
        instr_count: 10,
        shared_mem_bytes: 0,
        barrier_count: 0,
        workgroup: [16, 1, 1],
    };
    assert_eq!(kernel.to_cache_entry().target_id, "amd:rdna3");
}

#[test]
fn cache_entry_target_id_nv_sm_variants() {
    for (target, id) in [
        (GpuTarget::Nvidia(NvArch::Sm70), "nvidia:sm70"),
        (GpuTarget::Nvidia(NvArch::Sm75), "nvidia:sm75"),
        (GpuTarget::Nvidia(NvArch::Sm80), "nvidia:sm80"),
        (GpuTarget::Nvidia(NvArch::Sm86), "nvidia:sm86"),
        (GpuTarget::Nvidia(NvArch::Sm89), "nvidia:sm89"),
    ] {
        let kernel = CompiledKernel {
            binary: Bytes::from_static(b"\0"),
            source_hash: 0,
            target,
            gpr_count: 1,
            instr_count: 1,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [1, 1, 1],
        };
        assert_eq!(kernel.to_cache_entry().target_id, id);
    }
}

#[test]
fn cache_entry_target_id_amd_rdna4() {
    let kernel = CompiledKernel {
        binary: Bytes::from_static(b"\0"),
        source_hash: 0,
        target: GpuTarget::Amd(AmdArch::Rdna4),
        gpr_count: 1,
        instr_count: 1,
        shared_mem_bytes: 0,
        barrier_count: 0,
        workgroup: [1, 1, 1],
    };
    assert_eq!(kernel.to_cache_entry().target_id, "amd:rdna4");
}
