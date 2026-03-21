// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)
//! SM32 ALU instruction encoders.

use super::encoder::*;
use crate::codegen::ir::RegFile;

mod conv;
mod float;
mod float64;
mod int;
mod misc;

#[cfg(test)]
#[path = "misc_tests.rs"]
mod misc_tests;

#[cfg(test)]
#[path = "conv_tests.rs"]
mod conv_tests;

#[cfg(test)]
#[path = "float_alu_tests.rs"]
mod float_alu_tests;
