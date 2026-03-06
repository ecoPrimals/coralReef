// SPDX-License-Identifier: AGPL-3.0-only
//! PM4 command buffer construction for AMD RDNA2 compute dispatch.
//!
//! PM4 (Packet Manager 4) is the command packet format used by AMD GPUs.
//! Compute dispatch requires:
//! 1. `COMPUTE_PGM_LO/HI` — shader program base address
//! 2. `COMPUTE_PGM_RSRC1/2` — resource descriptors (VGPRs, SGPRs, etc.)
//! 3. `COMPUTE_NUM_THREAD_X/Y/Z` — workgroup size
//! 4. `DISPATCH_DIRECT` — launch the compute shader

use crate::DispatchDims;

// PM4 packet types
const PM4_TYPE3: u32 = 3 << 30;

// PM4 opcodes for compute
const PM4_SET_SH_REG: u32 = 0x76;
const PM4_DISPATCH_DIRECT: u32 = 0x15;

// Compute shader register offsets (RDNA2 — from SI_SH_REG_OFFSET)
const COMPUTE_PGM_LO: u32 = 0x2E0C;
const COMPUTE_PGM_RSRC1: u32 = 0x2E12;
const COMPUTE_PGM_RSRC2: u32 = 0x2E13;
const COMPUTE_NUM_THREAD_X: u32 = 0x2E07;

// SI shader register base for SET_SH_REG
const SI_SH_REG_BASE: u32 = 0x2C00;

/// Build a PM4 command stream for a compute dispatch.
///
/// Returns the PM4 words ready for submission via DRM_AMDGPU_CS.
pub fn build_compute_dispatch(shader_va: u64, dims: DispatchDims) -> Vec<u32> {
    let mut pm4 = Vec::with_capacity(32);

    // SET_SH_REG: COMPUTE_PGM_LO/HI (shader address, 256-byte aligned)
    let pgm_lo = (shader_va >> 8) as u32;
    let pgm_hi = (shader_va >> 40) as u32;
    emit_set_sh_reg(&mut pm4, COMPUTE_PGM_LO, &[pgm_lo, pgm_hi]);

    // SET_SH_REG: COMPUTE_PGM_RSRC1
    // VGPRS = (num_vgprs / 8) - 1, SGPRS = (num_sgprs / 16) - 1
    // Default: 16 VGPRs, 16 SGPRs
    let rsrc1 = compute_pgm_rsrc1(16, 16);
    emit_set_sh_reg(&mut pm4, COMPUTE_PGM_RSRC1, &[rsrc1]);

    // SET_SH_REG: COMPUTE_PGM_RSRC2
    let rsrc2 = compute_pgm_rsrc2();
    emit_set_sh_reg(&mut pm4, COMPUTE_PGM_RSRC2, &[rsrc2]);

    // SET_SH_REG: COMPUTE_NUM_THREAD_X/Y/Z (workgroup size — 1,1,1 for now)
    emit_set_sh_reg(&mut pm4, COMPUTE_NUM_THREAD_X, &[1, 1, 1]);

    // DISPATCH_DIRECT
    emit_dispatch_direct(&mut pm4, dims);

    pm4
}

/// Emit a PM4 SET_SH_REG packet.
fn emit_set_sh_reg(pm4: &mut Vec<u32>, reg_offset: u32, values: &[u32]) {
    let count = values.len() as u32;
    let header = pm4_type3_header(PM4_SET_SH_REG, count + 1);
    pm4.push(header);
    pm4.push(reg_offset - SI_SH_REG_BASE);
    pm4.extend_from_slice(values);
}

/// Emit a PM4 DISPATCH_DIRECT packet.
fn emit_dispatch_direct(pm4: &mut Vec<u32>, dims: DispatchDims) {
    let header = pm4_type3_header(PM4_DISPATCH_DIRECT, 4);
    pm4.push(header);
    pm4.push(dims.x);
    pm4.push(dims.y);
    pm4.push(dims.z);
    pm4.push(1); // initiator: ordinal dispatch
}

/// Build a PM4 Type 3 packet header.
///
/// Format: [31:30]=3 (type), [29:16]=count-1, [15:8]=opcode, [7:0]=reserved
fn pm4_type3_header(opcode: u32, count: u32) -> u32 {
    PM4_TYPE3 | (((count - 1) & 0x3FFF) << 16) | ((opcode & 0xFF) << 8)
}

/// Build COMPUTE_PGM_RSRC1 register value.
fn compute_pgm_rsrc1(num_vgprs: u32, num_sgprs: u32) -> u32 {
    let vgprs_field = (num_vgprs.div_ceil(8)).saturating_sub(1);
    let sgprs_field = (num_sgprs.div_ceil(16)).saturating_sub(1);
    vgprs_field | (sgprs_field << 6)
}

/// Build COMPUTE_PGM_RSRC2 register value.
fn compute_pgm_rsrc2() -> u32 {
    // Enable scratch, user SGPR count = 2, TGID enables for X
    let user_sgpr = 2_u32;
    let tgid_x_en = 1_u32;
    (user_sgpr << 1) | (tgid_x_en << 7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pm4_header_format() {
        let header = pm4_type3_header(PM4_SET_SH_REG, 3);
        assert_eq!(header >> 30, 3);
        assert_eq!((header >> 8) & 0xFF, PM4_SET_SH_REG);
        assert_eq!((header >> 16) & 0x3FFF, 2);
    }

    #[test]
    fn compute_dispatch_non_empty() {
        let pm4 = build_compute_dispatch(0x1_0000_0000, DispatchDims::linear(64));
        assert!(!pm4.is_empty());
        // Should contain SET_SH_REG packets and DISPATCH_DIRECT
        assert!(pm4.len() >= 10);
    }

    #[test]
    fn pgm_rsrc1_encoding() {
        let rsrc1 = compute_pgm_rsrc1(16, 16);
        let vgprs = rsrc1 & 0x3F;
        let sgprs = (rsrc1 >> 6) & 0xF;
        assert_eq!(vgprs, 1); // (16/8) - 1
        assert_eq!(sgprs, 0); // (16/16) - 1
    }

    #[test]
    fn dispatch_dims_linear() {
        let d = DispatchDims::linear(128);
        assert_eq!(d.x, 128);
        assert_eq!(d.y, 1);
        assert_eq!(d.z, 1);
    }
}
