// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

#![allow(clippy::wildcard_imports)]

pub(super) use super::super::sm70::ShaderModel70;
pub(super) use crate::codegen::ir::*;
pub(super) use crate::codegen::legalize::{
    LegalizeBuildHelpers, LegalizeBuilder, src_is_reg, src_is_upred_reg, swap_srcs_if_not_reg,
};
pub(super) use bitview::*;

use coral_reef_stubs::fxhash::FxHashMap;
pub(super) use std::ops::Range;

pub(super) trait SM70Op {
    fn legalize(&mut self, b: &mut LegalizeBuilder);
    fn encode(&self, e: &mut SM70Encoder<'_>);
}

pub(super) struct SM70Encoder<'a> {
    pub(super) sm: u8,
    pub(super) ip: usize,
    pub(super) labels: &'a FxHashMap<Label, usize>,
    pub(super) inst: [u32; 4],
}

impl BitViewable for SM70Encoder<'_> {
    fn bits(&self) -> usize {
        self.inst.bits()
    }

    fn get_bit_range_u64(&self, range: Range<usize>) -> u64 {
        self.inst.get_bit_range_u64(range)
    }
}

impl BitMutViewable for SM70Encoder<'_> {
    fn set_bit_range_u64(&mut self, range: Range<usize>, val: u64) {
        self.inst.set_bit_range_u64(range, val);
    }
}

