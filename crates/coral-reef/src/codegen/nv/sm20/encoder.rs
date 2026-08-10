// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)

pub(super) use super::super::sm30_instr_latencies::{
    KeplerInstructionEncoder, instr_exec_latency, instr_latency, latency_upper_bound,
};
pub(super) use super::encode_sm20_shader;
pub(super) use super::encoder_fields::legalize_ext_instr;
pub(super) use crate::codegen::ir::*;
pub(super) use crate::codegen::legalize::{
    LegalizeBuildHelpers, LegalizeBuilder, src_is_reg, swap_srcs_if_not_reg,
};
pub(super) use bitview::*;

use coral_reef_stubs::fxhash::FxHashMap;
use std::fmt;
pub(super) use std::ops::Range;

pub struct ShaderModel20 {
    sm: u8,
}

impl ShaderModel20 {
    pub fn new(sm: u8) -> Self {
        assert!((20..32).contains(&sm));
        Self { sm }
    }
}

impl ShaderModel for ShaderModel20 {
    fn sm(&self) -> u8 {
        self.sm
    }

    fn reg_count(&self, file: RegFile) -> u32 {
        match file {
            RegFile::GPR => 63,
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
        instr_latency(self.sm, a, a_dst_idx)
    }

    fn paw_latency(&self, _write: &Op, _dst_idx: usize) -> u32 {
        13
    }

    fn latency_upper_bound(&self) -> u32 {
        latency_upper_bound()
    }

    fn worst_latency(&self, write: &Op, dst_idx: usize) -> u32 {
        instr_latency(self.sm, write, dst_idx)
    }

    fn max_instr_delay(&self) -> u8 {
        32
    }

    fn legalize_op(&self, b: &mut LegalizeBuilder, op: &mut Op) -> Result<(), crate::CompileError> {
        if let Op::IMadSp(imadsp) = op {
            if let IMadSpMode::Explicit([_src0, src1, src2]) = imadsp.mode {
                if src2.unsigned() == IMadSpSrcType::U16Hi {
                    return Err(crate::CompileError::Encoding(
                        "SM20 IMadSp src2 U16Hi is not encodable".into(),
                    ));
                }
                if !matches!(src1.unsigned(), IMadSpSrcType::U16Lo | IMadSpSrcType::U24) {
                    return Err(crate::CompileError::Encoding(
                        "SM20 IMadSp src1 must be 16 or 24 bits".into(),
                    ));
                }
            }
        }
        op.legalize(b);
        Ok(())
    }

    fn encode_shader(&self, s: &Shader<'_>) -> Result<Vec<u32>, crate::CompileError> {
        crate::codegen::catch_ice(|| {
            if self.sm >= 30 {
                super::encode_sm30_shader(self, s)
            } else {
                encode_sm20_shader(self, s)
            }
        })
    }

    fn max_warps(&self) -> u32 {
        48
    }
}

pub(super) fn zero_reg() -> RegRef {
    RegRef::new(RegFile::GPR, 63, 1)
}

pub(super) fn true_reg() -> RegRef {
    RegRef::new(RegFile::Pred, 7, 1)
}

pub(super) enum AluSrc {
    None,
    Reg(RegRef),
    Imm(u32),
    CBuf(CBufRef),
}

impl AluSrc {
    pub(super) fn from_src(src: Option<&Src>) -> Self {
        if let Some(src) = src {
            assert!(src.swizzle.is_none());
            match &src.reference {
                SrcRef::Zero => Self::Reg(zero_reg()),
                SrcRef::Reg(r) => Self::Reg(*r),
                SrcRef::Imm32(x) => Self::Imm(*x),
                SrcRef::CBuf(x) => Self::CBuf(x.clone()),
                _ => crate::codegen::ice!("Unhandled ALU src type"),
            }
        } else {
            Self::None
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum SM20Unit {
    Float = 0,
    Double = 1,
    Imm32 = 2,
    Int = 3,
    Move = 4,
    Mem = 5,
    Tex = 6,
    Exec = 7,
}

impl fmt::Display for SM20Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Float => write!(f, "float"),
            Self::Double => write!(f, "double"),
            Self::Imm32 => write!(f, "imm32"),
            Self::Int => write!(f, "int"),
            Self::Move => write!(f, "move"),
            Self::Mem => write!(f, "mem"),
            Self::Tex => write!(f, "tex"),
            Self::Exec => write!(f, "exec"),
        }
    }
}

pub(super) trait SM20Op {
    fn legalize(&mut self, b: &mut LegalizeBuilder);
    fn encode(&self, e: &mut SM20Encoder<'_>);
}

pub(super) struct SM20Encoder<'a> {
    pub(super) sm: &'a ShaderModel20,
    pub(super) ip: usize,
    pub(super) labels: &'a FxHashMap<Label, usize>,
    pub(super) inst: [u32; 2],
}

impl BitViewable for SM20Encoder<'_> {
    fn bits(&self) -> usize {
        self.inst.bits()
    }

    fn get_bit_range_u64(&self, range: Range<usize>) -> u64 {
        self.inst.get_bit_range_u64(range)
    }
}

impl BitMutViewable for SM20Encoder<'_> {
    fn set_bit_range_u64(&mut self, range: Range<usize>, val: u64) {
        self.inst.set_bit_range_u64(range, val);
    }
}

macro_rules! sm20_op_match {
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
            Op::Flo($x) => $y,
            Op::IAdd2($x) => $y,
            Op::IAdd2X($x) => $y,
            Op::IMad($x) => $y,
            Op::IMul($x) => $y,
            Op::IMnMx($x) => $y,
            Op::ISetP($x) => $y,
            Op::Lop2($x) => $y,
            Op::PopC($x) => $y,
            Op::Shl($x) => $y,
            Op::Shr($x) => $y,
            Op::F2F($x) => $y,
            Op::F2I($x) => $y,
            Op::I2F($x) => $y,
            Op::I2I($x) => $y,
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

impl SM20Op for Op {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        sm20_op_match!(self, |op| op.legalize(b));
    }
    fn encode(&self, e: &mut SM20Encoder<'_>) {
        sm20_op_match!(self, |op| op.encode(e));
    }
}

impl KeplerInstructionEncoder for ShaderModel20 {
    fn encode_instr(
        &self,
        instr: &Instr,
        labels: &FxHashMap<Label, usize>,
        encoded: &mut Vec<u32>,
    ) {
        let mut e = SM20Encoder {
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
        sched_instr.set_field(0..4, 0b0111);
        sched_instr.set_field(60..64, 0b0010);
        new_subset(sched_instr, 4, 56)
    }
}
