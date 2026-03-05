// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Source and destination types: `Dst`, `Src`, `SrcRef`, `SrcMod`, `CBuf`, `CBufRef`.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::regs::*;
use super::{PredRef, PrmtSel};
use crate::nak::ssa_value::*;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::slice;

#[derive(Clone)]
pub enum Dst {
    None,
    SSA(SSARef),
    Reg(RegRef),
}

impl Dst {
    pub fn is_none(&self) -> bool {
        matches!(self, Dst::None)
    }

    pub fn as_reg(&self) -> Option<&RegRef> {
        match self {
            Dst::Reg(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_ssa(&self) -> Option<&SSARef> {
        match self {
            Dst::SSA(r) => Some(r),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn to_ssa(self) -> SSARef {
        match self {
            Dst::SSA(r) => r,
            _ => panic!("Expected ssa"),
        }
    }

    pub fn iter_ssa(&self) -> slice::Iter<'_, SSAValue> {
        match self {
            Dst::None | Dst::Reg(_) => &[],
            Dst::SSA(ssa) => ssa.deref(),
        }
        .iter()
    }

    pub fn iter_ssa_mut(&mut self) -> slice::IterMut<'_, SSAValue> {
        match self {
            Dst::None | Dst::Reg(_) => &mut [],
            Dst::SSA(ssa) => ssa.deref_mut(),
        }
        .iter_mut()
    }

    pub fn comps(&self) -> u8 {
        match self {
            Dst::None => 0,
            Dst::SSA(ssa) => ssa.comps(),
            Dst::Reg(reg) => reg.comps(),
        }
    }

    pub fn file(&self) -> Option<RegFile> {
        match self {
            Dst::None => None,
            Dst::SSA(ssa) => Some(ssa.file()),
            Dst::Reg(reg) => Some(reg.file()),
        }
    }
}

impl From<RegRef> for Dst {
    fn from(reg: RegRef) -> Dst {
        Dst::Reg(reg)
    }
}

impl<T: Into<SSARef>> From<T> for Dst {
    fn from(ssa: T) -> Dst {
        Dst::SSA(ssa.into())
    }
}

impl From<Option<SSAValue>> for Dst {
    fn from(ssa: Option<SSAValue>) -> Dst {
        match ssa {
            Some(ssa) => Dst::SSA(ssa.into()),
            None => Dst::None,
        }
    }
}

impl fmt::Display for Dst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Dst::None => write!(f, "null")?,
            Dst::SSA(v) => v.fmt(f)?,
            Dst::Reg(r) => r.fmt(f)?,
        }
        Ok(())
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub enum CBuf {
    Binding(u8),
    BindlessSSA([SSAValue; 2]),
    BindlessUGPR(RegRef),
}

impl fmt::Display for CBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CBuf::Binding(idx) => write!(f, "c[{idx:#x}]"),
            CBuf::BindlessSSA(v) => write!(f, "cx[{{{}, {}}}]", v[0], v[1]),
            CBuf::BindlessUGPR(r) => write!(f, "cx[{}]", r),
        }
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct CBufRef {
    pub buf: CBuf,
    pub offset: u16,
}

impl CBufRef {
    pub fn offset(self, offset: u16) -> CBufRef {
        CBufRef {
            buf: self.buf,
            offset: self.offset + offset,
        }
    }
}

impl fmt::Display for CBufRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{:#x}]", self.buf, self.offset)
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub enum SrcRef {
    Zero,
    True,
    False,
    Imm32(u32),
    CBuf(CBufRef),
    SSA(SSARef),
    Reg(RegRef),
}

impl SrcRef {
    pub fn is_alu(&self) -> bool {
        match self {
            SrcRef::Zero | SrcRef::Imm32(_) | SrcRef::CBuf(_) => true,
            SrcRef::SSA(ssa) => ssa.is_gpr(),
            SrcRef::Reg(reg) => reg.is_gpr(),
            SrcRef::True | SrcRef::False => false,
        }
    }

    pub fn is_bindless_cbuf(&self) -> bool {
        match self {
            SrcRef::CBuf(cbuf) => {
                matches!(cbuf.buf, CBuf::BindlessSSA(_) | CBuf::BindlessUGPR(_))
            }
            _ => false,
        }
    }

    pub fn is_predicate(&self) -> bool {
        match self {
            SrcRef::Zero | SrcRef::Imm32(_) | SrcRef::CBuf(_) => false,
            SrcRef::True | SrcRef::False => true,
            SrcRef::SSA(ssa) => ssa.is_predicate(),
            SrcRef::Reg(reg) => reg.is_predicate(),
        }
    }

