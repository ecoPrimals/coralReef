// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compiler error types.

use std::borrow::Cow;

/// Errors from the coral-reef compiler.
///
/// All message-carrying variants use `Cow<'static, str>`: static strings
/// (the common case for `NotImplemented`, `UnsupportedArch`) are zero-alloc,
/// while dynamic messages from `format!()` allocate only when needed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CompileError {
    /// Invalid input (empty module, malformed SPIR-V, etc.)
    #[error("invalid input: {0}")]
    InvalidInput(Cow<'static, str>),

    /// Feature not yet implemented.
    #[error("not implemented: {0}")]
    NotImplemented(Cow<'static, str>),

    /// IR validation failure.
    #[error("IR validation: {0}")]
    Validation(Cow<'static, str>),

    /// Register allocation failure.
    #[error("register allocation: {0}")]
    RegisterAllocation(Cow<'static, str>),

    /// Encoding failure (target-specific).
    #[error("encoding: {0}")]
    Encoding(Cow<'static, str>),

    /// Unsupported GPU architecture.
    #[error("unsupported architecture: {0}")]
    UnsupportedArch(Cow<'static, str>),

    /// Internal compiler error (ICE). These represent bugs in coralReef, not user errors.
    /// Caught from `panic!` / `ice!` in encoder backends via `catch_unwind`.
    #[error("internal compiler error: {0}")]
    Internal(Cow<'static, str>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

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

    #[test]
    fn test_error_source_chain() {
        let e = CompileError::InvalidInput("chain".into());
        assert!(e.source().is_none());
    }

    #[test]
    fn test_error_cow_static() {
        let e = CompileError::InvalidInput(Cow::Borrowed("static"));
        assert_eq!(e.to_string(), "invalid input: static");
    }

    #[test]
    fn test_error_cow_owned() {
        let msg = "dynamic".to_string();
        let e = CompileError::InvalidInput(Cow::Owned(msg));
        assert_eq!(e.to_string(), "invalid input: dynamic");
    }

    #[test]
    fn test_all_error_variants_display() {
        let variants = [
            CompileError::InvalidInput("i".into()),
            CompileError::NotImplemented("n".into()),
            CompileError::Validation("v".into()),
            CompileError::RegisterAllocation("r".into()),
            CompileError::Encoding("e".into()),
            CompileError::UnsupportedArch("u".into()),
            CompileError::Internal("ice".into()),
        ];
        for e in variants {
            let s = e.to_string();
            assert!(!s.is_empty());
            assert!(s.len() > 5);
        }
    }

    #[test]
    fn test_error_display_distinguishes_variants() {
        let a = CompileError::InvalidInput("test".into());
        let b = CompileError::NotImplemented("test".into());
        assert!(a.to_string().contains("invalid"));
        assert!(b.to_string().contains("not implemented"));
    }

    #[test]
    fn test_internal_error_display() {
        let e = CompileError::Internal("unhandled instruction".into());
        let s = e.to_string();
        assert!(s.contains("internal compiler error"));
        assert!(s.contains("unhandled instruction"));
    }

    #[test]
    fn test_catch_ice_success() {
        let result = crate::codegen::catch_ice(|| vec![1, 2, 3]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn test_catch_ice_catches_panic() {
        let result = crate::codegen::catch_ice(|| panic!("test ICE: unhandled op"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CompileError::Internal(_)));
        assert!(err.to_string().contains("unhandled op"));
    }

    #[test]
    fn test_catch_ice_catches_string_panic() {
        let result = crate::codegen::catch_ice(|| panic!("{}", format!("dynamic ICE: op {}", 42)));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("dynamic ICE"));
    }
}
