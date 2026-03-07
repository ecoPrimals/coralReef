// SPDX-License-Identifier: AGPL-3.0-only
//! Nouveau DRM ioctl definitions — pure Rust, no `*-sys` crates.
//!
//! Ioctl numbers and structures are derived from the Linux kernel
//! nouveau driver headers (`nouveau_drm.h`). Syscalls go through
//! `libc` for cross-architecture portability.
//!
//! ## Nouveau DRM ioctls used
//!
//! | Ioctl | Nr | Description |
//! |-------|----|-------------|
//! | `DRM_NOUVEAU_CHANNEL_ALLOC` | 0x00 | Allocate a GPU channel |
//! | `DRM_NOUVEAU_CHANNEL_FREE`  | 0x01 | Free a GPU channel |
//! | `DRM_NOUVEAU_GEM_NEW`       | 0x40 | Create a GEM buffer |
//! | `DRM_NOUVEAU_GEM_PUSHBUF`   | 0x41 | Submit pushbuf commands |

use crate::MemoryDomain;
use crate::drm;
use crate::error::{DriverError, DriverResult};
use std::os::unix::io::RawFd;

const DRM_COMMAND_BASE: u32 = 0x40;

const DRM_NOUVEAU_CHANNEL_ALLOC: u32 = DRM_COMMAND_BASE;
const DRM_NOUVEAU_CHANNEL_FREE: u32 = DRM_COMMAND_BASE + 0x01;
const DRM_NOUVEAU_GEM_NEW: u32 = DRM_COMMAND_BASE + 0x40;
const DRM_NOUVEAU_GEM_PUSHBUF: u32 = DRM_COMMAND_BASE + 0x41;
const DRM_NOUVEAU_GEM_CPU_PREP: u32 = DRM_COMMAND_BASE + 0x42;
const _DRM_NOUVEAU_GEM_CPU_FINI: u32 = DRM_COMMAND_BASE + 0x43;

const _NOUVEAU_GEM_DOMAIN_CPU: u32 = 1 << 0;
const NOUVEAU_GEM_DOMAIN_VRAM: u32 = 1 << 1;
const NOUVEAU_GEM_DOMAIN_GART: u32 = 1 << 2;
const NOUVEAU_GEM_DOMAIN_MAPPABLE: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// NVIF constants — aligned to Mesa `nvif/ioctl.h` (groundSpring V95)
// ---------------------------------------------------------------------------

/// NVIF route: standard NVIF-routed ioctl (Mesa `NVIF_IOCTL_V0_ROUTE_NVIF`).
pub const NVIF_ROUTE_NVIF: u8 = 0x00;

/// NVIF route: hidden/internal (Mesa `NVIF_IOCTL_V0_ROUTE_HIDDEN`).
pub const NVIF_ROUTE_HIDDEN: u8 = 0xFF;

/// NVIF owner: standard NVIF owner (Mesa `NVIF_IOCTL_V0_OWNER_NVIF`).
pub const NVIF_OWNER_NVIF: u8 = 0x00;

/// NVIF owner: wildcard (Mesa `NVIF_IOCTL_V0_OWNER_ANY`).
pub const NVIF_OWNER_ANY: u8 = 0xFF;

