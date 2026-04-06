// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)
//! SM20 ALU instruction encoders.

mod conv;
mod float;
mod float64;
mod int;
mod misc;

pub(super) use super::encoder::*;
pub(super) use crate::codegen::ir::RegFile;
