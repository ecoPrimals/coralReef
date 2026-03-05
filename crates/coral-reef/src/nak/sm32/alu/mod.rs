// Copyright © 2025 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! SM32 ALU instruction encoders.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::super::ir::RegFile;
use super::encoder::*;

mod conv;
mod float;
mod float64;
mod int;
mod misc;
