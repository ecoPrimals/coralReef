// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2025)
//! SM20 ALU instruction encoders.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

mod conv;
mod float;
mod float64;
mod int;
mod misc;

pub(super) use super::encoder::*;
pub(super) use crate::codegen::ir::RegFile;
