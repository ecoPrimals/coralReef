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

/// GEM create input/output.
#[repr(C)]
#[derive(Debug, Default)]
pub struct AmdgpuGemCreate {
    pub bo_size: u64,
    pub alignment: u64,
    pub domains: u64,
    pub domain_flags: u64,
    pub handle: u32,
    pub pad: u32,
}

/// GEM mmap output.
#[repr(C)]
#[derive(Debug, Default)]
pub struct AmdgpuGemMmap {
    pub handle: u32,
    pub pad: u32,
    pub offset: u64,
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
    // Safety: AmdgpuCtx is #[repr(C)] and matches the kernel struct.
    unsafe {
        crate::drm::drm_ioctl_typed(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_CTX, size_of_u32::<AmdgpuCtx>()),
            &mut ctx,
        )?;
    }
    Ok(ctx.ctx_id)
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
    // Safety: AmdgpuCtx is #[repr(C)] and matches the kernel struct.
    unsafe {
        crate::drm::drm_ioctl_typed(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_CTX, size_of_u32::<AmdgpuCtx>()),
            &mut ctx,
        )
    }
}

/// Create a GEM buffer object.
///
/// # Errors
///
/// Returns [`DriverError`] if the GEM create ioctl fails.
pub fn gem_create(fd: RawFd, size: u64, domains: u32) -> DriverResult<(u32, u64)> {
    let mut req = AmdgpuGemCreate {
        bo_size: size,
        alignment: 4096,
        domains: domains.into(),
        ..Default::default()
    };
    // Safety: AmdgpuGemCreate is #[repr(C)] and matches the kernel struct.
    unsafe {
        crate::drm::drm_ioctl_typed(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_GEM_CREATE, size_of_u32::<AmdgpuGemCreate>()),
            &mut req,
        )?;
    }
    Ok((req.handle, req.bo_size))
}

/// Get the mmap offset for a GEM buffer.
///
/// # Errors
///
/// Returns [`DriverError`] if the GEM mmap ioctl fails.
pub fn gem_mmap_offset(fd: RawFd, handle: u32) -> DriverResult<u64> {
    let mut req = AmdgpuGemMmap {
        handle,
        ..Default::default()
    };
    // Safety: AmdgpuGemMmap is #[repr(C)] and matches the kernel struct.
    unsafe {
        crate::drm::drm_ioctl_typed(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_GEM_MMAP, size_of_u32::<AmdgpuGemMmap>()),
            &mut req,
        )?;
    }
    Ok(req.offset)
}

/// Map a GEM buffer to a GPU virtual address.
///
/// # Errors
///
/// Returns [`DriverError`] if the VA map ioctl fails.
pub fn gem_va_map(fd: RawFd, handle: u32, va: u64, size: u64) -> DriverResult<()> {
    let mut req = AmdgpuGemVa {
        handle,
        operation: AMDGPU_VA_OP_MAP,
        va_address: va,
        map_size: size,
        ..Default::default()
    };
    // Safety: AmdgpuGemVa is #[repr(C)] and matches the kernel struct.
    unsafe {
        crate::drm::drm_ioctl_typed(
            fd,
            crate::drm::drm_iow_pub(DRM_AMDGPU_GEM_VA, size_of_u32::<AmdgpuGemVa>()),
            &mut req,
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
///
/// # Panics
///
/// Panics if `handles` contains more than `u32::MAX` entries.
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
        bo_number: u32::try_from(entries.len()).expect("BO count fits in u32"),
        bo_info_size: size_of_u32::<AmdgpuBoListEntry>(),
        bo_info_ptr: entries.first().map_or(0, kernel_ptr),
        ..Default::default()
    };

    // Safety: AmdgpuBoListIn is #[repr(C)] and matches the kernel union size.
    // entries slice lives until after the ioctl returns.
    unsafe {
        crate::drm::drm_ioctl_typed(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_BO_LIST, size_of_u32::<AmdgpuBoListIn>()),
            &mut req,
        )?;
    }

    // Kernel writes list_handle to first u32 (union overlay with drm_amdgpu_bo_list_out).
    // Safety: kernel writes list_handle (u32) at offset 0 of the output union.
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

    // Safety: AmdgpuBoListIn is #[repr(C)] and matches the kernel union.
    unsafe {
        crate::drm::drm_ioctl_typed(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_BO_LIST, size_of_u32::<AmdgpuBoListIn>()),
            &mut req,
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

    // Safety: All structs are #[repr(C)] and stack-allocated;
    // pointers remain valid for the duration of the synchronous ioctl.
    unsafe {
        crate::drm::drm_ioctl_typed(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_CS, size_of_u32::<AmdgpuCsIn>()),
            &mut cs,
        )?;
    }

    // Kernel writes fence handle to first 8 bytes (union drm_amdgpu_cs.out.handle).
    // Safety: kernel writes fence handle (u64) at offset 0 of the output union.
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
    let mut wait = AmdgpuWaitCsIn {
        handle: fence_handle,
        timeout: timeout_ns,
        ip_type: AMDGPU_HW_IP_COMPUTE,
        ctx_id,
        ..Default::default()
    };

    tracing::debug!(ctx = ctx_id, fence = fence_handle, "AMD fence sync");

    // Safety: AmdgpuWaitCsIn is #[repr(C)] and matches the kernel union.
    unsafe {
        crate::drm::drm_ioctl_typed(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_WAIT_CS, size_of_u32::<AmdgpuWaitCsIn>()),
            &mut wait,
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