impl SM70Encoder<'_> {
    /// Maximum encodable UGPR
    ///
    /// This may be different from the actual maximum supported by hardware.
    pub(super) fn ugpr_max(&self) -> u32 {
        if self.sm >= 100 { 255 } else { 63 }
    }

    pub(super) fn zero_reg(&self, file: RegFile) -> RegRef {
        let nr = match file {
            RegFile::GPR => 255,
            RegFile::UGPR => self.ugpr_max(),
            _ => panic!("Not a GPR"),
        };
        RegRef::new(file, nr, 1)
    }

    pub(super) fn true_reg(&self, file: RegFile) -> RegRef {
        RegRef::new(file, 7, 1)
    }

    pub(super) fn set_opcode(&mut self, opcode: u16) {
        self.set_field(0..12, opcode);
    }

    pub(super) fn set_reg(&mut self, range: Range<usize>, reg: RegRef) {
        assert!(range.len() == 8);
        assert!(reg.file() == RegFile::GPR);
        self.set_field(range, reg.base_idx());
    }

    pub(super) fn set_ureg(&mut self, range: Range<usize>, reg: RegRef) {
        assert!(self.sm >= 73);
        assert!(range.len() == 8);
        assert!(reg.file() == RegFile::UGPR);
        assert!(reg.base_idx() <= self.ugpr_max());
        self.set_field(range, reg.base_idx());
    }

    pub(super) fn set_pred_reg(&mut self, range: Range<usize>, reg: RegRef) {
        assert!(range.len() == 3);
        assert!(reg.base_idx() <= 7);
        assert!(reg.comps() == 1);
        self.set_field(range, reg.base_idx());
    }

    pub(super) fn set_reg_src(&mut self, range: Range<usize>, src: &Src) {
        assert!(src.is_unmodified());
        match src.reference {
            SrcRef::Zero => self.set_reg(range, self.zero_reg(RegFile::GPR)),
            SrcRef::Reg(reg) => self.set_reg(range, reg),
            SrcRef::SSA(_) | SrcRef::True | SrcRef::False | SrcRef::Imm32(_) | SrcRef::CBuf(_) => {
                panic!("Not a register")
            }
        }
    }

    pub(super) fn set_ureg_src(&mut self, range: Range<usize>, src: &Src) {
        assert!(src.modifier.is_none());
        match src.reference {
            SrcRef::Zero => self.set_ureg(range, self.zero_reg(RegFile::UGPR)),
            SrcRef::Reg(reg) => self.set_ureg(range, reg),
            SrcRef::SSA(_) | SrcRef::True | SrcRef::False | SrcRef::Imm32(_) | SrcRef::CBuf(_) => {
                panic!("Not a register")
            }
        }
    }

    pub(super) fn set_pred_dst(&mut self, range: Range<usize>, dst: &Dst) {
        match dst {
            Dst::None => self.set_pred_reg(range, self.true_reg(RegFile::Pred)),
            Dst::Reg(reg) => self.set_pred_reg(range, *reg),
            Dst::SSA(_) => panic!("Not a register"),
        }
    }

    pub(super) fn set_pred_src_file(
        &mut self,
        range: Range<usize>,
        not_bit: usize,
        src: &Src,
        file: RegFile,
    ) {
        let (not, reg) = match src.reference {
            SrcRef::True => (false, self.true_reg(file)),
            SrcRef::False => (true, self.true_reg(file)),
            SrcRef::Reg(reg) => {
                assert!(reg.file() == file);
                (false, reg)
            }
            _ => panic!("Not a register"),
        };
        self.set_pred_reg(range, reg);
        self.set_bit(not_bit, not ^ src_mod_is_bnot(src.modifier));
    }

    pub(super) fn set_pred_src(&mut self, range: Range<usize>, not_bit: usize, src: &Src) {
        self.set_pred_src_file(range, not_bit, src, RegFile::Pred);
    }

    pub(super) fn set_upred_src(&mut self, range: Range<usize>, not_bit: usize, src: &Src) {
        self.set_pred_src_file(range, not_bit, src, RegFile::UPred);
    }

    pub(super) fn set_rev_upred_src(&mut self, range: Range<usize>, not_bit: usize, src: &Src) {
        let file = RegFile::UPred;
        let (not, reg) = match src.reference {
            SrcRef::True => (false, self.true_reg(file)),
            SrcRef::False => (true, self.true_reg(file)),
            SrcRef::Reg(reg) => {
                assert!(reg.file() == file);
                (false, reg)
            }
            _ => panic!("Not a register"),
        };

        assert!(range.len() == 3);
        assert!(reg.base_idx() <= 7);
        assert!(reg.comps() == 1);

        // These sources are funky.  They're encoded backwards.
        self.set_field(range, 7 - reg.base_idx());

        self.set_bit(not_bit, not ^ src_mod_is_bnot(src.modifier));
    }

    pub(super) fn set_src_cb(&mut self, range: Range<usize>, cx_bit: usize, cb: &CBufRef) {
        let mut v = new_subset(&mut self.inst[..], range.start, range.len());
        v.set_field(6..22, cb.offset);
        match cb.buf {
            CBuf::Binding(idx) => {
                v.set_field(22..27, idx);
                self.set_bit(cx_bit, false);
            }
            CBuf::BindlessUGPR(reg) => {
                assert!(reg.base_idx() <= 63);
                assert!(reg.file() == RegFile::UGPR);
                v.set_field(0..6, reg.base_idx());
                self.set_bit(cx_bit, true);
            }
            CBuf::BindlessSSA(_) => panic!("SSA values must be lowered"),
        }
    }

    pub(super) fn set_pred(&mut self, pred: &Pred) {
        assert!(!pred.is_false());
        self.set_pred_reg(
            12..15,
            match pred.predicate {
                PredRef::None => self.true_reg(RegFile::Pred),
                PredRef::Reg(reg) => reg,
                PredRef::SSA(_) => panic!("SSA values must be lowered"),
            },
        );
        self.set_bit(15, pred.inverted);
    }

    pub(super) fn set_dst(&mut self, dst: &Dst) {
        match dst {
            Dst::None => self.set_reg(16..24, self.zero_reg(RegFile::GPR)),
            Dst::Reg(reg) => self.set_reg(16..24, *reg),
            Dst::SSA(_) => panic!("Not a register"),
        }
    }

    pub(super) fn set_udst(&mut self, dst: &Dst) {
        match dst {
            Dst::None => self.set_ureg(16..24, self.zero_reg(RegFile::UGPR)),
            Dst::Reg(reg) => self.set_ureg(16..24, *reg),
            Dst::SSA(_) => panic!("Not a register"),
        }
    }

    pub(super) fn set_bar_reg(&mut self, range: Range<usize>, reg: RegRef) {
        assert!(range.len() == 4);
        assert!(reg.file() == RegFile::Bar);
        assert!(reg.comps() == 1);
        self.set_field(range, reg.base_idx());
    }

    pub(super) fn set_bar_dst(&mut self, range: Range<usize>, dst: &Dst) {
        self.set_bar_reg(range, *dst.as_reg().unwrap());
    }

    pub(super) fn set_bar_src(&mut self, range: Range<usize>, src: &Src) {
        assert!(src.is_unmodified());
        self.set_bar_reg(range, *src.reference.as_reg().unwrap());
    }

    pub(super) fn set_instr_deps(&mut self, deps: &InstrDeps) {
        self.set_field(105..109, deps.delay);
        self.set_field(110..113, deps.wr_bar().unwrap_or(7));
        self.set_field(113..116, deps.rd_bar().unwrap_or(7));
        self.set_field(116..122, deps.wt_bar_mask);
        if self.sm < 120 {
            self.set_bit(109, deps.yld);
            self.set_field(122..126, deps.reuse_mask);
        }
    }
}

