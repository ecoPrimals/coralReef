// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! CPU compilation backend and shader validation for coralReef.
//!
//! Provides a naga IR tree-walk interpreter for executing WGSL compute shaders
//! on the CPU (with native `f64` support), plus a tolerance-based validation
//! engine for comparing GPU and CPU outputs.
//!
//! # Architecture
//!
//! The interpreter reads a `naga::Module` directly — no code generation step
//! required. This makes it suitable as a reference oracle against which JIT
//! backends (e.g. Cranelift) can be validated.
//!
//! Wire types in [`types`] are shared with `coralreef-core` for IPC.

pub mod interpret;
pub mod types;
pub mod validate;

pub use interpret::execute_cpu;
pub use types::{
    BindingData, CompileCpuRequest, ExecuteCpuRequest, ExecuteCpuResponse, ExpectedBinding,
    Mismatch, Tolerance, ValidateRequest, ValidateResponse,
};
pub use validate::validate;
