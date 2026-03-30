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
const PM4_ACQUIRE_MEM: u32 = 0x58;

// Compute shader register offsets (dword addresses, from SI_SH_REG_OFFSET)
const COMPUTE_START_X: u32 = 0x2E04;
const COMPUTE_NUM_THREAD_X: u32 = 0x2E07;
const COMPUTE_PERFCOUNT_ENABLE: u32 = 0x2E0B;
const COMPUTE_PGM_LO: u32 = 0x2E0C;
const COMPUTE_PGM_RSRC1: u32 = 0x2E12;
const COMPUTE_PGM_RSRC2: u32 = 0x2E13;
const COMPUTE_RESOURCE_LIMITS: u32 = 0x2E15;
const COMPUTE_STATIC_THREAD_MGMT_SE0: u32 = 0x2E16;
const COMPUTE_STATIC_THREAD_MGMT_SE1: u32 = 0x2E17;
const COMPUTE_TMPRING_SIZE: u32 = 0x2E18;
const COMPUTE_STATIC_THREAD_MGMT_SE2: u32 = 0x2E19;
const COMPUTE_STATIC_THREAD_MGMT_SE3: u32 = 0x2E1A;
const COMPUTE_USER_DATA_0: u32 = 0x2E40;

// SI shader register base for SET_SH_REG
const SI_SH_REG_BASE: u32 = 0x2C00;

