// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Source and destination types: `Dst`, `Src`, `SrcRef`, `SrcMod`, `CBuf`, `CBufRef`.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::regs::*;
use super::{PredRef, PrmtSel};
use crate::codegen::ssa_value::*;
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
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub const fn as_reg(&self) -> Option<&RegRef> {
        match self {
            Self::Reg(r) => Some(r),
            _ => None,
        }
    }

    pub const fn as_ssa(&self) -> Option<&SSARef> {
        match self {
            Self::SSA(r) => Some(r),
            _ => None,
        }
    }

    #[allow(dead_code, reason = "IR API reserved for future backend integration")]
    pub fn to_ssa(self) -> SSARef {
        match self {
            Self::SSA(r) => r,
            _ => panic!("ICE: Expected ssa"),
        }
    }

    pub fn iter_ssa(&self) -> slice::Iter<'_, SSAValue> {
        match self {
            Self::None | Self::Reg(_) => &[],
            Self::SSA(ssa) => ssa.deref(),
        }
        .iter()
    }

    pub fn iter_ssa_mut(&mut self) -> slice::IterMut<'_, SSAValue> {
        match self {
            Self::None | Self::Reg(_) => &mut [],
            Self::SSA(ssa) => ssa.deref_mut(),
        }
        .iter_mut()
    }

    pub fn comps(&self) -> u8 {
        match self {
            Self::None => 0,
            Self::SSA(ssa) => ssa.comps(),
            Self::Reg(reg) => reg.comps(),
        }
    }

    pub fn file(&self) -> Option<RegFile> {
        match self {
            Self::None => None,
            Self::SSA(ssa) => Some(ssa.file()),
            Self::Reg(reg) => Some(reg.file()),
        }
    }
}

impl From<RegRef> for Dst {
    fn from(reg: RegRef) -> Self {
        Self::Reg(reg)
    }
}

impl<T: Into<SSARef>> From<T> for Dst {
    fn from(ssa: T) -> Self {
        Self::SSA(ssa.into())
    }
}

impl From<Option<SSAValue>> for Dst {
    fn from(ssa: Option<SSAValue>) -> Self {
        ssa.map_or(Self::None, |ssa| Self::SSA(ssa.into()))
    }
}

impl fmt::Display for Dst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "null")?,
            Self::SSA(v) => v.fmt(f)?,
            Self::Reg(r) => r.fmt(f)?,
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
            Self::Binding(idx) => write!(f, "c[{idx:#x}]"),
            Self::BindlessSSA(v) => write!(f, "cx[{{{}, {}}}]", v[0], v[1]),
            Self::BindlessUGPR(r) => write!(f, "cx[{r}]"),
        }
    }
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct CBufRef {
    pub buf: CBuf,
    pub offset: u16,
}

impl CBufRef {
    pub const fn offset(self, offset: u16) -> Self {
        Self {
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
            Self::Zero | Self::Imm32(_) | Self::CBuf(_) => true,
            Self::SSA(ssa) => ssa.is_gpr(),
            Self::Reg(reg) => reg.is_gpr(),
            Self::True | Self::False => false,
        }
    }

    pub const fn is_bindless_cbuf(&self) -> bool {
        match self {
            Self::CBuf(cbuf) => {
                matches!(cbuf.buf, CBuf::BindlessSSA(_) | CBuf::BindlessUGPR(_))
            }
            _ => false,
        }
    }

    pub fn is_predicate(&self) -> bool {
        match self {
            Self::Zero | Self::Imm32(_) | Self::CBuf(_) => false,
            Self::True | Self::False => true,
            Self::SSA(ssa) => ssa.is_predicate(),
            Self::Reg(reg) => reg.is_predicate(),
        }
    }

    pub fn is_carry(&self) -> bool {
        match self {
            Self::SSA(ssa) => ssa.file() == RegFile::Carry,
            Self::Reg(reg) => reg.file() == RegFile::Carry,
            _ => false,
        }
    }

    pub fn is_barrier(&self) -> bool {
        match self {
            Self::SSA(ssa) => ssa.file() == RegFile::Bar,
            Self::Reg(reg) => reg.file() == RegFile::Bar,
            _ => false,
        }
    }

    pub const fn as_reg(&self) -> Option<&RegRef> {
        match self {
            Self::Reg(r) => Some(r),
            _ => None,
        }
    }

    pub const fn as_ssa(&self) -> Option<&SSARef> {
        match self {
            Self::SSA(r) => Some(r),
            _ => None,
        }
    }

    pub fn to_ssa(self) -> SSARef {
        match self {
            Self::SSA(r) => r,
            _ => panic!("ICE: Expected SSA reference"),
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::Zero => Some(0),
            Self::Imm32(u) => Some(*u),
            Self::CBuf(_) | Self::SSA(_) | Self::Reg(_) => None,
            _ => panic!("ICE: Invalid integer source"),
        }
    }

    pub const fn get_reg(&self) -> Option<&RegRef> {
        match self {
            Self::Zero | Self::True | Self::False | Self::Imm32(_) | Self::SSA(_) => None,
            Self::CBuf(cb) => match &cb.buf {
                CBuf::Binding(_) | CBuf::BindlessSSA(_) => None,
                CBuf::BindlessUGPR(reg) => Some(reg),
            },
            Self::Reg(reg) => Some(reg),
        }
    }

