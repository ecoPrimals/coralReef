// SPDX-License-Identifier: AGPL-3.0-only
//! Nouveau DRM ioctl definitions — pure Rust, no `*-sys` crates.
//!
//! Ioctl numbers and structures are derived from the Linux kernel
//! nouveau driver headers (`nouveau_drm.h`). Ioctl syscalls go through
//! [`crate::drm`] helpers built on `rustix::ioctl` — no inline assembly,
//! no libc dependency.
//!
//! ## Module structure
//!
//! - `mod.rs` — Legacy UAPI: channel alloc, GEM, pushbuf (pre-kernel 6.6)
//! - `new_uapi.rs` — New UAPI: `VM_INIT`, `VM_BIND`, `EXEC` (kernel 6.6+)
//! - `diag.rs` — Channel allocation diagnostics

pub mod diag;
pub mod new_uapi;

pub use diag::{ChannelAllocDiag, diagnose_channel_alloc, dump_channel_alloc_hex};
pub use diag::{
    FirmwareInventory, FwStatus, GpuIdentity, check_nouveau_firmware, firmware_inventory,
    probe_gpu_identity,
};
pub use new_uapi::{
    exec_submit, exec_submit_with_signal, syncobj_create, syncobj_destroy, syncobj_wait,
    vm_bind_map, vm_bind_unmap, vm_init,
};

use crate::MemoryDomain;
use crate::drm::{self, MappedRegion};
use crate::error::{DriverError, DriverResult};
use std::os::unix::io::RawFd;

/// Size of a `#[repr(C)]` struct as a `u32` for ioctl encoding.
#[expect(
    clippy::cast_possible_truncation,
    reason = "asserted in bounds; kernel ioctl structs are always < 4 GiB"
)]
const fn size_of_u32<T>() -> u32 {
    assert!(std::mem::size_of::<T>() <= u32::MAX as usize);
    std::mem::size_of::<T>() as u32
}

const DRM_COMMAND_BASE: u32 = 0x40;

// Legacy UAPI — channel management (present in all kernel versions).
// Offsets from kernel nouveau_drm.h: GETPARAM=0x00, SETPARAM=0x01,
// CHANNEL_ALLOC=0x02, CHANNEL_FREE=0x03.
const DRM_NOUVEAU_CHANNEL_ALLOC: u32 = DRM_COMMAND_BASE + 0x02;
const DRM_NOUVEAU_CHANNEL_FREE: u32 = DRM_COMMAND_BASE + 0x03;
const DRM_NOUVEAU_GEM_NEW: u32 = DRM_COMMAND_BASE + 0x40;
const DRM_NOUVEAU_GEM_PUSHBUF: u32 = DRM_COMMAND_BASE + 0x41;
const DRM_NOUVEAU_GEM_CPU_PREP: u32 = DRM_COMMAND_BASE + 0x42;
const _DRM_NOUVEAU_GEM_CPU_FINI: u32 = DRM_COMMAND_BASE + 0x43;

// New UAPI (kernel 6.6+) — required for Volta+ dispatch on modern kernels.
// NVK (Mesa 25.1+) uses this path: VM_INIT → GEM_NEW → VM_BIND → EXEC.
// Ecosystem Exp-051 confirmed: legacy CHANNEL_ALLOC → EINVAL on GV100 kernel 6.17.
// See: /usr/include/drm/nouveau_drm.h (drm_nouveau_vm_init, vm_bind, exec)
const DRM_NOUVEAU_VM_INIT: u32 = DRM_COMMAND_BASE + 0x10;
const DRM_NOUVEAU_VM_BIND: u32 = DRM_COMMAND_BASE + 0x11;
const DRM_NOUVEAU_EXEC: u32 = DRM_COMMAND_BASE + 0x12;

const _NOUVEAU_GEM_DOMAIN_CPU: u32 = 1 << 0;
const NOUVEAU_GEM_DOMAIN_VRAM: u32 = 1 << 1;
const NOUVEAU_GEM_DOMAIN_GART: u32 = 1 << 2;
const NOUVEAU_GEM_DOMAIN_MAPPABLE: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// NVIF constants — aligned to Mesa `nvif/ioctl.h`
// ---------------------------------------------------------------------------

/// NVIF route: standard NVIF-routed ioctl (Mesa `NVIF_IOCTL_V0_ROUTE_NVIF`).
pub const NVIF_ROUTE_NVIF: u8 = 0x00;

/// NVIF route: hidden/internal (Mesa `NVIF_IOCTL_V0_ROUTE_HIDDEN`).
pub const NVIF_ROUTE_HIDDEN: u8 = 0xff;