//
// Helpers for encoding of ALU instructions
//

pub(super) struct ALURegRef {
    pub(super) reg: RegRef,
    pub(super) abs: bool,
    pub(super) neg: bool,
    pub(super) swizzle: SrcSwizzle,
}

pub(super) struct ALUCBufRef {
    pub(super) cb: CBufRef,
    pub(super) abs: bool,
    pub(super) neg: bool,
    pub(super) swizzle: SrcSwizzle,
}

pub(super) enum ALUSrc {
    None,
    Imm32(u32),
    Reg(ALURegRef),
    UReg(ALURegRef),
    CBuf(ALUCBufRef),
}

pub(super) fn src_is_zero_or_gpr(src: &Src) -> bool {
    match src.reference {
        SrcRef::Zero => true,
        SrcRef::Reg(reg) => reg.file() == RegFile::GPR,
        _ => false,
    }
}

pub(super) fn src_mod_has_abs(modifier: SrcMod) -> bool {
    match modifier {
        SrcMod::None | SrcMod::FNeg | SrcMod::INeg | SrcMod::BNot => false,
        SrcMod::FAbs | SrcMod::FNegAbs => true,
    }
}

pub(super) fn src_mod_has_neg(modifier: SrcMod) -> bool {
    match modifier {
        SrcMod::None | SrcMod::FAbs => false,
        SrcMod::FNeg | SrcMod::FNegAbs | SrcMod::INeg | SrcMod::BNot => true,
    }
}

pub(super) fn src_mod_is_bnot(modifier: SrcMod) -> bool {
    match modifier {
        SrcMod::None => false,
        SrcMod::BNot => true,
        _ => panic!("Not an predicate source modifier"),
    }
}

pub(super) fn dst_is_bar(dst: &Dst) -> bool {
    match dst {
        Dst::None => false,
        Dst::SSA(ssa) => ssa.file() == RegFile::Bar,
        Dst::Reg(reg) => reg.file() == RegFile::Bar,
    }
}

