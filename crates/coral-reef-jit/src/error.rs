// SPDX-License-Identifier: AGPL-3.0-only
//! Error types for the JIT compilation pipeline.

/// Errors from JIT compilation and execution.
#[derive(Debug, thiserror::Error)]
pub enum JitError {
    /// JIT module setup failure (ISA detection, flag configuration).
    #[error("JIT setup: {0}")]
    Setup(String),

    /// `CoralIR` → Cranelift translation failure.
    #[error("translation: {0}")]
    Translation(String),

    /// Cranelift compilation failure.
    #[error("compilation: {0}")]
    Compilation(String),

    /// Unsupported `CoralIR` operation encountered.
    #[error("unsupported op: {0}")]
    UnsupportedOp(String),

    /// Runtime execution error.
    #[error("execution: {0}")]
    Execution(String),
}
