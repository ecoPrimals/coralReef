// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! System register and move operation encoding — Mov, S2R, CS2R.

use super::{AmdOpEncoder, EncodeOp, dst_to_vgpr_index, src_to_encoding};
use crate::CompileError;
use crate::codegen::amd::encoding::Rdna2Encoder;
use crate::codegen::amd::isa;
use crate::codegen::amd::reg::AmdRegRef;
use crate::codegen::ir::*;

// ---- Mov (VOP1: V_MOV_B32) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpMov {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = src_to_encoding(&self.src)?;
        let mut words =
            Rdna2Encoder::encode_vop1(isa::vop1::V_MOV_B32, AmdRegRef::vgpr(dst_reg), src_enc.src0);
        src_enc.extend_with_literal(&mut words);
        Ok(words)
    }
}

// ---- S2R (system register read) ----
//
// AMD compute dispatch preloads:
//   VGPRs: v0 = thread_id_x, v1 = thread_id_y, v2 = thread_id_z
//   SGPRs: s[0..N-1] = user data (buffer VAs), s[N..] = workgroup IDs
//
// Thread IDs are per-lane (VGPR), workgroup IDs are uniform (SGPR).

impl EncodeOp<AmdOpEncoder<'_>> for OpS2R {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = amd_sys_reg_src(self.idx, e.user_sgpr_count)?;
        Ok(Rdna2Encoder::encode_vop1(
            isa::vop1::V_MOV_B32,
            AmdRegRef::vgpr(dst_reg),
            src_enc,
        ))
    }
}

// ---- CS2R (uniform system register read) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpCS2R {
    fn encode(&self, e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_enc = amd_sys_reg_src(self.idx, e.user_sgpr_count)?;
        Ok(Rdna2Encoder::encode_vop1(
            isa::vop1::V_MOV_B32,
            AmdRegRef::vgpr(dst_reg),
            src_enc,
        ))
    }
}

/// Map NVIDIA system register indices to AMD SRC encoding values.
///
/// Thread IDs come from VGPRs (hardware preload):
///   v0 = thread_id_x, v1 = thread_id_y, v2 = thread_id_z → SRC 256+N
///
/// Workgroup IDs come from SGPRs after user data:
///   s[tgid_base+0] = workgroup_id_x, +1 = _y, +2 = _z → SRC = SGPR index
///
/// The `tgid_sgpr_base` is the SGPR index where the first workgroup ID
/// is placed by the hardware (= number of user data SGPRs).
fn amd_sys_reg_src(nv_idx: u8, tgid_sgpr_base: u16) -> Result<u16, CompileError> {
    Ok(match nv_idx {
        0x21 => 256,                // SR_TID_X → v0
        0x22 => 256 + 1,            // SR_TID_Y → v1
        0x23 => 256 + 2,            // SR_TID_Z → v2
        0x25 => tgid_sgpr_base,     // SR_CTAID_X → s[base]
        0x26 => tgid_sgpr_base + 1, // SR_CTAID_Y → s[base+1]
        0x27 => tgid_sgpr_base + 2, // SR_CTAID_Z → s[base+2]
        0x00 => 256,                // SR_LANEID → v0

        // NTID (workgroup_size) and NCTAID (num_workgroups) are passed
        // via additional user data SGPRs starting after workgroup IDs.
        // The PM4 builder must emit matching COMPUTE_USER_DATA values.
        0x29 => tgid_sgpr_base + 3, // SR_NTID_X → s[base+3]
        0x2a => tgid_sgpr_base + 4, // SR_NTID_Y → s[base+4]
        0x2b => tgid_sgpr_base + 5, // SR_NTID_Z → s[base+5]
        0x2d => tgid_sgpr_base + 6, // SR_NCTAID_X → s[base+6]
        0x2e => tgid_sgpr_base + 7, // SR_NCTAID_Y → s[base+7]
        0x2f => tgid_sgpr_base + 8, // SR_NCTAID_Z → s[base+8]
        other => {
            return Err(CompileError::NotImplemented(
                format!("AMD sys reg mapping for NVIDIA SR index 0x{other:02x}").into(),
            ));
        }
    })
}
