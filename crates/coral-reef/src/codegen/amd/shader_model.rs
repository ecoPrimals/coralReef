// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! AMD shader model — implements `ShaderModel` for RDNA2 (GFX1030).
//!
//! Maps the vendor-agnostic `ShaderModel` trait onto AMD's architecture:
//!
//! | ShaderModel concept | AMD equivalent |
//! |---------------------|---------------|
//! | sm() | GFX version (e.g. 103 for GFX1030) |
//! | GPR | VGPR |
//! | UGPR | SGPR |
//! | Pred | Exec mask bits / SCC |
//! | warp (32 threads) | wave (32 or 64 lanes) |

#[allow(
    clippy::wildcard_imports,
    reason = "op module re-exports are intentional for codegen"
)]
use super::super::ir::*;
use super::super::legalize::{LegalizeBuildHelpers, LegalizeBuilder};
use crate::CompileError;

use coral_reef_stubs::fxhash::FxHashMap;

/// AMD RDNA2 shader model — direct `ShaderModel` impl (no intermediary vtable).
///
/// Version encoding: GFX major*10 + minor, e.g. GFX1030 → 103.
/// RDNA2 supports wave32 (default for compute) and wave64.
pub struct ShaderModelRdna2 {
    gfx_version: u8,
    wave_size: u8,
}

impl ShaderModelRdna2 {
    #[must_use]
    pub fn new(gfx_version: u8) -> Self {
        Self {
            gfx_version,
            wave_size: 32,
        }
    }

    #[must_use]
    pub fn with_wave_size(mut self, wave_size: u8) -> Self {
        self.wave_size = wave_size;
        self
    }
}

impl ShaderModel for ShaderModelRdna2 {
    fn sm(&self) -> u8 {
        self.gfx_version
    }

    fn is_amd(&self) -> bool {
        true
    }

    fn reg_count(&self, file: RegFile) -> u32 {
        match file {
            RegFile::GPR => 256,
            RegFile::UGPR => 106,
            RegFile::Pred => 1,
            RegFile::UPred => 1,
            RegFile::Carry => 1,
            RegFile::Bar => 16,
            RegFile::Mem => 0,
        }
    }

    fn hw_reserved_gpr_count(&self) -> u32 {
        0
    }

    fn crs_size(&self, _max_crs_depth: u32) -> u32 {
        0
    }

    fn op_can_be_uniform(&self, _op: &Op) -> bool {
        false
    }

    fn exec_latency(&self, _op: &Op) -> u32 {
        1
    }

    fn raw_latency(&self, _write: &Op, _dst_idx: usize, _read: &Op, _src_idx: usize) -> u32 {
        5
    }

    fn war_latency(&self, _read: &Op, _src_idx: usize, _write: &Op, _dst_idx: usize) -> u32 {
        1
    }

    fn waw_latency(
        &self,
        _a: &Op,
        _a_dst_idx: usize,
        _a_has_pred: bool,
        _b: &Op,
        _b_dst_idx: usize,
    ) -> u32 {
        5
    }

    fn paw_latency(&self, _write: &Op, _dst_idx: usize) -> u32 {
        5
    }

    fn worst_latency(&self, _write: &Op, _dst_idx: usize) -> u32 {
        200
    }

    fn latency_upper_bound(&self) -> u32 {
        200
    }

    fn max_instr_delay(&self) -> u8 {
        0
    }

    fn legalize_op(&self, b: &mut LegalizeBuilder, op: &mut Op) -> Result<(), CompileError> {
        legalize_rdna2_op(b, op)
    }

    fn encode_shader(&self, s: &Shader<'_>) -> Result<Vec<u32>, CompileError> {
        encode_rdna2_shader(self, s)
    }

    fn max_warps(&self) -> u32 {
        // RDNA2 CU: up to 32 waves/SIMD × 2 SIMDs = 64 waves/CU (wave32)
        // Conservative: 32 waves per SIMD
        32
    }

    fn wave_size(&self) -> u32 {
        u32::from(self.wave_size)
    }

    fn total_reg_file(&self) -> u32 {
        // RDNA2: 1024 VGPRs per SIMD in wave32 mode (each 32-bit)
        // 2 SIMDs per CU → 2048 VGPRs total per CU
        1024 * 2
    }
}