    pub fn iter_ssa(&self) -> slice::Iter<'_, SSAValue> {
        match self {
            Self::Zero | Self::True | Self::False | Self::Imm32(_) | Self::Reg(_) => &[],
            Self::CBuf(cb) => match &cb.buf {
                CBuf::Binding(_) | CBuf::BindlessUGPR(_) => &[],
                CBuf::BindlessSSA(ssa) => &ssa[..],
            },
            Self::SSA(ssa) => ssa.deref(),
        }
        .iter()
    }

    pub fn iter_ssa_mut(&mut self) -> slice::IterMut<'_, SSAValue> {
        match self {
            Self::Zero | Self::True | Self::False | Self::Imm32(_) | Self::Reg(_) => &mut [],
            Self::CBuf(cb) => match &mut cb.buf {
                CBuf::Binding(_) | CBuf::BindlessUGPR(_) => &mut [],
                CBuf::BindlessSSA(ssa) => &mut ssa[..],
            },
            Self::SSA(ssa) => ssa.deref_mut(),
        }
        .iter_mut()
    }
}

impl From<bool> for SrcRef {
    fn from(b: bool) -> Self {
        if b { Self::True } else { Self::False }
    }
}

impl From<u32> for SrcRef {
    fn from(u: u32) -> Self {
        if u == 0 { Self::Zero } else { Self::Imm32(u) }
    }
}

impl From<f32> for SrcRef {
    fn from(f: f32) -> Self {
        f.to_bits().into()
    }
}

impl From<PrmtSel> for SrcRef {
    fn from(sel: PrmtSel) -> Self {
        u32::from(sel.0).into()
    }
}

impl From<CBufRef> for SrcRef {
    fn from(cb: CBufRef) -> Self {
        Self::CBuf(cb)
    }
}

impl From<RegRef> for SrcRef {
    fn from(reg: RegRef) -> Self {
        Self::Reg(reg)
    }
}

impl<T: Into<SSARef>> From<T> for SrcRef {
    fn from(ssa: T) -> Self {
        Self::SSA(ssa.into())
    }
}

impl From<PredRef> for SrcRef {
    fn from(value: PredRef) -> Self {
        match value {
            PredRef::None => Self::True,
            PredRef::Reg(reg) => Self::Reg(reg),
            PredRef::SSA(ssa) => Self::SSA(ssa.into()),
        }
    }
}

impl fmt::Display for SrcRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => write!(f, "rZ"),
            Self::True => write!(f, "pT"),
            Self::False => write!(f, "pF"),
            Self::Imm32(u) => write!(f, "{u:#x}"),
            Self::CBuf(c) => c.fmt(f),
            Self::SSA(v) => v.fmt(f),
            Self::Reg(r) => r.fmt(f),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum SrcMod {
    None,
    FAbs,
    FNeg,
    FNegAbs,
    INeg,
    BNot,
}

impl SrcMod {
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn has_fabs(&self) -> bool {
        match self {
            Self::None | Self::FNeg => false,
            Self::FAbs | Self::FNegAbs => true,
            _ => panic!("ICE: Not a float modifier"),
        }
    }

    pub fn has_fneg(&self) -> bool {
        match self {
            Self::None | Self::FAbs => false,
            Self::FNeg | Self::FNegAbs => true,
            _ => panic!("ICE: Not a float modifier"),
        }
    }

    pub fn is_ineg(&self) -> bool {
        match self {
            Self::None => false,
            Self::INeg => true,
            _ => panic!("ICE: Not an integer modifier"),
        }
    }

    pub fn is_bnot(&self) -> bool {
        match self {
            Self::None => false,
            Self::BNot => true,
            _ => panic!("ICE: Not a bitwise modifier"),
        }
    }

    pub fn fabs(self) -> Self {
        match self {
            Self::None | Self::FAbs | Self::FNeg | Self::FNegAbs => Self::FAbs,
            _ => panic!("ICE: Not a float source modifier"),
        }
    }

    pub fn fneg(self) -> Self {
        match self {
            Self::None => Self::FNeg,
            Self::FAbs => Self::FNegAbs,
            Self::FNeg => Self::None,
            Self::FNegAbs => Self::FAbs,
            _ => panic!("ICE: Not a float source modifier"),
        }
    }

    pub fn ineg(self) -> Self {
        match self {
            Self::None => Self::INeg,
            Self::INeg => Self::None,
            _ => panic!("ICE: Not an integer source modifier"),
        }
    }

    pub fn bnot(self) -> Self {
        match self {
            Self::None => Self::BNot,
            Self::BNot => Self::None,
            _ => panic!("ICE: Not a boolean source modifier"),
        }
    }

    pub fn modify(self, other: Self) -> Self {
        match other {
            Self::None => self,
            Self::FAbs => self.fabs(),
            Self::FNeg => self.fneg(),
            Self::FNegAbs => self.fabs().fneg(),
            Self::INeg => self.ineg(),
            Self::BNot => self.bnot(),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum SrcSwizzle {
    None,
    Xx,
    Yy,
}

impl SrcSwizzle {
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

impl fmt::Display for SrcSwizzle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => Ok(()),
            Self::Xx => write!(f, ".xx"),
            Self::Yy => write!(f, ".yy"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::RegFile;

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
