// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT

#![allow(clippy::wildcard_imports)]

use bitview::*;

pub use super::builder::{Builder, InstrBuilder, SSABuilder, SSAInstrBuilder};
use super::legalize::LegalizeBuilder;
use super::sm20::ShaderModel20;
use super::sm32::ShaderModel32;
use super::sm50::ShaderModel50;
use super::sm70::ShaderModel70;
use super::sph::{OutputTopology, PixelImap};
pub use super::ssa_value::*;
use coral_reef_stubs::as_slice::*;
use coral_reef_stubs::cfg::CFG;
use coral_reef_stubs::dataflow::ForwardDataflow;
use coral_reef_stubs::smallvec::SmallVec;
use nak_ir_proc::*;
use std::cmp::{max, min};
use std::fmt;

mod types;
pub use types::*;

mod regs;
pub use regs::*;

mod src_dst;
pub use src_dst::*;

use std::fmt::Write;
use std::iter::Zip;
use std::ops::{BitAnd, BitOr, Deref, DerefMut, Index, IndexMut, Not, Range};
use std::slice;

#[derive(Clone, PartialEq)]
pub struct Src {
    pub src_ref: SrcRef,
    pub src_mod: SrcMod,
    pub src_swizzle: SrcSwizzle,
}

impl Src {
    pub const ZERO: Src = Src {
        src_ref: SrcRef::Zero,
        src_mod: SrcMod::None,
        src_swizzle: SrcSwizzle::None,
    };

    pub fn new_imm_u32(u: u32) -> Src {
        u.into()
    }

    pub fn new_imm_bool(b: bool) -> Src {
        b.into()
    }

    pub fn is_unmodified(&self) -> bool {
        self.src_mod.is_none() && self.src_swizzle.is_none()
    }

    pub fn fabs(self) -> Src {
        Src {
            src_ref: self.src_ref,
            src_mod: self.src_mod.fabs(),
            src_swizzle: self.src_swizzle,
        }
    }

    pub fn fneg(self) -> Src {
        Src {
            src_ref: self.src_ref,
            src_mod: self.src_mod.fneg(),
            src_swizzle: self.src_swizzle,
        }
    }

    pub fn ineg(self) -> Src {
        Src {
            src_ref: self.src_ref,
            src_mod: self.src_mod.ineg(),
            src_swizzle: self.src_swizzle,
        }
    }

    pub fn bnot(self) -> Src {
        Src {
            src_ref: self.src_ref,
            src_mod: self.src_mod.bnot(),
            src_swizzle: self.src_swizzle,
        }
    }

    pub fn modify(mut self, src_mod: SrcMod) -> Src {
        self.src_mod = self.src_mod.modify(src_mod);
        self
    }

    pub fn swizzle(mut self, src_swizzle: SrcSwizzle) -> Src {
        // Since we only have xx, yy, and xy, for any composition of swizzles,
        // the inner-most non-xy swizzle wins.
        if matches!(self.src_swizzle, SrcSwizzle::None) {
            self.src_swizzle = src_swizzle;
        }
        self
    }

    pub fn without_swizzle(mut self) -> Src {
        self.src_swizzle = SrcSwizzle::None;
        self
    }

