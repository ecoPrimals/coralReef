// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Integer ALU instruction op structs.

use super::*;

mod bitwise;
pub use bitwise::*;

mod arith;
pub use arith::*;

mod shift;
pub use shift::*;