// ---------------------------------------------------------------------------
// Ioctl structures (must match kernel `nouveau_drm.h` layout)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Default)]
struct NouveauChannelAlloc {
    fb_ctxdma_handle: u32,
    tt_ctxdma_handle: u32,
    channel: i32,
    pushbuf_domains: u32,
    notifier_handle: u32,
    subchan: [NouveauSubchan; 8],
    nr_subchan: u32,
    pad: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct NouveauSubchan {
    handle: u32,
    grclass: u32,
}

#[repr(C)]
#[derive(Default)]
struct NouveauChannelFree {
    channel: i32,
    pad: u32,
}

#[repr(C)]
#[derive(Default)]
struct NouveauGemNew {
    info: NouveauGemInfo,
    channel_hint: u32,
    align: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct NouveauGemInfo {
    handle: u32,
    domain: u32,
    size: u64,
    offset: u64,
    map_handle: u64,
    tile_mode: u32,
    tile_flags: u32,
}

#[repr(C)]
#[derive(Default)]
struct NouveauGemPushbuf {
    channel: u32,
    nr_buffers: u32,
    buffers: u64,
    nr_relocs: u32,
    nr_push: u32,
    relocs: u64,
    push: u64,
    suffix0: u32,
    suffix1: u32,
    vram_available: u64,
    gart_available: u64,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct NouveauGemPushbufBo {
    user_priv: u64,
    handle: u32,
    read_domains: u32,
    write_domains: u32,
    valid_domains: u32,
    presumed: NouveauGemPushbufBoPresume,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct NouveauGemPushbufBoPresume {
    valid: u32,
    domain: u32,
    offset: u64,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct NouveauGemPushbufPush {
    bo_index: u32,
    pad: u32,
    offset: u64,
    length: u64,
}

/// Wait flags for `DRM_NOUVEAU_GEM_CPU_PREP`.
const NOUVEAU_GEM_CPU_PREP_WRITE: u32 = 0x04;

#[repr(C)]
#[derive(Default)]
struct NouveauGemCpuPrep {
    handle: u32,
    flags: u32,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a nouveau GPU channel for command submission.
///
/// # Errors
///
/// Returns [`DriverError::IoctlFailed`] if the kernel rejects the request.
pub fn create_channel(fd: RawFd) -> DriverResult<u32> {
    let mut alloc = NouveauChannelAlloc {
        pushbuf_domains: NOUVEAU_GEM_DOMAIN_VRAM | NOUVEAU_GEM_DOMAIN_GART,
        ..Default::default()
    };
    let ioctl_nr = drm::drm_iowr_pub(
        DRM_NOUVEAU_CHANNEL_ALLOC,
        std::mem::size_of::<NouveauChannelAlloc>() as u32,
    );
    // SAFETY: `NouveauChannelAlloc` is `#[repr(C)]` matching the kernel's
    // `drm_nouveau_channel_alloc` struct. Synchronous ioctl.
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut alloc)? };
    Ok(alloc.channel as u32)
}

/// Destroy a nouveau GPU channel.
///
/// # Errors
///
/// Returns [`DriverError::IoctlFailed`] if the kernel rejects the request.
pub fn destroy_channel(fd: RawFd, channel: u32) -> DriverResult<()> {
    let mut free = NouveauChannelFree {
        channel: channel as i32,
        pad: 0,
    };
    let ioctl_nr = drm::drm_iowr_pub(
        DRM_NOUVEAU_CHANNEL_FREE,
        std::mem::size_of::<NouveauChannelFree>() as u32,
    );
    // SAFETY: `NouveauChannelFree` is `#[repr(C)]` matching the kernel struct.
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut free) }
}

/// Create a nouveau GEM buffer object.
///
/// Returns the GEM handle on success.
///
/// # Errors
///
/// Returns [`DriverError::IoctlFailed`] on kernel failure.
pub fn gem_new(fd: RawFd, size: u64, domain: MemoryDomain) -> DriverResult<u32> {
    let nv_domain = match domain {
        MemoryDomain::Vram => NOUVEAU_GEM_DOMAIN_VRAM,
        MemoryDomain::Gtt => NOUVEAU_GEM_DOMAIN_GART | NOUVEAU_GEM_DOMAIN_MAPPABLE,
        MemoryDomain::VramOrGtt => {
            NOUVEAU_GEM_DOMAIN_VRAM | NOUVEAU_GEM_DOMAIN_GART | NOUVEAU_GEM_DOMAIN_MAPPABLE
        }
    };

    let mut req = NouveauGemNew {
        info: NouveauGemInfo {
            size,
            domain: nv_domain,
            ..Default::default()
        },
        align: 0x1000,
        ..Default::default()
    };

    let ioctl_nr = drm::drm_iowr_pub(
        DRM_NOUVEAU_GEM_NEW,
        std::mem::size_of::<NouveauGemNew>() as u32,
    );
    // SAFETY: `NouveauGemNew` is `#[repr(C)]` matching the kernel struct.
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut req)? };
    Ok(req.info.handle)
}