    pub fn as_u32(&self, src_type: SrcType) -> Option<u32> {
        let u = match &self.src_ref {
            SrcRef::Zero => 0,
            SrcRef::Imm32(u) => *u,
            _ => return None,
        };

        if self.is_unmodified() {
            return Some(u);
        }

        assert!(src_type == SrcType::F16v2 || self.src_swizzle.is_none());

        // INeg affects more than just the 32 bits of input data so it can't be
        // trivially folded.  In fact, -imm may not be representable as a 32-bit
        // immediate at all.
        if src_type == SrcType::I32 {
            return None;
        }

        Some(match src_type {
            SrcType::F16 => {
                let low = u & 0xFFFF;

                match self.src_mod {
                    SrcMod::None => low,
                    SrcMod::FAbs => low & !(1_u32 << 15),
                    SrcMod::FNeg => low ^ (1_u32 << 15),
                    SrcMod::FNegAbs => low | (1_u32 << 15),
                    _ => panic!("Not a float source modifier"),
                }
            }
            SrcType::F16v2 => {
                let u = match self.src_swizzle {
                    SrcSwizzle::None => u,
                    SrcSwizzle::Xx => (u << 16) | (u & 0xffff),
                    SrcSwizzle::Yy => (u & 0xffff_0000) | (u >> 16),
                };

                match self.src_mod {
                    SrcMod::None => u,
                    SrcMod::FAbs => u & 0x7FFF_7FFF,
                    SrcMod::FNeg => u ^ 0x8000_8000,
                    SrcMod::FNegAbs => u | 0x8000_8000,
                    _ => panic!("Not a float source modifier"),
                }
            }
            SrcType::F32 | SrcType::F64 => match self.src_mod {
                SrcMod::None => u,
                SrcMod::FAbs => u & !(1_u32 << 31),
                SrcMod::FNeg => u ^ (1_u32 << 31),
                SrcMod::FNegAbs => u | (1_u32 << 31),
                _ => panic!("Not a float source modifier"),
            },
            SrcType::I32 => match self.src_mod {
                SrcMod::None => u,
                SrcMod::INeg => -(u as i32) as u32,
                _ => panic!("Not an integer source modifier"),
            },
            SrcType::B32 => match self.src_mod {
                SrcMod::None => u,
                SrcMod::BNot => !u,
                _ => panic!("Not a bitwise source modifier"),
            },
            _ => {
                assert!(self.is_unmodified());
                u
            }
        })
    }

    pub fn as_ssa(&self) -> Option<&SSARef> {
        if self.is_unmodified() {
            self.src_ref.as_ssa()
        } else {
            None
        }
    }

    pub fn to_ssa(self) -> SSARef {
        if self.is_unmodified() {
            self.src_ref.to_ssa()
        } else {
            panic!("Did not expect src_mod");
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match &self.src_ref {
            SrcRef::True => Some(!self.src_mod.is_bnot()),
            SrcRef::False => Some(self.src_mod.is_bnot()),
            SrcRef::SSA(vec) => {
                assert!(vec.is_predicate() && vec.comps() == 1);
                None
            }
            SrcRef::Reg(reg) => {
                assert!(reg.is_predicate() && reg.comps() == 1);
                None
            }
            _ => panic!("Not a boolean source"),
        }
    }

    pub fn as_imm_not_i20(&self) -> Option<u32> {
        match self.src_ref {
            SrcRef::Imm32(i) => {
                assert!(self.is_unmodified());
                let top = i & 0xfff8_0000;
                if top == 0 || top == 0xfff8_0000 {
                    None
                } else {
                    Some(i)
                }
            }
            _ => None,
        }
    }

    pub fn as_imm_not_f20(&self) -> Option<u32> {
        match self.src_ref {
            SrcRef::Imm32(i) => {
                assert!(self.is_unmodified());
                if (i & 0xfff) == 0 { None } else { Some(i) }
            }
            _ => None,
        }
    }

    pub fn iter_ssa(&self) -> slice::Iter<'_, SSAValue> {
        self.src_ref.iter_ssa()
    }

