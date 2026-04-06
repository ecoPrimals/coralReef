// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Texture and surface instruction op structs.

use super::*;

mod sample;
pub use sample::*;

mod surface;
pub use surface::*;

mod surface_addr;
pub use surface_addr::*;
