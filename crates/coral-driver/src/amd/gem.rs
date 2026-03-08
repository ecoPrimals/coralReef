// SPDX-License-Identifier: AGPL-3.0-only
//! GEM (Graphics Execution Manager) buffer objects for AMD GPUs.

use super::ioctl;
use crate::MemoryDomain;
use crate::drm::MappedRegion;
use crate::error::{DriverError, DriverResult};
use std::os::unix::io::RawFd;

/// A GEM buffer object backed by amdgpu.
#[derive(Debug)]
pub struct GemBuffer {
    /// Kernel GEM handle.
    pub gem_handle: u32,
    /// Allocated size in bytes.
    pub size: u64,
    /// GPU virtual address (set after VA mapping).
    pub gpu_va: u64,
    /// Memory domain.
    pub domain: MemoryDomain,
}

impl GemBuffer {
    /// Create a new GEM buffer via amdgpu ioctl.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the GEM create or VA map ioctl fails.
    pub fn create(fd: RawFd, size: u64, domain: MemoryDomain) -> DriverResult<Self> {
        let domain_flags = match domain {
            MemoryDomain::Vram => ioctl::AMDGPU_GEM_DOMAIN_VRAM,
            MemoryDomain::Gtt => ioctl::AMDGPU_GEM_DOMAIN_GTT,
            MemoryDomain::VramOrGtt => ioctl::AMDGPU_GEM_DOMAIN_VRAM | ioctl::AMDGPU_GEM_DOMAIN_GTT,
        };

        let (handle, actual_size) = ioctl::gem_create(fd, size, domain_flags)?;

        let gpu_va = 0x0000_8000_0000_u64 + u64::from(handle) * 0x0100_0000;

        ioctl::gem_va_map(fd, handle, gpu_va, actual_size)?;

        Ok(Self {
            gem_handle: handle,
            size: actual_size,
            gpu_va,
            domain,
        })
    }

    /// Write data into the buffer via mmap.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the write exceeds buffer bounds or mmap fails.
    pub fn write(&self, fd: RawFd, offset: u64, data: &[u8]) -> DriverResult<()> {
        if offset + data.len() as u64 > self.size {
            return Err(DriverError::MmapFailed(
                format!(
                    "write out of bounds: offset={offset}, len={}, size={}",
                    data.len(),
                    self.size
                )
                .into(),
            ));
        }
        let mmap_offset = ioctl::gem_mmap_offset(fd, self.gem_handle)?;
        let buf_len = usize::try_from(self.size).map_err(|_| {
            DriverError::platform_overflow("buffer size exceeds platform pointer width")
        })?;
        let mmap_off = libc::off_t::try_from(mmap_offset).map_err(|_| {
            DriverError::platform_overflow("mmap offset exceeds platform off_t range")
        })?;
        let mut region = MappedRegion::new(
            buf_len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            mmap_off,
        )?;
        let byte_offset = usize::try_from(offset)
            .map_err(|_| DriverError::platform_overflow("offset exceeds platform pointer width"))?;
        region
            .slice_at_mut(byte_offset, data.len())?
            .copy_from_slice(data);
        Ok(())
    }

    /// Read data from the buffer via mmap.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the read exceeds buffer bounds or mmap fails.
    pub fn read(&self, fd: RawFd, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        if offset + len as u64 > self.size {
            return Err(DriverError::MmapFailed(
                format!(
                    "read out of bounds: offset={offset}, len={len}, size={}",
                    self.size
                )
                .into(),
            ));
        }
        let mmap_offset = ioctl::gem_mmap_offset(fd, self.gem_handle)?;
        let buf_len = usize::try_from(self.size).map_err(|_| {
            DriverError::platform_overflow("buffer size exceeds platform pointer width")
        })?;
        let mmap_off = libc::off_t::try_from(mmap_offset).map_err(|_| {
            DriverError::platform_overflow("mmap offset exceeds platform off_t range")
        })?;
        let region = MappedRegion::new(buf_len, libc::PROT_READ, libc::MAP_SHARED, fd, mmap_off)?;
        let byte_offset = usize::try_from(offset)
            .map_err(|_| DriverError::platform_overflow("offset exceeds platform pointer width"))?;
        Ok(region.slice_at(byte_offset, len)?.to_vec())
    }

    /// Close/free the GEM buffer object via `DRM_IOCTL_GEM_CLOSE`.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the GEM close ioctl fails.
    pub fn close(self, fd: RawFd) -> DriverResult<()> {
        crate::drm::gem_close(fd, self.gem_handle)?;
        tracing::debug!(handle = self.gem_handle, "GEM buffer closed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gem_buffer_fields() {
        let buf = GemBuffer {
            gem_handle: 42,
            size: 4096,
            gpu_va: 0x1000,
            domain: MemoryDomain::Vram,
        };
        assert_eq!(buf.gem_handle, 42);
        assert_eq!(buf.size, 4096);
        assert_eq!(buf.gpu_va, 0x1000);
        assert!(matches!(buf.domain, MemoryDomain::Vram));
    }

    #[test]
    fn gem_buffer_debug() {
        let buf = GemBuffer {
            gem_handle: 1,
            size: 256,
            gpu_va: 0x2000,
            domain: MemoryDomain::Gtt,
        };
        let dbg = format!("{buf:?}");
        assert!(dbg.contains("GemBuffer"));
        assert!(dbg.contains("256"));
    }

    #[test]
    fn write_out_of_bounds_returns_error() {
        let buf = GemBuffer {
            gem_handle: 0,
            size: 100,
            gpu_va: 0,
            domain: MemoryDomain::Vram,
        };
        // Write beyond buffer size - should fail at bounds check before ioctl
        let result = buf.write(-1, 50, &[0u8; 100]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of bounds"));
    }

    #[test]
    fn read_out_of_bounds_returns_error() {
        let buf = GemBuffer {
            gem_handle: 0,
            size: 100,
            gpu_va: 0,
            domain: MemoryDomain::Vram,
        };
        // Read beyond buffer size
        let result = buf.read(-1, 50, 100);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of bounds"));
    }
}