/// Query GEM buffer info (offset/map_handle).
pub(crate) fn gem_info(fd: RawFd, handle: u32) -> DriverResult<(u64, u64)> {
    let mut req = NouveauGemNew {
        info: NouveauGemInfo {
            handle,
            ..Default::default()
        },
        ..Default::default()
    };
    let ioctl_nr = drm::drm_iowr_pub(
        DRM_NOUVEAU_GEM_NEW,
        std::mem::size_of::<NouveauGemNew>() as u32,
    );
    // SAFETY: Same struct, kernel fills in offset/map_handle.
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut req)? };
    Ok((req.info.offset, req.info.map_handle))
}

/// Submit a pushbuf command buffer to the GPU.
///
/// `channel` is the channel handle from `create_channel`.
/// `gem_handle` is the GEM handle of the command buffer.
/// `push_offset` is the byte offset within the GEM buffer.
/// `push_length` is the byte length of the push data.
/// `bo_handles` are the GEM handles of all buffer objects referenced.
///
/// # Errors
///
/// Returns [`DriverError::IoctlFailed`] on kernel failure.
pub fn pushbuf_submit(
    fd: RawFd,
    channel: u32,
    gem_handle: u32,
    push_offset: u64,
    push_length: u64,
    bo_handles: &[u32],
) -> DriverResult<()> {
    let mut buffers: Vec<NouveauGemPushbufBo> = bo_handles
        .iter()
        .map(|&h| NouveauGemPushbufBo {
            handle: h,
            read_domains: NOUVEAU_GEM_DOMAIN_VRAM | NOUVEAU_GEM_DOMAIN_GART,
            write_domains: NOUVEAU_GEM_DOMAIN_VRAM | NOUVEAU_GEM_DOMAIN_GART,
            valid_domains: NOUVEAU_GEM_DOMAIN_VRAM | NOUVEAU_GEM_DOMAIN_GART,
            ..Default::default()
        })
        .collect();

    let push_bo_idx = buffers
        .iter()
        .position(|b| b.handle == gem_handle)
        .unwrap_or_else(|| {
            buffers.push(NouveauGemPushbufBo {
                handle: gem_handle,
                read_domains: NOUVEAU_GEM_DOMAIN_VRAM | NOUVEAU_GEM_DOMAIN_GART,
                valid_domains: NOUVEAU_GEM_DOMAIN_VRAM | NOUVEAU_GEM_DOMAIN_GART,
                ..Default::default()
            });
            buffers.len() - 1
        });

    let push = [NouveauGemPushbufPush {
        bo_index: push_bo_idx as u32,
        pad: 0,
        offset: push_offset,
        length: push_length,
    }];

    let nr_buffers = u32::try_from(buffers.len())
        .map_err(|_| DriverError::platform_overflow("buffer count fits in u32"))?;

    let mut pb = NouveauGemPushbuf {
        channel,
        nr_buffers,
        buffers: buffers.as_mut_ptr() as u64,
        nr_relocs: 0,
        nr_push: 1,
        relocs: 0,
        push: push.as_ptr() as u64,
        ..Default::default()
    };

    let ioctl_nr = drm::drm_iowr_pub(
        DRM_NOUVEAU_GEM_PUSHBUF,
        std::mem::size_of::<NouveauGemPushbuf>() as u32,
    );
    // SAFETY: All pointer fields point to valid, stack- or heap-allocated
    // `#[repr(C)]` structures. The ioctl is synchronous.
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut pb) }
}