    pub fn is_carry(&self) -> bool {
        match self {
            SrcRef::SSA(ssa) => ssa.file() == RegFile::Carry,
            SrcRef::Reg(reg) => reg.file() == RegFile::Carry,
            _ => false,
        }
    }

    pub fn is_barrier(&self) -> bool {
        match self {
            SrcRef::SSA(ssa) => ssa.file() == RegFile::Bar,
            SrcRef::Reg(reg) => reg.file() == RegFile::Bar,
            _ => false,
        }
    }

    pub fn as_reg(&self) -> Option<&RegRef> {
        match self {
            SrcRef::Reg(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_ssa(&self) -> Option<&SSARef> {
        match self {
            SrcRef::SSA(r) => Some(r),
            _ => None,
        }
    }

    pub fn to_ssa(self) -> SSARef {
        match self {
            SrcRef::SSA(r) => r,
            _ => panic!(),
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            SrcRef::Zero => Some(0),
            SrcRef::Imm32(u) => Some(*u),
            SrcRef::CBuf(_) | SrcRef::SSA(_) | SrcRef::Reg(_) => None,
            _ => panic!("Invalid integer source"),
        }
    }

    pub fn get_reg(&self) -> Option<&RegRef> {
        match self {
            SrcRef::Zero | SrcRef::True | SrcRef::False | SrcRef::Imm32(_) | SrcRef::SSA(_) => None,
            SrcRef::CBuf(cb) => match &cb.buf {
                CBuf::Binding(_) | CBuf::BindlessSSA(_) => None,
                CBuf::BindlessUGPR(reg) => Some(reg),
            },
            SrcRef::Reg(reg) => Some(reg),
        }
    }

    pub fn iter_ssa(&self) -> slice::Iter<'_, SSAValue> {
        match self {
            SrcRef::Zero | SrcRef::True | SrcRef::False | SrcRef::Imm32(_) | SrcRef::Reg(_) => &[],
            SrcRef::CBuf(cb) => match &cb.buf {
                CBuf::Binding(_) | CBuf::BindlessUGPR(_) => &[],
                CBuf::BindlessSSA(ssa) => &ssa[..],
            },
            SrcRef::SSA(ssa) => ssa.deref(),
        }
        .iter()
    }

    pub fn iter_ssa_mut(&mut self) -> slice::IterMut<'_, SSAValue> {
        match self {
            SrcRef::Zero | SrcRef::True | SrcRef::False | SrcRef::Imm32(_) | SrcRef::Reg(_) => {
                &mut []
            }
            SrcRef::CBuf(cb) => match &mut cb.buf {
                CBuf::Binding(_) | CBuf::BindlessUGPR(_) => &mut [],
                CBuf::BindlessSSA(ssa) => &mut ssa[..],
            },
            SrcRef::SSA(ssa) => ssa.deref_mut(),
        }
        .iter_mut()
    }
}

impl From<bool> for SrcRef {
    fn from(b: bool) -> SrcRef {
        if b { SrcRef::True } else { SrcRef::False }
    }
}

impl From<u32> for SrcRef {
    fn from(u: u32) -> SrcRef {
        if u == 0 {
            SrcRef::Zero
        } else {
            SrcRef::Imm32(u)
        }
    }
}

impl From<f32> for SrcRef {
    fn from(f: f32) -> SrcRef {
        f.to_bits().into()
    }
}

impl From<PrmtSel> for SrcRef {
    fn from(sel: PrmtSel) -> SrcRef {
        u32::from(sel.0).into()
    }
}

impl From<CBufRef> for SrcRef {
    fn from(cb: CBufRef) -> SrcRef {
        SrcRef::CBuf(cb)
    }
}

impl From<RegRef> for SrcRef {
    fn from(reg: RegRef) -> SrcRef {
        SrcRef::Reg(reg)
    }
}

impl<T: Into<SSARef>> From<T> for SrcRef {
    fn from(ssa: T) -> SrcRef {
        SrcRef::SSA(ssa.into())
    }
}

impl From<PredRef> for SrcRef {
    fn from(value: PredRef) -> Self {
        match value {
            PredRef::None => SrcRef::True,
            PredRef::Reg(reg) => SrcRef::Reg(reg),
            PredRef::SSA(ssa) => SrcRef::SSA(ssa.into()),
        }
    }
}

impl fmt::Display for SrcRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SrcRef::Zero => write!(f, "rZ"),
            SrcRef::True => write!(f, "pT"),
            SrcRef::False => write!(f, "pF"),
            SrcRef::Imm32(u) => write!(f, "{u:#x}"),
            SrcRef::CBuf(c) => c.fmt(f),
            SrcRef::SSA(v) => v.fmt(f),
            SrcRef::Reg(r) => r.fmt(f),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SrcMod {
    None,
    FAbs,
    FNeg,
    FNegAbs,
    INeg,
    BNot,
}

impl SrcMod {
    pub fn is_none(&self) -> bool {
        matches!(self, SrcMod::None)
    }

