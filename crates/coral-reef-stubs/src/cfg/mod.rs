// SPDX-License-Identifier: AGPL-3.0-or-later
//! Control-flow graph — replacement for `compiler::cfg`.
//!
//! Provides a directed graph over basic blocks with predecessor/successor
//! tracking, used for dominance analysis, loop detection, and
//! instruction scheduling.
//!
//! Dominator analysis lives in the `dom` submodule and is computed lazily
//! on first access via any dominance/loop query method.

pub(crate) mod dom;

mod builder;
mod ops;
mod traverse;
mod types;

pub use builder::CFGBuilder;
pub use types::{CFG, Edge, NodeId};

#[cfg(test)]
mod tests;
