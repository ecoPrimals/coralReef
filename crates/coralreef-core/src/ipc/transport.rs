// SPDX-License-Identifier: AGPL-3.0-or-later
//! Re-export from top-level `transport` module for backward compatibility.
//!
//! The G66 transport abstraction lives at `crate::transport` so it's available
//! unconditionally (not behind `#[cfg(test)]` or feature gates). IPC code
//! continues to use `super::transport::*` or `crate::ipc::transport::*`
//! transparently via this re-export.
pub use crate::transport::*;
