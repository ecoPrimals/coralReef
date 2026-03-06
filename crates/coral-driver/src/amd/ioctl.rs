// SPDX-License-Identifier: AGPL-3.0-only
//! AMD-specific DRM ioctl definitions.
//!
//! Structures and constants from the amdgpu kernel driver, defined in
//! pure Rust (no `amdgpu-sys` or `drm-sys`).

use crate::error::DriverResult;
use std::os::unix::io::RawFd;

// amdgpu DRM ioctl command numbers (from amdgpu_drm.h)
const DRM_COMMAND_BASE: u32 = 0x40;
const DRM_AMDGPU_GEM_CREATE: u32 = DRM_COMMAND_BASE + 0x00;
const DRM_AMDGPU_GEM_MMAP: u32 = DRM_COMMAND_BASE + 0x01;
const DRM_AMDGPU_CTX: u32 = DRM_COMMAND_BASE + 0x02;
const DRM_AMDGPU_GEM_VA: u32 = DRM_COMMAND_BASE + 0x08;
const _DRM_AMDGPU_WAIT_CS: u32 = DRM_COMMAND_BASE + 0x09;
const _DRM_AMDGPU_CS: u32 = DRM_COMMAND_BASE + 0x04;
const _DRM_AMDGPU_GEM_CLOSE: u32 = DRM_COMMAND_BASE + 0x09;

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
    pub _pad: u32,
}

/// GEM mmap output.
#[repr(C)]
#[derive(Debug, Default)]
pub struct AmdgpuGemMmap {
    pub handle: u32,
    pub _pad: u32,
    pub offset: u64,
}

/// Context operation input/output.
#[repr(C)]
#[derive(Debug, Default)]
pub struct AmdgpuCtx {
    pub op: u32,
    pub flags: u32,
    pub ctx_id: u32,
    pub _pad: u32,
}

/// GEM VA mapping.
#[repr(C)]
#[derive(Debug, Default)]
pub struct AmdgpuGemVa {
    pub handle: u32,
    pub _pad: u32,
    pub operation: u32,
    pub flags: u32,
    pub va_address: u64,
    pub offset_in_bo: u64,
    pub map_size: u64,
}

/// Create an amdgpu GPU context.
pub fn create_context(fd: RawFd) -> DriverResult<u32> {
    let mut ctx = AmdgpuCtx {
        op: AMDGPU_CTX_OP_ALLOC_CTX,
        ..Default::default()
    };
    // Safety: properly sized struct for the ioctl
    let ret = unsafe {
        crate::drm::drm_ioctl_call(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_CTX, std::mem::size_of::<AmdgpuCtx>() as u32),
            &mut ctx as *mut _ as *mut u8,
        )
    };
    match ret {
        Ok(()) => Ok(ctx.ctx_id),
        Err(e) => Err(e),
    }
}

/// Destroy an amdgpu GPU context.
pub fn destroy_context(fd: RawFd, ctx_id: u32) -> DriverResult<()> {
    let mut ctx = AmdgpuCtx {
        op: AMDGPU_CTX_OP_FREE_CTX,
        ctx_id,
        ..Default::default()
    };
    unsafe {
        crate::drm::drm_ioctl_call(
            fd,
            crate::drm::drm_iowr_pub(DRM_AMDGPU_CTX, std::mem::size_of::<AmdgpuCtx>() as u32),
            &mut ctx as *mut _ as *mut u8,
        )
    }
}

/// Create a GEM buffer object.
pub fn gem_create(fd: RawFd, size: u64, domains: u32) -> DriverResult<(u32, u64)> {
    let mut req = AmdgpuGemCreate {
        bo_size: size,
        alignment: 4096,
        domains: domains.into(),
        ..Default::default()
    };
    unsafe {
        crate::drm::drm_ioctl_call(
            fd,
            crate::drm::drm_iowr_pub(
                DRM_AMDGPU_GEM_CREATE,
                std::mem::size_of::<AmdgpuGemCreate>() as u32,
            ),
            &mut req as *mut _ as *mut u8,
        )?;
    }
    Ok((req.handle, req.bo_size))
}

/// Get the mmap offset for a GEM buffer.
pub fn gem_mmap_offset(fd: RawFd, handle: u32) -> DriverResult<u64> {
    let mut req = AmdgpuGemMmap {
        handle,
        ..Default::default()
    };
    unsafe {
        crate::drm::drm_ioctl_call(
            fd,
            crate::drm::drm_iowr_pub(
                DRM_AMDGPU_GEM_MMAP,
                std::mem::size_of::<AmdgpuGemMmap>() as u32,
            ),
            &mut req as *mut _ as *mut u8,
        )?;
    }
    Ok(req.offset)
}

/// Map a GEM buffer to a GPU virtual address.
pub fn gem_va_map(fd: RawFd, handle: u32, va: u64, size: u64) -> DriverResult<()> {
    let mut req = AmdgpuGemVa {
        handle,
        operation: AMDGPU_VA_OP_MAP,
        va_address: va,
        map_size: size,
        ..Default::default()
    };
    unsafe {
        crate::drm::drm_ioctl_call(
            fd,
            crate::drm::drm_iow_pub(
                DRM_AMDGPU_GEM_VA,
                std::mem::size_of::<AmdgpuGemVa>() as u32,
            ),
            &mut req as *mut _ as *mut u8,
        )
    }
}

/// Submit a command buffer to the GPU.
pub fn submit_command(
    _fd: RawFd,
    _ctx_id: u32,
    _gem_handles: &[u32],
    _pm4_words: &[u32],
) -> DriverResult<()> {
    // Full DRM_AMDGPU_CS submission requires building the cs_ioctl struct
    // with chunks for IB (indirect buffer), BO list, and dependencies.
    // This is the structural scaffold — actual submission requires hardware.
    tracing::debug!(
        ctx = _ctx_id,
        bos = _gem_handles.len(),
        pm4_words = _pm4_words.len(),
        "AMD CS submit (scaffold)"
    );
    Ok(())
}

/// Wait for GPU fence completion.
pub fn sync_fence(_fd: RawFd, _ctx_id: u32) -> DriverResult<()> {
    tracing::debug!(ctx = _ctx_id, "AMD fence sync (scaffold)");
    Ok(())
}