    pub fn has_fabs(&self) -> bool {
        match self {
            SrcMod::None | SrcMod::FNeg => false,
            SrcMod::FAbs | SrcMod::FNegAbs => true,
            _ => panic!("Not a float modifier"),
        }
    }

    pub fn has_fneg(&self) -> bool {
        match self {
            SrcMod::None | SrcMod::FAbs => false,
            SrcMod::FNeg | SrcMod::FNegAbs => true,
            _ => panic!("Not a float modifier"),
        }
    }

    pub fn is_ineg(&self) -> bool {
        match self {
            SrcMod::None => false,
            SrcMod::INeg => true,
            _ => panic!("Not an integer modifier"),
        }
    }

    pub fn is_bnot(&self) -> bool {
        match self {
            SrcMod::None => false,
            SrcMod::BNot => true,
            _ => panic!("Not a bitwise modifier"),
        }
    }

    pub fn fabs(self) -> SrcMod {
        match self {
            SrcMod::None | SrcMod::FAbs | SrcMod::FNeg | SrcMod::FNegAbs => SrcMod::FAbs,
            _ => panic!("Not a float source modifier"),
        }
    }

    pub fn fneg(self) -> SrcMod {
        match self {
            SrcMod::None => SrcMod::FNeg,
            SrcMod::FAbs => SrcMod::FNegAbs,
            SrcMod::FNeg => SrcMod::None,
            SrcMod::FNegAbs => SrcMod::FAbs,
            _ => panic!("Not a float source modifier"),
        }
    }

    pub fn ineg(self) -> SrcMod {
        match self {
            SrcMod::None => SrcMod::INeg,
            SrcMod::INeg => SrcMod::None,
            _ => panic!("Not an integer source modifier"),
        }
    }

    pub fn bnot(self) -> SrcMod {
        match self {
            SrcMod::None => SrcMod::BNot,
            SrcMod::BNot => SrcMod::None,
            _ => panic!("Not a boolean source modifier"),
        }
    }

    pub fn modify(self, other: SrcMod) -> SrcMod {
        match other {
            SrcMod::None => self,
            SrcMod::FAbs => self.fabs(),
            SrcMod::FNeg => self.fneg(),
            SrcMod::FNegAbs => self.fabs().fneg(),
            SrcMod::INeg => self.ineg(),
            SrcMod::BNot => self.bnot(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SrcSwizzle {
    None,
    Xx,
    Yy,
}

impl SrcSwizzle {
    pub fn is_none(&self) -> bool {
        matches!(self, SrcSwizzle::None)
    }
}

impl fmt::Display for SrcSwizzle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SrcSwizzle::None => Ok(()),
            SrcSwizzle::Xx => write!(f, ".xx"),
            SrcSwizzle::Yy => write!(f, ".yy"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nak::ir::RegFile;

    #[test]
    fn test_dst_none() {
        let dst = Dst::None;
        assert!(dst.is_none());
        assert_eq!(dst.comps(), 0);
        assert!(dst.file().is_none());
    }

    #[test]
    fn test_src_ref_zero() {
        let src = SrcRef::Zero;
        assert_eq!(src.as_u32(), Some(0));
        assert!(src.is_alu());
    }

    #[test]
    fn test_src_ref_imm32() {
        let src = SrcRef::Imm32(0x42);
        assert_eq!(src.as_u32(), Some(0x42));
    }

    #[test]
    fn test_src_ref_true_false() {
        let t = SrcRef::True;
        let f = SrcRef::False;
        assert!(t.is_predicate());
        assert!(f.is_predicate());
    }

    #[test]
    fn test_cbuf_ref_offset() {
        let cbuf = CBufRef {
            buf: CBuf::Binding(0),
            offset: 100,
        };
        let shifted = cbuf.offset(50);
        assert_eq!(shifted.offset, 150);
    }

    #[test]
    fn test_dst_from_reg_ref() {
        let reg = RegRef::new(RegFile::GPR, 0, 1);
        let dst: Dst = reg.into();
        assert!(!dst.is_none());
        assert_eq!(dst.comps(), 1);
        assert!(dst.as_reg().is_some());
    }
}
