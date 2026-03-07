// SPDX-License-Identifier: AGPL-3.0-only
//! Nouveau-specific DRM ioctl definitions.

use crate::MemoryDomain;
use crate::error::DriverResult;
use std::os::unix::io::RawFd;

/// Create a nouveau GPU channel for command submission.
pub fn create_channel(_fd: RawFd) -> DriverResult<u32> {
    tracing::debug!("nouveau channel create (scaffold)");
    Ok(0)
}

/// Destroy a nouveau GPU channel.
pub fn destroy_channel(_fd: RawFd, _channel: u32) -> DriverResult<()> {
    tracing::debug!("nouveau channel destroy (scaffold)");
    Ok(())
}

/// Create a nouveau GEM buffer.
pub fn gem_new(_fd: RawFd, _size: u64, _domain: MemoryDomain) -> DriverResult<u32> {
    tracing::debug!(size = _size, "nouveau GEM new (scaffold)");
    Ok(1)
}
