// SPDX-License-Identifier: AGPL-3.0-or-later
//! Structured IPC errors absorbed from rhizoCrypt/loamSpine ecosystem pattern.
//!
//! `IpcErrorPhase` replaces raw `String` errors throughout the IPC layer,
//! giving callers structured information for retry logic and observability.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Phase in the IPC lifecycle where the error occurred.
///
/// Enables callers to make retry decisions: `Transport` errors are retryable,
/// `Dispatch` errors indicate a method-level problem, `Internal` errors
/// should be reported and not retried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcPhase {
    /// Transport-level failure (connection, serialization, timeout).
    Transport,
    /// Method dispatch failure (unknown method, bad params).
    Dispatch,
    /// Handler-level failure (compilation error, resource unavailable).
    Handler,
    /// Internal error (bug, assertion, OOM).
    Internal,
}

impl IpcPhase {
    /// Whether the error is likely retryable.
    #[must_use]
    pub const fn retryable(self) -> bool {
        matches!(self, Self::Transport)
    }

    /// JSON-RPC 2.0 error code for this phase.
    #[must_use]
    pub const fn jsonrpc_code(self) -> i32 {
        match self {
            Self::Transport | Self::Handler => -32000,
            Self::Dispatch => -32601,
            Self::Internal => -32603,
        }
    }
}

/// Structured IPC error with phase, message, and optional detail.
///
/// Replaces `Result<T, String>` throughout the IPC layer.
/// Serializable for wire transmission and structured logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcServiceError {
    /// Where in the IPC pipeline the error occurred.
    pub phase: IpcPhase,
    /// Human-readable error message.
    pub message: String,
    /// Optional machine-readable error code for the caller.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl IpcServiceError {
    /// Create a new transport-phase error.
    #[must_use]
    pub fn transport(message: impl Into<String>) -> Self {
        Self {
            phase: IpcPhase::Transport,
            message: message.into(),
            code: None,
        }
    }

    /// Create a new dispatch-phase error (unknown method, bad params).
    #[must_use]
    pub fn dispatch(message: impl Into<String>) -> Self {
        Self {
            phase: IpcPhase::Dispatch,
            message: message.into(),
            code: None,
        }
    }

    /// Create a new handler-phase error (compilation failure, etc.).
    #[must_use]
    pub fn handler(message: impl Into<String>) -> Self {
        Self {
            phase: IpcPhase::Handler,
            message: message.into(),
            code: None,
        }
    }

    /// Create a new internal error.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            phase: IpcPhase::Internal,
            message: message.into(),
            code: None,
        }
    }

    /// Attach an error code for machine-readable categorization.
    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}

impl fmt::Display for IpcServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.phase, self.message)?;
        if let Some(code) = &self.code {
            write!(f, " ({code})")?;
        }
        Ok(())
    }
}

impl std::error::Error for IpcServiceError {}

impl From<IpcServiceError> for String {
    fn from(e: IpcServiceError) -> Self {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_error_is_retryable() {
        let err = IpcServiceError::transport("connection reset");
        assert!(err.phase.retryable());
        assert_eq!(err.phase.jsonrpc_code(), -32000);
    }

    #[test]
    fn test_dispatch_error_not_retryable() {
        let err = IpcServiceError::dispatch("method not found: foo.bar");
        assert!(!err.phase.retryable());
        assert_eq!(err.phase.jsonrpc_code(), -32601);
    }

    #[test]
    fn test_handler_error_not_retryable() {
        let err = IpcServiceError::handler("compilation failed");
        assert!(!err.phase.retryable());
    }

    #[test]
    fn test_internal_error() {
        let err = IpcServiceError::internal("unexpected state");
        assert_eq!(err.phase, IpcPhase::Internal);
        assert_eq!(err.phase.jsonrpc_code(), -32603);
    }

    #[test]
    fn test_error_with_code() {
        let err = IpcServiceError::handler("unsupported arch").with_code("UNSUPPORTED_ARCH");
        assert_eq!(err.code.as_deref(), Some("UNSUPPORTED_ARCH"));
        let display = err.to_string();
        assert!(display.contains("UNSUPPORTED_ARCH"));
    }

    #[test]
    fn test_serde_roundtrip() {
        let err = IpcServiceError::handler("test error").with_code("TEST");
        let json = serde_json::to_string(&err).unwrap();
        let roundtrip: IpcServiceError = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.phase, IpcPhase::Handler);
        assert_eq!(roundtrip.message, "test error");
        assert_eq!(roundtrip.code.as_deref(), Some("TEST"));
    }

    #[test]
    fn test_display_format() {
        let err = IpcServiceError::transport("timeout");
        assert_eq!(err.to_string(), "[Transport] timeout");
    }

    #[test]
    fn test_into_string() {
        let err = IpcServiceError::dispatch("bad method");
        let s: String = err.into();
        assert!(s.contains("bad method"));
    }
}
