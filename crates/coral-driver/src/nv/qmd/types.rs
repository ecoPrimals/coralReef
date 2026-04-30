// SPDX-License-Identifier: AGPL-3.0-or-later
//! Types and layout constants shared by QMD builders.

use crate::DispatchDims;

/// Driver constant buffer index — `c[7]` holds grid dims (num_workgroups).
///
/// Must match `DRIVER_CBUF_INDEX` in coral-reef `func_builtins.rs`.
pub const DRIVER_CBUF_INDEX: u32 = 7;

/// Minimum driver constants buffer size (bytes). 64 bytes satisfies
/// Turing+ CBUF alignment; first 12 bytes are `[grid_x, grid_y, grid_z]`.
pub const DRIVER_CONST_SIZE: u32 = 64;

/// QMD size in u32 words for pre-Hopper (256 bytes = 64 words).
pub const QMD_SIZE_WORDS: usize = 64;

/// QMD size in u32 words for Hopper+ / Blackwell (384 bytes = 96 words).
pub const QMD_V4_PLUS_SIZE_WORDS: usize = 96;

/// Maximum constant buffers per dispatch.
pub const MAX_CBUFS: usize = 8;

/// A constant buffer binding for the QMD.
#[derive(Debug, Clone, Copy)]
pub struct CbufBinding {
    /// CBUF slot index (0–7).
    pub index: u32,
    /// GPU virtual address of the buffer.
    pub addr: u64,
    /// Buffer size in bytes.
    pub size: u32,
}

/// Parameters for QMD construction.
///
/// All fields the compiler and driver need to pass into the QMD.
#[derive(Debug, Clone)]
pub struct QmdParams {
    /// GPU virtual address of the compiled shader binary.
    pub shader_va: u64,
    /// Dispatch grid dimensions (number of CTAs).
    pub grid: DispatchDims,
    /// Workgroup (CTA) thread dimensions.
    pub workgroup: [u32; 3],
    /// General-purpose register count (from compiler compilation info).
    pub gpr_count: u32,
    /// Shared memory size in bytes (from compiler analysis).
    pub shared_mem_bytes: u32,
    /// Barrier count used by the shader.
    pub barrier_count: u32,
    /// Per-thread local memory size in bytes (from compiler analysis).
    pub local_mem_low_bytes: u32,
    /// Constant buffer bindings (storage buffers, uniforms).
    pub cbufs: Vec<CbufBinding>,
}

impl QmdParams {
    /// Create minimal params for a simple compute dispatch.
    #[must_use]
    pub fn simple(shader_va: u64, grid: DispatchDims, gpr_count: u32) -> Self {
        Self {
            shader_va,
            grid,
            workgroup: [64, 1, 1],
            gpr_count: gpr_count.max(4),
            shared_mem_bytes: 0,
            barrier_count: 0,
            local_mem_low_bytes: 0,
            cbufs: Vec::new(),
        }
    }
}
