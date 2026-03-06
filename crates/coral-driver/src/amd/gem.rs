// SPDX-License-Identifier: AGPL-3.0-only
//! GEM (Graphics Execution Manager) buffer objects for AMD GPUs.

use super::ioctl;
use crate::error::{DriverError, DriverResult};
use crate::MemoryDomain;
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
    pub fn create(fd: RawFd, size: u64, domain: MemoryDomain) -> DriverResult<Self> {
        let domain_flags = match domain {
            MemoryDomain::Vram => ioctl::AMDGPU_GEM_DOMAIN_VRAM,
            MemoryDomain::Gtt => ioctl::AMDGPU_GEM_DOMAIN_GTT,
            MemoryDomain::VramOrGtt => {
                ioctl::AMDGPU_GEM_DOMAIN_VRAM | ioctl::AMDGPU_GEM_DOMAIN_GTT
            }
        };

        let (handle, actual_size) = ioctl::gem_create(fd, size, domain_flags)?;

        // Allocate a GPU VA for this buffer
        // In practice, VA ranges are managed by a VA allocator.
        // For the scaffold, we use the handle as a stand-in.
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

        // Safety: mmap the buffer, copy data, munmap
        unsafe {
            let ptr = libc_mmap(
                std::ptr::null_mut(),
                self.size as usize,
                0x3, // PROT_READ | PROT_WRITE
                0x1, // MAP_SHARED
                fd,
                mmap_offset as i64,
            );
            if ptr == usize::MAX as *mut u8 {
                return Err(DriverError::MmapFailed("mmap returned MAP_FAILED".into()));
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.add(offset as usize), data.len());
            libc_munmap(ptr, self.size as usize);
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

        unsafe {
            let ptr = libc_mmap(
                std::ptr::null_mut(),
                self.size as usize,
                0x1, // PROT_READ
                0x1, // MAP_SHARED
                fd,
                mmap_offset as i64,
            );
            if ptr == usize::MAX as *mut u8 {
                return Err(DriverError::MmapFailed("mmap returned MAP_FAILED".into()));
            }
            let mut result = vec![0u8; len];
            std::ptr::copy_nonoverlapping(ptr.add(offset as usize), result.as_mut_ptr(), len);
            libc_munmap(ptr, self.size as usize);
            Ok(result)
        }
    }

    /// Close/free the GEM buffer object.
    pub fn close(self, _fd: RawFd) -> DriverResult<()> {
        // DRM_IOCTL_GEM_CLOSE to release the kernel handle
        // The kernel reclaims the BO when the last reference drops.
        tracing::debug!(handle = self.gem_handle, "GEM close (scaffold)");
        Ok(())
    }
}

/// Raw mmap syscall (SYS_mmap = 9 on x86_64).
///
/// # Safety
///
/// Caller must ensure valid fd and offset.
#[cfg(target_arch = "x86_64")]
unsafe fn libc_mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: RawFd, offset: i64) -> *mut u8 {
    let ret: u64;
    unsafe {
        std::arch::asm!(
            "syscall",
            in("rax") 9_u64,
            in("rdi") addr as u64,
            in("rsi") len as u64,
            in("rdx") prot as u64,
            in("r10") flags as u64,
            in("r8") fd as u64,
            in("r9") offset as u64,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret as *mut u8
}

/// Raw munmap syscall (SYS_munmap = 11 on x86_64).
///
/// # Safety
///
/// Caller must ensure the pointer was returned by mmap.
#[cfg(target_arch = "x86_64")]
unsafe fn libc_munmap(addr: *mut u8, len: usize) {
    unsafe {
        std::arch::asm!(
            "syscall",
            in("rax") 11_u64,
            in("rdi") addr as u64,
            in("rsi") len as u64,
            lateout("rax") _,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
}

/// Stub for non-x86_64 architectures.
#[cfg(not(target_arch = "x86_64"))]
unsafe fn libc_mmap(_addr: *mut u8, _len: usize, _prot: i32, _flags: i32, _fd: RawFd, _offset: i64) -> *mut u8 {
    usize::MAX as *mut u8
}

#[cfg(not(target_arch = "x86_64"))]
unsafe fn libc_munmap(_addr: *mut u8, _len: usize) {}