impl ALUSrc {
    pub(super) fn from_src(e: &SM70Encoder<'_>, src: Option<&Src>, op_is_uniform: bool) -> Self {
        let Some(src) = src else {
            return Self::None;
        };

        match &src.reference {
            SrcRef::Zero | SrcRef::Reg(_) => {
                let reg = match src.reference {
                    SrcRef::Zero => {
                        let file = if op_is_uniform {
                            RegFile::UGPR
                        } else {
                            RegFile::GPR
                        };
                        e.zero_reg(file)
                    }
                    SrcRef::Reg(reg) => reg,
                    _ => panic!("Invalid source ref"),
                };
                assert!(reg.comps() <= 2);
                let alu_ref = ALURegRef {
                    reg,
                    abs: src_mod_has_abs(src.modifier),
                    neg: src_mod_has_neg(src.modifier),
                    swizzle: src.swizzle,
                };
                if op_is_uniform {
                    assert!(reg.file() == RegFile::UGPR);
                    Self::Reg(alu_ref)
                } else {
                    match reg.file() {
                        RegFile::GPR => Self::Reg(alu_ref),
                        RegFile::UGPR => Self::UReg(alu_ref),
                        _ => panic!("Invalid ALU register file"),
                    }
                }
            }
            SrcRef::Imm32(i) => {
                assert!(src.is_unmodified());
                assert!(src.swizzle.is_none());
                Self::Imm32(*i)
            }
            SrcRef::CBuf(cb) => {
                let alu_ref = ALUCBufRef {
                    cb: cb.clone(),
                    abs: src_mod_has_abs(src.modifier),
                    neg: src_mod_has_neg(src.modifier),
                    swizzle: src.swizzle,
                };
                Self::CBuf(alu_ref)
            }
            _ => panic!("Invalid ALU source"),
        }
    }
}

impl OffsetStride {
    pub(super) fn encode_sm75(&self) -> u8 {
        match self {
            Self::X1 => 0,
            Self::X4 => 1,
            Self::X8 => 2,
            Self::X16 => 3,
        }
    }
}

