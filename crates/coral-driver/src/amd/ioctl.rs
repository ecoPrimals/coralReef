// SPDX-License-Identifier: AGPL-3.0-only
//! AMD-specific DRM ioctl definitions.
//!
//! Structures and constants from the amdgpu kernel driver, defined in
//! pure Rust (no `amdgpu-sys` or `drm-sys`).

#[cfg(doc)]
use crate::error::DriverError;
use crate::error::DriverResult;
use std::os::unix::io::RawFd;

// amdgpu DRM ioctl command numbers (from amdgpu_drm.h)
const DRM_COMMAND_BASE: u32 = 0x40;
const DRM_AMDGPU_GEM_CREATE: u32 = DRM_COMMAND_BASE;
const DRM_AMDGPU_GEM_MMAP: u32 = DRM_COMMAND_BASE + 0x01;
const DRM_AMDGPU_CTX: u32 = DRM_COMMAND_BASE + 0x02;
const DRM_AMDGPU_GEM_VA: u32 = DRM_COMMAND_BASE + 0x08;
const DRM_AMDGPU_BO_LIST: u32 = DRM_COMMAND_BASE + 0x03;
const DRM_AMDGPU_CS: u32 = DRM_COMMAND_BASE + 0x04;
const DRM_AMDGPU_WAIT_CS: u32 = DRM_COMMAND_BASE + 0x09;

// Domain flags
pub const AMDGPU_GEM_DOMAIN_VRAM: u32 = 0x4;
pub const AMDGPU_GEM_DOMAIN_GTT: u32 = 0x2;

// Context operations
const AMDGPU_CTX_OP_ALLOC_CTX: u32 = 1;
const AMDGPU_CTX_OP_FREE_CTX: u32 = 2;

// VA operations
pub const AMDGPU_VA_OP_MAP: u32 = 1;
pub const AMDGPU_VA_OP_UNMAP: u32 = 2;
pub const AMDGPU_VA_FLAGS_NONE: u64 = 0;

// VM page protection flags (from amdgpu_drm.h)
pub const AMDGPU_VM_PAGE_READABLE: u32 = 1 << 1;
pub const AMDGPU_VM_PAGE_WRITEABLE: u32 = 1 << 2;
pub const AMDGPU_VM_PAGE_EXECUTABLE: u32 = 1 << 3;

/// GEM create — matches `union drm_amdgpu_gem_create` (32 bytes).
///
/// Input: bo_size, alignment, domains, domain_flags.
/// Output: kernel overwrites first 8 bytes with `{ handle: u32, pad: u32 }`.
#[repr(C)]
#[derive(Debug, Default)]
pub struct AmdgpuGemCreate {
    pub bo_size: u64,
    pub alignment: u64,
    pub domains: u64,
    pub domain_flags: u64,
}

/// GEM mmap — matches `union drm_amdgpu_gem_mmap` (8 bytes).
///
/// Input: `handle` (u32) at offset 0.
/// Output: kernel overwrites with `addr_ptr` (u64) at offset 0.
#[repr(C)]
#[derive(Debug, Default)]
pub struct AmdgpuGemMmap {
    pub handle_or_addr: u64,
}

/// Context operation input/output.
#[repr(C)]
#[derive(Debug, Default)]
pub struct AmdgpuCtx {
    pub op: u32,
    pub flags: u32,
    pub ctx_id: u32,
    pub pad: u32,
}

/// GEM VA mapping.
#[repr(C)]
#[derive(Debug, Default)]
pub struct AmdgpuGemVa {
    pub handle: u32,
    pub pad: u32,
    pub operation: u32,
    pub flags: u32,
    pub va_address: u64,
    pub offset_in_bo: u64,
    pub map_size: u64,
}

/// Size of a `#[repr(C)]` struct as a `u32` for ioctl encoding.
#[expect(
    clippy::cast_possible_truncation,
    reason = "asserted in bounds; kernel ioctl structs are always < 4 GiB"
)]
const fn size_of_u32<T>() -> u32 {
    assert!(std::mem::size_of::<T>() <= u32::MAX as usize);
    std::mem::size_of::<T>() as u32
}

