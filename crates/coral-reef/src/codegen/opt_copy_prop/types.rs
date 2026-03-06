// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

use crate::codegen::ir::*;

pub(super) enum CBufRule {
    Yes,
    No,
    BindlessRequiresBlock(usize),
}

impl CBufRule {
    pub(super) fn allows_src(&self, src_bi: usize, src: &Src) -> bool {
        let SrcRef::CBuf(cb) = &src.reference else {
            return true;
        };

        match self {
            Self::Yes => true,
            Self::No => false,
            Self::BindlessRequiresBlock(bi) => match cb.buf {
                CBuf::Binding(_) => true,
                CBuf::BindlessSSA(_) => src_bi == *bi,
                CBuf::BindlessUGPR(_) => false, // Not in SSA form, skip propagation
            },
        }
    }
}

pub(super) struct CopyEntry {
    pub(super) bi: usize,
    pub(super) src_type: SrcType,
    pub(super) src: Src,
}

pub(super) struct PrmtEntry {
    pub(super) bi: usize,
    pub(super) sel: PrmtSel,
    pub(super) srcs: [Src; 2],
}

/// This entry tracks b2i conversions
pub(super) struct ConvBoolToInt {
    pub(super) src: Src,
}

pub(super) enum CopyPropEntry {
    Copy(CopyEntry),
    Prmt(PrmtEntry),
    ConvBoolToInt(ConvBoolToInt),
}
