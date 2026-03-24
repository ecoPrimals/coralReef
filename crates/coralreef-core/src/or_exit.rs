// SPDX-License-Identifier: AGPL-3.0-only
//! Zero-panic validation for binary entry points.
//!
//! Absorbed from wetSpring/rhizoCrypt ecosystem pattern. Instead of
//! `unwrap()` or `expect()` which trigger panic hooks and stack traces,
//! `OrExit` provides a clean `process::exit` with structured logging.
//!
//! # Usage
//!
//! ```rust,ignore
//! use coralreef_core::or_exit::OrExit;
//!
//! let config = Config::load(&path).or_exit("config load");
//! let addr = bind_addr.parse::<SocketAddr>().or_exit("bind address");
//! ```

use std::fmt;
use std::process;

/// Extension trait for `Result` and `Option` providing clean exit on failure.
///
/// Replaces `unwrap()`/`expect()` in binary entry points with structured
/// tracing + `process::exit(1)` — no panics, no stack traces in production.
pub trait OrExit<T> {
    /// Unwrap the value or exit the process with a descriptive error.
    ///
    /// Logs the error via `tracing::error!` and calls `process::exit(1)`.
    fn or_exit(self, context: &str) -> T;

    /// Unwrap the value or exit with a custom exit code.
    fn or_exit_code(self, context: &str, code: i32) -> T;
}

impl<T, E: fmt::Display> OrExit<T> for Result<T, E> {
    fn or_exit(self, context: &str) -> T {
        self.or_exit_code(context, 1)
    }

    fn or_exit_code(self, context: &str, code: i32) -> T {
        match self {
            Ok(val) => val,
            Err(e) => {
                tracing::error!(context, error = %e, "fatal");
                process::exit(code);
            }
        }
    }
}

impl<T> OrExit<T> for Option<T> {
    fn or_exit(self, context: &str) -> T {
        self.or_exit_code(context, 1)
    }

    fn or_exit_code(self, context: &str, code: i32) -> T {
        self.unwrap_or_else(|| {
            tracing::error!(context, "fatal: value required but absent");
            process::exit(code);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_result_ok_returns_value() {
        let result: Result<i32, &str> = Ok(42);
        assert_eq!(result.or_exit("test"), 42);
    }

    #[test]
    fn test_option_some_returns_value() {
        let opt: Option<i32> = Some(42);
        assert_eq!(opt.or_exit("test"), 42);
    }

    #[test]
    fn test_result_or_exit_code_ok_returns_value() {
        let result: Result<i32, &str> = Ok(99);
        assert_eq!(result.or_exit_code("test", 42), 99);
    }

    #[test]
    fn test_option_or_exit_code_some_returns_value() {
        let opt: Option<String> = Some("hello".into());
        assert_eq!(opt.or_exit_code("test", 2), "hello");
    }

    #[test]
    fn result_err_display_used_by_fatal_logging() {
        let err: Result<(), &str> = Err("boom");
        assert_eq!(
            err.expect_err("expected Err branch for display check")
                .to_string(),
            "boom"
        );
    }
}
