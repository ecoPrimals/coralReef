// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! System register and move operation encoding — Mov, S2R, CS2R.

use super::{AmdOpEncoder, EncodeOp, dst_to_vgpr_index, src_to_encoding};
use crate::CompileError;
use crate::codegen::amd::encoding::Rdna2Encoder;
use crate::codegen::amd::isa;
use crate::codegen::amd::reg::AmdRegRef;
#[expect(
    clippy::wildcard_imports,
    reason = "op module re-exports are intentional for codegen"
)]
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

// ---- S2R (system register read → V_MOV_B32 from hardware VGPR) ----
//
// RDNA2 thread IDs are pre-loaded into VGPRs by the hardware dispatch:
//   v0 = thread_id_x, v1 = thread_id_y, v2 = thread_id_z
// Workgroup IDs come from SGPRs (s0/s1/s2 by convention).

impl EncodeOp<AmdOpEncoder<'_>> for OpS2R {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_vgpr = amd_sys_reg_vgpr(self.idx)?;
        Ok(Rdna2Encoder::encode_vop1(
            isa::vop1::V_MOV_B32,
            AmdRegRef::vgpr(dst_reg),
            256 + src_vgpr,
        ))
    }
}

// ---- CS2R (uniform system register read) ----

impl EncodeOp<AmdOpEncoder<'_>> for OpCS2R {
    fn encode(&self, _e: &mut AmdOpEncoder<'_>) -> Result<Vec<u32>, CompileError> {
        let dst_reg = dst_to_vgpr_index(&self.dst)?;
        let src_vgpr = amd_sys_reg_vgpr(self.idx)?;
        Ok(Rdna2Encoder::encode_vop1(
            isa::vop1::V_MOV_B32,
            AmdRegRef::vgpr(dst_reg),
            256 + src_vgpr,
        ))
    }
}

/// Map NVIDIA system register indices to AMD RDNA2 hardware VGPR locations.
///
/// RDNA2 compute dispatch pre-loads:
/// - v0 = thread_id_x, v1 = thread_id_y, v2 = thread_id_z
/// - Workgroup ID comes from SGPR user data (s0/s1/s2 by convention).
fn amd_sys_reg_vgpr(nv_idx: u8) -> Result<u16, CompileError> {
    Ok(match nv_idx {
        0x21 => 0, // SR_TID_X → v0
        0x22 => 1, // SR_TID_Y → v1
        0x23 => 2, // SR_TID_Z → v2
        0x25 => 3, // SR_CTAID_X → v3 (mapped from SGPR user data)
        0x26 => 4, // SR_CTAID_Y → v4
        0x27 => 5, // SR_CTAID_Z → v5
        0x00 => 0, // SR_LANEID → v0 (lane within wave)
        0x28 => 6,
        0x29 => 7,
        0x2A => 8,
        0x2B => 9,
        0x2C => 10,
        0x2D => 11,
        other => {
            return Err(CompileError::NotImplemented(
                format!("AMD sys reg mapping for NVIDIA SR index 0x{other:02x}").into(),
            ));
        }
    })
}