/// AMD-specific legalization for RDNA2.
///
/// Adapts IR ops to RDNA2 constraints:
/// - At most one SGPR or constant source per VALU instruction (VOP2/VOP1)
/// - VOP3 allows up to 3 sources from any register file
/// - f64 ops always use VOP3 encoding
fn legalize_rdna2_op(b: &mut LegalizeBuilder, op: &mut Op) -> Result<(), CompileError> {
    let gpr = RegFile::GPR;
    match op {
        Op::FAdd(op) => {
            let [src0, src1] = &mut op.srcs;
            super::super::legalize::swap_srcs_if_not_reg(src0, src1, gpr);
            b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
        }
        Op::FMul(op) => {
            let [src0, src1] = &mut op.srcs;
            super::super::legalize::swap_srcs_if_not_reg(src0, src1, gpr);
            b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
        }
        Op::FFma(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::F32);
        }
        Op::DAdd(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::F64);
        }
        Op::DFma(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::F64);
        }
        Op::DMul(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::F64);
        }
        Op::IAdd3(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::ALU);
        }
        Op::IMad(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::ALU);
        }
        Op::Mov(_) | Op::Copy(_) | Op::Swap(_) | Op::ParCopy(_) => {}
        Op::Ld(_) | Op::St(_) | Op::Atom(_) => {}
        Op::Bra(_) | Op::Exit(_) | Op::Nop(_) => {}
        Op::Sel(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[1], gpr, SrcType::ALU);
            b.copy_alu_src_if_not_reg(&mut op.srcs[2], gpr, SrcType::ALU);
        }
        Op::FSetP(op) => {
            let [src0, src1, _] = &mut op.srcs;
            super::super::legalize::swap_srcs_if_not_reg(src0, src1, gpr);
            b.copy_alu_src_if_not_reg(src0, gpr, SrcType::F32);
        }
        Op::DSetP(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::F64);
        }
        Op::ISetP(op) => {
            let [src0, src1, _, _] = &mut op.srcs;
            super::super::legalize::swap_srcs_if_not_reg(src0, src1, gpr);
            b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        }
        Op::F2F(op) => {
            b.copy_alu_src_if_not_reg(&mut op.src, gpr, SrcType::F32);
        }
        Op::F2I(op) => {
            b.copy_alu_src_if_not_reg(&mut op.src, gpr, SrcType::F32);
        }
        Op::I2F(op) => {
            b.copy_alu_src_if_not_reg(&mut op.src, gpr, SrcType::ALU);
        }
        Op::I2I(op) => {
            b.copy_alu_src_if_not_reg(&mut op.src, gpr, SrcType::ALU);
        }
        Op::Lop2(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::ALU);
        }
        Op::Lop3(op) => {
            b.copy_alu_src_if_not_reg(&mut op.srcs[0], gpr, SrcType::ALU);
        }
        Op::Shl(op) => {
            b.copy_alu_src_if_not_reg(op.src_mut(), gpr, SrcType::ALU);
        }
        Op::Shr(op) => {
            b.copy_alu_src_if_not_reg(op.src_mut(), gpr, SrcType::ALU);
        }
        Op::Bar(_) | Op::S2R(_) | Op::CS2R(_) => {}
        Op::Undef(_)
        | Op::PhiSrcs(_)
        | Op::PhiDsts(_)
        | Op::Pin(_)
        | Op::Unpin(_)
        | Op::RegOut(_)
        | Op::SrcBar(_)
        | Op::Annotate(_) => {}
        _ => {}
    }
    Ok(())
}

/// Encode an AMD RDNA2 shader to instruction words.
///
/// Compute shaders have no header (unlike NVIDIA's SPH).
fn encode_rdna2_shader(_sm: &ShaderModelRdna2, s: &Shader<'_>) -> Result<Vec<u32>, CompileError> {
    if s.functions.is_empty() {
        return Err(CompileError::InvalidInput("empty shader".into()));
    }
    let func = &s.functions[0];
    let scratch_vgpr_0 = u16::from(s.info.gpr_count);
    let scratch_vgpr_1 = scratch_vgpr_0 + 1;

    let mut ip = 0_usize;
    let mut labels: FxHashMap<Label, usize> = FxHashMap::default();
    for b in &func.blocks {
        labels.insert(b.label, ip);
        for instr in &b.instrs {
            if let Op::Nop(nop) = &instr.op {
                if let Some(label) = nop.label {
                    labels.insert(label, ip);
                }
            }
            ip += estimate_instr_size(&instr.op);
        }
    }

    let mut encoded = Vec::new();
    for b in &func.blocks {
        for instr in &b.instrs {
            let words = super::super::ops::encode_amd_op(
                &instr.op,
                &instr.pred,
                &labels,
                encoded.len(),
                scratch_vgpr_0,
                scratch_vgpr_1,
            )?;
            encoded.extend_from_slice(&words);
        }
    }

    if encoded.is_empty() || !ends_with_endpgm(&encoded) {
        encoded.extend_from_slice(&super::encoding::encode_s_endpgm());
    }

    Ok(encoded)
}

fn ends_with_endpgm(words: &[u32]) -> bool {
    words.last() == Some(&0xBF81_0000)
}