impl SM70Encoder<'_> {
    pub(super) fn set_swizzle(&mut self, range: Range<usize>, swizzle: SrcSwizzle) {
        assert!(range.len() == 2);

        self.set_field(
            range,
            match swizzle {
                SrcSwizzle::None => 0x00_u8,
                SrcSwizzle::Xx => 0x02_u8,
                SrcSwizzle::Yy => 0x03_u8,
            },
        );
    }

    pub(super) fn set_alu_reg(
        &mut self,
        range: Range<usize>,
        abs_bit: usize,
        neg_bit: usize,
        swizzle_range: Range<usize>,
        file: RegFile,
        is_fp16_alu: bool,
        reg: &ALURegRef,
    ) {
        match file {
            RegFile::GPR => self.set_reg(range, reg.reg),
            RegFile::UGPR => self.set_ureg(range, reg.reg),
            _ => panic!("Invalid ALU src register file"),
        }

        self.set_bit(abs_bit, reg.abs);
        self.set_bit(neg_bit, reg.neg);

        if is_fp16_alu {
            self.set_swizzle(swizzle_range, reg.swizzle);
        } else {
            assert!(reg.swizzle == SrcSwizzle::None);
        }
    }

    pub(super) fn encode_alu_src0(&mut self, src: &ALUSrc, file: RegFile, is_fp16_alu: bool) {
        let reg = match src {
            ALUSrc::None => return,
            ALUSrc::Reg(reg) => reg,
            _ => panic!("Invalid ALU src"),
        };
        self.set_alu_reg(24..32, 73, 72, 74..76, file, is_fp16_alu, reg);
    }

    pub(super) fn encode_alu_src2(&mut self, src: &ALUSrc, file: RegFile, is_fp16_alu: bool) {
        let reg = match src {
            ALUSrc::None => return,
            ALUSrc::Reg(reg) => reg,
            _ => panic!("Invalid ALU src"),
        };
        self.set_alu_reg(
            64..72,
            if is_fp16_alu { 83 } else { 74 },
            if is_fp16_alu { 84 } else { 75 },
            81..83,
            file,
            is_fp16_alu,
            reg,
        );
    }

    pub(super) fn encode_alu_reg(&mut self, reg: &ALURegRef, is_fp16_alu: bool) {
        self.set_alu_reg(32..40, 62, 63, 60..62, RegFile::GPR, is_fp16_alu, reg);
    }

    pub(super) fn encode_alu_ureg(&mut self, reg: &ALURegRef, is_fp16_alu: bool) {
        self.set_ureg(32..40, reg.reg);
        self.set_bit(62, reg.abs);
        self.set_bit(63, reg.neg);

        if is_fp16_alu {
            self.set_swizzle(60..62, reg.swizzle);
        } else {
            assert!(reg.swizzle == SrcSwizzle::None);
        }

        self.set_bit(91, true);
    }

    pub(super) fn encode_alu_imm(&mut self, imm: &u32) {
        self.set_field(32..64, *imm);
    }

    pub(super) fn encode_alu_cb(&mut self, cb: &ALUCBufRef, is_fp16_alu: bool) {
        self.set_src_cb(32..59, 91, &cb.cb);
        self.set_bit(62, cb.abs);
        self.set_bit(63, cb.neg);

        if is_fp16_alu {
            self.set_swizzle(60..62, cb.swizzle);
        } else {
            assert!(cb.swizzle == SrcSwizzle::None);
        }
    }

    pub(super) fn encode_alu_base(
        &mut self,
        opcode: u16,
        dst: Option<&Dst>,
        src0: Option<&Src>,
        src1: Option<&Src>,
        src2: Option<&Src>,
        is_fp16_alu: bool,
    ) {
        if let Some(dst) = dst {
            self.set_dst(dst);
        }

        let src0 = ALUSrc::from_src(self, src0, false);
        let src1 = ALUSrc::from_src(self, src1, false);
        let src2 = ALUSrc::from_src(self, src2, false);

        self.encode_alu_src0(&src0, RegFile::GPR, is_fp16_alu);

        let form = match &src2 {
            ALUSrc::None | ALUSrc::Reg(_) => {
                self.encode_alu_src2(&src2, RegFile::GPR, is_fp16_alu);
                match &src1 {
                    ALUSrc::None => 1_u8, // form
                    ALUSrc::Reg(reg1) => {
                        self.encode_alu_reg(reg1, is_fp16_alu);
                        1_u8 // form
                    }
                    ALUSrc::UReg(reg1) => {
                        self.encode_alu_ureg(reg1, is_fp16_alu);
                        6_u8 // form
                    }
                    ALUSrc::Imm32(imm1) => {
                        self.encode_alu_imm(imm1);
                        4_u8 // form
                    }
                    ALUSrc::CBuf(cb1) => {
                        self.encode_alu_cb(cb1, is_fp16_alu);
                        5_u8 // form
                    }
                }
            }
            ALUSrc::UReg(reg2) => {
                self.encode_alu_ureg(reg2, is_fp16_alu);
                self.encode_alu_src2(&src1, RegFile::GPR, is_fp16_alu);
                7_u8 // form
            }
            ALUSrc::Imm32(imm2) => {
                self.encode_alu_imm(imm2);
                self.encode_alu_src2(&src1, RegFile::GPR, is_fp16_alu);
                2_u8 // form
            }
            ALUSrc::CBuf(cb2) => {
                // TODO set_src_cx
                self.encode_alu_cb(cb2, is_fp16_alu);
                self.encode_alu_src2(&src1, RegFile::GPR, is_fp16_alu);
                3_u8 // form
            }
        };

        self.set_field(0..9, opcode);
        self.set_field(9..12, form);
    }

    pub(super) fn encode_alu(
        &mut self,
        opcode: u16,
        dst: Option<&Dst>,
        src0: Option<&Src>,
        src1: Option<&Src>,
        src2: Option<&Src>,
    ) {
        self.encode_alu_base(opcode, dst, src0, src1, src2, false);
    }

    pub(super) fn encode_fp16_alu(
        &mut self,
        opcode: u16,
        dst: Option<&Dst>,
        src0: Option<&Src>,
        src1: Option<&Src>,
        src2: Option<&Src>,
    ) {
        self.encode_alu_base(opcode, dst, src0, src1, src2, true);
    }

    pub(super) fn encode_ualu(
        &mut self,
        opcode: u16,
        dst: Option<&Dst>,
        src0: Option<&Src>,
        src1: Option<&Src>,
        src2: Option<&Src>,
    ) {
        if let Some(dst) = dst {
            self.set_udst(dst);
        }

        let src0 = ALUSrc::from_src(self, src0, true);
        let src1 = ALUSrc::from_src(self, src1, true);
        let src2 = ALUSrc::from_src(self, src2, true);

        // All uniform ALU requires bit 91 set
        self.set_bit(91, true);

        self.encode_alu_src0(&src0, RegFile::UGPR, false);
        let form = match &src2 {
            ALUSrc::None | ALUSrc::Reg(_) => {
                self.encode_alu_src2(&src2, RegFile::UGPR, false);
                match &src1 {
                    ALUSrc::None => 1_u8, // form
                    ALUSrc::Reg(reg1) => {
                        self.encode_alu_ureg(reg1, false);
                        1_u8 // form
                    }
                    ALUSrc::UReg(_) => panic!("UALU never has UReg"),
                    ALUSrc::Imm32(imm1) => {
                        self.encode_alu_imm(imm1);
                        4_u8 // form
                    }
                    ALUSrc::CBuf(_) => panic!("UALU does not support cbufs"),
                }
            }
            ALUSrc::UReg(_) => panic!("UALU never has UReg"),
            ALUSrc::Imm32(imm2) => {
                self.encode_alu_imm(imm2);
                self.encode_alu_src2(&src1, RegFile::UGPR, false);
                2_u8 // form
            }
            ALUSrc::CBuf(_) => panic!("UALU does not support cbufs"),
        };

        self.set_field(0..9, opcode);
        self.set_field(9..12, form);
    }

    pub(super) fn set_rnd_mode(&mut self, range: Range<usize>, rnd_mode: FRndMode) {
        assert!(range.len() == 2);
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
}