/// NVIF owner: standard NVIF owner (Mesa `NVIF_IOCTL_V0_OWNER_NVIF`).
pub const NVIF_OWNER_NVIF: u8 = 0x00;

/// NVIF owner: wildcard (Mesa `NVIF_IOCTL_V0_OWNER_ANY`).
pub const NVIF_OWNER_ANY: u8 = 0xff;

// ---------------------------------------------------------------------------
// NVIF object class definitions — from NVK / nouveau kernel headers
// ---------------------------------------------------------------------------
//
// The kernel instantiates engine objects for each (handle, grclass) in the
// subchan array. Compute dispatch uses the compute class; 2D and copy
// engines are used by NVK for buffer copies. Reference: Mesa NVK channel setup.

/// Fermi 2D engine — used by NVK for 2D blits.
/// Kernel class: `FERMI_TWOD_A`.
pub const NVIF_CLASS_FERMI_TWOD_A: u32 = 0x902D;

/// Kepler inline-to-memory copy engine — used by NVK for buffer copies.
/// Kernel class: `KEPLER_INLINE_TO_MEMORY_B`.
pub const NVIF_CLASS_KEPLER_INLINE_TO_MEMORY_B: u32 = 0xA0B5;

/// Volta compute engine (GV100). Kernel class: `VOLTA_COMPUTE_A`.
pub const NVIF_CLASS_VOLTA_COMPUTE_A: u32 = 0xC3C0;

/// Turing compute engine. Kernel class: `TURING_COMPUTE_A`.
pub const NVIF_CLASS_TURING_COMPUTE_A: u32 = 0xC5C0;

/// Ampere compute engine. Kernel class: `AMPERE_COMPUTE_A`.
pub const NVIF_CLASS_AMPERE_COMPUTE_A: u32 = 0xC6C0;

/// Subchannel specification for channel creation.
///
/// Each subchannel binds an NVIF engine object (grclass) to a handle.
/// Subchannel index in the array corresponds to the subchan field in
/// push buffer headers (bits `[15:13]`).
#[derive(Clone, Copy, Debug)]
pub struct SubchanSpec {
    /// NVIF object handle (typically 1, 2, 3, ... for each subchannel).
    pub handle: u32,
    /// GPU engine class (e.g. [`NVIF_CLASS_VOLTA_COMPUTE_A`]).
    pub grclass: u32,
}

// ---------------------------------------------------------------------------
// Ioctl structures (must match kernel `nouveau_drm.h` layout)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Default, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct NouveauChannelAlloc {
    fb_ctxdma_handle: u32,
    tt_ctxdma_handle: u32,
    channel: i32,
    pushbuf_domains: u32,
    notifier_handle: u32,
    subchan: [NouveauSubchan; 8],
    nr_subchan: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct NouveauSubchan {
    handle: u32,
    grclass: u32,
}

#[repr(C)]
#[derive(Default)]
struct NouveauChannelFree {
    channel: i32,
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

/// Create a nouveau GPU channel with a compute subchannel.
///
/// `compute_class` is the GPU compute engine class (e.g. `0xC3C0` for Volta).
/// The kernel instantiates the NVIF compute object and binds it to subchannel 0.
///
/// # Errors
///
/// Returns [`DriverError`] if the kernel rejects the request (e.g. the
/// compute class is unsupported for this GPU or the kernel nouveau driver
/// lacks compute support).
pub fn create_channel(fd: RawFd, compute_class: u32) -> DriverResult<u32> {
    create_channel_with_subchannels(
        fd,
        &[SubchanSpec {
            handle: 1,
            grclass: compute_class,
        }],
    )
}

/// Create a nouveau GPU channel with multiple NVIF subchannel objects.
///
/// NVK-style setup uses [`NVIF_CLASS_FERMI_TWOD_A`], [`NVIF_CLASS_KEPLER_INLINE_TO_MEMORY_B`],
/// and a compute class. For compute-only dispatch, a single compute subchannel
/// is sufficient. The first subchannel (index 0) receives handle 1, the next
/// handle 2, etc. Push buffer method calls use the subchan index to target
/// the correct engine.
///
/// # Errors
///
/// Returns [`DriverError`] if the kernel rejects the request.
pub fn create_channel_with_subchannels(fd: RawFd, subchans: &[SubchanSpec]) -> DriverResult<u32> {
    let nr = subchans
        .len()
        .min(8)
        .try_into()
        .map_err(|_| DriverError::platform_overflow("nr_subchan fits in u32"))?;

    let mut alloc = NouveauChannelAlloc {
        pushbuf_domains: NOUVEAU_GEM_DOMAIN_VRAM | NOUVEAU_GEM_DOMAIN_GART,
        nr_subchan: nr,
        ..Default::default()
    };

    for (i, spec) in subchans.iter().take(8).enumerate() {
        alloc.subchan[i] = NouveauSubchan {
            handle: spec.handle,
            grclass: spec.grclass,
        };
    }

    let ioctl_nr = drm::drm_iowr_pub(
        DRM_NOUVEAU_CHANNEL_ALLOC,
        size_of_u32::<NouveauChannelAlloc>(),
    );
    drm::drm_ioctl_named(fd, ioctl_nr, &mut alloc, "nouveau_channel_alloc")?;
    #[expect(
        clippy::cast_sign_loss,
        reason = "kernel returns non-negative channel id on success"
    )]
    let channel = alloc.channel as u32;
    Ok(channel)
}

