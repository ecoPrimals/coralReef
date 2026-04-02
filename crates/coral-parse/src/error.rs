// SPDX-License-Identifier: AGPL-3.0-only
//! Parse error types for coral-parse.

/// Errors that can occur during parsing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ParseError {
    /// Syntax error at a given byte offset.
    #[error("syntax error at offset {offset}: {message}")]
    Syntax { offset: u32, message: String },

    /// Unsupported language construct.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// Lowering error (AST -> CoralIR).
    #[error("lowering error: {0}")]
    Lowering(String),
}
