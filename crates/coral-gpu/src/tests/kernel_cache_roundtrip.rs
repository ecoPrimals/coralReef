// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! [`CompiledKernel`] / [`KernelCacheEntry`] cache serialization.

use bytes::Bytes;

use crate::{CompiledKernel, GpuContext, KernelCacheEntry};
use coral_reef::{GpuTarget, NvArch};

#[test]
fn from_cache_entry_ignores_entry_target_id() {
    // `from_cache_entry` does not validate `target_id`; the caller-supplied `target` wins.
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm86)).unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    let mut entry = kernel.to_cache_entry();
    entry.target_id = "nvidia:sm75".into();
    let roundtrip = CompiledKernel::from_cache_entry(&entry, GpuTarget::Nvidia(NvArch::Sm86));
    assert_eq!(roundtrip.target, GpuTarget::Nvidia(NvArch::Sm86));
    assert_eq!(entry.target_id, "nvidia:sm75");
}

#[test]
fn from_cache_entry_empty_binary_roundtrip() {
    let entry = KernelCacheEntry {
        binary: Bytes::new(),
        target_id: "nvidia:sm86".into(),
        gpr_count: 0,
        instr_count: 0,
        shared_mem_bytes: 0,
        barrier_count: 0,
        workgroup: [1, 1, 1],
        source_hash: 0,
    };
    let k = CompiledKernel::from_cache_entry(&entry, GpuTarget::Nvidia(NvArch::Sm86));
    assert!(k.binary.is_empty());
    assert_eq!(k.gpr_count, 0);
}

#[test]
fn cache_entry_roundtrip_all_fields_populated() {
    let entry = KernelCacheEntry {
        binary: Bytes::from_static(&[1, 2, 3, 4]),
        target_id: "amd:rdna2".into(),
        gpr_count: 42,
        instr_count: 100,
        shared_mem_bytes: 4096,
        barrier_count: 3,
        workgroup: [64, 2, 1],
        source_hash: 0xfeed_beef,
    };
    let k = CompiledKernel::from_cache_entry(&entry, GpuTarget::Nvidia(NvArch::Sm75));
    assert_eq!(k.binary.as_ref(), [1, 2, 3, 4]);
    assert_eq!(k.source_hash, 0xfeed_beef);
    assert_eq!(k.gpr_count, 42);
    assert_eq!(k.instr_count, 100);
    assert_eq!(k.shared_mem_bytes, 4096);
    assert_eq!(k.barrier_count, 3);
    assert_eq!(k.workgroup, [64, 2, 1]);
    assert_eq!(k.target, GpuTarget::Nvidia(NvArch::Sm75));
}