//
// Legalization helpers
//

pub(super) fn op_gpr(op: &impl DstsAsSlice) -> RegFile {
    if op.is_uniform() {
        RegFile::UGPR
    } else {
        RegFile::GPR
    }
}

/// Helper to legalize extended or external instructions
///
/// These are instructions which reach out external units such as load/store
/// and texture ops.  They typically can't take anything but GPRs and are the
/// only types of instructions that support vectors.  They also can never be
/// uniform so we always evict uniform sources.
///
pub(super) fn legalize_ext_instr(op: &mut impl SrcsAsSlice, b: &mut LegalizeBuilder) {
    let src_types = op.src_types();
    for (i, src) in op.srcs_as_mut_slice().iter_mut().enumerate() {
        match src_types[i] {
            SrcType::SSA | SrcType::GPR => match &mut src.reference {
                SrcRef::Zero | SrcRef::True | SrcRef::False => {
                    assert!(src_types[i] != SrcType::SSA);
                }
                SrcRef::SSA(ssa) => {
                    b.copy_ssa_ref_if_uniform(ssa);
                }
                _ => panic!("Unsupported source reference"),
            },
            SrcType::ALU
            | SrcType::F16
            | SrcType::F16v2
            | SrcType::F32
            | SrcType::F64
            | SrcType::I32
            | SrcType::B32 => {
                panic!("ALU srcs must be legalized explicitly");
            }
            SrcType::Pred => {
                panic!("Predicates must be legalized explicitly");
            }
            SrcType::Carry => {
                panic!("Carry is invalid on Volta+");
            }
            SrcType::Bar => (),
        }
    }
}

//
// Implementations of SM70Op for each op we support on Volta+
//