/// Build a PM4 command stream for a compute dispatch.
///
/// `buffer_vas` contains the GPU virtual addresses of each bound buffer.
/// These are loaded into `COMPUTE_USER_DATA` registers so the shader can
/// read them from user SGPRs (2 SGPRs per 64-bit VA).
///
/// `gfx_major`: 9=GCN5/Vega, 10=RDNA2, 11=RDNA3, 12=RDNA4.
/// Controls register encoding differences (MEM_ORDERED, WGP_MODE, cache GCR).
///
/// Uses compiler-derived `info` for workgroup size and register allocation.
/// Returns the PM4 words ready for submission via `DRM_AMDGPU_CS`.
#[must_use]
pub fn build_compute_dispatch(
    shader_va: u64,
    dims: DispatchDims,
    info: &ShaderInfo,
    buffer_vas: &[u64],
    gfx_major: u8,
) -> Vec<u32> {
    let mut pm4 = Vec::with_capacity(96);

    // ── Preamble registers (matches Mesa radeonsi / RADV GFX9 preamble) ──

    emit_set_sh_reg(&mut pm4, COMPUTE_PERFCOUNT_ENABLE, &[0]);

    // Enable all CUs on all shader engines (MI50 has 4 SEs)
    let cu_en = 0xFFFF_FFFFu32;
    emit_set_sh_reg(&mut pm4, COMPUTE_STATIC_THREAD_MGMT_SE0, &[cu_en]);
    emit_set_sh_reg(&mut pm4, COMPUTE_STATIC_THREAD_MGMT_SE1, &[cu_en]);
    emit_set_sh_reg(&mut pm4, COMPUTE_STATIC_THREAD_MGMT_SE2, &[cu_en]);
    emit_set_sh_reg(&mut pm4, COMPUTE_STATIC_THREAD_MGMT_SE3, &[cu_en]);

    emit_set_sh_reg(&mut pm4, COMPUTE_START_X, &[0, 0, 0]);

    // ── Per-dispatch shader state ──

    #[expect(
        clippy::cast_possible_truncation,
        reason = "ISA register field is 32-bit wide"
    )]
    let pgm_lo = (shader_va >> 8) as u32;
    let pgm_hi = (shader_va >> 40) as u32;
    emit_set_sh_reg(&mut pm4, COMPUTE_PGM_LO, &[pgm_lo, pgm_hi]);

    // +3 for TID save prologue (v0/v1/v2 → safe VGPRs), +2 for scratch
    let vgpr_count = (info.gpr_count + 5).max(4);
    let sgpr_count = 16_u32;
    let vgpr_gran = if info.wave_size >= 64 { 4 } else { 8 };
    let rsrc1 = compute_pgm_rsrc1(vgpr_count, sgpr_count, vgpr_gran, gfx_major);
    emit_set_sh_reg(&mut pm4, COMPUTE_PGM_RSRC1, &[rsrc1]);

    // User data layout (SGPRs):
    //   s[0..2N-1]     buffer VAs  (N buffers × 2 dwords each)
    //   s[2N..2N+2]    NTID        (workgroup_size.x/y/z)
    //   s[2N+3..2N+5]  NCTAID      (num_workgroups.x/y/z)
    //   ── hardware-appended after user_sgpr_count ──
    //   s[2N+6]        TGID_X      (workgroup_id.x)
    //   s[2N+7]        TGID_Y      (workgroup_id.y)
    //   s[2N+8]        TGID_Z      (workgroup_id.z)
    #[expect(
        clippy::cast_possible_truncation,
        reason = "buffer count limited to 5 (10 + 6 system = 16 user SGPRs max)"
    )]
    let user_sgpr_count = (buffer_vas.len() as u32) * 2 + 6;

    {
        let mut user_data = Vec::with_capacity(user_sgpr_count as usize);
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
        user_data.push(info.workgroup[0]);
        user_data.push(info.workgroup[1]);
        user_data.push(info.workgroup[2]);
        user_data.push(dims.x);
        user_data.push(dims.y);
        user_data.push(dims.z);
        emit_set_sh_reg(&mut pm4, COMPUTE_USER_DATA_0, &user_data);
    }

    let rsrc2 = compute_pgm_rsrc2(user_sgpr_count);
    emit_set_sh_reg(&mut pm4, COMPUTE_PGM_RSRC2, &[rsrc2]);

    // GFX9: WAVES_PER_SH must be nonzero (Mesa: "set the limit to max
    // instead of 0 to fix high priority compute"). MI50 has 60 CUs × 4
    // SIMDs × 10 waves/SIMD = 2400 max waves, ~600 per shader engine.
    let resource_limits = compute_resource_limits(info);
    emit_set_sh_reg(&mut pm4, COMPUTE_RESOURCE_LIMITS, &[resource_limits]);

    emit_set_sh_reg(&mut pm4, COMPUTE_TMPRING_SIZE, &[0]);

    emit_set_sh_reg(&mut pm4, COMPUTE_NUM_THREAD_X, &info.workgroup);

    emit_cache_invalidate(&mut pm4, gfx_major);

    emit_dispatch_direct(&mut pm4, dims, info.wave_size);

    emit_acquire_mem(&mut pm4, gfx_major);

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
fn emit_dispatch_direct(pm4: &mut Vec<u32>, dims: DispatchDims, wave_size: u32) {
    let header = pm4_type3_header(PM4_DISPATCH_DIRECT, 4);
    pm4.push(header);
    pm4.push(dims.x);
    pm4.push(dims.y);
    pm4.push(dims.z);
    // DISPATCH_INITIATOR: COMPUTE_SHADER_EN=1 | FORCE_START_AT_000=4 | ORDER_MODE=16
    // CS_W32_EN (bit 15) only for RDNA wave32; GCN5 wave64 must leave it clear.
    let mut initiator = 1 | 4 | 16;
    if wave_size <= 32 {
        initiator |= 1 << 15;
    }
    pm4.push(initiator);
}