fn is_inline_constant(val: u32) -> bool {
    val == 0 || (1..=64).contains(&val) || (0xFFFF_FFF0..=0xFFFF_FFFF).contains(&val)
}

fn src_needs_materialization(src: &Src) -> bool {
    matches!(&src.reference, SrcRef::Imm32(val) if !is_inline_constant(*val))
}

fn src_is_vgpr(src: &Src) -> bool {
    matches!(&src.reference, SrcRef::Reg(reg) if reg.file() == RegFile::GPR)
        || matches!(&src.reference, SrcRef::Zero)
}

fn literal_materialization_overhead(srcs: &[Src]) -> usize {
    srcs.iter()
        .filter(|s| src_needs_materialization(s))
        .take(2)
        .count()
        * 2
}

fn estimate_instr_size(op: &Op) -> usize {
    match op {
        Op::DAdd(op) => {
            2 + literal_materialization_overhead(&[op.srcs[0].clone(), op.srcs[1].clone()])
        }
        Op::DFma(op) => {
            2 + literal_materialization_overhead(&[
                op.srcs[0].clone(),
                op.srcs[1].clone(),
                op.srcs[2].clone(),
            ])
        }
        Op::DMul(op) => {
            2 + literal_materialization_overhead(&[op.srcs[0].clone(), op.srcs[1].clone()])
        }
        Op::DMnMx(op) => {
            2 + literal_materialization_overhead(&[op.srcs[0].clone(), op.srcs[1].clone()])
        }
        Op::DSetP(op) => {
            2 + literal_materialization_overhead(&[op.srcs[0].clone(), op.srcs[1].clone()])
        }
        Op::F64Sqrt(op) => 2 + literal_materialization_overhead(std::slice::from_ref(&op.src)),
        Op::F64Rcp(op) => 2 + literal_materialization_overhead(std::slice::from_ref(&op.src)),
        Op::F64Exp2(op) => 3 + literal_materialization_overhead(std::slice::from_ref(&op.src)),
        Op::F64Log2(op) => 3 + literal_materialization_overhead(std::slice::from_ref(&op.src)),
        Op::F64Sin(op) => 3 + literal_materialization_overhead(std::slice::from_ref(&op.src)),
        Op::F64Cos(op) => 3 + literal_materialization_overhead(std::slice::from_ref(&op.src)),
        Op::FAdd(op) => {
            let overhead = if src_is_vgpr(&op.srcs[0]) || src_is_vgpr(&op.srcs[1]) {
                0
            } else {
                literal_materialization_overhead(&[op.srcs[0].clone(), op.srcs[1].clone()])
            };
            1 + overhead
        }
        Op::FMul(op) => {
            let overhead = if src_is_vgpr(&op.srcs[0]) || src_is_vgpr(&op.srcs[1]) {
                0
            } else {
                literal_materialization_overhead(&[op.srcs[0].clone(), op.srcs[1].clone()])
            };
            1 + overhead
        }
        Op::FFma(op) => {
            2 + literal_materialization_overhead(&[
                op.srcs[0].clone(),
                op.srcs[1].clone(),
                op.srcs[2].clone(),
            ])
        }
        Op::FMnMx(op) => {
            let overhead = if src_is_vgpr(&op.srcs[0]) || src_is_vgpr(&op.srcs[1]) {
                0
            } else {
                literal_materialization_overhead(&[op.srcs[0].clone(), op.srcs[1].clone()])
            };
            1 + overhead
        }
        Op::Mov(_) => 1,
        Op::Bra(_) | Op::Exit(_) | Op::Nop(_) | Op::Bar(_) => 1,
        Op::Ld(_) | Op::St(_) | Op::Atom(_) => 2,
        Op::Undef(_)
        | Op::PhiSrcs(_)
        | Op::PhiDsts(_)
        | Op::Pin(_)
        | Op::Unpin(_)
        | Op::RegOut(_)
        | Op::SrcBar(_)
        | Op::Annotate(_)
        | Op::Copy(_)
        | Op::Swap(_)
        | Op::ParCopy(_) => 0,
        Op::IAdd3(op) => {
            let overhead = if src_is_vgpr(&op.srcs[0]) || src_is_vgpr(&op.srcs[1]) {
                0
            } else {
                literal_materialization_overhead(&[op.srcs[0].clone(), op.srcs[1].clone()])
            };
            1 + overhead
        }
        Op::IMad(op) => {
            2 + literal_materialization_overhead(&[
                op.srcs[0].clone(),
                op.srcs[1].clone(),
                op.srcs[2].clone(),
            ])
        }
        Op::IMnMx(op) => {
            let overhead = if src_is_vgpr(&op.srcs[0]) || src_is_vgpr(&op.srcs[1]) {
                0
            } else {
                literal_materialization_overhead(&[op.srcs[0].clone(), op.srcs[1].clone()])
            };
            1 + overhead
        }
        Op::Lop2(op) => {
            let overhead = if src_is_vgpr(&op.srcs[0]) || src_is_vgpr(&op.srcs[1]) {
                0
            } else {
                literal_materialization_overhead(&[op.srcs[0].clone(), op.srcs[1].clone()])
            };
            1 + overhead
        }
        Op::Lop3(op) => {
            2 + literal_materialization_overhead(&[
                op.srcs[0].clone(),
                op.srcs[1].clone(),
                op.srcs[2].clone(),
            ])
        }
        Op::FSetP(op) => {
            let overhead = if src_is_vgpr(&op.srcs[0]) || src_is_vgpr(&op.srcs[1]) {
                0
            } else {
                literal_materialization_overhead(&[op.srcs[0].clone(), op.srcs[1].clone()])
            };
            1 + overhead
        }
        Op::ISetP(op) => {
            let overhead = if src_is_vgpr(&op.srcs[0]) || src_is_vgpr(&op.srcs[1]) {
                0
            } else {
                literal_materialization_overhead(&[op.srcs[0].clone(), op.srcs[1].clone()])
            };
            1 + overhead
        }
        Op::Shl(op) => {
            let overhead = if src_needs_materialization(op.shift()) {
                2
            } else {
                0
            };
            1 + overhead
        }
        Op::Shr(op) => {
            let overhead = if src_needs_materialization(op.shift()) {
                2
            } else {
                0
            };
            1 + overhead
        }
        Op::Shf(op) => {
            2 + literal_materialization_overhead(&[
                op.high().clone(),
                op.low().clone(),
                op.shift().clone(),
            ])
        }
        Op::Sel(op) => {
            let overhead = if src_needs_materialization(&op.srcs[0]) {
                2
            } else {
                0
            };
            1 + overhead
        }
        Op::PopC(op) => {
            let overhead = if src_needs_materialization(&op.src) {
                2
            } else {
                0
            };
            1 + overhead
        }
        Op::Bfe(op) => {
            2 + literal_materialization_overhead(&[op.base().clone(), op.range().clone()])
        }
        Op::BMsk(op) => {
            2 + literal_materialization_overhead(&[op.pos().clone(), op.width().clone()])
        }
        Op::F2F(op) => {
            let overhead = if src_needs_materialization(&op.src) {
                2
            } else {
                0
            };
            1 + overhead
        }
        Op::F2I(op) => {
            let overhead = if src_needs_materialization(&op.src) {
                2
            } else {
                0
            };
            1 + overhead
        }
        Op::I2F(op) => {
            let overhead = if src_needs_materialization(&op.src) {
                2
            } else {
                0
            };
            1 + overhead
        }
        Op::I2I(op) => {
            let overhead = if src_needs_materialization(&op.src) {
                2
            } else {
                0
            };
            1 + overhead
        }
        _ => 1,
    }
}

