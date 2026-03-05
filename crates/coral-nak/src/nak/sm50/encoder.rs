// Copyright © 2023 Collabora, Ltd.
// SPDX-License-Identifier: MIT

#![allow(clippy::wildcard_imports)]

pub(super) use super::super::ir::*;
pub(super) use super::super::legalize::{
    LegalizeBuildHelpers, LegalizeBuilder, src_is_reg, swap_srcs_if_not_reg,
};
pub(super) use super::encode_sm50_shader;
pub(super) use bitview::*;

use rustc_hash::FxHashMap;
pub(super) use std::ops::Range;

pub fn instr_latency(_sm: u8, op: &Op, dst_idx: usize) -> u32 {
    let file = match &op.dsts_as_slice()[dst_idx] {
        Dst::None => return 0,
        Dst::SSA(vec) => vec.file(),
        Dst::Reg(reg) => reg.file(),
    };

    let (gpr_latency, pred_latency) = match op {
            // Double-precision float ALU
            Op::DAdd(_)
            | Op::DFma(_)
            | Op::DMnMx(_)
            | Op::DMul(_)
            | Op::DSetP(_)
            // Half-precision float ALU
            | Op::HAdd2(_)
            | Op::HFma2(_)
            | Op::HMul2(_)
            | Op::HSet2(_)
            | Op::HSetP2(_)
            | Op::HMnMx2(_) => {
                (13, 14)
            }
            _ => (6, 13)
    };

    // This is BS and we know it
    match file {
        RegFile::GPR => gpr_latency,
        RegFile::Pred => pred_latency,
        RegFile::UGPR | RegFile::UPred => panic!("No uniform registers"),
        RegFile::Bar => 0, // Barriers have a HW scoreboard
        RegFile::Carry => 6,
        RegFile::Mem => panic!("Not a register"),
    }
}

pub struct ShaderModel50 {
    sm: u8,
}

impl ShaderModel50 {
    pub fn new(sm: u8) -> Self {
        assert!(sm >= 50 && sm < 70);
        Self { sm }
    }
}

impl ShaderModel for ShaderModel50 {
    fn sm(&self) -> u8 {
        self.sm
    }

    fn num_regs(&self, file: RegFile) -> u32 {
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

    fn hw_reserved_gprs(&self) -> u32 {
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
        match op {
            Op::CCtl(_)
            | Op::MemBar(_)
            | Op::Bra(_)
            | Op::SSy(_)
            | Op::Sync(_)
            | Op::Brk(_)
            | Op::PBk(_)
            | Op::Cont(_)
            | Op::PCnt(_)
            | Op::Exit(_)
            | Op::Bar(_)
            | Op::Kill(_)
            | Op::OutFinal(_) => 13,
            _ => 1,
        }
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

    fn latency_upper_bound(&self) -> u32 {
        14
    }

    fn worst_latency(&self, write: &Op, dst_idx: usize) -> u32 {
        instr_latency(self.sm, write, dst_idx)
    }

    fn max_instr_delay(&self) -> u8 {
        15
    }

    fn legalize_op(&self, b: &mut LegalizeBuilder, op: &mut Op) -> Result<(), crate::CompileError> {
        op.legalize(b);
        Ok(())
    }

    fn encode_shader(&self, s: &Shader<'_>) -> Result<Vec<u32>, crate::CompileError> {
        Ok(encode_sm50_shader(self, s))
    }
}

pub(super) trait SM50Op {
    fn legalize(&mut self, b: &mut LegalizeBuilder);
    fn encode(&self, e: &mut SM50Encoder<'_>);
}

pub(super) struct SM50Encoder<'a> {
    pub(super) sm: &'a ShaderModel50,
    pub(super) ip: usize,
    pub(super) labels: &'a FxHashMap<Label, usize>,
    pub(super) inst: [u32; 2],
    pub(super) sched: u32,
}

impl BitViewable for SM50Encoder<'_> {
    fn bits(&self) -> usize {
        self.inst.bits()
    }

    fn get_bit_range_u64(&self, range: Range<usize>) -> u64 {
        self.inst.get_bit_range_u64(range)
    }
}

impl BitMutViewable for SM50Encoder<'_> {
    fn set_bit_range_u64(&mut self, range: Range<usize>, val: u64) {
        self.inst.set_bit_range_u64(range, val);
    }
}

