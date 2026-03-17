// SPDX-License-Identifier: AGPL-3.0-only
//! PM4 command buffer construction for AMD RDNA2 compute dispatch.
//!
//! PM4 (Packet Manager 4) is the command packet format used by AMD GPUs.
//! Compute dispatch requires:
//! 1. `COMPUTE_PGM_LO`/`HI` — shader program base address
//! 2. `COMPUTE_PGM_RSRC1`/`2` — resource descriptors (VGPRs, SGPRs, etc.)
//! 3. `COMPUTE_NUM_THREAD_X`/`Y`/`Z` — workgroup size
//! 4. `DISPATCH_DIRECT` — launch the compute shader

use crate::{DispatchDims, ShaderInfo};

// PM4 packet types
const PM4_TYPE3: u32 = 3 << 30;

// PM4 opcodes for compute
const PM4_SET_SH_REG: u32 = 0x76;
const PM4_DISPATCH_DIRECT: u32 = 0x15;
const PM4_NOP: u32 = 0x10;

// Compute shader register offsets (RDNA2 — from SI_SH_REG_OFFSET)
const COMPUTE_PGM_LO: u32 = 0x2E0C;
const COMPUTE_PGM_RSRC1: u32 = 0x2E12;
const COMPUTE_PGM_RSRC2: u32 = 0x2E13;
const COMPUTE_RESOURCE_LIMITS: u32 = 0x2E15;
const COMPUTE_NUM_THREAD_X: u32 = 0x2E07;
const COMPUTE_TMPRING_SIZE: u32 = 0x2E18;
const COMPUTE_USER_DATA_0: u32 = 0x2E40;

// SI shader register base for SET_SH_REG
const SI_SH_REG_BASE: u32 = 0x2C00;

/// Build a PM4 command stream for a compute dispatch.
///
/// `buffer_vas` contains the GPU virtual addresses of each bound buffer.
/// These are loaded into `COMPUTE_USER_DATA` registers so the shader can
/// read them from user SGPRs (2 SGPRs per 64-bit VA).
///
/// Uses compiler-derived `info` for workgroup size and register allocation.
/// Returns the PM4 words ready for submission via `DRM_AMDGPU_CS`.
#[must_use]
pub fn build_compute_dispatch(
    shader_va: u64,
    dims: DispatchDims,
    info: &ShaderInfo,
    buffer_vas: &[u64],
) -> Vec<u32> {
    let mut pm4 = Vec::with_capacity(64);

    // SET_SH_REG: COMPUTE_PGM_LO/HI (shader address, 256-byte aligned)
    #[expect(
        clippy::cast_possible_truncation,
        reason = "ISA register field is 32-bit wide"
    )]
    let pgm_lo = (shader_va >> 8) as u32;
    let pgm_hi = (shader_va >> 40) as u32;
    emit_set_sh_reg(&mut pm4, COMPUTE_PGM_LO, &[pgm_lo, pgm_hi]);

    // RDNA2 VGPR granularity is 8, SGPR granularity is 16.
    // Ensure at least 4 VGPRs and 16 SGPRs (hardware minimum).
    let vgpr_count = info.gpr_count.max(4);
    let sgpr_count = 16_u32;
    let rsrc1 = compute_pgm_rsrc1(vgpr_count, sgpr_count);
    emit_set_sh_reg(&mut pm4, COMPUTE_PGM_RSRC1, &[rsrc1]);

    // USER DATA: pass buffer VAs to shader via user SGPRs.
    // Each 64-bit VA occupies 2 consecutive COMPUTE_USER_DATA registers.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "buffer count limited to 8 (16 user SGPRs max)"
    )]
    let user_sgpr_count = (buffer_vas.len() as u32) * 2;

    if !buffer_vas.is_empty() {
        let mut user_data = Vec::with_capacity(buffer_vas.len() * 2);
        for &va in buffer_vas {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "splitting 64-bit VA into 32-bit halves"
            )]
            {
                user_data.push(va as u32);
                user_data.push((va >> 32) as u32);
            }
        }
        emit_set_sh_reg(&mut pm4, COMPUTE_USER_DATA_0, &user_data);
    }

    let rsrc2 = compute_pgm_rsrc2(user_sgpr_count);
    emit_set_sh_reg(&mut pm4, COMPUTE_PGM_RSRC2, &[rsrc2]);

    // COMPUTE_RESOURCE_LIMITS: allow max waves, no CU restrictions
    emit_set_sh_reg(&mut pm4, COMPUTE_RESOURCE_LIMITS, &[0]);

    // No scratch ring needed for trivial shaders
    emit_set_sh_reg(&mut pm4, COMPUTE_TMPRING_SIZE, &[0]);

    // SET_SH_REG: COMPUTE_NUM_THREAD_X/Y/Z from compiler workgroup size
    emit_set_sh_reg(&mut pm4, COMPUTE_NUM_THREAD_X, &info.workgroup);

    // DISPATCH_DIRECT with COMPUTE_SHADER_EN | FORCE_START_AT_000
    emit_dispatch_direct(&mut pm4, dims);

    // Trailing NOP for IB alignment
    emit_nop(&mut pm4);

    pm4
}

