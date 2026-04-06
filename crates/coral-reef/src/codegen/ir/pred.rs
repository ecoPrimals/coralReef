// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
//! Predicate reference and predicate types for the IR.

use super::SSAValue;
use super::regs::RegRef;
use std::fmt;
use std::slice;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub enum PredRef {
    None,
    SSA(SSAValue),
    Reg(RegRef),
}

impl PredRef {
    /// Extract register reference when variant is `Reg`. Part of the IR API.
    pub fn as_reg(&self) -> Option<&RegRef> {
        match self {
            Self::Reg(r) => Some(r),
            _ => None,
        }
    }

    /// Extract SSA value when variant is `SSA`. Part of the IR API.
    pub fn as_ssa(&self) -> Option<&SSAValue> {
        match self {
            Self::SSA(r) => Some(r),
            _ => None,
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn iter_ssa(&self) -> slice::Iter<'_, SSAValue> {
        match self {
            Self::None | Self::Reg(_) => &[],
            Self::SSA(ssa) => slice::from_ref(ssa),
        }
        .iter()
    }

    pub fn iter_ssa_mut(&mut self) -> slice::IterMut<'_, SSAValue> {
        match self {
            Self::None | Self::Reg(_) => &mut [],
            Self::SSA(ssa) => slice::from_mut(ssa),
        }
        .iter_mut()
    }
}

impl From<RegRef> for PredRef {
    fn from(reg: RegRef) -> Self {
        Self::Reg(reg)
    }
}

impl From<SSAValue> for PredRef {
    fn from(ssa: SSAValue) -> Self {
        Self::SSA(ssa)
    }
}

impl fmt::Display for PredRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "pT"),
            Self::SSA(ssa) => ssa.fmt(f),
            Self::Reg(reg) => reg.fmt(f),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Pred {
    pub predicate: PredRef,
    pub inverted: bool,
}

impl Pred {
    pub fn is_true(&self) -> bool {
        self.predicate.is_none() && !self.inverted
    }

    pub fn is_false(&self) -> bool {
        self.predicate.is_none() && self.inverted
    }

    pub fn iter_ssa(&self) -> slice::Iter<'_, SSAValue> {
        self.predicate.iter_ssa()
    }

    pub fn iter_ssa_mut(&mut self) -> slice::IterMut<'_, SSAValue> {
        self.predicate.iter_ssa_mut()
    }

    pub fn bnot(self) -> Self {
        Self {
            predicate: self.predicate,
            inverted: !self.inverted,
        }
    }
}

impl From<bool> for Pred {
    fn from(b: bool) -> Self {
        Self {
            predicate: PredRef::None,
            inverted: !b,
        }
    }
}

impl<T: Into<PredRef>> From<T> for Pred {
    fn from(p: T) -> Self {
        Self {
            predicate: p.into(),
            inverted: false,
        }
    }
}

impl fmt::Display for Pred {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.inverted {
            write!(f, "!")?;
        }
        self.predicate.fmt(f)
    }
}