/// Create a GV100 (Volta) compute channel with NVK-style subchannel binding.
///
/// Binds `FERMI_TWOD_A` (subchan 0), `KEPLER_INLINE_TO_MEMORY_B` (subchan 1),
/// and `VOLTA_COMPUTE_A` (subchan 2). For compute-only workloads, prefer
/// [`create_channel`] with [`NVIF_CLASS_VOLTA_COMPUTE_A`] — that binds
/// compute to subchan 0, matching the push buffer's default subchan.
///
/// # Errors
///
/// Returns [`DriverError`] if the kernel rejects the request (e.g. GPU
/// is not Volta or kernel lacks support).
pub fn create_gv100_compute_channel(fd: RawFd) -> DriverResult<(u32, u8)> {
    let subchans = [
        SubchanSpec {
            handle: 1,
            grclass: NVIF_CLASS_FERMI_TWOD_A,
        },
        SubchanSpec {
            handle: 2,
            grclass: NVIF_CLASS_KEPLER_INLINE_TO_MEMORY_B,
        },
        SubchanSpec {
            handle: 3,
            grclass: NVIF_CLASS_VOLTA_COMPUTE_A,
        },
    ];
    let channel = create_channel_with_subchannels(fd, &subchans)?;
    // Compute is on subchan 2 when using NVK-style multi-engine setup.
    Ok((channel, 2))
}

