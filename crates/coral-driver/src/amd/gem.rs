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

    fn as_ptr(&self) -> *mut u8 {
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
    pub fn write(&self, fd: RawFd, offset: u64, data: &[u8]) -> DriverResult<()> {
        if offset + data.len() as u64 > self.size {
            return Err(DriverError::MmapFailed(format!(
                "write out of bounds: offset={offset}, len={}, size={}",
                data.len(),
                self.size
            )));
        }
        let mmap_offset = ioctl::gem_mmap_offset(fd, self.gem_handle)?;
        let region = MappedRegion::new(
            self.size as usize,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            mmap_offset as libc::off_t,
        )?;
        // Safety: region is valid for self.size bytes, bounds checked above.
        unsafe {
            ptr::copy_nonoverlapping(
                data.as_ptr(),
                region.as_ptr().add(offset as usize),
                data.len(),
            );
        }
        Ok(())
    }

    /// Read data from the buffer via mmap.
    pub fn read(&self, fd: RawFd, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        if offset + len as u64 > self.size {
            return Err(DriverError::MmapFailed(format!(
                "read out of bounds: offset={offset}, len={len}, size={}",
                self.size
            )));
        }
        let mmap_offset = ioctl::gem_mmap_offset(fd, self.gem_handle)?;
        let region = MappedRegion::new(
            self.size as usize,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            mmap_offset as libc::off_t,
        )?;
        let mut result = vec![0u8; len];
        // Safety: region is valid for self.size bytes, bounds checked above.
        unsafe {
            ptr::copy_nonoverlapping(
                region.as_ptr().add(offset as usize),
                result.as_mut_ptr(),
                len,
            );
        }
        Ok(result)
    }

    /// Close/free the GEM buffer object via `DRM_IOCTL_GEM_CLOSE`.
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
