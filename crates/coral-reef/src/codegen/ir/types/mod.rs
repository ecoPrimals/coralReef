// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Type enums used by instruction operands: comparison ops, float/int types,
//! texture types, memory types, interpolation modes, etc.

use std::fmt;

use super::{ShaderModel, Src};

mod cmp;
pub use cmp::*;

mod scalar;
pub use scalar::*;

mod tex;
pub use tex::*;

mod mem;
pub use mem::*;
