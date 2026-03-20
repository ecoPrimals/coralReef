// SPDX-License-Identifier: AGPL-3.0-only
//! Minimal IR-like types for exercising proc-macro expansion.

use std::fmt;

/// Per-crate `DisplayOp` used by the `DisplayOp` derive (unqualified in generated code).
pub trait DisplayOp {
    /// Format destination operands.
    fn fmt_dsts(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    /// Format the opcode portion.
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

/// Placeholder source operand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Src(pub u8);

/// Placeholder destination operand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dst(pub u8);

/// Attributes for `SrcsAsSlice` tests — includes `DEFAULT` for omitted `#[src_type]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)] // Matches generated `SrcType::DEFAULT` / `DstType::DEFAULT`.
pub enum SrcType {
    /// Fallback when no attribute is present.
    DEFAULT,
    A,
    B,
    C,
}

/// Attributes for `DstsAsSlice` tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)]
pub enum DstType {
    #[allow(dead_code)] // Used when `#[dst_type]` is omitted in other tests.
    /// Fallback when no attribute is present.
    DEFAULT,
    Out,
}
