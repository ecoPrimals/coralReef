// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Type enums used by instruction operands: comparison ops, float/int types,
//! texture types, memory types, interpolation modes, etc.

use std::fmt;
use std::ops::{BitAnd, BitOr, Not, Range};

use super::{ShaderModel, Src};

mod cmp;
pub use cmp::*;

mod scalar;
pub use scalar::*;

mod tex;
pub use tex::*;

mod mem;
pub use mem::*;