pub(super) fn zero_reg() -> RegRef {
    RegRef::new(RegFile::GPR, 255, 1)
}

pub(super) fn true_reg() -> RegRef {
    RegRef::new(RegFile::Pred, 7, 1)
}

impl SM50Encoder<'_> {
    pub(super) fn set_opcode(&mut self, opcode: u16) {
        self.set_field(48..64, opcode);
    }

    pub(super) fn set_pred_reg(&mut self, range: Range<usize>, reg: RegRef) {
        assert!(range.len() == 3);
        assert!(reg.file() == RegFile::Pred);
        assert!(reg.base_idx() <= 7);
        assert!(reg.comps() == 1);
        self.set_field(range, reg.base_idx());
    }

    pub(super) fn set_pred(&mut self, pred: &Pred) {
        assert!(!pred.is_false());
        self.set_pred_reg(
            16..19,
            match pred.pred_ref {
                PredRef::None => true_reg(),
                PredRef::Reg(reg) => reg,
                PredRef::SSA(_) => panic!("SSA values must be lowered"),
            },
        );
        self.set_bit(19, pred.pred_inv);
    }

    pub(super) fn set_instr_deps(&mut self, deps: &InstrDeps) {
        self.sched.set_field(0..4, deps.delay);
        self.sched.set_bit(4, deps.yld);
        self.sched.set_field(5..8, deps.wr_bar().unwrap_or(7));
        self.sched.set_field(8..11, deps.rd_bar().unwrap_or(7));
        self.sched.set_field(11..17, deps.wt_bar_mask);
        self.sched.set_field(17..21, deps.reuse_mask);
    }

    pub(super) fn set_reg(&mut self, range: Range<usize>, reg: RegRef) {
        assert!(range.len() == 8);
        assert!(reg.file() == RegFile::GPR);
        self.set_field(range, reg.base_idx());
    }

    pub(super) fn set_reg_src_ref(&mut self, range: Range<usize>, src_ref: &SrcRef) {
        match src_ref {
            SrcRef::Zero => self.set_reg(range, zero_reg()),
            SrcRef::Reg(reg) => self.set_reg(range, *reg),
            _ => panic!("Not a register"),
        }
    }

    pub(super) fn set_reg_src(&mut self, range: Range<usize>, src: &Src) {
        assert!(src.is_unmodified());
        self.set_reg_src_ref(range, &src.src_ref);
    }

    pub(super) fn set_reg_fmod_src(
        &mut self,
        range: Range<usize>,
        abs_bit: usize,
        neg_bit: usize,
        src: &Src,
    ) {
        self.set_reg_src_ref(range, &src.src_ref);
        self.set_bit(abs_bit, src.src_mod.has_fabs());
        self.set_bit(neg_bit, src.src_mod.has_fneg());
    }

    pub(super) fn set_reg_ineg_src(&mut self, range: Range<usize>, neg_bit: usize, src: &Src) {
        self.set_reg_src_ref(range, &src.src_ref);
        self.set_bit(neg_bit, src.src_mod.is_ineg());
    }

    pub(super) fn set_reg_bnot_src(&mut self, range: Range<usize>, not_bit: usize, src: &Src) {
        self.set_reg_src_ref(range, &src.src_ref);
        self.set_bit(not_bit, src.src_mod.is_bnot());
    }

    pub(super) fn set_pred_dst(&mut self, range: Range<usize>, dst: &Dst) {
        match dst {
            Dst::None => {
                self.set_pred_reg(range, true_reg());
            }
            Dst::Reg(reg) => self.set_pred_reg(range, *reg),
            Dst::SSA(_) => panic!("Not a register"),
        }
    }

    pub(super) fn set_pred_src(&mut self, range: Range<usize>, not_bit: usize, src: &Src) {
        let (not, reg) = match src.src_ref {
            SrcRef::True => (false, true_reg()),
            SrcRef::False => (true, true_reg()),
            SrcRef::Reg(reg) => (false, reg),
            SrcRef::Zero | SrcRef::SSA(_) | SrcRef::Imm32(_) | SrcRef::CBuf(_) => {
                panic!("Not a register")
            }
        };
        self.set_pred_reg(range, reg);
        self.set_bit(not_bit, not ^ src.src_mod.is_bnot());
    }

    pub(super) fn set_dst(&mut self, dst: &Dst) {
        let reg = match dst {
            Dst::None => zero_reg(),
            Dst::Reg(reg) => *reg,
            Dst::SSA(_) => panic!("invalid dst {dst}"),
        };
        self.set_reg(0..8, reg);
    }

    pub(super) fn set_src_imm32(&mut self, range: Range<usize>, u: u32) {
        assert!(range.len() == 32);
        self.set_field(range, u);
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

    pub(super) fn set_src_cb(&mut self, range: Range<usize>, cb: &CBufRef) {
        let mut v = new_subset(&mut self.inst[..], range.start, range.len());

        assert!(cb.offset % 4 == 0);

        v.set_field(0..14, cb.offset >> 2);
        if let CBuf::Binding(idx) = cb.buf {
            v.set_field(14..19, idx);
        } else {
            panic!("Must be a bound constant buffer");
        }
    }

    pub(super) fn set_cb_fmod_src(
        &mut self,
        range: Range<usize>,
        abs_bit: usize,
        neg_bit: usize,
        src: &Src,
    ) {
        if let SrcRef::CBuf(cb) = &src.src_ref {
            self.set_src_cb(range, cb);
        } else {
            panic!("Not a CBuf source");
        }

        self.set_bit(abs_bit, src.src_mod.has_fabs());
        self.set_bit(neg_bit, src.src_mod.has_fneg());
    }

    pub(super) fn set_cb_ineg_src(&mut self, range: Range<usize>, neg_bit: usize, src: &Src) {
        if let SrcRef::CBuf(cb) = &src.src_ref {
            self.set_src_cb(range, cb);
        } else {
            panic!("Not a CBuf source");
        }

        self.set_bit(neg_bit, src.src_mod.is_ineg());
    }

    pub(super) fn set_cb_bnot_src(&mut self, range: Range<usize>, not_bit: usize, src: &Src) {
        if let SrcRef::CBuf(cb) = &src.src_ref {
            self.set_src_cb(range, cb);
        } else {
            panic!("Not a CBuf source");
        }

        self.set_bit(not_bit, src.src_mod.is_bnot());
    }
}