/// Wait for GPU operations on a GEM buffer to complete.
///
/// Blocks until the GPU is no longer using the buffer, or returns
/// `DriverError::IoctlFailed` on timeout/error.
///
/// # Errors
///
/// Returns [`DriverError::IoctlFailed`] if the kernel rejects the wait.
pub fn gem_cpu_prep(fd: RawFd, gem_handle: u32) -> DriverResult<()> {
    let mut prep = NouveauGemCpuPrep {
        handle: gem_handle,
        flags: NOUVEAU_GEM_CPU_PREP_WRITE,
    };
    let ioctl_nr = drm::drm_iowr_pub(
        DRM_NOUVEAU_GEM_CPU_PREP,
        std::mem::size_of::<NouveauGemCpuPrep>() as u32,
    );
    // SAFETY: `NouveauGemCpuPrep` is `#[repr(C)]` matching the kernel struct.
    // Synchronous ioctl — blocks until the buffer is idle.
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut prep) }
}

/// Map a GEM buffer for CPU access via mmap.
///
/// Returns a raw pointer and the mapping size.
///
/// # Safety
///
/// The returned pointer is valid until `munmap` is called.
pub(crate) fn gem_mmap(fd: RawFd, map_handle: u64, size: u64) -> DriverResult<*mut u8> {
    // SAFETY: mmap with a valid fd and map_handle from the kernel.
    // MAP_SHARED is required for coherent GPU/CPU access.
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size as libc::size_t,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            map_handle as libc::off_t,
        )
    };
    if ptr == libc::MAP_FAILED {
        return Err(DriverError::MmapFailed("nouveau gem mmap failed".into()));
    }
    Ok(ptr.cast::<u8>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_are_consistent() {
        assert_eq!(DRM_NOUVEAU_CHANNEL_ALLOC, 0x40);
        assert_eq!(DRM_NOUVEAU_GEM_NEW, 0x80);
        assert_eq!(DRM_NOUVEAU_GEM_PUSHBUF, 0x81);
    }

    #[test]
    fn gem_domain_flags() {
        assert_eq!(_NOUVEAU_GEM_DOMAIN_CPU, 1);
        assert_eq!(NOUVEAU_GEM_DOMAIN_VRAM, 2);
        assert_eq!(NOUVEAU_GEM_DOMAIN_GART, 4);
        assert_eq!(NOUVEAU_GEM_DOMAIN_MAPPABLE, 64);
    }

    #[test]
    fn struct_sizes_are_reasonable() {
        assert!(std::mem::size_of::<NouveauChannelAlloc>() > 0);
        assert!(std::mem::size_of::<NouveauGemNew>() > 0);
        assert!(std::mem::size_of::<NouveauGemPushbuf>() > 0);
    }

    #[test]
    fn nvif_constants_match_mesa() {
        assert_eq!(NVIF_ROUTE_NVIF, 0x00);
        assert_eq!(NVIF_ROUTE_HIDDEN, 0xFF);
        assert_eq!(NVIF_OWNER_NVIF, 0x00);
        assert_eq!(NVIF_OWNER_ANY, 0xFF);
    }

    #[test]
    fn pushbuf_bo_struct_layout() {
        assert_eq!(
            std::mem::size_of::<NouveauGemPushbufBo>(),
            40,
            "NouveauGemPushbufBo must be 40 bytes (kernel ABI)"
        );
    }

    #[test]
    fn pushbuf_push_struct_layout() {
        assert_eq!(
            std::mem::size_of::<NouveauGemPushbufPush>(),
            24,
            "NouveauGemPushbufPush must be 24 bytes (kernel ABI)"
        );
    }

    #[test]
    fn channel_alloc_struct_has_subchan_array() {
        let alloc = NouveauChannelAlloc::default();
        assert_eq!(alloc.subchan.len(), 8);
    }
}
