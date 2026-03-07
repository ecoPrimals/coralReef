// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
//! Source operand type, modifiers, type classification, and slice traits.

use std::fmt;
use std::slice;

use coral_reef_stubs::as_slice::*;

use super::pred::Pred;
use super::regs::{HasRegFile, RegFile};
use super::src_dst::{SrcMod, SrcRef, SrcSwizzle};
use super::{SSARef, SSAValue};

#[derive(Clone, PartialEq)]
pub struct Src {
    pub reference: SrcRef,
    pub modifier: SrcMod,
    pub swizzle: SrcSwizzle,
}

impl Src {
    pub const ZERO: Self = Self {
        reference: SrcRef::Zero,
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    };

    pub fn new_imm_u32(u: u32) -> Self {
        u.into()
    }

    pub fn new_imm_bool(b: bool) -> Self {
        b.into()
    }

    pub fn is_unmodified(&self) -> bool {
        self.modifier.is_none() && self.swizzle.is_none()
    }

    pub fn fabs(self) -> Self {
        Self {
            reference: self.reference,
            modifier: self.modifier.fabs(),
            swizzle: self.swizzle,
        }
    }

    pub fn fneg(self) -> Self {
        Self {
            reference: self.reference,
            modifier: self.modifier.fneg(),
            swizzle: self.swizzle,
        }
    }

    pub fn ineg(self) -> Self {
        Self {
            reference: self.reference,
            modifier: self.modifier.ineg(),
            swizzle: self.swizzle,
        }
    }

    pub fn bnot(self) -> Self {
        Self {
            reference: self.reference,
            modifier: self.modifier.bnot(),
            swizzle: self.swizzle,
        }
    }

    pub fn modify(mut self, modifier: SrcMod) -> Self {
        self.modifier = self.modifier.modify(modifier);
        self
    }

    pub const fn swizzle(mut self, swizzle: SrcSwizzle) -> Self {
        // Since we only have xx, yy, and xy, for any composition of swizzles,
        // the inner-most non-xy swizzle wins.
        if matches!(self.swizzle, SrcSwizzle::None) {
            self.swizzle = swizzle;
        }
        self
    }

    pub const fn without_swizzle(mut self) -> Self {
        self.swizzle = SrcSwizzle::None;
        self
    }

