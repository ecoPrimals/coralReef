// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)

pub(super) use super::super::sm30_instr_latencies::{
    KeplerInstructionEncoder, instr_exec_latency, instr_latency, latency_upper_bound,
};
pub(super) use crate::codegen::ir::*;
pub(super) use crate::codegen::legalize::{
    LegalizeBuildHelpers, LegalizeBuilder, PadValue, src_is_reg, swap_srcs_if_not_reg,
};

pub(super) use bitview::*;
use coral_reef_stubs::fxhash::FxHashMap;
pub(super) use std::ops::Range;

pub struct ShaderModel32 {
    sm: u8,
}

impl ShaderModel32 {
    pub fn new(sm: u8) -> Self {
        assert!((32..50).contains(&sm));
        Self { sm }
    }
}

impl ShaderModel for ShaderModel32 {
    fn sm(&self) -> u8 {
        self.sm
    }

    fn reg_count(&self, file: RegFile) -> u32 {
        match file {
            RegFile::GPR => 255,
            RegFile::UGPR => 0,
            RegFile::Pred => 7,
            RegFile::UPred => 0,
            RegFile::Carry => 1,
            RegFile::Bar => 0,
            RegFile::Mem => RegRef::MAX_IDX + 1,
        }
    }

    fn hw_reserved_gpr_count(&self) -> u32 {
        0
    }

    fn crs_size(&self, max_crs_depth: u32) -> u32 {
        if max_crs_depth <= 16 {
            0
        } else if max_crs_depth <= 32 {
            1024
        } else {
            ((max_crs_depth + 32) * 16).next_multiple_of(512)
        }
    }

    fn op_can_be_uniform(&self, _op: &Op) -> bool {
        false
    }

    fn exec_latency(&self, op: &Op) -> u32 {
        instr_exec_latency(self.sm, op)
    }

    fn raw_latency(&self, write: &Op, dst_idx: usize, _read: &Op, _src_idx: usize) -> u32 {
        instr_latency(self.sm, write, dst_idx)
    }

    fn war_latency(&self, _read: &Op, _src_idx: usize, _write: &Op, _dst_idx: usize) -> u32 {
        // We assume the source gets read in the first 4 cycles.  We don't know
        // how quickly the write will happen.  This is all a guess.
        4
    }

    fn waw_latency(
        &self,
        a: &Op,
        a_dst_idx: usize,
        _a_has_pred: bool,
        _b: &Op,
        _b_dst_idx: usize,
    ) -> u32 {
        // We know our latencies are wrong so assume the wrote could happen
        // anywhere between 0 and instr_latency(a) cycles
        instr_latency(self.sm, a, a_dst_idx)
    }

    fn paw_latency(&self, _write: &Op, _dst_idx: usize) -> u32 {
        13
    }

    fn worst_latency(&self, write: &Op, dst_idx: usize) -> u32 {
        instr_latency(self.sm, write, dst_idx)
    }

    fn latency_upper_bound(&self) -> u32 {
        latency_upper_bound()
    }

    fn max_instr_delay(&self) -> u8 {
        32
    }

    fn legalize_op(&self, b: &mut LegalizeBuilder, op: &mut Op) -> Result<(), crate::CompileError> {
        if let Op::IMadSp(imadsp) = op {
            if let IMadSpMode::Explicit([_src0, src1, src2]) = imadsp.mode {
                if src2.unsigned() == IMadSpSrcType::U16Hi {
                    return Err(crate::CompileError::Encoding(
                        "SM32 IMadSp src2 U16Hi is not encodable".into(),
                    ));
                }
                if !matches!(src1.unsigned(), IMadSpSrcType::U16Lo | IMadSpSrcType::U24) {
                    return Err(crate::CompileError::Encoding(
                        "SM32 IMadSp src1 must be 16 or 24 bits".into(),
                    ));
                }
            }
        }
        op.legalize(b);
        Ok(())
    }

    fn encode_shader(&self, s: &Shader<'_>) -> Result<Vec<u32>, crate::CompileError> {
        crate::codegen::catch_ice(|| super::encode_sm32_shader(self, s))
    }

    fn max_warps(&self) -> u32 {
        64
    }
}

