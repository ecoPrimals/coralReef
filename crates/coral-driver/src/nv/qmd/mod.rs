// SPDX-License-Identifier: AGPL-3.0-or-later
//! QMD (Queue Management Descriptor) construction for NVIDIA compute dispatch.
//!
//! Supports multiple QMD versions:
//! - v2.1 (256-byte, 64-word): Kepler, Maxwell, Pascal (SM35-SM62)
//! - v2.2 (256-byte, 64-word): Volta, Turing (SM70-SM79)
//! - v2.3 (256-byte, 64-word): Ampere (SM80-SM88) — confirmed by NVK/CUDA
//! - v3.0 (256-byte, 64-word): Ada, Hopper (SM89-SM99)
//! - v5.0 (384-byte, 96-word): Blackwell (SM100+) — required for CTS compliance
//!
//! Key differences between v2.3 and v3.0: `SHADER_LOCAL_MEMORY_LOW_SIZE`
//! is at bits 928-951 (v2.3) vs 736-759 (v3.0), and `BARRIER_COUNT` is
//! at bits 955-959 (v2.3) vs 763-767 (v3.0).
//!
//! Also provides the shared CBUF binding layout ([`build_standard_cbufs`])
//! and driver constants encoder ([`encode_driver_constants`]) used by all
//! three NVIDIA dispatch paths (UVM, VFIO, nouveau).
//!
//! Field layout derived from Mesa NVK (`nvk_compute.c`) and the NVIDIA
//! open GPU headers.

mod build;
mod field;
mod sm_config;
mod types;
mod v21_v22;
mod v23;
mod v30;
mod v50;

#[cfg(test)]
mod tests;

use crate::DispatchDims;

pub use build::{build_compute_qmd, build_qmd, build_qmd_for_sm};
pub use types::{
    CbufBinding, DRIVER_CBUF_INDEX, DRIVER_CONST_SIZE, MAX_CBUFS, QMD_SIZE_WORDS,
    QMD_V4_PLUS_SIZE_WORDS, QmdParams,
};
pub use v21_v22::{build_qmd_v21, build_qmd_v22};
pub use v23::build_qmd_v23;
pub use v30::build_qmd_v30;
pub use v50::build_qmd_v50;

/// Build the standard NVIDIA CBUF binding layout used by all dispatch paths.
///
/// Slots 0-6 mirror the descriptor table (`desc_addr` / `desc_size`).
/// Slot 7 binds driver constants at `driver_const_addr` / `driver_const_size`.
///
/// # Descriptor table layout
///
/// The compiler (coral-reef `naga_translate`) emits `c[group][binding * 16]`
/// to load 64-bit buffer addresses on NVIDIA, yielding a **16-byte stride**
/// per binding:
///
/// ```text
/// offset  0: [va_lo₀, va_hi₀, size₀, pad₀]
/// offset 16: [va_lo₁, va_hi₁, size₁, pad₁]
/// offset 32: [va_lo₂, va_hi₂, size₂, pad₂]
/// ```
///
/// `arrayLength()` reads `c[group][binding * 16 + 8]`, which at 16-byte
/// stride correctly yields the size dword for each binding without aliasing
/// the next binding's address.
///
/// AMD uses an 8-byte stride instead (buffer VAs are passed through SGPRs,
/// not CBUFs), so the stride is target-dependent in the compiler.
///
/// Returns a `Vec<CbufBinding>` with exactly 8 entries.
#[must_use]
pub fn build_standard_cbufs(
    desc_addr: u64,
    desc_size: u32,
    driver_const_addr: u64,
    driver_const_size: u32,
) -> Vec<CbufBinding> {
    let mut cbufs: Vec<CbufBinding> = (0..7)
        .map(|i| CbufBinding {
            index: i,
            addr: desc_addr,
            size: desc_size,
        })
        .collect();
    cbufs.push(CbufBinding {
        index: DRIVER_CBUF_INDEX,
        addr: driver_const_addr,
        size: driver_const_size,
    });
    cbufs
}

/// Encode driver constants (grid dimensions) into a byte buffer.
///
/// Writes `[grid_x, grid_y, grid_z, 0]` as little-endian u32s.
/// Returns a fixed-size array suitable for upload as CBUF 7.
#[must_use]
pub fn encode_driver_constants(dims: &DispatchDims) -> [u8; DRIVER_CONST_SIZE as usize] {
    let mut buf = [0u8; DRIVER_CONST_SIZE as usize];
    buf[0..4].copy_from_slice(&dims.x.to_le_bytes());
    buf[4..8].copy_from_slice(&dims.y.to_le_bytes());
    buf[8..12].copy_from_slice(&dims.z.to_le_bytes());
    buf
}
