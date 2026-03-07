// SPDX-License-Identifier: AGPL-3.0-only
//! GEM (Graphics Execution Manager) buffer objects for AMD GPUs.

use super::ioctl;
use crate::MemoryDomain;
use crate::error::{DriverError, DriverResult};
use std::os::unix::io::RawFd;
use std::ptr;

/// RAII wrapper around a memory-mapped region. Unmaps on drop.
struct MappedRegion {
    ptr: *mut libc::c_void,
    len: usize,
}

impl MappedRegion {
    /// Map a file descriptor region into memory.
    fn new(
        len: usize,
        prot: libc::c_int,
        flags: libc::c_int,
        fd: RawFd,
        offset: libc::off_t,
    ) -> DriverResult<Self> {
        // Safety: standard POSIX mmap with validated arguments.
        let ptr = unsafe { libc::mmap(ptr::null_mut(), len, prot, flags, fd, offset) };
        if ptr == libc::MAP_FAILED {
            return Err(DriverError::MmapFailed("mmap returned MAP_FAILED".into()));
        }
        Ok(Self { ptr, len })
    }

    const fn as_ptr(&self) -> *mut u8 {
        self.ptr.cast()
    }
}

impl Drop for MappedRegion {
    fn drop(&mut self) {
        // Safety: ptr was returned by a successful mmap call with the same len.
        unsafe { libc::munmap(self.ptr, self.len) };
    }
}

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

        let gpu_va = 0x0001_0000_0000_u64 + u64::from(handle) * 0x1000_0000;

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
    ///
    /// # Panics
    ///
    /// Panics if the buffer size or offset exceeds the platform pointer width
    /// (impossible on 64-bit systems where this driver runs).
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
        let buf_len =
            usize::try_from(self.size).expect("buffer size exceeds platform pointer width");
        let mmap_off =
            libc::off_t::try_from(mmap_offset).expect("mmap offset exceeds platform off_t range");
        let region = MappedRegion::new(
            buf_len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            mmap_off,
        )?;
        let byte_offset = usize::try_from(offset).expect("offset exceeds platform pointer width");
        // Safety: region is valid for self.size bytes, bounds checked above.
        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr(), region.as_ptr().add(byte_offset), data.len());
        }
        Ok(())
    }

    /// Read data from the buffer via mmap.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the read exceeds buffer bounds or mmap fails.
    ///
    /// # Panics
    ///
    /// Panics if the buffer size or offset exceeds the platform pointer width
    /// (impossible on 64-bit systems where this driver runs).
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
        let buf_len =
            usize::try_from(self.size).expect("buffer size exceeds platform pointer width");
        let mmap_off =
            libc::off_t::try_from(mmap_offset).expect("mmap offset exceeds platform off_t range");
        let region = MappedRegion::new(buf_len, libc::PROT_READ, libc::MAP_SHARED, fd, mmap_off)?;
        let byte_offset = usize::try_from(offset).expect("offset exceeds platform pointer width");
        let mut result = vec![0u8; len];
        // Safety: region is valid for self.size bytes, bounds checked above.
        unsafe {
            ptr::copy_nonoverlapping(region.as_ptr().add(byte_offset), result.as_mut_ptr(), len);
        }
        Ok(result)
    }

    /// Close/free the GEM buffer object via `DRM_IOCTL_GEM_CLOSE`.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the GEM close ioctl fails.
    pub fn close(self, fd: RawFd) -> DriverResult<()> {
        let mut args = crate::drm::DrmGemClose {
            handle: self.gem_handle,
            pad: 0,
        };
        // Safety: DrmGemClose is #[repr(C)] and matches the kernel struct.
        unsafe { crate::drm::drm_ioctl_typed(fd, crate::drm::DRM_IOCTL_GEM_CLOSE, &mut args)? };
        tracing::debug!(handle = self.gem_handle, "GEM buffer closed");
        Ok(())
    }
}