/// Encode a Rust reference as a kernel-compatible `u64` pointer.
///
/// DRM ioctls use `__u64` for all pointer fields to support 32-bit userspace
/// on 64-bit kernels. This helper makes the intent explicit and centralizes
/// the raw-pointer conversion.
fn kernel_ptr<T>(r: &T) -> u64 {
    std::ptr::from_ref(r) as u64
}

/// Read the kernel's output from a `#[repr(C)]` ioctl struct.
///
/// DRM ioctls use C unions: the kernel writes output fields at the start of
/// the same struct used for input. This reads the first `size_of::<R>()` bytes.
///
/// # Safety
///
/// `T` must be `#[repr(C)]` and at least `size_of::<R>()` bytes.
/// The kernel must have successfully written output via the ioctl.
const unsafe fn read_ioctl_output<T, R: Copy>(arg: &T) -> R {
    // SAFETY: Caller guarantees `T` is `#[repr(C)]` with at least
    // `size_of::<R>()` bytes at offset 0. The kernel wrote valid data
    // there during the preceding successful ioctl call.
    unsafe { std::ptr::read(std::ptr::from_ref(arg).cast::<R>()) }
}

/// Create an amdgpu GPU context.
///
/// # Errors
///
/// Returns [`DriverError`] if the context allocation ioctl fails.
pub fn create_context(fd: RawFd) -> DriverResult<u32> {
    let mut ctx = AmdgpuCtx {
        op: AMDGPU_CTX_OP_ALLOC_CTX,
        ..Default::default()
    };
    // SAFETY: `AmdgpuCtx` is `#[repr(C)]` matching the kernel's `drm_amdgpu_ctx`.
    // Stack-allocated, valid for the synchronous ioctl lifetime.
    unsafe {
        crate::drm::drm_ioctl_named(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_CTX, size_of_u32::<AmdgpuCtx>()),
            &mut ctx,
            "AMDGPU_CTX_ALLOC",
        )?;
    }
    // Kernel writes { ctx_id: u32, _pad: u32 } at offset 0 (output union overlay).
    // SAFETY: kernel wrote ctx_id at offset 0 after successful alloc.
    let ctx_id = unsafe { read_ioctl_output::<_, u32>(&ctx) };
    Ok(ctx_id)
}

/// Destroy an amdgpu GPU context.
///
/// # Errors
///
/// Returns [`DriverError`] if the context free ioctl fails.
pub fn destroy_context(fd: RawFd, ctx_id: u32) -> DriverResult<()> {
    let mut ctx = AmdgpuCtx {
        op: AMDGPU_CTX_OP_FREE_CTX,
        ctx_id,
        ..Default::default()
    };
    // SAFETY: `AmdgpuCtx` is `#[repr(C)]` matching the kernel struct. Synchronous ioctl.
    unsafe {
        crate::drm::drm_ioctl_named(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_CTX, size_of_u32::<AmdgpuCtx>()),
            &mut ctx,
            "AMDGPU_CTX_FREE",
        )
    }
}

/// Create a GEM buffer object.
///
/// # Errors
///
/// Returns [`DriverError`] if the GEM create ioctl fails.
pub fn gem_create(fd: RawFd, size: u64, domains: u32) -> DriverResult<(u32, u64)> {
    let page_size = 4096_u64;
    let aligned_size = size.next_multiple_of(page_size);
    let mut req = AmdgpuGemCreate {
        bo_size: aligned_size,
        alignment: page_size,
        domains: domains.into(),
        ..Default::default()
    };
    // SAFETY: AmdgpuGemCreate is #[repr(C)] and matches the kernel union (32 bytes).
    unsafe {
        crate::drm::drm_ioctl_named(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_GEM_CREATE, size_of_u32::<AmdgpuGemCreate>()),
            &mut req,
            "AMDGPU_GEM_CREATE",
        )?;
    }
    // Kernel writes { handle: u32, pad: u32 } at offset 0 (union overlay).
    // SAFETY: kernel wrote output at offset 0 after successful ioctl.
    let handle = unsafe { read_ioctl_output::<_, u32>(&req) };
    Ok((handle, aligned_size))
}