/// Destroy a nouveau GPU channel.
///
/// # Errors
///
/// Returns [`DriverError`] if the kernel rejects the request.
pub fn destroy_channel(fd: RawFd, channel: u32) -> DriverResult<()> {
    #[expect(
        clippy::cast_possible_wrap,
        reason = "channel ids fit in i32 (kernel allocates small positive values)"
    )]
    let mut free = NouveauChannelFree {
        channel: channel as i32,
    };
    let ioctl_nr = drm::drm_iowr_pub(
        DRM_NOUVEAU_CHANNEL_FREE,
        size_of_u32::<NouveauChannelFree>(),
    );
    drm::drm_ioctl_named(fd, ioctl_nr, &mut free, "nouveau_channel_free")
}

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

    let ioctl_nr = drm::drm_iowr_pub(DRM_NOUVEAU_GEM_PUSHBUF, size_of_u32::<NouveauGemPushbuf>());
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
    let ioctl_nr = drm::drm_iowr_pub(DRM_NOUVEAU_GEM_CPU_PREP, size_of_u32::<NouveauGemCpuPrep>());
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
    fn ioctl_numbers_match_kernel_header() {
        assert_eq!(
            DRM_NOUVEAU_CHANNEL_ALLOC, 0x42,
            "CHANNEL_ALLOC = DRM_COMMAND_BASE + 0x02"
        );
        assert_eq!(
            DRM_NOUVEAU_CHANNEL_FREE, 0x43,
            "CHANNEL_FREE = DRM_COMMAND_BASE + 0x03"
        );
        assert_eq!(
            DRM_NOUVEAU_GEM_NEW, 0x80,
            "GEM_NEW = DRM_COMMAND_BASE + 0x40"
        );
        assert_eq!(
            DRM_NOUVEAU_GEM_PUSHBUF, 0x81,
            "GEM_PUSHBUF = DRM_COMMAND_BASE + 0x41"
        );
        assert_eq!(
            DRM_NOUVEAU_GEM_CPU_PREP, 0x82,
            "GEM_CPU_PREP = DRM_COMMAND_BASE + 0x42"
        );
        assert_eq!(
            DRM_NOUVEAU_VM_INIT, 0x50,
            "VM_INIT = DRM_COMMAND_BASE + 0x10"
        );
        assert_eq!(
            DRM_NOUVEAU_VM_BIND, 0x51,
            "VM_BIND = DRM_COMMAND_BASE + 0x11"
        );
        assert_eq!(DRM_NOUVEAU_EXEC, 0x52, "EXEC = DRM_COMMAND_BASE + 0x12");
    }

    #[test]
    fn gem_domain_flags() {
        assert_eq!(_NOUVEAU_GEM_DOMAIN_CPU, 1);
        assert_eq!(NOUVEAU_GEM_DOMAIN_VRAM, 2);
        assert_eq!(NOUVEAU_GEM_DOMAIN_GART, 4);
        assert_eq!(
            NOUVEAU_GEM_DOMAIN_MAPPABLE, 8,
            "MAPPABLE = (1 << 3) per kernel header"
        );
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
    fn nvif_compute_class_definitions() {
        assert_eq!(NVIF_CLASS_FERMI_TWOD_A, 0x902D);
        assert_eq!(NVIF_CLASS_KEPLER_INLINE_TO_MEMORY_B, 0xA0B5);
        assert_eq!(NVIF_CLASS_VOLTA_COMPUTE_A, 0xC3C0);
        assert_eq!(NVIF_CLASS_TURING_COMPUTE_A, 0xC5C0);
        assert_eq!(NVIF_CLASS_AMPERE_COMPUTE_A, 0xC6C0);
    }

    #[test]
    fn subchan_spec_layout() {
        let s = SubchanSpec {
            handle: 1,
            grclass: NVIF_CLASS_VOLTA_COMPUTE_A,
        };
        assert_eq!(s.handle, 1);
        assert_eq!(s.grclass, 0xC3C0);
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

    #[test]
    fn channel_alloc_struct_size_matches_kernel_abi() {
        // NouveauChannelAlloc (kernel drm_nouveau_channel_alloc):
        //   fb_ctxdma_handle: u32 (4)
        //   tt_ctxdma_handle: u32 (4)
        //   channel: i32 (4)
        //   pushbuf_domains: u32 (4)
        //   notifier_handle: u32 (4)
        //   subchan: [NouveauSubchan; 8] = 8 * 8 = 64
        //   nr_subchan: u32 (4)
        //   Total: 88 bytes (20 header + 64 subchan + 4 trailer)
        assert_eq!(
            std::mem::size_of::<NouveauChannelAlloc>(),
            88,
            "NouveauChannelAlloc must match kernel drm_nouveau_channel_alloc (88 bytes)"
        );
    }

    #[test]
    fn channel_free_struct_size() {
        assert_eq!(
            std::mem::size_of::<NouveauChannelFree>(),
            4,
            "NouveauChannelFree must match kernel drm_nouveau_channel_free (4 bytes)"
        );
    }

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
    fn nouveau_subchan_struct_size() {
        assert_eq!(
            std::mem::size_of::<NouveauSubchan>(),
            8,
            "NouveauSubchan must be 8 bytes (handle + grclass)"
        );
    }

    #[test]
    fn dump_channel_alloc_hex_is_nonempty() {
        let hex = dump_channel_alloc_hex(NVIF_CLASS_VOLTA_COMPUTE_A);
        assert!(hex.contains("NouveauChannelAlloc"));
        assert!(hex.contains("bytes"));
    }

    #[test]
    fn ioctl_uses_drm_iowr_pub() {
        use crate::drm;
        let nr = drm::drm_iowr_pub(
            DRM_NOUVEAU_CHANNEL_ALLOC,
            size_of_u32::<NouveauChannelAlloc>(),
        );
        assert!(nr > 0);
        assert_eq!(nr & 0xFF, 0x42, "encoded NR field = CHANNEL_ALLOC = 0x42");
    }

    #[test]
    fn nouveau_gem_cpu_prep_layout() {
        assert_eq!(std::mem::size_of::<NouveauGemCpuPrep>(), 8);
    }

    #[test]
    #[expect(clippy::cast_possible_truncation, reason = "test structs are small")]
    fn size_of_u32_matches_struct_sizes() {
        assert_eq!(
            size_of_u32::<NouveauChannelAlloc>(),
            std::mem::size_of::<NouveauChannelAlloc>() as u32
        );
        assert_eq!(
            size_of_u32::<NouveauGemNew>(),
            std::mem::size_of::<NouveauGemNew>() as u32
        );
        assert_eq!(
            size_of_u32::<NouveauGemPushbuf>(),
            std::mem::size_of::<NouveauGemPushbuf>() as u32
        );
    }
}