    pub fn iter_ssa_mut(&mut self) -> slice::IterMut<'_, SSAValue> {
        self.src_ref.iter_ssa_mut()
    }

    pub fn is_uniform(&self) -> bool {
        match &self.src_ref {
            SrcRef::Zero | SrcRef::True | SrcRef::False | SrcRef::Imm32(_) | SrcRef::CBuf(_) => {
                true
            }
            SrcRef::SSA(ssa) => ssa.is_uniform(),
            SrcRef::Reg(reg) => reg.is_uniform(),
        }
    }

    pub fn is_bindless_cbuf(&self) -> bool {
        self.src_ref.is_bindless_cbuf()
    }

    pub fn is_upred_reg(&self) -> bool {
        match &self.src_ref {
            SrcRef::SSA(ssa) => ssa.file() == RegFile::UPred,
            SrcRef::Reg(reg) => reg.file() == RegFile::UPred,
            _ => false,
        }
    }

    pub fn is_predicate(&self) -> bool {
        self.src_ref.is_predicate()
    }

    pub fn is_zero(&self) -> bool {
        match self.src_ref {
            SrcRef::Zero | SrcRef::Imm32(0) => match self.src_mod {
                SrcMod::None | SrcMod::FAbs => true,
                // INeg affects more than just the 32 bits of input data so -0
                // may not be equivalent to 0.
                SrcMod::FNeg | SrcMod::FNegAbs | SrcMod::BNot | SrcMod::INeg => false,
            },
            _ => false,
        }
    }

    pub fn is_nonzero(&self) -> bool {
        assert!(self.is_unmodified());
        matches!(self.src_ref, SrcRef::Imm32(x) if x != 0)
    }

    pub fn is_true(&self) -> bool {
        self.as_bool() == Some(true)
    }

    pub fn is_fneg_zero(&self, src_type: SrcType) -> bool {
        match self.as_u32(src_type) {
            Some(0x0000_8000) => src_type == SrcType::F16,
            Some(0x8000_0000) => src_type == SrcType::F32 || src_type == SrcType::F64,
            Some(0x8000_8000) => src_type == SrcType::F16v2,
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub fn supports_type(&self, src_type: &SrcType) -> bool {
        match src_type {
            SrcType::SSA => {
                if !self.is_unmodified() {
                    return false;
                }

                matches!(self.src_ref, SrcRef::SSA(_) | SrcRef::Reg(_))
            }
            SrcType::GPR => {
                if !self.is_unmodified() {
                    return false;
                }

                matches!(self.src_ref, SrcRef::Zero | SrcRef::SSA(_) | SrcRef::Reg(_))
            }
            SrcType::ALU => self.is_unmodified() && self.src_ref.is_alu(),
            SrcType::F16 | SrcType::F32 | SrcType::F64 | SrcType::F16v2 => {
                match self.src_mod {
                    SrcMod::None | SrcMod::FAbs | SrcMod::FNeg | SrcMod::FNegAbs => (),
                    _ => return false,
                }

                self.src_ref.is_alu()
            }
            SrcType::I32 => {
                match self.src_mod {
                    SrcMod::None | SrcMod::INeg => (),
                    _ => return false,
                }

                self.src_ref.is_alu()
            }
            SrcType::B32 => {
                match self.src_mod {
                    SrcMod::None | SrcMod::BNot => (),
                    _ => return false,
                }

                self.src_ref.is_alu()
            }
            SrcType::Pred => {
                match self.src_mod {
                    SrcMod::None | SrcMod::BNot => (),
                    _ => return false,
                }

                self.src_ref.is_predicate()
            }
            SrcType::Carry => self.is_unmodified() && self.src_ref.is_carry(),
            SrcType::Bar => self.is_unmodified() && self.src_ref.is_barrier(),
        }
    }
}

impl<T: Into<SrcRef>> From<T> for Src {
    fn from(value: T) -> Src {
        Src {
            src_ref: value.into(),
            src_mod: SrcMod::None,
            src_swizzle: SrcSwizzle::None,
        }
    }
}

impl From<Pred> for Src {
    fn from(value: Pred) -> Self {
        Src {
            src_ref: value.pred_ref.into(),
            src_mod: if value.pred_inv {
                SrcMod::BNot
            } else {
                SrcMod::None
            },
            src_swizzle: SrcSwizzle::None,
        }
    }
}

impl fmt::Display for Src {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.src_mod {
            SrcMod::None => write!(f, "{}{}", self.src_ref, self.src_swizzle),
            SrcMod::FAbs => write!(f, "|{}{}|", self.src_ref, self.src_swizzle),
            SrcMod::FNeg | SrcMod::INeg => write!(f, "-{}{}", self.src_ref, self.src_swizzle),
            SrcMod::FNegAbs => {
                write!(f, "-|{}{}|", self.src_ref, self.src_swizzle)
            }
            SrcMod::BNot => write!(f, "!{}{}", self.src_ref, self.src_swizzle),
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SrcType {
    SSA,
    GPR,
    ALU,
    F16,
    F16v2,
    F32,
    F64,
    I32,
    B32,
    Pred,
    Carry,
    Bar,
}

impl SrcType {
    const DEFAULT: SrcType = SrcType::GPR;
}

pub type SrcTypeList = AttrList<SrcType>;

pub trait SrcsAsSlice: AsSlice<Src, Attr = SrcType> {
    fn srcs_as_slice(&self) -> &[Src] {
        self.as_slice()
    }

    fn srcs_as_mut_slice(&mut self) -> &mut [Src] {
        self.as_mut_slice()
    }

    fn src_types(&self) -> SrcTypeList {
        self.attrs()
    }

    fn src_idx(&self, src: &Src) -> usize {
        let slice = self.srcs_as_slice();
        let base = slice.as_ptr() as usize;
        let elem = std::ptr::from_ref(src) as usize;
        let idx = (elem - base) / std::mem::size_of::<Src>();
        assert!(idx < slice.len(), "src not in slice");
        idx
    }
}

impl<T: AsSlice<Src, Attr = SrcType>> SrcsAsSlice for T {}

fn all_dsts_uniform(dsts: &[Dst]) -> bool {
    let mut uniform = None;
    for dst in dsts {
        let dst_uniform = match dst {
            Dst::None => continue,
            Dst::Reg(r) => r.is_uniform(),
            Dst::SSA(r) => r.file().is_uniform(),
        };
        assert!(uniform.is_none() || uniform == Some(dst_uniform));
        uniform = Some(dst_uniform);
    }
    uniform == Some(true)
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DstType {
    Pred,
    GPR,
    F16,
    F16v2,
    F32,
    F64,
    Carry,
    Bar,
    Vec,
}

impl DstType {
    const DEFAULT: DstType = DstType::Vec;
}

pub type DstTypeList = AttrList<DstType>;

pub trait DstsAsSlice: AsSlice<Dst, Attr = DstType> {
    fn dsts_as_slice(&self) -> &[Dst] {
        self.as_slice()
    }

    fn dsts_as_mut_slice(&mut self) -> &mut [Dst] {
        self.as_mut_slice()
    }

    // Currently only used by test code
    #[allow(dead_code)]
    fn dst_types(&self) -> DstTypeList {
        self.attrs()
    }

    fn dst_idx(&self, dst: &Dst) -> usize {
        let slice = self.dsts_as_slice();
        let base = slice.as_ptr() as usize;
        let elem = std::ptr::from_ref(dst) as usize;
        let idx = (elem - base) / std::mem::size_of::<Dst>();
        assert!(idx < slice.len(), "dst not in slice");
        idx
    }
}

impl<T: AsSlice<Dst, Attr = DstType>> DstsAsSlice for T {}

pub trait IsUniform {
    fn is_uniform(&self) -> bool;
}

impl<T: DstsAsSlice> IsUniform for T {
    fn is_uniform(&self) -> bool {
        all_dsts_uniform(self.dsts_as_slice())
    }
}

fn fmt_dst_slice(f: &mut fmt::Formatter<'_>, dsts: &[Dst]) -> fmt::Result {
    if dsts.is_empty() {
        return Ok(());
    }

    // Figure out the last non-null dst
    //
    // Note: By making the top inclusive and starting at 0, we ensure that
    // at least one dst always gets printed.
    let mut last_dst = 0;
    for (i, dst) in dsts.iter().enumerate() {
        if !dst.is_none() {
            last_dst = i;
        }
    }

    for i in 0..(last_dst + 1) {
        if i != 0 {
            write!(f, " ")?;
        }
        write!(f, "{}", &dsts[i])?;
    }
    Ok(())
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum FoldData {
    Pred(bool),
    Carry(bool),
    U32(u32),
    Vec2([u32; 2]),
}

pub struct OpFoldData<'a> {
    pub dsts: &'a mut [FoldData],
    pub srcs: &'a [FoldData],
}

impl OpFoldData<'_> {
    #[allow(dead_code)]
    pub fn get_pred_src(&self, op: &impl SrcsAsSlice, src: &Src) -> bool {
        let i = op.src_idx(src);
        let b = match src.src_ref {
            SrcRef::Zero | SrcRef::Imm32(_) => panic!("Expected a predicate"),
            SrcRef::True => true,
            SrcRef::False => false,
            _ => {
                if let FoldData::Pred(b) = self.srcs[i] {
                    b
                } else {
                    panic!("FoldData is not a predicate");
                }
            }
        };
        b ^ src.src_mod.is_bnot()
    }

    pub fn get_u32_src(&self, op: &impl SrcsAsSlice, src: &Src) -> u32 {
        let i = op.src_idx(src);
        match src.src_ref {
            SrcRef::Zero => 0,
            SrcRef::Imm32(imm) => imm,
            SrcRef::True | SrcRef::False => panic!("Unexpected predicate"),
            _ => {
                if let FoldData::U32(u) = self.srcs[i] {
                    u
                } else {
                    panic!("FoldData is not a U32");
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn get_u32_bnot_src(&self, op: &impl SrcsAsSlice, src: &Src) -> u32 {
        let x = self.get_u32_src(op, src);
        if src.src_mod.is_bnot() { !x } else { x }
    }

    #[allow(dead_code)]
    pub fn get_carry_src(&self, op: &impl SrcsAsSlice, src: &Src) -> bool {
        assert!(src.src_ref.as_ssa().is_some());
        let i = op.src_idx(src);
        if let FoldData::Carry(b) = self.srcs[i] {
            b
        } else {
            panic!("FoldData is not a predicate");
        }
    }

    #[allow(dead_code)]
    pub fn get_f32_src(&self, op: &impl SrcsAsSlice, src: &Src) -> f32 {
        f32::from_bits(self.get_u32_src(op, src))
    }

    #[allow(dead_code)]
    pub fn get_f64_src(&self, op: &impl SrcsAsSlice, src: &Src) -> f64 {
        let i = op.src_idx(src);
        match src.src_ref {
            SrcRef::Zero => 0.0,
            SrcRef::Imm32(imm) => f64::from_bits(u64::from(imm) << 32),
            SrcRef::True | SrcRef::False => panic!("Unexpected predicate"),
            _ => {
                if let FoldData::Vec2(v) = self.srcs[i] {
                    let u = u64::from(v[0]) | (u64::from(v[1]) << 32);
                    f64::from_bits(u)
                } else {
                    panic!("FoldData is not a U32");
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn set_pred_dst(&mut self, op: &impl DstsAsSlice, dst: &Dst, b: bool) {
        self.dsts[op.dst_idx(dst)] = FoldData::Pred(b);
    }

    #[allow(dead_code)]
    pub fn set_carry_dst(&mut self, op: &impl DstsAsSlice, dst: &Dst, b: bool) {
        self.dsts[op.dst_idx(dst)] = FoldData::Carry(b);
    }

    pub fn set_u32_dst(&mut self, op: &impl DstsAsSlice, dst: &Dst, u: u32) {
        self.dsts[op.dst_idx(dst)] = FoldData::U32(u);
    }

    #[allow(dead_code)]
    pub fn set_f32_dst(&mut self, op: &impl DstsAsSlice, dst: &Dst, f: f32) {
        self.set_u32_dst(op, dst, f.to_bits());
    }

    #[allow(dead_code)]
    pub fn set_f64_dst(&mut self, op: &impl DstsAsSlice, dst: &Dst, f: f64) {
        let u = f.to_bits();
        let v = [u as u32, (u >> 32) as u32];
        self.dsts[op.dst_idx(dst)] = FoldData::Vec2(v);
    }
}

pub trait Foldable: SrcsAsSlice + DstsAsSlice {
    // Currently only used by test code
    #[allow(dead_code)]
    fn fold(&self, sm: &ShaderModelInfo, f: &mut OpFoldData<'_>);
}

pub trait DisplayOp: DstsAsSlice {
    fn fmt_dsts(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_dst_slice(f, self.dsts_as_slice())
    }

    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

// Hack struct so we can re-use Formatters.  Shamelessly stolen from
// https://users.rust-lang.org/t/reusing-an-fmt-formatter/8531/4
pub struct Fmt<F>(pub F)
where
    F: Fn(&mut fmt::Formatter) -> fmt::Result;

impl<F> fmt::Display for Fmt<F>
where
    F: Fn(&mut fmt::Formatter) -> fmt::Result,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        (self.0)(f)
    }
}

macro_rules! impl_display_for_op {
    ($op: ident) => {
        impl fmt::Display for $op {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut s = String::new();
                write!(s, "{}", Fmt(|f| self.fmt_dsts(f)))?;
                if !s.is_empty() {
                    write!(f, "{} = ", s)?;
                }
                self.fmt_op(f)
            }
        }
    };
}

mod op_float;
pub use op_float::*;

mod op_int;
pub use op_int::*;

mod op_conv;
pub use op_conv::*;

mod op_tex;
pub use op_tex::*;

mod op_mem;
pub use op_mem::*;

mod op_cf;
pub use op_cf::*;

mod op_misc;
pub use op_misc::*;

mod op;
pub use op::*;

mod instr;
pub use instr::*;

mod program;
pub use program::*;

mod shader_info;
pub use shader_info::*;

// Op enum and impls moved to op.rs

// InstrDeps, Instr, MappedInstrs moved to instr.rs

// BasicBlock, InstrIdx, Function moved to program.rs

// (Op impl removed - now in op.rs)

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum PredRef {
    None,
    SSA(SSAValue),
    Reg(RegRef),
}

impl PredRef {
    #[allow(dead_code)]
    pub fn as_reg(&self) -> Option<&RegRef> {
        match self {
            PredRef::Reg(r) => Some(r),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_ssa(&self) -> Option<&SSAValue> {
        match self {
            PredRef::SSA(r) => Some(r),
            _ => None,
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, PredRef::None)
    }

    pub fn iter_ssa(&self) -> slice::Iter<'_, SSAValue> {
        match self {
            PredRef::None | PredRef::Reg(_) => &[],
            PredRef::SSA(ssa) => slice::from_ref(ssa),
        }
        .iter()
    }

    pub fn iter_ssa_mut(&mut self) -> slice::IterMut<'_, SSAValue> {
        match self {
            PredRef::None | PredRef::Reg(_) => &mut [],
            PredRef::SSA(ssa) => slice::from_mut(ssa),
        }
        .iter_mut()
    }
}

impl From<RegRef> for PredRef {
    fn from(reg: RegRef) -> PredRef {
        PredRef::Reg(reg)
    }
}

impl From<SSAValue> for PredRef {
    fn from(ssa: SSAValue) -> PredRef {
        PredRef::SSA(ssa)
    }
}

impl fmt::Display for PredRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PredRef::None => write!(f, "pT"),
            PredRef::SSA(ssa) => ssa.fmt(f),
            PredRef::Reg(reg) => reg.fmt(f),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Pred {
    pub pred_ref: PredRef,
    pub pred_inv: bool,
}

impl Pred {
    pub fn is_true(&self) -> bool {
        self.pred_ref.is_none() && !self.pred_inv
    }

    pub fn is_false(&self) -> bool {
        self.pred_ref.is_none() && self.pred_inv
    }

    pub fn iter_ssa(&self) -> slice::Iter<'_, SSAValue> {
        self.pred_ref.iter_ssa()
    }

    pub fn iter_ssa_mut(&mut self) -> slice::IterMut<'_, SSAValue> {
        self.pred_ref.iter_ssa_mut()
    }

    pub fn bnot(self) -> Self {
        Pred {
            pred_ref: self.pred_ref,
            pred_inv: !self.pred_inv,
        }
    }
}

impl From<bool> for Pred {
    fn from(b: bool) -> Self {
        Pred {
            pred_ref: PredRef::None,
            pred_inv: !b,
        }
    }
}

impl<T: Into<PredRef>> From<T> for Pred {
    fn from(p: T) -> Self {
        Pred {
            pred_ref: p.into(),
            pred_inv: false,
        }
    }
}

impl fmt::Display for Pred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.pred_inv {
            write!(f, "!")?;
        }
        self.pred_ref.fmt(f)
    }
}

// MIN_INSTR_DELAY, InstrDeps, Instr, MappedInstrs moved to instr.rs
// BasicBlock, InstrIdx, Function moved to program.rs

// (InstrDeps, Instr, BasicBlock, Function, InstrIdx - see instr.rs and program.rs)
