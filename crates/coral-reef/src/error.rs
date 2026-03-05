// SPDX-License-Identifier: AGPL-3.0-only
//! Compiler error types.

/// Errors from the coral-reef compiler.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CompileError {
    /// Invalid input (empty module, malformed SPIR-V, etc.)
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Feature not yet implemented (stub phase).
    #[error("not implemented: {0}")]
    NotImplemented(String),

    /// IR validation failure.
    #[error("IR validation: {0}")]
    Validation(String),

    /// Register allocation failure.
    #[error("register allocation: {0}")]
    RegisterAllocation(String),

    /// Encoding failure (target-specific).
    #[error("encoding: {0}")]
    Encoding(String),

    /// Unsupported GPU architecture.
    #[error("unsupported architecture: {0}")]
    UnsupportedArch(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let e = CompileError::InvalidInput("bad data".into());
        assert_eq!(e.to_string(), "invalid input: bad data");

        let e = CompileError::NotImplemented("f64 lowering".into());
        assert_eq!(e.to_string(), "not implemented: f64 lowering");

        let e = CompileError::Validation("type mismatch".into());
        assert_eq!(e.to_string(), "IR validation: type mismatch");

        let e = CompileError::RegisterAllocation("spill failed".into());
        assert_eq!(e.to_string(), "register allocation: spill failed");

        let e = CompileError::Encoding("bad opcode".into());
        assert_eq!(e.to_string(), "encoding: bad opcode");

        let e = CompileError::UnsupportedArch("sm_10".into());
        assert_eq!(e.to_string(), "unsupported architecture: sm_10");
    }

    #[test]
    fn test_error_is_std_error() {
        let e: Box<dyn std::error::Error> = Box::new(CompileError::InvalidInput("test".into()));
        assert!(e.to_string().contains("test"));
    }

    #[test]
    fn test_error_debug() {
        let e = CompileError::InvalidInput("dbg".into());
        let dbg = format!("{e:?}");
        assert!(dbg.contains("InvalidInput"));
    }
}
