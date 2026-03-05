// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Integer ALU instruction op structs.

#![allow(clippy::wildcard_imports)]

use super::*;

mod bitwise;
pub use bitwise::*;

mod arith;
pub use arith::*;

mod shift;
pub use shift::*;