/// Emit a PM4 `SET_SH_REG` packet.
fn emit_set_sh_reg(pm4: &mut Vec<u32>, reg_offset: u32, values: &[u32]) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "register values list is always small"
    )]
    let count = values.len() as u32;
    let header = pm4_type3_header(PM4_SET_SH_REG, count + 1);
    pm4.push(header);
    pm4.push(reg_offset - SI_SH_REG_BASE);
    pm4.extend_from_slice(values);
}

/// Emit a PM4 `DISPATCH_DIRECT` packet.
fn emit_dispatch_direct(pm4: &mut Vec<u32>, dims: DispatchDims) {
    let header = pm4_type3_header(PM4_DISPATCH_DIRECT, 4);
    pm4.push(header);
    pm4.push(dims.x);
    pm4.push(dims.y);
    pm4.push(dims.z);
    // DISPATCH_INITIATOR: COMPUTE_SHADER_EN | FORCE_START_AT_000 | ORDER_MODE | CS_W32_EN
    let cs_w32_en = 1_u32 << 15;
    pm4.push(1 | 4 | 16 | cs_w32_en);
}

/// Emit a PM4 NOP packet (used for IB padding).
fn emit_nop(pm4: &mut Vec<u32>) {
    let header = pm4_type3_header(PM4_NOP, 1);
    pm4.push(header);
    pm4.push(0);
}

/// Build a PM4 Type 3 packet header.
///
/// Format: [31:30]=3 (type), [29:16]=count-1, [15:8]=opcode, [7:0]=reserved
const fn pm4_type3_header(opcode: u32, count: u32) -> u32 {
    PM4_TYPE3 | (((count - 1) & 0x3FFF) << 16) | ((opcode & 0xFF) << 8)
}

/// Build `COMPUTE_PGM_RSRC1` register value.
const fn compute_pgm_rsrc1(vgpr_count: u32, sgpr_count: u32) -> u32 {
    let vgpr_encoded = (vgpr_count.div_ceil(8)).saturating_sub(1);
    let sgpr_encoded = (sgpr_count.div_ceil(16)).saturating_sub(1);
    vgpr_encoded | (sgpr_encoded << 6)
}

