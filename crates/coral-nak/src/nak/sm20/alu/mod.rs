// Copyright © 2025 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! SM20 ALU instruction encoders.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

mod conv;
mod float;
mod float64;
mod int;
mod misc;

pub(super) use super::super::ir::RegFile;
pub(super) use super::encoder::*;
