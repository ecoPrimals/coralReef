// SPDX-License-Identifier: AGPL-3.0-only
//! `CoralIR` comparison ops → Cranelift condition code mappings.

use coral_reef::codegen::ir::{FloatCmpOp, IntCmpOp};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};

/// Map a `CoralIR` float comparison to a Cranelift float condition code.
#[must_use]
pub const fn float_cmp_to_cc(cmp: FloatCmpOp) -> FloatCC {
    match cmp {
        FloatCmpOp::OrdEq | FloatCmpOp::UnordEq => FloatCC::Equal,
        FloatCmpOp::OrdNe | FloatCmpOp::UnordNe => FloatCC::NotEqual,
        FloatCmpOp::OrdLt => FloatCC::LessThan,
        FloatCmpOp::OrdLe => FloatCC::LessThanOrEqual,
        FloatCmpOp::OrdGt => FloatCC::GreaterThan,
        FloatCmpOp::OrdGe => FloatCC::GreaterThanOrEqual,
        FloatCmpOp::UnordLt => FloatCC::UnorderedOrLessThan,
        FloatCmpOp::UnordLe => FloatCC::UnorderedOrLessThanOrEqual,
        FloatCmpOp::UnordGt => FloatCC::UnorderedOrGreaterThan,
        FloatCmpOp::UnordGe => FloatCC::UnorderedOrGreaterThanOrEqual,
        FloatCmpOp::IsNum => FloatCC::Ordered,
        FloatCmpOp::IsNan => FloatCC::Unordered,
    }
}

/// Map a `CoralIR` integer comparison to a Cranelift integer condition code.
#[must_use]
pub const fn int_cmp_to_cc(cmp: IntCmpOp, unsigned: bool) -> IntCC {
    match (cmp, unsigned) {
        (IntCmpOp::Eq | IntCmpOp::True | IntCmpOp::False, _) => IntCC::Equal,
        (IntCmpOp::Ne, _) => IntCC::NotEqual,
        (IntCmpOp::Lt, false) => IntCC::SignedLessThan,
        (IntCmpOp::Lt, true) => IntCC::UnsignedLessThan,
        (IntCmpOp::Le, false) => IntCC::SignedLessThanOrEqual,
        (IntCmpOp::Le, true) => IntCC::UnsignedLessThanOrEqual,
        (IntCmpOp::Gt, false) => IntCC::SignedGreaterThan,
        (IntCmpOp::Gt, true) => IntCC::UnsignedGreaterThan,
        (IntCmpOp::Ge, false) => IntCC::SignedGreaterThanOrEqual,
        (IntCmpOp::Ge, true) => IntCC::UnsignedGreaterThanOrEqual,
    }
}
