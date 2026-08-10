// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! SM70 ALU instruction encoders.

use super::*;

mod conv;
mod float;
mod float16;
mod float64;
mod int;
mod misc;

#[cfg(test)]
#[path = "conv_tests.rs"]
mod conv_tests;

#[cfg(test)]
#[path = "float_tests.rs"]
mod float_tests;

#[cfg(test)]
#[path = "float64_tests.rs"]
mod float64_tests;
