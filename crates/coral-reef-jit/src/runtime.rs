// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign JIT memory allocator — pure-Rust executable memory via `rustix`.
//!
//! Replaces `cranelift-jit`'s `libc`-dependent memory management with direct
//! Linux syscalls through `rustix`, eliminating the `libc`, `region`, and
//! `wasmtime-internal-jit-icache-coherence` transitive dependencies.
//!
//! ## Memory model
//!
//! JIT code pages follow the W^X (write XOR execute) discipline:
//! 1. Allocate pages as writable: `mmap(PROT_READ | PROT_WRITE)`
//! 2. Write compiled machine code into the pages
//! 3. Transition to executable: `mprotect(PROT_READ | PROT_EXEC)`
//! 4. Flush instruction cache (required on aarch64, no-op on x86-64)
//!
//! ## Platform
//!
//! `rustix` with the `linux_raw` backend makes syscalls directly to the Linux
//! kernel, bypassing `libc` entirely. This is the same approach used by
//! `coral-driver` for GPU `ioctl` calls.

use std::ptr::NonNull;

use rustix::mm::{MapFlags, MprotectFlags, ProtFlags};

use crate::error::JitError;

/// Minimum allocation granularity (4 KiB pages on both x86-64 and aarch64).
const PAGE_SIZE: usize = 4096;

/// Round `size` up to the nearest page boundary.
const fn page_align(size: usize) -> usize {
    (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

/// A region of executable memory allocated via `rustix` direct syscalls.
///
/// Manages the lifecycle of JIT-compiled code: allocation, writing, W^X
/// transition, and deallocation on drop. This replaces `cranelift-jit`'s
/// use of `libc::mmap` / `libc::mprotect` / `libc::munmap`.
pub struct JitMemory {
    ptr: NonNull<u8>,
    len: usize,
    executable: bool,
}

impl JitMemory {
    /// Allocate `size` bytes of writable memory for JIT code generation.
    ///
    /// The memory is allocated as `PROT_READ | PROT_WRITE` (not yet executable).
    /// After writing machine code, call [`make_executable`](Self::make_executable)
    /// to apply the W^X transition.
    ///
    /// # Errors
    ///
    /// Returns [`JitError::Execution`] if the `mmap` syscall fails.
    pub fn allocate(size: usize) -> Result<Self, JitError> {
        let len = page_align(size.max(1));

        // SAFETY: mmap_anonymous with non-null length and valid flags is safe.
        // The returned pointer is page-aligned and the kernel guarantees the
        // mapping is valid for `len` bytes.
        #[expect(unsafe_code, reason = "mmap requires unsafe")]
        let ptr = unsafe {
            rustix::mm::mmap_anonymous(
                std::ptr::null_mut(),
                len,
                ProtFlags::READ | ProtFlags::WRITE,
                MapFlags::PRIVATE,
            )
        }
        .map_err(|e| JitError::Execution(format!("mmap failed: {e}")))?;

        let ptr = NonNull::new(ptr.cast::<u8>())
            .ok_or_else(|| JitError::Execution("mmap returned null".into()))?;

        Ok(Self {
            ptr,
            len,
            executable: false,
        })
    }

    /// Write machine code into the buffer at the given offset.
    ///
    /// # Errors
    ///
    /// Returns [`JitError::Execution`] if the write would exceed the allocation
    /// or if the memory has already been made executable.
    pub fn write(&mut self, offset: usize, data: &[u8]) -> Result<(), JitError> {
        if self.executable {
            return Err(JitError::Execution(
                "cannot write to executable memory (W^X)".into(),
            ));
        }
        let end = offset
            .checked_add(data.len())
            .ok_or_else(|| JitError::Execution("write offset overflow".into()))?;
        if end > self.len {
            return Err(JitError::Execution(format!(
                "write {end} exceeds allocation {len}",
                len = self.len
            )));
        }

        // SAFETY: ptr + offset is within the mmap'd region, and we've verified bounds.
        #[expect(unsafe_code, reason = "raw pointer write to mmap'd region")]
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.ptr.as_ptr().add(offset), data.len());
        }
        Ok(())
    }

    /// Transition the memory from writable to executable (W^X).
    ///
    /// After this call, the memory becomes `PROT_READ | PROT_EXEC` and can no
    /// longer be written to. On aarch64, this also flushes the instruction cache.
    ///
    /// # Errors
    ///
    /// Returns [`JitError::Execution`] if the `mprotect` syscall fails.
    pub fn make_executable(&mut self) -> Result<(), JitError> {
        // SAFETY: ptr and len define a valid mmap'd region that we own.
        #[expect(unsafe_code, reason = "mprotect requires unsafe")]
        unsafe {
            rustix::mm::mprotect(
                self.ptr.as_ptr().cast(),
                self.len,
                MprotectFlags::READ | MprotectFlags::EXEC,
            )
        }
        .map_err(|e| JitError::Execution(format!("mprotect R+X failed: {e}")))?;

        self.executable = true;
        flush_icache(self.ptr.as_ptr(), self.len);
        Ok(())
    }

    /// Get the base pointer to the executable code region.
    ///
    /// # Errors
    ///
    /// Returns [`JitError::Execution`] if the memory hasn't been made executable yet.
    pub fn code_ptr(&self) -> Result<*const u8, JitError> {
        if !self.executable {
            return Err(JitError::Execution("memory not yet executable".into()));
        }
        Ok(self.ptr.as_ptr())
    }

    /// Get the allocated length in bytes (page-aligned).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the allocation is zero-sized (never true in practice).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Drop for JitMemory {
    fn drop(&mut self) {
        // SAFETY: ptr and len were produced by our mmap call and are still valid.
        #[expect(unsafe_code, reason = "munmap requires unsafe")]
        let _ = unsafe { rustix::mm::munmap(self.ptr.as_ptr().cast(), self.len) };
    }
}

