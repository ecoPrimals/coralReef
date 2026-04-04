// SPDX-License-Identifier: AGPL-3.0-only
//! Error types for the `coralctl` binary.

use std::fmt;
use std::io::ErrorKind;

/// Unix socket connect failure with the target path (for actionable hints).
#[derive(Debug)]
pub(crate) struct SocketConnectError {
    /// Glowplug or ember socket path.
    pub path: String,
    /// Underlying connect error.
    pub source: std::io::Error,
}

impl fmt::Display for SocketConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.source.kind() {
            ErrorKind::PermissionDenied => {
                writeln!(f, "permission denied connecting to {}", self.path)?;
                writeln!(f, "hint: add yourself to the coralreef group:")?;
                writeln!(f, "  sudo groupadd -r coralreef")?;
                writeln!(f, "  sudo usermod -aG coralreef $USER")?;
                write!(f, "  newgrp coralreef  # or log out and back in")
            }
            ErrorKind::NotFound => {
                writeln!(f, "socket not found at {}", self.path)?;
                write!(
                    f,
                    "hint: is coral-glowplug running?  systemctl status coral-glowplug"
                )
            }
            _ => write!(f, "failed to connect to {}: {}", self.path, self.source),
        }
    }
}

impl std::error::Error for SocketConnectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Top-level CLI failure (printed once from `main`).
#[derive(Debug, thiserror::Error)]
pub(crate) enum CliError {
    /// JSON-RPC `error` object from the daemon.
    #[error("RPC error [{code}]: {message}")]
    Rpc {
        /// JSON-RPC error code.
        code: i64,
        /// Error message string.
        message: String,
    },
    /// Failed to connect to the Unix socket.
    #[error("{0}")]
    Connection(#[from] SocketConnectError),
    /// General I/O (files, socket read/write after connect).
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON parse/serialize failures.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Successful RPC envelope but missing `result` field.
    #[error("no result in RPC response")]
    NoResult,
    /// Invalid CLI arguments or derived validation.
    #[error("{0}")]
    InvalidArg(String),
    /// Glowplug config could not be loaded for `deploy-udev`.
    #[error("{0}")]
    Config(String),
}

impl CliError {
    /// Build a [`SocketConnectError`] for the given path and connect failure.
    pub(crate) fn connection(path: impl Into<String>, source: std::io::Error) -> Self {
        Self::Connection(SocketConnectError {
            path: path.into(),
            source,
        })
    }
}