/// Emit a PM4 `ACQUIRE_MEM` packet to invalidate caches before dispatch.
///
/// Before compute dispatch, CPU-uploaded data may not be visible to the GPU
/// because stale entries in L1/L2 shadow the new content. This packet
/// invalidates both cache levels so GLOBAL_LOAD reads fresh data.
///
/// GFX9:  6-dword body; cache control in CP_COHER_CNTL (body\[0\]).
/// GFX10+: 7-dword body; CP_COHER_CNTL unused, GCR_CNTL in body\[6\].
fn emit_cache_invalidate(pm4: &mut Vec<u32>, gfx_major: u8) {
    if gfx_major >= 10 {
        let header = pm4_type3_header(PM4_ACQUIRE_MEM, 7);
        pm4.push(header);
        pm4.push(0);            // CP_COHER_CNTL (unused on GFX10+)
        pm4.push(0xFFFF_FFFF);  // COHER_SIZE
        pm4.push(0x0000_00FF);  // COHER_SIZE_HI
        pm4.push(0);            // COHER_BASE_LO
        pm4.push(0);            // COHER_BASE_HI
        pm4.push(0);            // reserved
        // GCR_CNTL (PM4 ACQUIRE_MEM dword 6):
        //   GL2_INV [14] | GL1_INV [9] | GLV_INV [8] | GLK_INV [7] | GLM_INV [5]
        pm4.push((1 << 14) | (1 << 9) | (1 << 8) | (1 << 7) | (1 << 5));
    } else {
        let header = pm4_type3_header(PM4_ACQUIRE_MEM, 6);
        pm4.push(header);
        // GFX9 CP_COHER_CNTL: TC_ACTION_ENA [23] | TCL1_ACTION_ENA [25]
        pm4.push((1 << 23) | (1 << 25));
        pm4.push(0xFFFF_FFFF);  // COHER_SIZE
        pm4.push(0x0000_00FF);  // COHER_SIZE_HI
        pm4.push(0);            // COHER_BASE_LO
        pm4.push(0);            // COHER_BASE_HI
        pm4.push(10);           // POLL_INTERVAL
    }
}

