// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)

#![allow(clippy::wildcard_imports)]

use super::ir::*;

use coral_reef_stubs::fxhash::FxHashMap;

pub struct ConstTracker {
    map: FxHashMap<SSAValue, SrcRef>,
}

/// A tracker struct for finding re-materializable constants
///
/// Anything which is an immediate, Zero, or a bound cbuf can trivially be
/// re-materialized anywhere in the shader and it's probably cheaper to do so
/// than to try and keep them around in GPRs forever.  This is just a helper
/// struct for implementing this logic in compiler passes.
impl ConstTracker {
    pub fn new() -> Self {
        Self {
            map: FxHashMap::default(),
        }
    }

    /// Registers a copy instruction
    ///
    /// If the source of the copy is a constant, the destination SSA value and
    /// the constant value get stored as a key/value pair.
    pub fn add_copy(&mut self, op: &OpCopy) {
        let Some(dst) = op.dst.as_ssa() else {
            return;
        };
        debug_assert!(dst.comps() == 1);
        let dst = dst[0];

        if !op.src.is_unmodified() {
            return;
        }
        let is_const = match &op.src.reference {
            SrcRef::Zero | SrcRef::True | SrcRef::False | SrcRef::Imm32(_) => true,
            SrcRef::CBuf(cb) => matches!(cb.buf, CBuf::Binding(_)),
            _ => false,
        };

        if is_const {
            self.map.insert(dst, op.src.reference.clone());
        }
    }

    /// Tests if the ConstTracker contains the given SSAValue
    pub fn contains(&self, ssa: &SSAValue) -> bool {
        self.map.contains_key(ssa)
    }

    /// Returns the SrcRef associated with this SSAValue, if any
    pub fn get(&self, ssa: &SSAValue) -> Option<&SrcRef> {
        self.map.get(ssa)
    }
}
