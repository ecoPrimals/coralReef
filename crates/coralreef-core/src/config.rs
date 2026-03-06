// SPDX-License-Identifier: AGPL-3.0-only
//! Configuration constants for coralreef-core.

use std::time::Duration;

/// Default timeout for graceful shutdown (SIGTERM/SIGINT).
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
