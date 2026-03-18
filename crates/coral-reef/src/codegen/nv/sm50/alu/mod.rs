// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)
//! SM50 ALU instruction encoders.

use super::encoder::*;
use crate::codegen::ir::RegFile;

mod conv;
mod float;
mod float64;
mod int;
mod misc;