    pub fn as_u32(&self, src_type: SrcType) -> Option<u32> {
        let u = match &self.reference {
            SrcRef::Zero => 0,
            SrcRef::Imm32(u) => *u,
            _ => return None,
        };

        if self.is_unmodified() {
            return Some(u);
        }

        assert!(src_type == SrcType::F16v2 || self.swizzle.is_none());

        // INeg affects more than just the 32 bits of input data so it can't be
        // trivially folded.  In fact, -imm may not be representable as a 32-bit
        // immediate at all.
        if src_type == SrcType::I32 {
            return None;
        }

        Some(match src_type {
            SrcType::F16 => {
                let low = u & 0xFFFF;

                match self.modifier {
                    SrcMod::None => low,
                    SrcMod::FAbs => low & !(1_u32 << 15),
                    SrcMod::FNeg => low ^ (1_u32 << 15),
                    SrcMod::FNegAbs => low | (1_u32 << 15),
                    _ => panic!("Not a float source modifier"),
                }
            }
            SrcType::F16v2 => {
                let u = match self.swizzle {
                    SrcSwizzle::None => u,
                    SrcSwizzle::Xx => (u << 16) | (u & 0xffff),
                    SrcSwizzle::Yy => (u & 0xffff_0000) | (u >> 16),
                };

                match self.modifier {
                    SrcMod::None => u,
                    SrcMod::FAbs => u & 0x7FFF_7FFF,
                    SrcMod::FNeg => u ^ 0x8000_8000,
                    SrcMod::FNegAbs => u | 0x8000_8000,
                    _ => panic!("Not a float source modifier"),
                }
            }
            SrcType::F32 | SrcType::F64 => match self.modifier {
                SrcMod::None => u,
                SrcMod::FAbs => u & !(1_u32 << 31),
                SrcMod::FNeg => u ^ (1_u32 << 31),
                SrcMod::FNegAbs => u | (1_u32 << 31),
                _ => panic!("Not a float source modifier"),
            },
            SrcType::I32 => match self.modifier {
                SrcMod::None => u,
                SrcMod::INeg => -(u as i32) as u32,
                _ => panic!("Not an integer source modifier"),
            },
            SrcType::B32 => match self.modifier {
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
            self.reference.as_ssa()
        } else {
            None
        }
    }

    pub fn to_ssa(self) -> SSARef {
        if self.is_unmodified() {
            self.reference.to_ssa()
        } else {
            panic!("Did not expect modifier");
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match &self.reference {
            SrcRef::True => Some(!self.modifier.is_bnot()),
            SrcRef::False => Some(self.modifier.is_bnot()),
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
        match self.reference {
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
        match self.reference {
            SrcRef::Imm32(i) => {
                assert!(self.is_unmodified());
                if (i & 0xfff) == 0 { None } else { Some(i) }
            }
            _ => None,
        }
    }

    pub fn iter_ssa(&self) -> slice::Iter<'_, SSAValue> {
        self.reference.iter_ssa()
    }

    pub fn iter_ssa_mut(&mut self) -> slice::IterMut<'_, SSAValue> {
        self.reference.iter_ssa_mut()
    }

    pub fn is_uniform(&self) -> bool {
        match &self.reference {
            SrcRef::Zero | SrcRef::True | SrcRef::False | SrcRef::Imm32(_) | SrcRef::CBuf(_) => {
                true
            }
            SrcRef::SSA(ssa) => ssa.is_uniform(),
            SrcRef::Reg(reg) => reg.is_uniform(),
        }
    }

    pub const fn is_bindless_cbuf(&self) -> bool {
        self.reference.is_bindless_cbuf()
    }

    pub fn is_upred_reg(&self) -> bool {
        match &self.reference {
            SrcRef::SSA(ssa) => ssa.file() == RegFile::UPred,
            SrcRef::Reg(reg) => reg.file() == RegFile::UPred,
            _ => false,
        }
    }

    pub fn is_predicate(&self) -> bool {
        self.reference.is_predicate()
    }

    pub const fn is_zero(&self) -> bool {
        match self.reference {
            SrcRef::Zero | SrcRef::Imm32(0) => match self.modifier {
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
        matches!(self.reference, SrcRef::Imm32(x) if x != 0)
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

                matches!(self.reference, SrcRef::SSA(_) | SrcRef::Reg(_))
            }
            SrcType::GPR => {
                if !self.is_unmodified() {
                    return false;
                }

                matches!(
                    self.reference,
                    SrcRef::Zero | SrcRef::SSA(_) | SrcRef::Reg(_)
                )
            }
            SrcType::ALU => self.is_unmodified() && self.reference.is_alu(),
            SrcType::F16 | SrcType::F32 | SrcType::F64 | SrcType::F16v2 => {
                match self.modifier {
                    SrcMod::None | SrcMod::FAbs | SrcMod::FNeg | SrcMod::FNegAbs => (),
                    _ => return false,
                }

                self.reference.is_alu()
            }
            SrcType::I32 => {
                match self.modifier {
                    SrcMod::None | SrcMod::INeg => (),
                    _ => return false,
                }

                self.reference.is_alu()
            }
            SrcType::B32 => {
                match self.modifier {
                    SrcMod::None | SrcMod::BNot => (),
                    _ => return false,
                }

                self.reference.is_alu()
            }
            SrcType::Pred => {
                match self.modifier {
                    SrcMod::None | SrcMod::BNot => (),
                    _ => return false,
                }

                self.reference.is_predicate()
            }
            SrcType::Carry => self.is_unmodified() && self.reference.is_carry(),
            SrcType::Bar => self.is_unmodified() && self.reference.is_barrier(),
        }
    }
}

impl<T: Into<SrcRef>> From<T> for Src {
    fn from(value: T) -> Self {
        Self {
            reference: value.into(),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        }
    }
}

impl From<Pred> for Src {
    fn from(value: Pred) -> Self {
        Self {
            reference: value.predicate.into(),
            modifier: if value.inverted {
                SrcMod::BNot
            } else {
                SrcMod::None
            },
            swizzle: SrcSwizzle::None,
        }
    }
}

impl fmt::Display for Src {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.modifier {
            SrcMod::None => write!(f, "{}{}", self.reference, self.swizzle),
            SrcMod::FAbs => write!(f, "|{}{}|", self.reference, self.swizzle),
            SrcMod::FNeg | SrcMod::INeg => write!(f, "-{}{}", self.reference, self.swizzle),
            SrcMod::FNegAbs => {
                write!(f, "-|{}{}|", self.reference, self.swizzle)
            }
            SrcMod::BNot => write!(f, "!{}{}", self.reference, self.swizzle),
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
    pub(crate) const DEFAULT: Self = Self::GPR;
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