/// Get the mmap offset for a GEM buffer.
///
/// # Errors
///
/// Returns [`DriverError`] if the GEM mmap ioctl fails.
pub fn gem_mmap_offset(fd: RawFd, handle: u32) -> DriverResult<u64> {
    let mut req = AmdgpuGemMmap {
        handle_or_addr: u64::from(handle),
    };
    // SAFETY: AmdgpuGemMmap is #[repr(C)] and matches the kernel union (8 bytes).
    unsafe {
        crate::drm::drm_ioctl_named(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_GEM_MMAP, size_of_u32::<AmdgpuGemMmap>()),
            &mut req,
            "AMDGPU_GEM_MMAP",
        )?;
    }
    // Kernel writes addr_ptr (u64) at offset 0.
    Ok(req.handle_or_addr)
}

/// Map a GEM buffer to a GPU virtual address.
///
/// # Errors
///
/// Returns [`DriverError`] if the VA map ioctl fails.
pub fn gem_va_map(fd: RawFd, handle: u32, va: u64, size: u64) -> DriverResult<()> {
    let flags = AMDGPU_VM_PAGE_READABLE | AMDGPU_VM_PAGE_WRITEABLE | AMDGPU_VM_PAGE_EXECUTABLE;
    let mut req = AmdgpuGemVa {
        handle,
        operation: AMDGPU_VA_OP_MAP,
        flags,
        va_address: va,
        map_size: size,
        ..Default::default()
    };
    // SAFETY: AmdgpuGemVa is #[repr(C)] and matches the kernel struct.
    unsafe {
        crate::drm::drm_ioctl_named(
            fd,
            crate::drm::drm_iow_pub(DRM_AMDGPU_GEM_VA, size_of_u32::<AmdgpuGemVa>()),
            &mut req,
            "AMDGPU_GEM_VA_MAP",
        )
    }
}

// --- BO list structs (drm_amdgpu_bo_list) ---

const AMDGPU_BO_LIST_OP_CREATE: u32 = 0;
const AMDGPU_BO_LIST_OP_DESTROY: u32 = 1;

#[repr(C)]
#[derive(Debug, Default)]
struct AmdgpuBoListEntry {
    bo_handle: u32,
    bo_priority: u32,
}

/// Input for `DRM_IOCTL_AMDGPU_BO_LIST` — matches `drm_amdgpu_bo_list_in`.
/// Union output (`list_handle`) overlaps the first 4 bytes.
#[repr(C)]
#[derive(Debug, Default)]
struct AmdgpuBoListIn {
    operation: u32,
    list_handle: u32,
    bo_number: u32,
    bo_info_size: u32,
    bo_info_ptr: u64,
}

// --- CS submission structs (drm_amdgpu_cs) ---

const AMDGPU_CHUNK_ID_IB: u32 = 0x01;
const AMDGPU_HW_IP_COMPUTE: u32 = 1;

#[repr(C)]
#[derive(Debug, Default)]
struct AmdgpuCsChunk {
    chunk_id: u32,
    length_dw: u32,
    chunk_data: u64,
}

/// IB chunk data — matches `drm_amdgpu_cs_chunk_ib`.
#[repr(C)]
#[derive(Debug, Default)]
struct AmdgpuCsChunkIb {
    pad: u32,
    flags: u32,
    va_start: u64,
    ib_bytes: u32,
    ip_type: u32,
    ip_instance: u32,
    ring: u32,
}

/// CS input — matches `drm_amdgpu_cs_in` (24 bytes, same as union).
/// After ioctl, first 8 bytes contain the fence handle (`drm_amdgpu_cs_out`).
#[repr(C)]
#[derive(Debug, Default)]
struct AmdgpuCsIn {
    ctx_id: u32,
    bo_list_handle: u32,
    num_chunks: u32,
    flags: u32,
    chunks: u64,
}

// --- Wait CS structs (drm_amdgpu_wait_cs) ---

/// Wait CS input — matches `drm_amdgpu_wait_cs_in` (32 bytes, same as union).
/// After ioctl, first 8 bytes contain status (`drm_amdgpu_wait_cs_out`).
#[repr(C)]
#[derive(Debug, Default)]
struct AmdgpuWaitCsIn {
    handle: u64,
    timeout: u64,
    ip_type: u32,
    ip_instance: u32,
    ring: u32,
    ctx_id: u32,
}