//
// Legalization helpers
//

/// Helper to legalize extended or external instructions
///
/// These are instructions which reach out external units such as load/store
/// and texture ops.  They typically can't take anything but GPRs and are the
/// only types of instructions that support vectors.
///
pub(super) fn legalize_ext_instr(op: &mut impl SrcsAsSlice, _b: &mut LegalizeBuilder) {
    let src_types = op.src_types();
    for (i, src) in op.srcs_as_mut_slice().iter_mut().enumerate() {
        match src_types[i] {
            SrcType::SSA => {
                assert!(src.as_ssa().is_some());
            }
            SrcType::GPR => {
                assert!(src_is_reg(src, RegFile::GPR));
            }
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
                panic!("Carry values must be legalized explicitly");
            }
            SrcType::Bar => panic!("Barrier regs are Volta+"),
        }
    }
}

//
// Implementations of SM50Op for each op we support on Maxwell/Pascal
//

impl SM50Encoder<'_> {
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

impl SM50Encoder<'_> {
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
                _ => panic!("Unknown LOD mode"),
            },
        );
    }

    pub(super) fn set_tex_ndv(&mut self, bit: usize, deriv_mode: TexDerivMode) {
        let ndv = match deriv_mode {
            TexDerivMode::Auto => false,
            TexDerivMode::NonDivergent => true,
            _ => panic!("{deriv_mode} is not supported"),
        };
        self.set_bit(bit, ndv);
    }

    pub(super) fn set_tex_channel_mask(&mut self, range: Range<usize>, channel_mask: ChannelMask) {
        self.set_field(range, channel_mask.to_bits());
    }
}

