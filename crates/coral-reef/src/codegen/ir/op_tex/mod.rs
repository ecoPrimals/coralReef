// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Texture and surface instruction op structs.

#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

mod sample;
pub use sample::*;

mod surface;
pub use surface::*;

mod surface_addr;
pub use surface_addr::*;
