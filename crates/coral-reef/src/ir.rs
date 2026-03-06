// SPDX-License-Identifier: AGPL-3.0-only
//! Public IR types re-exported from the codegen module.
//!
//! The full internal IR lives in `codegen::ir`.  This module provides
//! the public-facing subset that external consumers may need without
//! coupling to codegen internals.

pub use crate::codegen::ir::RegFile;
pub use crate::codegen::ir::TranscendentalOp;