/// Create a BO (buffer object) list for command submission.
///
/// # Errors
///
/// Returns [`DriverError`] if the BO list creation ioctl fails.
pub fn create_bo_list(fd: RawFd, handles: &[u32]) -> DriverResult<u32> {
    let entries: Vec<AmdgpuBoListEntry> = handles
        .iter()
        .map(|&h| AmdgpuBoListEntry {
            bo_handle: h,
            bo_priority: 0,
        })
        .collect();

    let mut req = AmdgpuBoListIn {
        operation: AMDGPU_BO_LIST_OP_CREATE,
        bo_number: u32::try_from(entries.len())
            .map_err(|_| crate::error::DriverError::platform_overflow("BO count fits in u32"))?,
        bo_info_size: size_of_u32::<AmdgpuBoListEntry>(),
        bo_info_ptr: entries.first().map_or(0, kernel_ptr),
        ..Default::default()
    };

    // SAFETY: AmdgpuBoListIn is #[repr(C)] and matches the kernel union size.
    // entries slice lives until after the ioctl returns.
    unsafe {
        crate::drm::drm_ioctl_named(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_BO_LIST, size_of_u32::<AmdgpuBoListIn>()),
            &mut req,
            "AMDGPU_BO_LIST_CREATE",
        )?;
    }

    // Kernel writes list_handle to first u32 (union overlay with drm_amdgpu_bo_list_out).
    // SAFETY: kernel writes list_handle (u32) at offset 0 of the output union.
    Ok(unsafe { read_ioctl_output::<_, u32>(&req) })
}

/// Destroy a BO list.
///
/// # Errors
///
/// Returns [`DriverError`] if the BO list destruction ioctl fails.
pub fn destroy_bo_list(fd: RawFd, list_handle: u32) -> DriverResult<()> {
    let mut req = AmdgpuBoListIn {
        operation: AMDGPU_BO_LIST_OP_DESTROY,
        list_handle,
        ..Default::default()
    };

    // SAFETY: AmdgpuBoListIn is #[repr(C)] and matches the kernel union.
    unsafe {
        crate::drm::drm_ioctl_named(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_BO_LIST, size_of_u32::<AmdgpuBoListIn>()),
            &mut req,
            "AMDGPU_BO_LIST_DESTROY",
        )
    }
}

/// Submit an indirect buffer (IB) to the compute ring.
///
/// The IB must reside in a GPU-mapped GEM buffer at `ib_va`.
/// Returns the fence sequence number for synchronization.
///
/// # Errors
///
/// Returns [`DriverError`] if the CS ioctl fails.
pub fn submit_command(
    fd: RawFd,
    ctx_id: u32,
    bo_list_handle: u32,
    ib_va: u64,
    ib_bytes: u32,
) -> DriverResult<u64> {
    let ib = AmdgpuCsChunkIb {
        va_start: ib_va,
        ib_bytes,
        ip_type: AMDGPU_HW_IP_COMPUTE,
        ..Default::default()
    };

    let chunk = AmdgpuCsChunk {
        chunk_id: AMDGPU_CHUNK_ID_IB,
        length_dw: size_of_u32::<AmdgpuCsChunkIb>() / 4,
        chunk_data: kernel_ptr(&ib),
    };

    let chunk_ptrs: [u64; 1] = [kernel_ptr(&chunk)];

    let mut cs = AmdgpuCsIn {
        ctx_id,
        bo_list_handle,
        num_chunks: 1,
        chunks: kernel_ptr(&chunk_ptrs[0]),
        ..Default::default()
    };

    tracing::debug!(ctx = ctx_id, ib_va, ib_bytes, "AMD CS submit");

    // SAFETY: All structs are #[repr(C)] and stack-allocated;
    // pointers remain valid for the duration of the synchronous ioctl.
    unsafe {
        crate::drm::drm_ioctl_named(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_CS, size_of_u32::<AmdgpuCsIn>()),
            &mut cs,
            "AMDGPU_CS_SUBMIT",
        )?;
    }

    // Kernel writes fence handle to first 8 bytes (union drm_amdgpu_cs.out.handle).
    // SAFETY: kernel writes fence handle (u64) at offset 0 of the output union.
    let fence = unsafe { read_ioctl_output::<_, u64>(&cs) };
    Ok(fence)
}

