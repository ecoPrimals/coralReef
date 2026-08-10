// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)
//! SM20 encoder bit-field packing and form encoding helpers.
//!
//! Split from `encoder.rs` for code-size compliance. These methods set
//! individual instruction fields on [`SM20Encoder`] and encode the three
//! instruction forms (A, B, imm32).

use super::encoder::*;
pub(super) use std::ops::Range;

impl SM20Encoder<'_> {
    pub(super) fn set_opcode(&mut self, unit: SM20Unit, opcode: u8) {
        self.set_field(0..3, unit as u8);
        self.set_field(58..64, opcode);
    }

    pub(super) fn set_pred_reg(&mut self, range: Range<usize>, reg: RegRef) {
        assert!(range.len() == 3);
        assert!(reg.file() == RegFile::Pred);
        assert!(reg.base_idx() <= 7);
        assert!(reg.comps() == 1);
        self.set_field(range, reg.base_idx());
    }

    pub(super) fn set_pred_src(&mut self, range: Range<usize>, src: &Src) {
        let (not, reg) = match src.reference {
            SrcRef::True => (false, true_reg()),
            SrcRef::False => (true, true_reg()),
            SrcRef::Reg(reg) => (false, reg),
            _ => crate::codegen::ice!("Not a register"),
        };
        self.set_pred_reg(range.start..(range.end - 1), reg);
        self.set_bit(range.end - 1, not ^ src.modifier.is_bnot());
    }

    pub(super) fn set_pred_dst(&mut self, range: Range<usize>, dst: &Dst) {
        let reg = match dst {
            Dst::None => true_reg(),
            Dst::Reg(reg) => *reg,
            Dst::SSA(_) => crate::codegen::ice!("Dst is not pred {dst}"),
        };
        self.set_pred_reg(range, reg);
    }

    pub(super) fn set_pred_dst2(&mut self, range1: Range<usize>, range2: Range<usize>, dst: &Dst) {
        let reg = match dst {
            Dst::None => true_reg(),
            Dst::Reg(reg) => *reg,
            Dst::SSA(_) => crate::codegen::ice!("Dst is not pred {dst}"),
        };
        assert!(reg.file() == RegFile::Pred);
        assert!(reg.comps() == 1);
        self.set_field2(range1, range2, reg.base_idx());
    }

    pub(super) fn set_pred(&mut self, pred: &Pred) {
        assert!(!pred.is_false());
        self.set_pred_reg(
            10..13,
            match pred.predicate {
                PredRef::None => true_reg(),
                PredRef::Reg(reg) => reg,
                PredRef::SSA(_) => crate::codegen::ice!("SSA values must be lowered"),
            },
        );
        self.set_bit(13, pred.inverted);
    }

    pub(super) fn set_reg(&mut self, range: Range<usize>, reg: RegRef) {
        assert!(range.len() == 6);
        assert!(reg.file() == RegFile::GPR);
        self.set_field(range, reg.base_idx());
    }

    pub(super) fn set_reg_src_ref(&mut self, range: Range<usize>, reference: &SrcRef) {
        match reference {
            SrcRef::Zero => self.set_reg(range, zero_reg()),
            SrcRef::Reg(reg) => self.set_reg(range, *reg),
            SrcRef::SSA(_) | SrcRef::True | SrcRef::False | SrcRef::Imm32(_) | SrcRef::CBuf(_) => {
                crate::codegen::ice!("Not a register")
            }
        }
    }

    pub(super) fn set_reg_src(&mut self, range: Range<usize>, src: &Src) {
        assert!(src.swizzle.is_none());
        self.set_reg_src_ref(range, &src.reference);
    }

    pub(super) fn set_dst(&mut self, range: Range<usize>, dst: &Dst) {
        let reg = match dst {
            Dst::None => zero_reg(),
            Dst::Reg(reg) => *reg,
            Dst::SSA(_) => crate::codegen::ice!("Invalid dst {dst}"),
        };
        self.set_reg(range, reg);
    }

    pub(super) fn set_carry_in(&mut self, bit: usize, src: &Src) {
        assert!(src.is_unmodified());
        match src.reference {
            SrcRef::Zero => self.set_bit(bit, false),
            SrcRef::Reg(reg) => {
                assert!(reg == RegRef::new(RegFile::Carry, 0, 1));
                self.set_bit(bit, true);
            }
            SrcRef::SSA(_) | SrcRef::True | SrcRef::False | SrcRef::Imm32(_) | SrcRef::CBuf(_) => {
                crate::codegen::ice!("Invalid carry in: {src}")
            }
        }
    }

    pub(super) fn set_carry_out(&mut self, bit: usize, dst: &Dst) {
        match dst {
            Dst::None => self.set_bit(bit, false),
            Dst::Reg(reg) => {
                assert!(*reg == RegRef::new(RegFile::Carry, 0, 1));
                self.set_bit(bit, true);
            }
            Dst::SSA(_) => crate::codegen::ice!("Invalid carry out: {dst}"),
        }
    }

    pub(super) fn set_src_imm_i20(&mut self, range: Range<usize>, sign_bit: usize, i: u32) {
        assert!(range.len() == 19);
        assert!((i & 0xfff8_0000) == 0 || (i & 0xfff8_0000) == 0xfff8_0000);
        self.set_field(range, i & 0x7_ffff);
        self.set_field(sign_bit..sign_bit + 1, (i & 0x8_0000) >> 19);
    }

    pub(super) fn set_src_imm_f20(&mut self, range: Range<usize>, sign_bit: usize, f: u32) {
        assert!(range.len() == 19);
        assert!((f & 0x0000_0fff) == 0);
        self.set_field(range, (f >> 12) & 0x7_ffff);
        self.set_field(sign_bit..sign_bit + 1, f >> 31);
    }

    pub(super) fn encode_form_a_opt_dst(
        &mut self,
        unit: SM20Unit,
        opcode: u8,
        dst: Option<&Dst>,
        src0: &Src,
        src1: &Src,
        src2: Option<&Src>,
    ) {
        self.set_opcode(unit, opcode);
        if let Some(dst) = dst {
            self.set_dst(14..20, dst);
        }
        if let AluSrc::Reg(reg0) = AluSrc::from_src(Some(src0)) {
            self.set_reg(20..26, reg0);
        } else {
            crate::codegen::ice!("Unsupported src0");
        }
        match AluSrc::from_src(Some(src1)) {
            AluSrc::None => crate::codegen::ice!("Unsupported src1"),
            AluSrc::Reg(reg1) => match AluSrc::from_src(src2) {
                AluSrc::None => self.set_reg(26..32, reg1),
                AluSrc::Reg(reg2) => {
                    self.set_reg(26..32, reg1);
                    self.set_reg(49..55, reg2);
                }
                AluSrc::Imm(_) => crate::codegen::ice!("Immediates are only allowed in src1"),
                AluSrc::CBuf(cb) => {
                    let CBuf::Binding(idx) = cb.buf else {
                        crate::codegen::ice!("Must be a bound constant buffer");
                    };
                    self.set_field(26..42, cb.offset);
                    self.set_field(42..46, idx);
                    self.set_field(46..48, 2_u8);
                    self.set_reg(49..55, reg1);
                }
            },
            AluSrc::Imm(imm32) => {
                match unit {
                    SM20Unit::Float | SM20Unit::Double => {
                        self.set_src_imm_f20(26..45, 45, imm32);
                    }
                    SM20Unit::Int | SM20Unit::Move | SM20Unit::Tex => {
                        self.set_src_imm_i20(26..45, 45, imm32);
                    }
                    _ => crate::codegen::ice!("Unknown unit for immediate: {unit}"),
                }
                self.set_field(46..48, 3_u8);
                if let Some(src2) = src2 {
                    self.set_reg_src_ref(49..55, &src2.reference);
                }
            }
            AluSrc::CBuf(cb) => {
                let CBuf::Binding(idx) = cb.buf else {
                    crate::codegen::ice!("Must be a bound constant buffer");
                };
                self.set_field(26..42, cb.offset);
                self.set_field(42..46, idx);
                self.set_field(46..48, 1_u8);
                if let Some(src2) = src2 {
                    self.set_reg_src_ref(49..55, &src2.reference);
                }
            }
        }
    }

    pub(super) fn encode_form_a(
        &mut self,
        unit: SM20Unit,
        opcode: u8,
        dst: &Dst,
        src0: &Src,
        src1: &Src,
        src2: Option<&Src>,
    ) {
        self.encode_form_a_opt_dst(unit, opcode, Some(dst), src0, src1, src2);
    }

    pub(super) fn encode_form_a_no_dst(
        &mut self,
        unit: SM20Unit,
        opcode: u8,
        src0: &Src,
        src1: &Src,
    ) {
        self.encode_form_a_opt_dst(unit, opcode, None, src0, src1, None);
    }

    pub(super) fn encode_form_a_imm32(&mut self, opcode: u8, dst: &Dst, src0: &Src, imm_src1: u32) {
        self.set_opcode(SM20Unit::Imm32, opcode);
        self.set_dst(14..20, dst);
        if let AluSrc::Reg(reg0) = AluSrc::from_src(Some(src0)) {
            self.set_reg(20..26, reg0);
        } else {
            crate::codegen::ice!("Unsupported src0");
        }
        self.set_field(26..58, imm_src1);
    }

    pub(super) fn encode_form_b(&mut self, unit: SM20Unit, opcode: u8, dst: &Dst, src: &Src) {
        self.set_opcode(unit, opcode);
        self.set_dst(14..20, dst);
        match AluSrc::from_src(Some(src)) {
            AluSrc::None => crate::codegen::ice!("src is always Some"),
            AluSrc::Reg(reg) => self.set_reg(26..32, reg),
            AluSrc::Imm(imm32) => {
                match unit {
                    SM20Unit::Float | SM20Unit::Double => {
                        self.set_src_imm_f20(26..45, 45, imm32);
                    }
                    SM20Unit::Int | SM20Unit::Move | SM20Unit::Tex => {
                        self.set_src_imm_i20(26..45, 45, imm32);
                    }
                    _ => crate::codegen::ice!("Unknown unit for immediate: {unit}"),
                }
                self.set_field(46..48, 3_u8);
            }
            AluSrc::CBuf(cb) => {
                let CBuf::Binding(idx) = cb.buf else {
                    crate::codegen::ice!("Must be a bound constant buffer");
                };
                self.set_field(26..42, cb.offset);
                self.set_field(42..46, idx);
                self.set_field(46..48, 1_u8);
            }
        }
    }

    pub(super) fn encode_form_b_imm32(&mut self, opcode: u8, dst: &Dst, imm_src: u32) {
        self.set_opcode(SM20Unit::Imm32, opcode);
        self.set_dst(14..20, dst);
        self.set_field(26..58, imm_src);
    }

    pub(super) fn set_rnd_mode(&mut self, range: Range<usize>, rnd_mode: FRndMode) {
        self.set_field(
            range,
            match rnd_mode {
                FRndMode::NearestEven => 0_u8,
                FRndMode::NegInf => 1_u8,
                FRndMode::PosInf => 2_u8,
                FRndMode::Zero => 3_u8,
            },
        );
    }

    pub(super) fn set_float_cmp_op(&mut self, range: Range<usize>, op: FloatCmpOp) {
        assert!(range.len() == 4);
        self.set_field(
            range,
            match op {
                FloatCmpOp::OrdLt => 0x01_u8,
                FloatCmpOp::OrdEq => 0x02_u8,
                FloatCmpOp::OrdLe => 0x03_u8,
                FloatCmpOp::OrdGt => 0x04_u8,
                FloatCmpOp::OrdNe => 0x05_u8,
                FloatCmpOp::OrdGe => 0x06_u8,
                FloatCmpOp::UnordLt => 0x09_u8,
                FloatCmpOp::UnordEq => 0x0a_u8,
                FloatCmpOp::UnordLe => 0x0b_u8,
                FloatCmpOp::UnordGt => 0x0c_u8,
                FloatCmpOp::UnordNe => 0x0d_u8,
                FloatCmpOp::UnordGe => 0x0e_u8,
                FloatCmpOp::IsNum => 0x07_u8,
                FloatCmpOp::IsNan => 0x08_u8,
            },
        );
    }

    pub(super) fn set_pred_set_op(&mut self, range: Range<usize>, op: PredSetOp) {
        assert!(range.len() == 2);
        self.set_field(
            range,
            match op {
                PredSetOp::And => 0_u8,
                PredSetOp::Or => 1_u8,
                PredSetOp::Xor => 2_u8,
            },
        );
    }

    pub(super) fn set_int_cmp_op(&mut self, range: Range<usize>, op: IntCmpOp) {
        assert!(range.len() == 3);
        self.set_field(
            range,
            match op {
                IntCmpOp::False => 0_u8,
                IntCmpOp::True => 7_u8,
                IntCmpOp::Eq => 2_u8,
                IntCmpOp::Ne => 5_u8,
                IntCmpOp::Lt => 1_u8,
                IntCmpOp::Le => 3_u8,
                IntCmpOp::Gt => 4_u8,
                IntCmpOp::Ge => 6_u8,
            },
        );
    }

    pub(super) fn set_tex_dim(&mut self, range: Range<usize>, dim: TexDim) {
        assert!(range.len() == 3);
        self.set_field(
            range,
            match dim {
                TexDim::_1D => 0_u8,
                TexDim::Array1D => 1_u8,
                TexDim::_2D => 2_u8,
                TexDim::Array2D => 3_u8,
                TexDim::_3D => 4_u8,
                TexDim::Cube => 6_u8,
                TexDim::ArrayCube => 7_u8,
            },
        );
    }

    pub(super) fn set_tex_lod_mode(&mut self, range: Range<usize>, lod_mode: TexLodMode) {
        assert!(range.len() == 2);
        self.set_field(
            range,
            match lod_mode {
                TexLodMode::Auto => 0_u8,
                TexLodMode::Zero => 1_u8,
                TexLodMode::Bias => 2_u8,
                TexLodMode::Lod => 3_u8,
                _ => crate::codegen::ice!("Unknown LOD mode"),
            },
        );
    }

    pub(super) fn set_tex_ndv(&mut self, bit: usize, deriv_mode: TexDerivMode) {
        let ndv = match deriv_mode {
            TexDerivMode::Auto => false,
            TexDerivMode::NonDivergent => true,
            _ => crate::codegen::ice!("{deriv_mode} is not supported"),
        };
        self.set_bit(bit, ndv);
    }

    pub(super) fn set_tex_channel_mask(&mut self, range: Range<usize>, channel_mask: ChannelMask) {
        self.set_field(range, channel_mask.to_bits());
    }

    pub(super) fn set_mem_type(&mut self, range: Range<usize>, mem_type: MemType) {
        assert!(range.len() == 3);
        self.set_field(
            range,
            match mem_type {
                MemType::U8 => 0_u8,
                MemType::I8 => 1_u8,
                MemType::U16 => 2_u8,
                MemType::I16 => 3_u8,
                MemType::B32 => 4_u8,
                MemType::B64 => 5_u8,
                MemType::B128 => 6_u8,
            },
        );
    }

    pub(super) fn set_ld_cache_op(&mut self, range: Range<usize>, op: LdCacheOp) {
        let cache_op = match op {
            LdCacheOp::CacheAll => 0_u8,
            LdCacheOp::CacheGlobal => 1_u8,
            LdCacheOp::CacheStreaming => 2_u8,
            LdCacheOp::CacheInvalidate => 3_u8,
            LdCacheOp::CacheIncoherent => crate::codegen::ice!("Unsupported cache op: ld{op}"),
        };
        self.set_field(range, cache_op);
    }

    pub(super) fn set_st_cache_op(&mut self, range: Range<usize>, op: StCacheOp) {
        let cache_op = match op {
            StCacheOp::WriteBack => 0_u8,
            StCacheOp::CacheGlobal => 1_u8,
            StCacheOp::CacheStreaming => 2_u8,
            StCacheOp::WriteThrough => 3_u8,
        };
        self.set_field(range, cache_op);
    }

    pub(super) fn set_su_ga_offset_mode(&mut self, range: Range<usize>, off_type: SuGaOffsetMode) {
        assert!(range.len() == 2);
        self.set_field(
            range,
            match off_type {
                SuGaOffsetMode::U32 => 0_u8,
                SuGaOffsetMode::S32 => 1_u8,
                SuGaOffsetMode::U8 => 2_u8,
                SuGaOffsetMode::S8 => 3_u8,
            },
        );
    }

    pub(super) fn set_rel_offset(&mut self, range: Range<usize>, label: &Label) {
        let ip = u32::try_from(self.ip).expect("instruction pointer overflow");
        let ip = i32::try_from(ip).expect("instruction pointer overflow");
        let target_ip = *self
            .labels
            .get(label)
            .expect("label must exist in well-formed IR");
        let target_ip = u32::try_from(target_ip).expect("target instruction pointer overflow");
        let target_ip = i32::try_from(target_ip).expect("target instruction pointer overflow");
        let rel_offset = target_ip - ip - 8;
        self.set_field(range, rel_offset);
    }
}

/// Helper to legalize extended or external instructions.
pub(super) fn legalize_ext_instr(op: &mut impl SrcsAsSlice, _b: &mut LegalizeBuilder) {
    let src_types = op.src_types();
    for (i, src) in op.srcs_as_mut_slice().iter_mut().enumerate() {
        match src_types[i] {
            SrcType::SSA => assert!(src.as_ssa().is_some()),
            SrcType::GPR => assert!(src_is_reg(src, RegFile::GPR)),
            SrcType::ALU
            | SrcType::F16
            | SrcType::F16v2
            | SrcType::F32
            | SrcType::F64
            | SrcType::I32
            | SrcType::B32 => crate::codegen::ice!("ALU srcs must be legalized explicitly"),
            SrcType::Pred => assert!(src_is_reg(src, RegFile::Pred)),
            SrcType::Carry => crate::codegen::ice!("Carry values must be legalized explicitly"),
            SrcType::Bar => crate::codegen::ice!("Barrier regs are Volta+"),
        }
    }
}
