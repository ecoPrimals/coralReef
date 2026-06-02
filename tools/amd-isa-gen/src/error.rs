// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! Error types for the AMD ISA table generator.

use std::path::Path;

/// Errors from the AMD ISA table generator.
#[derive(Debug, thiserror::Error)]
pub enum IsaGenError {
    /// IO error with path context.
    #[error("{context}: {source}")]
    Io {
        /// What operation failed.
        context: String,
        /// The underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// XML parsing error.
    #[error("XML parse error: {0}")]
    XmlParse(String),

    /// Code generation logic error.
    #[error("codegen error: {0}")]
    Codegen(String),
}

impl IsaGenError {
    /// IO error with path context.
    pub fn io(path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            context: format!("{}", path.display()),
            source,
        }
    }

    /// IO error with custom message.
    pub fn io_ctx(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

impl From<std::fmt::Error> for IsaGenError {
    fn from(e: std::fmt::Error) -> Self {
        Self::Codegen(format!("format error: {e}"))
    }
}