/// Wait for a GPU fence to signal.
///
/// Blocks until the fence identified by `fence_handle` completes or
/// `timeout_ns` nanoseconds elapse.
///
/// # Errors
///
/// Returns [`DriverError::FenceTimeout`] if the fence does not complete
/// within `timeout_ns`, or [`DriverError`] if the ioctl fails.
pub fn sync_fence(fd: RawFd, ctx_id: u32, fence_handle: u64, timeout_ns: u64) -> DriverResult<()> {
    // amdgpu WAIT_CS expects an absolute timeout (CLOCK_MONOTONIC nanoseconds)
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: timespec is a valid output struct for clock_gettime.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    let now_ns = ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64;
    let abs_timeout = now_ns.saturating_add(timeout_ns);

    let mut wait = AmdgpuWaitCsIn {
        handle: fence_handle,
        timeout: abs_timeout,
        ip_type: AMDGPU_HW_IP_COMPUTE,
        ctx_id,
        ..Default::default()
    };

    tracing::debug!(ctx = ctx_id, fence = fence_handle, "AMD fence sync");

    // SAFETY: AmdgpuWaitCsIn is #[repr(C)] and matches the kernel union.
    unsafe {
        crate::drm::drm_ioctl_named(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_WAIT_CS, size_of_u32::<AmdgpuWaitCsIn>()),
            &mut wait,
            "AMDGPU_WAIT_CS",
        )?;
    }

    // Kernel writes status to first 8 bytes (union drm_amdgpu_wait_cs.out.status).
    // 0 = completed, 1 = timed out.
    let status = unsafe { read_ioctl_output::<_, u64>(&wait) };
    if status != 0 {
        return Err(crate::error::DriverError::FenceTimeout {
            ms: timeout_ns / 1_000_000,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn gem_create_layout() {
        assert_eq!(size_of::<AmdgpuGemCreate>(), 32);
        assert_eq!(offset_of!(AmdgpuGemCreate, bo_size), 0);
        assert_eq!(offset_of!(AmdgpuGemCreate, alignment), 8);
        assert_eq!(offset_of!(AmdgpuGemCreate, domains), 16);
        assert_eq!(offset_of!(AmdgpuGemCreate, domain_flags), 24);
    }

    #[test]
    fn gem_mmap_layout() {
        assert_eq!(size_of::<AmdgpuGemMmap>(), 8);
        assert_eq!(offset_of!(AmdgpuGemMmap, handle_or_addr), 0);
    }

    #[test]
    fn ctx_layout() {
        assert_eq!(size_of::<AmdgpuCtx>(), 16);
        assert_eq!(offset_of!(AmdgpuCtx, op), 0);
        assert_eq!(offset_of!(AmdgpuCtx, flags), 4);
        assert_eq!(offset_of!(AmdgpuCtx, ctx_id), 8);
    }

    #[test]
    fn gem_va_layout() {
        assert_eq!(size_of::<AmdgpuGemVa>(), 40);
        assert_eq!(offset_of!(AmdgpuGemVa, handle), 0);
        assert_eq!(offset_of!(AmdgpuGemVa, operation), 8);
        assert_eq!(offset_of!(AmdgpuGemVa, flags), 12);
        assert_eq!(offset_of!(AmdgpuGemVa, va_address), 16);
        assert_eq!(offset_of!(AmdgpuGemVa, offset_in_bo), 24);
        assert_eq!(offset_of!(AmdgpuGemVa, map_size), 32);
    }

    #[test]
    fn bo_list_entry_layout() {
        assert_eq!(size_of::<AmdgpuBoListEntry>(), 8);
        assert_eq!(offset_of!(AmdgpuBoListEntry, bo_handle), 0);
        assert_eq!(offset_of!(AmdgpuBoListEntry, bo_priority), 4);
    }

    #[test]
    fn bo_list_in_layout() {
        assert_eq!(size_of::<AmdgpuBoListIn>(), 24);
        assert_eq!(offset_of!(AmdgpuBoListIn, operation), 0);
        assert_eq!(offset_of!(AmdgpuBoListIn, list_handle), 4);
        assert_eq!(offset_of!(AmdgpuBoListIn, bo_number), 8);
        assert_eq!(offset_of!(AmdgpuBoListIn, bo_info_size), 12);
        assert_eq!(offset_of!(AmdgpuBoListIn, bo_info_ptr), 16);
    }

    #[test]
    fn cs_chunk_layout() {
        assert_eq!(size_of::<AmdgpuCsChunk>(), 16);
        assert_eq!(offset_of!(AmdgpuCsChunk, chunk_id), 0);
        assert_eq!(offset_of!(AmdgpuCsChunk, length_dw), 4);
        assert_eq!(offset_of!(AmdgpuCsChunk, chunk_data), 8);
    }

    #[test]
    fn cs_chunk_ib_layout() {
        assert_eq!(size_of::<AmdgpuCsChunkIb>(), 32);
        assert_eq!(offset_of!(AmdgpuCsChunkIb, pad), 0);
        assert_eq!(offset_of!(AmdgpuCsChunkIb, flags), 4);
        assert_eq!(offset_of!(AmdgpuCsChunkIb, va_start), 8);
        assert_eq!(offset_of!(AmdgpuCsChunkIb, ib_bytes), 16);
        assert_eq!(offset_of!(AmdgpuCsChunkIb, ip_type), 20);
        assert_eq!(offset_of!(AmdgpuCsChunkIb, ip_instance), 24);
        assert_eq!(offset_of!(AmdgpuCsChunkIb, ring), 28);
    }

    #[test]
    fn cs_in_layout() {
        assert_eq!(size_of::<AmdgpuCsIn>(), 24);
        assert_eq!(offset_of!(AmdgpuCsIn, ctx_id), 0);
        assert_eq!(offset_of!(AmdgpuCsIn, bo_list_handle), 4);
        assert_eq!(offset_of!(AmdgpuCsIn, num_chunks), 8);
        assert_eq!(offset_of!(AmdgpuCsIn, flags), 12);
        assert_eq!(offset_of!(AmdgpuCsIn, chunks), 16);
    }

    #[test]
    fn wait_cs_in_layout() {
        assert_eq!(size_of::<AmdgpuWaitCsIn>(), 32);
        assert_eq!(offset_of!(AmdgpuWaitCsIn, handle), 0);
        assert_eq!(offset_of!(AmdgpuWaitCsIn, timeout), 8);
        assert_eq!(offset_of!(AmdgpuWaitCsIn, ip_type), 16);
        assert_eq!(offset_of!(AmdgpuWaitCsIn, ip_instance), 20);
        assert_eq!(offset_of!(AmdgpuWaitCsIn, ring), 24);
        assert_eq!(offset_of!(AmdgpuWaitCsIn, ctx_id), 28);
    }

    #[test]
    fn size_of_u32_helper() {
        assert_eq!(size_of_u32::<AmdgpuGemCreate>(), 32);
        assert_eq!(size_of_u32::<AmdgpuCtx>(), 16);
        assert_eq!(size_of_u32::<AmdgpuGemMmap>(), 8);
        assert_eq!(size_of_u32::<AmdgpuGemVa>(), 40);
        assert_eq!(size_of_u32::<AmdgpuBoListIn>(), 24);
        assert_eq!(size_of_u32::<AmdgpuCsIn>(), 24);
        assert_eq!(size_of_u32::<AmdgpuWaitCsIn>(), 32);
    }

    #[test]
    fn read_ioctl_output_extracts_first_field() {
        let cs = AmdgpuCsIn {
            ctx_id: 0xDEAD_BEEF,
            bo_list_handle: 0xCAFE,
            ..Default::default()
        };
        let out: u32 = unsafe { read_ioctl_output(&cs) };
        assert_eq!(out, 0xDEAD_BEEF);
    }

    #[test]
    fn kernel_ptr_round_trips() {
        let val: u32 = 42;
        let ptr = kernel_ptr(&val);
        assert_eq!(ptr, std::ptr::from_ref(&val) as u64);
    }

    #[test]
    fn default_structs_are_zeroed() {
        let gem = AmdgpuGemCreate::default();
        assert_eq!(gem.bo_size, 0);
        assert_eq!(gem.domains, 0);

        let ctx = AmdgpuCtx::default();
        assert_eq!(ctx.op, 0);
        assert_eq!(ctx.ctx_id, 0);

        let wait = AmdgpuWaitCsIn::default();
        assert_eq!(wait.handle, 0);
        assert_eq!(wait.timeout, 0);
    }
}
