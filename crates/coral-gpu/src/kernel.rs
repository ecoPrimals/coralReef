// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

use bytes::Bytes;

use coral_reef::GpuTarget;

/// A compiled compute shader ready for dispatch.
///
/// Uses `bytes::Bytes` for the native binary to enable zero-copy sharing
/// across IPC boundaries and between threads.
#[derive(Debug, Clone)]
pub struct CompiledKernel {
    /// Native GPU binary (zero-copy shareable via `Bytes`).
    pub binary: Bytes,
    /// Source WGSL (for diagnostics).
    pub source_hash: u64,
    /// Target this was compiled for.
    pub target: GpuTarget,
    /// GPR count from the compiler (for QMD construction).
    pub gpr_count: u32,
    /// Instruction count (for diagnostics).
    pub instr_count: u32,
    /// Shared memory used by the shader (bytes, for QMD).
    pub shared_mem_bytes: u32,
    /// Barrier count used by the shader (for QMD).
    pub barrier_count: u32,
    /// Workgroup dimensions from `@workgroup_size(x, y, z)`.
    pub workgroup: [u32; 3],
    /// Wave/warp size: 32 for NVIDIA / RDNA wave32, 64 for GCN wave64.
    pub wave_size: u32,
}

/// Serializable kernel cache entry for `dispatch_binary` / cached dispatch.
///
/// Produced by [`CompiledKernel::to_cache_entry`], consumed by
/// [`CompiledKernel::from_cache_entry`]. Separates the binary from
/// metadata so that callers can cache across sessions without
/// recompilation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KernelCacheEntry {
    /// Native GPU binary (zero-copy via `Bytes`).
    pub binary: Bytes,
    /// Target identifier string (e.g. `"nvidia:sm86"`, `"amd:rdna2"`).
    pub target_id: String,
    /// GPR count.
    pub gpr_count: u32,
    /// Instruction count.
    pub instr_count: u32,
    /// Shared memory in bytes.
    pub shared_mem_bytes: u32,
    /// Barrier count.
    pub barrier_count: u32,
    /// Workgroup size `[x, y, z]`.
    pub workgroup: [u32; 3],
    /// Wave/warp size.
    pub wave_size: u32,
    /// Hash of the source WGSL.
    pub source_hash: u64,
}

impl CompiledKernel {
    /// Convert to a serializable cache entry for on-disk persistence.
    #[must_use]
    pub fn to_cache_entry(&self) -> KernelCacheEntry {
        KernelCacheEntry {
            binary: self.binary.clone(),
            target_id: format!("{}:{}", self.target.vendor(), self.target.arch_name()),
            gpr_count: self.gpr_count,
            instr_count: self.instr_count,
            shared_mem_bytes: self.shared_mem_bytes,
            barrier_count: self.barrier_count,
            workgroup: self.workgroup,
            wave_size: self.wave_size,
            source_hash: self.source_hash,
        }
    }

    /// Reconstruct from a cache entry. `target` must match the `target_id`
    /// in the entry — caller is responsible for validation.
    #[must_use]
    pub fn from_cache_entry(entry: &KernelCacheEntry, target: GpuTarget) -> Self {
        Self {
            binary: entry.binary.clone(),
            source_hash: entry.source_hash,
            target,
            gpr_count: entry.gpr_count,
            instr_count: entry.instr_count,
            shared_mem_bytes: entry.shared_mem_bytes,
            barrier_count: entry.barrier_count,
            workgroup: entry.workgroup,
            wave_size: entry.wave_size,
        }
    }
}