// Old monolithic encode_rdna2_op and helpers have been migrated to
// codegen/ops/ modules. See ops/mod.rs for the unified dispatch.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_model_rdna2_version() {
        let sm = ShaderModelRdna2::new(103);
        assert_eq!(sm.sm(), 103);
    }

    #[test]
    fn shader_model_rdna2_reg_counts() {
        let sm = ShaderModelRdna2::new(103);
        assert_eq!(sm.reg_count(RegFile::GPR), 256);
        assert_eq!(sm.reg_count(RegFile::UGPR), 106);
    }

    #[test]
    fn shader_model_rdna2_latencies() {
        let sm = ShaderModelRdna2::new(103);
        assert_eq!(sm.exec_latency(&Op::Nop(OpNop { label: None })), 1);
        assert!(sm.latency_upper_bound() >= 200);
    }

    #[test]
    fn shader_model_rdna2_no_crs() {
        let sm = ShaderModelRdna2::new(103);
        assert_eq!(sm.crs_size(10), 0);
    }

    #[test]
    fn shader_model_rdna2_max_warps() {
        let sm = ShaderModelRdna2::new(103);
        assert_eq!(sm.max_warps(), 32);
    }

    #[test]
    fn shader_model_rdna2_wave_size() {
        let sm = ShaderModelRdna2::new(103).with_wave_size(64);
        assert_eq!(sm.wave_size, 64);
    }

    #[test]
    fn shader_model_rdna2_nvidia_compat_returns_false() {
        let sm = ShaderModelRdna2::new(103);
        assert!(!sm.is_volta());
        assert!(!sm.is_turing());
        assert!(!sm.is_ampere());
        assert!(!sm.is_ada());
    }
}
