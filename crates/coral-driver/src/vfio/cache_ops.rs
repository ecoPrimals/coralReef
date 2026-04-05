// SPDX-License-Identifier: AGPL-3.0-only
//! VFIO-oriented re-exports of crate-level cache coherence intrinsics.
//!
//! The canonical implementations live in [`crate::cache`]. This module
//! preserves the existing `vfio::cache_ops` import paths for backward
//! compatibility.

pub use crate::cache::cache_line_flush;
pub use crate::cache::clflush_range;
pub use crate::cache::memory_fence;
