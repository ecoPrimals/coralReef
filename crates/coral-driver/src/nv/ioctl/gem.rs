// SPDX-License-Identifier: AGPL-3.0-or-later
//! Nouveau GEM buffer management — allocation, mapping, submission.
//!
//! Extracted from `ioctl/mod.rs` as a natural domain boundary: GEM operations
//! manage GPU memory objects, while channel operations manage PFIFO channels.
//! All ioctl syscalls go through [`crate::drm`] helpers built on `rustix`.

use crate::MemoryDomain;
use crate::drm::{self, MappedRegion};
use crate::error::{DriverError, DriverResult};
use std::os::unix::io::RawFd;

use super::{
    DRM_NOUVEAU_GEM_CPU_PREP, DRM_NOUVEAU_GEM_NEW, DRM_NOUVEAU_GEM_PUSHBUF,
    NOUVEAU_GEM_DOMAIN_GART, NOUVEAU_GEM_DOMAIN_MAPPABLE, NOUVEAU_GEM_DOMAIN_VRAM, size_of_u32,
};

// ---------------------------------------------------------------------------
// GEM ioctl structures (must match kernel `nouveau_drm.h` layout)
// ---------------------------------------------------------------------------

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

/// Result of a GEM buffer creation.
pub struct GemNewResult {
    /// Kernel GEM handle for this buffer.
    pub handle: u32,
    /// Kernel-assigned GPU virtual address offset (legacy UAPI).
    pub offset: u64,
    /// Mmap handle for CPU access.
    pub map_handle: u64,
}

/// Create a nouveau GEM buffer object.
///
/// Returns the GEM handle, offset, and mmap handle on success.
/// The offset is the kernel-assigned GPU VA (legacy UAPI); for new UAPI,
/// the GPU VA is assigned via `vm_bind_map` instead.
///
/// # Errors
///
/// Returns [`DriverError`] on kernel failure.
pub fn gem_new(fd: RawFd, size: u64, domain: MemoryDomain) -> DriverResult<GemNewResult> {
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

    let ioctl_nr = drm::drm_iowr_pub(DRM_NOUVEAU_GEM_NEW, size_of_u32::<NouveauGemNew>());
    drm::drm_ioctl_named(fd, ioctl_nr, &mut req, "nouveau_gem_new")?;
    Ok(GemNewResult {
        handle: req.info.handle,
        offset: req.info.offset,
        map_handle: req.info.map_handle,
    })
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
/// Returns [`DriverError`] on kernel failure.
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

    #[expect(
        clippy::cast_possible_truncation,
        reason = "BO list length is capped by kernel; always < u32::MAX"
    )]
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

    let ioctl_nr =
        drm::drm_iowr_pub(DRM_NOUVEAU_GEM_PUSHBUF, size_of_u32::<NouveauGemPushbuf>());
    drm::drm_ioctl_named(fd, ioctl_nr, &mut pb, "nouveau_gem_pushbuf")
}

/// Wait for GPU operations on a GEM buffer to complete.
///
/// Blocks until the GPU is no longer using the buffer, or returns
/// [`DriverError`] on timeout/error.
///
/// # Errors
///
/// Returns [`DriverError`] if the kernel rejects the wait.
pub fn gem_cpu_prep(fd: RawFd, gem_handle: u32) -> DriverResult<()> {
    let mut prep = NouveauGemCpuPrep {
        handle: gem_handle,
        flags: NOUVEAU_GEM_CPU_PREP_WRITE,
    };
    let ioctl_nr =
        drm::drm_iowr_pub(DRM_NOUVEAU_GEM_CPU_PREP, size_of_u32::<NouveauGemCpuPrep>());
    drm::drm_ioctl_named(fd, ioctl_nr, &mut prep, "nouveau_gem_cpu_prep")
}

/// Map a nouveau GEM buffer into CPU address space with RAII lifetime.
///
/// Returns a [`MappedRegion`] that provides safe slice access and
/// automatically unmaps on drop. Uses the unified mmap abstraction.
pub(crate) fn gem_mmap_region(fd: RawFd, map_handle: u64, size: u64) -> DriverResult<MappedRegion> {
    let len = usize::try_from(size).map_err(|_| {
        DriverError::platform_overflow("buffer size exceeds platform pointer width")
    })?;
    MappedRegion::new(
        len,
        rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
        rustix::mm::MapFlags::SHARED,
        fd,
        map_handle,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gem_new_struct_size() {
        assert_eq!(
            std::mem::size_of::<NouveauGemNew>(),
            48,
            "NouveauGemNew must match kernel drm_nouveau_gem_new"
        );
    }

    #[test]
    fn gem_pushbuf_struct_size() {
        assert_eq!(
            std::mem::size_of::<NouveauGemPushbuf>(),
            64,
            "NouveauGemPushbuf must match kernel drm_nouveau_gem_pushbuf"
        );
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
    fn nouveau_gem_cpu_prep_layout() {
        assert_eq!(std::mem::size_of::<NouveauGemCpuPrep>(), 8);
    }
}