/// Emit a PM4 `ACQUIRE_MEM` packet to flush the GPU L2 cache.
///
/// After compute dispatch, GLOBAL stores may reside in L2. This packet
/// forces L2 writeback so subsequent CPU reads see the correct data.
///
/// GFX9:  6-dword body; TC_WB_ACTION_ENA / TC_ACTION_ENA in CP_COHER_CNTL.
/// GFX10+: 7-dword body; GL2_WB / GL2_INV / GL1_INV in GCR_CNTL (body\[6\]).
fn emit_acquire_mem(pm4: &mut Vec<u32>, gfx_major: u8) {
    if gfx_major >= 10 {
        let header = pm4_type3_header(PM4_ACQUIRE_MEM, 7);
        pm4.push(header);
        pm4.push(0);            // CP_COHER_CNTL (unused on GFX10+)
        pm4.push(0xFFFF_FFFF);  // COHER_SIZE
        pm4.push(0x0000_00FF);  // COHER_SIZE_HI
        pm4.push(0);            // COHER_BASE_LO
        pm4.push(0);            // COHER_BASE_HI
        pm4.push(0);            // reserved
        // GCR_CNTL: GL2_WB [15] | GL2_INV [14] | GL1_INV [9]
        pm4.push((1 << 15) | (1 << 14) | (1 << 9));
    } else {
        let header = pm4_type3_header(PM4_ACQUIRE_MEM, 6);
        pm4.push(header);
        // GFX9 CP_COHER_CNTL: TC_WB_ACTION_ENA [18] | TC_ACTION_ENA [23]
        pm4.push((1 << 18) | (1 << 23));
        pm4.push(0xFFFF_FFFF);  // COHER_SIZE
        pm4.push(0x0000_00FF);  // COHER_SIZE_HI
        pm4.push(0);            // COHER_BASE_LO
        pm4.push(0);            // COHER_BASE_HI
        pm4.push(10);           // POLL_INTERVAL
    }
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
///
/// `vgpr_granularity`: 4 for GCN5 wave64, 8 for RDNA wave32.
/// `gfx_major`: 9=GCN5, 10+=RDNA. Controls MEM_ORDERED/WGP_MODE/FWD_PROGRESS.
const fn compute_pgm_rsrc1(
    vgpr_count: u32,
    sgpr_count: u32,
    vgpr_granularity: u32,
    gfx_major: u8,
) -> u32 {
    let vgpr_encoded = (vgpr_count.div_ceil(vgpr_granularity)).saturating_sub(1);
    let sgpr_encoded = (sgpr_count.div_ceil(16)).saturating_sub(1);
    // FLOAT_MODE [19:12] = 0xC0 (IEEE f64 denorms enabled, matches Mesa default)
    // DX10_CLAMP [21] = 1 (clamp NaN to 0, required by Mesa/RADV)
    // IEEE_MODE  [23] = 1 (IEEE compliance for f64)
    let float_mode = 0xC0_u32;
    let mut rsrc1 = vgpr_encoded
        | (sgpr_encoded << 6)
        | (float_mode << 12)
        | (1 << 21) // DX10_CLAMP
        | (1 << 23); // IEEE_MODE

    // GFX10+ (RDNA): additional RSRC1 bits that don't exist on GFX9.
    //   [29] WGP_MODE    = 1 — use full Work Group Processor (2 CUs) for compute
    //   [30] MEM_ORDERED  = 1 — stores complete before S_ENDPGM (CRITICAL!)
    //        Without this, GLOBAL_STORE may be silently dropped when the wave retires.
    //   [31] FWD_PROGRESS = 1 — forward progress guarantee
    if gfx_major >= 10 {
        rsrc1 |= 1 << 29; // WGP_MODE
        rsrc1 |= 1 << 30; // MEM_ORDERED
        rsrc1 |= 1 << 31; // FWD_PROGRESS
    }

    rsrc1
}

/// Build `COMPUTE_RESOURCE_LIMITS` register value.
///
/// On GFX9, WAVES_PER_SH must be set to the max rather than 0
/// (Mesa: "Gfx9 should set the limit to max instead of 0 to fix
/// high priority compute").
const fn compute_resource_limits(info: &ShaderInfo) -> u32 {
    let threads_per_wg = info.workgroup[0] * info.workgroup[1] * info.workgroup[2];
    let waves_per_threadgroup = threads_per_wg.div_ceil(info.wave_size);

    // SIMD_DEST_CNTL: round-robin when waves_per_threadgroup is multiple of 4
    let simd_dest = if waves_per_threadgroup.is_multiple_of(4) {
        1_u32
    } else {
        0
    };

    // MI50 (Vega 20): 60 CUs, 4 SEs → 15 CUs/SE, 4 SIMDs/CU, 10 waves/SIMD
    // max_waves_per_sh = 15 * 4 * 10 = 600
    let max_waves_per_sh = 600_u32;

    (simd_dest << 4) | (max_waves_per_sh << 12)
}

/// Build `COMPUTE_PGM_RSRC2` register value.
///
/// `user_sgpr_count` is the number of SGPRs populated from `COMPUTE_USER_DATA`
/// (0..16). Workgroup IDs (TGID X/Y/Z) are placed by hardware starting at the
/// first SGPR after user data.
///
/// TIDIG_COMP_CNT controls how many thread ID dimensions the hardware
/// initializes in VGPRs: 0=X only (v0), 1=X+Y (v0,v1), 2=X+Y+Z (v0,v1,v2).
const fn compute_pgm_rsrc2(user_sgpr_count: u32) -> u32 {
    let user_sgpr = if user_sgpr_count > 0 {
        user_sgpr_count
    } else {
        2
    };
    let tgid_x_en = 1_u32;
    let tgid_y_en = 1_u32;
    let tgid_z_en = 1_u32;
    let tidig_comp_cnt = 2_u32; // initialize v0=TID.X, v1=TID.Y, v2=TID.Z
    (user_sgpr << 1) | (tgid_x_en << 7) | (tgid_y_en << 8) | (tgid_z_en << 9)
        | (tidig_comp_cnt << 11)
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
            wave_size: 32,
            workgroup: [64, 1, 1],
        };
        let pm4 = build_compute_dispatch(0x1_0000_0000, DispatchDims::linear(64), &info, &[], 10);
        assert!(!pm4.is_empty());
        assert!(pm4.len() >= 10);
    }

    #[test]
    fn compute_dispatch_with_buffer_vas() {
        let info = ShaderInfo {
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
            wave_size: 32,
            workgroup: [64, 1, 1],
        };
        let buf_vas = [0x1_0000_0000_u64, 0x2_0000_0000_u64];
        let pm4 = build_compute_dispatch(0x3_0000_0000, DispatchDims::linear(64), &info, &buf_vas, 10);
        assert!(!pm4.is_empty());
        assert!(pm4.len() > 14, "PM4 should contain user data packets");
    }

    #[test]
    fn pgm_rsrc1_encoding() {
        let rsrc1 = compute_pgm_rsrc1(16, 16, 8, 9);
        let vgprs = rsrc1 & 0x3F;
        let sgprs = (rsrc1 >> 6) & 0xF;
        assert_eq!(vgprs, 1); // (16/8) - 1
        assert_eq!(sgprs, 0); // (16/16) - 1
        let float_mode = (rsrc1 >> 12) & 0xFF;
        assert_eq!(float_mode, 0xC0); // f64 denorms
        assert_eq!((rsrc1 >> 21) & 1, 1); // DX10_CLAMP
        assert_eq!((rsrc1 >> 23) & 1, 1); // IEEE_MODE
    }

    #[test]
    fn pgm_rsrc1_gfx10_sets_mem_ordered() {
        let rsrc1 = compute_pgm_rsrc1(16, 16, 8, 10);
        assert_ne!(rsrc1 & (1 << 29), 0, "WGP_MODE for GFX10+");
        assert_ne!(rsrc1 & (1 << 30), 0, "MEM_ORDERED for GFX10+");
        assert_ne!(rsrc1 & (1 << 31), 0, "FWD_PROGRESS for GFX10+");
    }

    #[test]
    fn pgm_rsrc1_gfx9_no_mem_ordered() {
        let rsrc1 = compute_pgm_rsrc1(16, 16, 4, 9);
        assert_eq!(rsrc1 & (1 << 29), 0, "no WGP_MODE on GFX9");
        assert_eq!(rsrc1 & (1 << 30), 0, "no MEM_ORDERED on GFX9");
        assert_eq!(rsrc1 & (1 << 31), 0, "no FWD_PROGRESS on GFX9");
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
            wave_size: 32,
            workgroup: [1, 1, 1],
        };
        let pm4 = build_compute_dispatch(0, DispatchDims::new(1, 1, 1), &info, &[], 10);
        assert!(!pm4.is_empty());
        assert!(pm4.len() >= 8);
    }

    #[test]
    fn pm4_compute_dispatch_minimal_gpr() {
        let info = ShaderInfo {
            gpr_count: 0,
            shared_mem_bytes: 0,
            barrier_count: 0,
            wave_size: 32,
            workgroup: [32, 1, 1],
        };
        let pm4 = build_compute_dispatch(0x1000, DispatchDims::linear(32), &info, &[], 10);
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
            wave_size: 32,
            workgroup: [64, 2, 1],
        };
        let buf_vas = [0x1_0000_0000_u64, 0x2_0000_0000_u64, 0x3_0000_0000_u64];
        let pm4 =
            build_compute_dispatch(0x4_0000_0000, DispatchDims::new(128, 4, 2), &info, &buf_vas, 10);
        assert!(pm4.len() > 20);
    }

    #[test]
    fn pm4_compute_dispatch_ends_with_nop() {
        let info = ShaderInfo {
            gpr_count: 8,
            shared_mem_bytes: 0,
            barrier_count: 0,
            wave_size: 32,
            workgroup: [16, 1, 1],
        };
        let pm4 = build_compute_dispatch(0x1000, DispatchDims::linear(16), &info, &[], 10);
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
        assert_eq!((rsrc2_with_user >> 8) & 1, 1, "tgid_y_en");
        assert_eq!((rsrc2_with_user >> 9) & 1, 1, "tgid_z_en");
    }

    #[test]
    fn pm4_dispatch_direct_dims() {
        let info = ShaderInfo {
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
            wave_size: 32,
            workgroup: [32, 4, 2],
        };
        let dims = DispatchDims::new(128, 64, 8);
        let pm4 = build_compute_dispatch(0x1000, dims, &info, &[], 10);
        let dispatch_start = pm4
            .iter()
            .position(|&w| (w >> 8) & 0xFF == PM4_DISPATCH_DIRECT && w >> 30 == 3)
            .expect("DISPATCH_DIRECT packet not found");
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
            wave_size: 32,
            workgroup: [64, 1, 1],
        };
        let pm4 = build_compute_dispatch(shader_va, DispatchDims::linear(1), &info, &[], 10);
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
            wave_size: 32,
            workgroup: [1, 1, 1],
        };
        let pm4 = build_compute_dispatch(0, DispatchDims::new(1, 1, 1), &info, &[], 10);
        let last_header = pm4[pm4.len() - 2];
        assert_eq!((last_header >> 8) & 0xFF, PM4_NOP, "IB should end with NOP");
    }

    #[test]
    fn compute_pgm_rsrc1_minimum_vgpr() {
        let rsrc1 = compute_pgm_rsrc1(4, 16, 8, 10);
        let vgprs = rsrc1 & 0x3F;
        assert_eq!(vgprs, 0, "4 VGPRs encodes as 0 (ceil(4/8)-1)");
        assert_eq!((rsrc1 >> 12) & 0xFF, 0xC0); // FLOAT_MODE
    }

    #[test]
    fn compute_pgm_rsrc1_gcn5_granularity() {
        let rsrc1 = compute_pgm_rsrc1(26, 16, 4, 9);
        let vgprs = rsrc1 & 0x3F;
        assert_eq!(vgprs, 6, "26 VGPRs at granularity 4: ceil(26/4)-1 = 6");
    }

    #[test]
    fn pm4_set_sh_reg_packet_structure() {
        let info = ShaderInfo {
            gpr_count: 8,
            shared_mem_bytes: 0,
            barrier_count: 0,
            wave_size: 32,
            workgroup: [1, 1, 1],
        };
        let pm4 = build_compute_dispatch(0x1000, DispatchDims::new(1, 1, 1), &info, &[], 10);
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

    #[test]
    fn compute_resource_limits_waves_multiple_of_four_sets_simd_dest() {
        let info = ShaderInfo {
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
            wave_size: 64,
            workgroup: [256, 1, 1],
        };
        let lim = compute_resource_limits(&info);
        assert_eq!(
            (lim >> 4) & 1,
            1,
            "SIMD_DEST_CNTL when waves/threadgroup % 4 == 0"
        );
        assert_eq!((lim >> 12) & 0xFFFF, 600);
    }

    #[test]
    fn compute_resource_limits_waves_not_multiple_of_four_clears_simd_dest() {
        let info = ShaderInfo {
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
            wave_size: 64,
            workgroup: [64, 1, 1],
        };
        let lim = compute_resource_limits(&info);
        assert_eq!((lim >> 4) & 1, 0);
    }

    #[test]
    fn pm4_dispatch_initiator_wave32_sets_cs_w32_en() {
        let info = ShaderInfo {
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
            wave_size: 32,
            workgroup: [32, 1, 1],
        };
        let pm4 = build_compute_dispatch(0x1000, DispatchDims::linear(1), &info, &[], 10);
        let dispatch_start = pm4
            .iter()
            .position(|&w| (w >> 8) & 0xFF == PM4_DISPATCH_DIRECT && w >> 30 == 3)
            .expect("DISPATCH_DIRECT header");
        let initiator = pm4[dispatch_start + 4];
        assert_ne!(initiator & (1 << 15), 0, "CS_W32_EN for wave32");
    }

    #[test]
    fn pm4_dispatch_initiator_wave64_clears_cs_w32_en() {
        let info = ShaderInfo {
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
            wave_size: 64,
            workgroup: [64, 1, 1],
        };
        let pm4 = build_compute_dispatch(0x1000, DispatchDims::linear(1), &info, &[], 9);
        let dispatch_start = pm4
            .iter()
            .position(|&w| (w >> 8) & 0xFF == PM4_DISPATCH_DIRECT && w >> 30 == 3)
            .expect("DISPATCH_DIRECT header");
        let initiator = pm4[dispatch_start + 4];
        assert_eq!(
            initiator & (1 << 15),
            0,
            "GCN5 wave64 leaves CS_W32_EN clear"
        );
    }

    #[test]
    fn pm4_acquire_mem_after_dispatch_gfx9_has_tc_wb() {
        let info = ShaderInfo {
            gpr_count: 8,
            shared_mem_bytes: 0,
            barrier_count: 0,
            wave_size: 64,
            workgroup: [64, 1, 1],
        };
        let pm4 = build_compute_dispatch(0x2000, DispatchDims::linear(1), &info, &[], 9);
        let dispatch_idx = pm4
            .iter()
            .position(|&w| (w >> 8) & 0xFF == PM4_DISPATCH_DIRECT && w >> 30 == 3)
            .expect("dispatch");
        let post_dispatch = &pm4[dispatch_idx..];
        let acquire_after = post_dispatch
            .windows(2)
            .find(|w| (w[0] >> 8) & 0xFF == PM4_ACQUIRE_MEM && w[0] >> 30 == 3)
            .expect("post-dispatch ACQUIRE_MEM");
        assert_ne!(
            acquire_after[1] & (1 << 18),
            0,
            "L2 writeback (TC_WB_ACTION_ENA) after dispatch on GFX9"
        );
    }

    #[test]
    fn pm4_acquire_mem_after_dispatch_gfx10_has_gl2_wb() {
        let info = ShaderInfo {
            gpr_count: 8,
            shared_mem_bytes: 0,
            barrier_count: 0,
            wave_size: 32,
            workgroup: [32, 1, 1],
        };
        let pm4 = build_compute_dispatch(0x2000, DispatchDims::linear(1), &info, &[], 10);
        let dispatch_idx = pm4
            .iter()
            .position(|&w| (w >> 8) & 0xFF == PM4_DISPATCH_DIRECT && w >> 30 == 3)
            .expect("dispatch");
        let post_dispatch = &pm4[dispatch_idx..];
        let acquire_idx = post_dispatch
            .iter()
            .position(|&w| (w >> 8) & 0xFF == PM4_ACQUIRE_MEM && w >> 30 == 3)
            .expect("post-dispatch ACQUIRE_MEM");
        // GFX10+ ACQUIRE_MEM: 7 body dwords, CP_COHER_CNTL (body[0]) unused,
        // GCR_CNTL at body[6] = header + 7
        assert_eq!(
            post_dispatch[acquire_idx + 1],
            0,
            "CP_COHER_CNTL should be 0 on GFX10+"
        );
        let gcr_cntl = post_dispatch[acquire_idx + 7];
        assert_ne!(gcr_cntl & (1 << 15), 0, "GL2_WB [15] in GCR_CNTL");
        assert_ne!(gcr_cntl & (1 << 14), 0, "GL2_INV [14] in GCR_CNTL");
        assert_ne!(gcr_cntl & (1 << 9), 0, "GL1_INV [9] in GCR_CNTL");
    }
}