// SAFETY: JitMemory owns its allocation exclusively; no aliasing.
#[expect(unsafe_code, reason = "JitMemory owns its mmap'd region")]
unsafe impl Send for JitMemory {}

/// Flush the instruction cache for the given memory region.
///
/// Required on aarch64 where instruction and data caches are not coherent.
/// On x86-64 this is a no-op (caches are always coherent).
#[allow(clippy::missing_const_for_fn)]
fn flush_icache(ptr: *const u8, len: usize) {
    #[cfg(target_arch = "aarch64")]
    {
        // ARM cache maintenance: clean data cache, invalidate instruction cache.
        // Each line must be processed at cache-line granularity (typically 64 bytes).
        const CACHE_LINE: usize = 64;
        let start = ptr as usize;
        let end = start + len;
        let mut addr = start & !(CACHE_LINE - 1);
        while addr < end {
            // SAFETY: inline asm for cache maintenance on valid address range.
            #[expect(unsafe_code, reason = "ARM cache maintenance instructions")]
            unsafe {
                std::arch::asm!(
                    "dc civac, {addr}",
                    "ic ivau, {addr}",
                    addr = in(reg) addr,
                    options(nostack, preserves_flags),
                );
            }
            addr += CACHE_LINE;
        }
        // SAFETY: barrier instructions.
        #[expect(unsafe_code, reason = "ARM barrier instructions")]
        unsafe {
            std::arch::asm!("dsb ish", "isb", options(nostack, preserves_flags));
        }
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
        let _ = (ptr, len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_and_write() {
        let mut mem = JitMemory::allocate(128).expect("allocate");
        assert!(mem.len() >= 128);
        assert!(!mem.is_empty());

        let code = [0x90u8; 64]; // NOP sled
        mem.write(0, &code).expect("write");
    }

    #[test]
    fn write_bounds_check() {
        let mut mem = JitMemory::allocate(PAGE_SIZE).expect("allocate");
        let result = mem.write(PAGE_SIZE - 1, &[0, 0]);
        assert!(result.is_err(), "should reject out-of-bounds write");
    }

    #[test]
    fn wxe_transition() {
        let mut mem = JitMemory::allocate(PAGE_SIZE).expect("allocate");
        let code = [0xC3u8]; // x86-64 RET
        mem.write(0, &code).expect("write");
        mem.make_executable().expect("mprotect");

        let ptr = mem.code_ptr().expect("code_ptr");
        assert!(!ptr.is_null());

        let write_after = mem.write(0, &[0x90]);
        assert!(write_after.is_err(), "write after executable should fail");
    }

    #[test]
    fn page_alignment() {
        assert_eq!(page_align(0), 0);
        assert_eq!(page_align(1), PAGE_SIZE);
        assert_eq!(page_align(PAGE_SIZE), PAGE_SIZE);
        assert_eq!(page_align(PAGE_SIZE + 1), PAGE_SIZE * 2);
    }

    #[test]
    fn code_ptr_before_executable_fails() {
        let mem = JitMemory::allocate(64).expect("allocate");
        assert!(mem.code_ptr().is_err());
    }
}