impl SM50Encoder<'_> {
    pub(super) fn set_rel_offset(&mut self, range: Range<usize>, label: &Label) {
        let ip = u32::try_from(self.ip).unwrap();
        let ip = i32::try_from(ip).unwrap();

        let target_ip = *self.labels.get(label).unwrap();
        let target_ip = u32::try_from(target_ip).unwrap();
        let target_ip = i32::try_from(target_ip).unwrap();

        let rel_offset = target_ip - ip - 8;

        self.set_field(range, rel_offset);
    }
}

macro_rules! sm50_op_match {
    ($op: expr, |$x: ident| $y: expr) => {
        match $op {
            Op::FAdd($x) => $y,
            Op::FMnMx($x) => $y,
            Op::FMul($x) => $y,
            Op::FFma($x) => $y,
            Op::FSet($x) => $y,
            Op::FSetP($x) => $y,
            Op::FSwzAdd($x) => $y,
            Op::Rro($x) => $y,
            Op::MuFu($x) => $y,
            Op::Flo($x) => $y,
            Op::DAdd($x) => $y,
            Op::DFma($x) => $y,
            Op::DMnMx($x) => $y,
            Op::DMul($x) => $y,
            Op::DSetP($x) => $y,
            Op::IAdd2($x) => $y,
            Op::IAdd2X($x) => $y,
            Op::Mov($x) => $y,
            Op::Sel($x) => $y,
            Op::Shfl($x) => $y,
            Op::Vote($x) => $y,
            Op::PSetP($x) => $y,
            Op::SuSt($x) => $y,
            Op::S2R($x) => $y,
            Op::PopC($x) => $y,
            Op::Prmt($x) => $y,
            Op::Ld($x) => $y,
            Op::Ldc($x) => $y,
            Op::St($x) => $y,
            Op::Lop2($x) => $y,
            Op::Shf($x) => $y,
            Op::Shl($x) => $y,
            Op::Shr($x) => $y,
            Op::F2F($x) => $y,
            Op::F2I($x) => $y,
            Op::I2F($x) => $y,
            Op::I2I($x) => $y,
            Op::IMad($x) => $y,
            Op::IMul($x) => $y,
            Op::IMnMx($x) => $y,
            Op::ISetP($x) => $y,
            Op::Tex($x) => $y,
            Op::Tld($x) => $y,
            Op::Tld4($x) => $y,
            Op::Tmml($x) => $y,
            Op::Txd($x) => $y,
            Op::Txq($x) => $y,
            Op::Ipa($x) => $y,
            Op::AL2P($x) => $y,
            Op::ALd($x) => $y,
            Op::ASt($x) => $y,
            Op::CCtl($x) => $y,
            Op::MemBar($x) => $y,
            Op::Atom($x) => $y,
            Op::Bra($x) => $y,
            Op::SSy($x) => $y,
            Op::Sync($x) => $y,
            Op::Brk($x) => $y,
            Op::PBk($x) => $y,
            Op::Cont($x) => $y,
            Op::PCnt($x) => $y,
            Op::Exit($x) => $y,
            Op::Bar($x) => $y,
            Op::SuLd($x) => $y,
            Op::SuAtom($x) => $y,
            Op::Kill($x) => $y,
            Op::CS2R($x) => $y,
            Op::Nop($x) => $y,
            Op::PixLd($x) => $y,
            Op::Isberd($x) => $y,
            Op::Out($x) => $y,
            Op::Bfe($x) => $y,
            _ => panic!("Unhandled instruction {}", $op),
        }
    };
}

impl SM50Op for Op {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        sm50_op_match!(self, |op| op.legalize(b));
    }
    fn encode(&self, e: &mut SM50Encoder<'_>) {
        sm50_op_match!(self, |op| op.encode(e));
    }
}