pub(super) trait SM32Op {
    fn legalize(&mut self, b: &mut LegalizeBuilder);
    fn encode(&self, e: &mut SM32Encoder<'_>);
}

pub(super) fn zero_reg() -> RegRef {
    RegRef::new(RegFile::GPR, 255, 1)
}

pub(super) fn true_reg() -> RegRef {
    RegRef::new(RegFile::Pred, 7, 1)
}

pub(super) struct SM32Encoder<'a> {
    pub(super) sm: &'a ShaderModel32,
    pub(super) ip: usize,
    pub(super) labels: &'a FxHashMap<Label, usize>,
    pub(super) inst: [u32; 2],
}

impl BitViewable for SM32Encoder<'_> {
    fn bits(&self) -> usize {
        self.inst.bits()
    }

    fn get_bit_range_u64(&self, range: Range<usize>) -> u64 {
        self.inst.get_bit_range_u64(range)
    }
}

impl BitMutViewable for SM32Encoder<'_> {
    fn set_bit_range_u64(&mut self, range: Range<usize>, val: u64) {
        self.inst.set_bit_range_u64(range, val);
    }
}

impl SM32Encoder<'_> {
    pub(super) fn set_opcode(&mut self, opcode: u16, functional_unit: u8) {
        self.set_field(52..64, opcode);

        assert!(functional_unit < 3);
        self.set_field(0..2, functional_unit);
    }

    pub(super) fn set_pred_reg(&mut self, range: Range<usize>, reg: RegRef) {
        assert!(range.len() == 3);
        assert!(reg.file() == RegFile::Pred);
        assert!(reg.base_idx() <= 7);
        assert!(reg.comps() == 1);
        self.set_field(range, reg.base_idx());
    }

    pub(super) fn set_pred_src(&mut self, range: Range<usize>, src: &Src) {
        // The default for predicates is true
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

    pub(super) fn set_pred(&mut self, pred: &Pred) {
        // predicates are 4 bits starting at 18, last one denotes inversion
        assert!(!pred.is_false());
        self.set_pred_reg(
            18..21,
            match pred.predicate {
                PredRef::None => true_reg(),
                PredRef::Reg(reg) => reg,
                PredRef::SSA(_) => crate::codegen::ice!("SSA values must be lowered"),
            },
        );
        self.set_bit(21, pred.inverted);
    }

    pub(super) fn set_reg(&mut self, range: Range<usize>, reg: RegRef) {
        assert!(range.len() == 8);
        assert!(reg.file() == RegFile::GPR);
        self.set_field(range, reg.base_idx());
    }

    pub(super) fn set_reg_src_ref(&mut self, range: Range<usize>, reference: &SrcRef) {
        let reg = match reference {
            SrcRef::Zero => zero_reg(),
            SrcRef::Reg(reg) => *reg,
            SrcRef::SSA(_) | SrcRef::True | SrcRef::False | SrcRef::Imm32(_) | SrcRef::CBuf(_) => {
                crate::codegen::ice!("Not a register")
            }
        };
        self.set_reg(range, reg);
    }

    pub(super) fn set_reg_src(&mut self, range: Range<usize>, src: &Src) {
        assert!(src.swizzle.is_none());
        self.set_reg_src_ref(range, &src.reference);
    }

    pub(super) fn set_reg_fmod_src(
        &mut self,
        range: Range<usize>,
        abs_bit: usize,
        neg_bit: usize,
        src: &Src,
    ) {
        self.set_reg_src_ref(range, &src.reference);
        self.set_bit(abs_bit, src.modifier.has_fabs());
        self.set_bit(neg_bit, src.modifier.has_fneg());
    }

    pub(super) fn set_dst(&mut self, dst: &Dst) {
        let reg = match dst {
            Dst::None => zero_reg(),
            Dst::Reg(reg) => *reg,
            Dst::SSA(_) => crate::codegen::ice!("Invalid dst {dst}"),
        };
        self.set_reg(2..10, reg);
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

    pub(super) fn set_src_cbuf(&mut self, range: Range<usize>, cb: &CBufRef) {
        let mut v = new_subset(&mut self.inst[..], range.start, range.len());

        assert!(cb.offset % 4 == 0);
        v.set_field(0..14, cb.offset >> 2);

        let CBuf::Binding(idx) = cb.buf else {
            crate::codegen::ice!("Must be a bound constant buffer");
        };

        v.set_field(14..19, idx);
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
// Small helper for encoding of ALU instructions
//
pub(super) enum AluSrc {
    Reg(RegRef),
    Imm(u32),
    CBuf(CBufRef),
}

impl AluSrc {
    pub(super) fn from_src(src: &Src) -> Self {
        assert!(src.swizzle.is_none());
        // do not assert modifier, can be encoded by opcode.

        match &src.reference {
            SrcRef::Zero => Self::Reg(zero_reg()),
            SrcRef::Reg(r) => Self::Reg(*r),
            SrcRef::Imm32(x) => Self::Imm(*x),
            SrcRef::CBuf(x) => Self::CBuf(x.clone()),
            _ => crate::codegen::ice!("Unhandled ALU src type"),
        }
    }
}

impl SM32Encoder<'_> {
    pub(super) fn encode_form_immreg(
        &mut self,
        opcode_imm: u16,
        opcode_reg: u16,
        dst: Option<&Dst>,
        src0: &Src,
        src1: &Src,
        src2: Option<&Src>,
        is_imm_float: bool,
    ) {
        // There are 4 possible forms:
        // rir: 10...01  (only one with immediate)
        // rcr: 01...10  (c = constant buf reference)
        // rrc: 10...10
        // rrr: 11...10
        // other forms are invalid (or encode other instructions)
        enum Form {
            RIR,
            RCR,
            RRC,
            RRR,
        }
        let src1 = AluSrc::from_src(src1);
        let src2 = src2.map(AluSrc::from_src);

        if let Some(dst) = dst {
            self.set_dst(dst);
        }

        // SRC[0] must always be a register
        self.set_reg_src(10..18, src0);

        let form = match src1 {
            AluSrc::Imm(imm) => {
                self.set_opcode(opcode_imm, 1);

                if is_imm_float {
                    self.set_src_imm_f20(23..42, 59, imm);
                } else {
                    self.set_src_imm_i20(23..42, 59, imm);
                }
                match src2 {
                    None => {}
                    Some(AluSrc::Reg(src2)) => self.set_reg(42..50, src2),
                    _ => crate::codegen::ice!("Invalid form"),
                }
                Form::RIR
            }
            AluSrc::CBuf(cb) => {
                self.set_opcode(opcode_reg, 2);
                self.set_src_cbuf(23..42, &cb);
                match src2 {
                    None => {}
                    Some(AluSrc::Reg(src2)) => self.set_reg(42..50, src2),
                    _ => crate::codegen::ice!("Invalid form"),
                }
                Form::RCR
            }
            AluSrc::Reg(r1) => {
                self.set_opcode(opcode_reg, 2);
                match src2 {
                    None => {
                        self.set_reg(23..31, r1);
                        Form::RRR
                    }
                    Some(AluSrc::Reg(r2)) => {
                        self.set_reg(23..31, r1);
                        self.set_reg(42..50, r2);
                        Form::RRR
                    }
                    Some(AluSrc::CBuf(cb)) => {
                        self.set_src_cbuf(23..42, &cb);
                        self.set_reg(42..50, r1);
                        Form::RRC
                    }
                    _ => crate::codegen::ice!("Invalid form"),
                }
            }
        };

        // Set form selector
        let form_sel: u8 = match form {
            Form::RCR => 0b01,
            Form::RRC => 0b10,
            Form::RRR => 0b11,
            Form::RIR => return, // don't set high bits, reserved for opcode
        };
        assert!(self.get_bit_range_u64(62..64) == 0);
        self.set_field(62..64, form_sel);
    }
}

//
// Implementations of SM32Op for each op we support on KeplerB
//

macro_rules! sm32_op_match {
    ($op: expr, |$x: ident| $y: expr) => {
        match $op {
            Op::FAdd($x) => $y,
            Op::FFma($x) => $y,
            Op::FMnMx($x) => $y,
            Op::FMul($x) => $y,
            Op::Rro($x) => $y,
            Op::Transcendental($x) => $y,
            Op::FSet($x) => $y,
            Op::FSetP($x) => $y,
            Op::FSwz($x) => $y,
            Op::DAdd($x) => $y,
            Op::DFma($x) => $y,
            Op::DMnMx($x) => $y,
            Op::DMul($x) => $y,
            Op::DSetP($x) => $y,
            Op::Bfe($x) => $y,
            Op::BRev($x) => $y,
            Op::Flo($x) => $y,
            Op::IAdd2($x) => $y,
            Op::IAdd2X($x) => $y,
            Op::IMad($x) => $y,
            Op::IMul($x) => $y,
            Op::IMnMx($x) => $y,
            Op::ISetP($x) => $y,
            Op::Lop2($x) => $y,
            Op::PopC($x) => $y,
            Op::Shf($x) => $y,
            Op::Shl($x) => $y,
            Op::Shr($x) => $y,
            Op::F2F($x) => $y,
            Op::F2I($x) => $y,
            Op::I2F($x) => $y,
            Op::I2I($x) => $y,
            Op::FRnd($x) => $y,
            Op::Mov($x) => $y,
            Op::Prmt($x) => $y,
            Op::Sel($x) => $y,
            Op::Shfl($x) => $y,
            Op::PSetP($x) => $y,
            Op::Tex($x) => $y,
            Op::Tld($x) => $y,
            Op::Tld4($x) => $y,
            Op::Tmml($x) => $y,
            Op::Txd($x) => $y,
            Op::Txq($x) => $y,
            Op::SuClamp($x) => $y,
            Op::SuBfm($x) => $y,
            Op::SuEau($x) => $y,
            Op::IMadSp($x) => $y,
            Op::SuLdGa($x) => $y,
            Op::SuStGa($x) => $y,
            Op::Ld($x) => $y,
            Op::Ldc($x) => $y,
            Op::LdSharedLock($x) => $y,
            Op::St($x) => $y,
            Op::StSCheckUnlock($x) => $y,
            Op::Atom($x) => $y,
            Op::AL2P($x) => $y,
            Op::ALd($x) => $y,
            Op::ASt($x) => $y,
            Op::Ipa($x) => $y,
            Op::CCtl($x) => $y,
            Op::MemBar($x) => $y,
            Op::Bra($x) => $y,
            Op::SSy($x) => $y,
            Op::Sync($x) => $y,
            Op::Brk($x) => $y,
            Op::PBk($x) => $y,
            Op::Cont($x) => $y,
            Op::PCnt($x) => $y,
            Op::Exit($x) => $y,
            Op::Bar($x) => $y,
            Op::TexDepBar($x) => $y,
            Op::ViLd($x) => $y,
            Op::Kill($x) => $y,
            Op::Nop($x) => $y,
            Op::PixLd($x) => $y,
            Op::S2R($x) => $y,
            Op::Vote($x) => $y,
            Op::Out($x) => $y,
            _ => crate::codegen::ice!("Unhandled instruction {}", $op),
        }
    };
}

impl SM32Op for Op {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        sm32_op_match!(self, |op| op.legalize(b));
    }
    fn encode(&self, e: &mut SM32Encoder<'_>) {
        sm32_op_match!(self, |op| op.encode(e));
    }
}

impl KeplerInstructionEncoder for ShaderModel32 {
    fn encode_instr(
        &self,
        instr: &Instr,
        labels: &FxHashMap<Label, usize>,
        encoded: &mut Vec<u32>,
    ) {
        let mut e = SM32Encoder {
            sm: self,
            ip: encoded.len() * 4,
            labels,
            inst: [0_u32; 2],
        };
        instr.op.encode(&mut e);
        e.set_pred(&instr.pred);
        encoded.extend(&e.inst[..]);
    }

    fn prepare_sched_instr<'a>(&self, sched_instr: &'a mut [u32; 2]) -> impl BitMutViewable + 'a {
        let bv = sched_instr;
        bv.set_field(0..2, 0b00);
        bv.set_field(58..64, 0b00_0010); // 0x08

        new_subset(bv, 2, 56)
    }
}