/// Build `COMPUTE_PGM_RSRC2` register value.
///
/// `user_sgpr_count` is the number of SGPRs populated from `COMPUTE_USER_DATA`
/// (0..16). The workgroup ID X is placed in the first SGPR after user data.
const fn compute_pgm_rsrc2(user_sgpr_count: u32) -> u32 {
    let user_sgpr = if user_sgpr_count > 0 {
        user_sgpr_count
    } else {
        2
    };
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
        let info = ShaderInfo {
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [64, 1, 1],
        };
        let pm4 = build_compute_dispatch(0x1_0000_0000, DispatchDims::linear(64), &info, &[]);
        assert!(!pm4.is_empty());
        assert!(pm4.len() >= 10);
    }

    #[test]
    fn compute_dispatch_with_buffer_vas() {
        let info = ShaderInfo {
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [64, 1, 1],
        };
        let buf_vas = [0x1_0000_0000_u64, 0x2_0000_0000_u64];
        let pm4 = build_compute_dispatch(0x3_0000_0000, DispatchDims::linear(64), &info, &buf_vas);
        assert!(!pm4.is_empty());
        assert!(pm4.len() > 14, "PM4 should contain user data packets");
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

    #[test]
    fn pm4_compute_dispatch_empty_buffer_vas() {
        let info = ShaderInfo {
            gpr_count: 4,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [1, 1, 1],
        };
        let pm4 = build_compute_dispatch(0, DispatchDims::new(1, 1, 1), &info, &[]);
        assert!(!pm4.is_empty());
        assert!(pm4.len() >= 8);
    }

    #[test]
    fn pm4_compute_dispatch_minimal_gpr() {
        let info = ShaderInfo {
            gpr_count: 0,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [32, 1, 1],
        };
        let pm4 = build_compute_dispatch(0x1000, DispatchDims::linear(32), &info, &[]);
        assert!(!pm4.is_empty());
        assert!(
            pm4.len() >= 8,
            "PM4 with gpr_count=0 should still produce valid stream"
        );
    }

    #[test]
    fn pm4_compute_dispatch_multiple_buffer_vas() {
        let info = ShaderInfo {
            gpr_count: 32,
            shared_mem_bytes: 256,
            barrier_count: 1,
            workgroup: [64, 2, 1],
        };
        let buf_vas = [0x1_0000_0000_u64, 0x2_0000_0000_u64, 0x3_0000_0000_u64];
        let pm4 =
            build_compute_dispatch(0x4_0000_0000, DispatchDims::new(128, 4, 2), &info, &buf_vas);
        assert!(pm4.len() > 20);
    }

    #[test]
    fn pm4_compute_dispatch_ends_with_nop() {
        let info = ShaderInfo {
            gpr_count: 8,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [16, 1, 1],
        };
        let pm4 = build_compute_dispatch(0x1000, DispatchDims::linear(16), &info, &[]);
        assert!(pm4.len() >= 2);
        let last_header = pm4[pm4.len() - 2];
        assert_eq!(last_header >> 30, 3, "trailing packet should be Type 3");
    }

    #[test]
    fn compute_pgm_rsrc2_encoding() {
        let rsrc2_zero = compute_pgm_rsrc2(0);
        assert_eq!(rsrc2_zero & 0x7E, 4, "zero user_sgpr uses default 2");
        let rsrc2_with_user = compute_pgm_rsrc2(4);
        assert_eq!((rsrc2_with_user >> 1) & 0x3F, 4);
        assert_eq!((rsrc2_with_user >> 7) & 1, 1, "tgid_x_en");
    }

    #[test]
    fn pm4_dispatch_direct_dims() {
        let info = ShaderInfo {
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [32, 4, 2],
        };
        let dims = DispatchDims::new(128, 64, 8);
        let pm4 = build_compute_dispatch(0x1000, dims, &info, &[]);
        // DISPATCH_DIRECT is second-to-last packet: header + 4 dwords (x,y,z,initiator)
        // NOP is last: header + 1 dword
        let dispatch_start = pm4.len() - 7;
        assert_eq!((pm4[dispatch_start] >> 8) & 0xFF, PM4_DISPATCH_DIRECT);
        assert_eq!(pm4[dispatch_start + 1], 128);
        assert_eq!(pm4[dispatch_start + 2], 64);
        assert_eq!(pm4[dispatch_start + 3], 8);
    }

    #[test]
    fn pm4_shader_address_encoding() {
        let shader_va = 0x1_2345_6789_ABCD_u64;
        let info = ShaderInfo {
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [64, 1, 1],
        };
        let pm4 = build_compute_dispatch(shader_va, DispatchDims::linear(1), &info, &[]);
        let pgm_lo_expected = (shader_va >> 8) as u32;
        let pgm_hi_expected = (shader_va >> 40) as u32;
        assert!(
            pm4.windows(3)
                .any(|w| w[1] == pgm_lo_expected && w[2] == pgm_hi_expected),
            "PGM_LO/HI values should appear in stream"
        );
    }

    #[test]
    fn pm4_nop_opcode() {
        let info = ShaderInfo {
            gpr_count: 4,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [1, 1, 1],
        };
        let pm4 = build_compute_dispatch(0, DispatchDims::new(1, 1, 1), &info, &[]);
        let nop_header = pm4[pm4.len() - 2];
        assert_eq!((nop_header >> 8) & 0xFF, PM4_NOP);
    }

    #[test]
    fn compute_pgm_rsrc1_minimum_vgpr() {
        let rsrc1 = compute_pgm_rsrc1(4, 16);
        let vgprs = rsrc1 & 0x3F;
        assert_eq!(vgprs, 0, "4 VGPRs encodes as 0 (ceil(4/8)-1)");
    }

    #[test]
    fn pm4_set_sh_reg_packet_structure() {
        let info = ShaderInfo {
            gpr_count: 8,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [1, 1, 1],
        };
        let pm4 = build_compute_dispatch(0x1000, DispatchDims::new(1, 1, 1), &info, &[]);
        // First packet: SET_SH_REG for PGM_LO/HI (header + reg_offset + 2 values)
        assert!(pm4.len() >= 4);
        let first_header = pm4[0];
        assert_eq!(first_header >> 30, 3, "Type 3 packet");
        assert_eq!((first_header >> 8) & 0xFF, PM4_SET_SH_REG);
    }

    #[test]
    fn pm4_user_data_va_split() {
        let va = 0x1234_5678_9ABC_DEF0_u64;
        let lo = va as u32;
        let hi = (va >> 32) as u32;
        assert_eq!(lo, 0x9ABC_DEF0);
        assert_eq!(hi, 0x1234_5678);
    }
}
