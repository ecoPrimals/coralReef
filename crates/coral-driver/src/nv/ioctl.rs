// SPDX-License-Identifier: AGPL-3.0-only
//! Nouveau-specific DRM ioctl definitions.

use crate::MemoryDomain;
#[cfg(doc)]
use crate::error::DriverError;
use crate::error::DriverResult;
use std::os::unix::io::RawFd;

/// Create a nouveau GPU channel for command submission.
///
/// # Errors
///
/// Returns [`DriverError::Unsupported`] — nouveau channel ioctls are not yet implemented.
pub fn create_channel(_fd: RawFd) -> DriverResult<u32> {
    Err(crate::error::DriverError::Unsupported(
        "nouveau channel creation not yet implemented".into(),
    ))
}

/// Destroy a nouveau GPU channel.
///
/// # Errors
///
/// Returns [`DriverError::Unsupported`] — nouveau channel ioctls are not yet implemented.
pub fn destroy_channel(_fd: RawFd, _channel: u32) -> DriverResult<()> {
    Err(crate::error::DriverError::Unsupported(
        "nouveau channel destruction not yet implemented".into(),
    ))
}

/// Create a nouveau GEM buffer.
///
/// # Errors
///
/// Returns [`DriverError::Unsupported`] — nouveau GEM ioctls are not yet implemented.
pub fn gem_new(_fd: RawFd, _size: u64, _domain: MemoryDomain) -> DriverResult<u32> {
    Err(crate::error::DriverError::Unsupported(
        "nouveau GEM allocation not yet implemented".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_channel_returns_unsupported() {
        let err = create_channel(-1).unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }

    #[test]
    fn destroy_channel_returns_unsupported() {
        let err = destroy_channel(-1, 0).unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }

    #[test]
    fn gem_new_returns_unsupported() {
        let err = gem_new(-1, 4096, MemoryDomain::Vram).unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }
}
