// SPDX-License-Identifier: AGPL-3.0-only
//! Nouveau DRM ioctl definitions — pure Rust, no `*-sys` crates.
//!
//! Ioctl numbers and structures are derived from the Linux kernel
//! nouveau driver headers (`nouveau_drm.h`). Ioctl syscalls use
//! inline assembly (see `drm.rs`) — zero libc dependency.
//!
//! ## Nouveau DRM ioctls used
//!
//! | Ioctl | Nr | Description |
//! |-------|----|-------------|
//! | `CHANNEL_ALLOC` | `0x00` | Allocate a GPU channel |
//! | `CHANNEL_FREE`  | `0x01` | Free a GPU channel |
//! | `GEM_NEW`       | `0x40` | Create a GEM buffer |
//! | `GEM_PUSHBUF`   | `0x41` | Submit pushbuf commands |

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
    // SAFETY:
    // 1. Validity:   NouveauChannelAlloc is #[repr(C)] matching kernel drm_nouveau_channel_alloc
    // 2. Alignment:  stack-allocated, naturally aligned
    // 3. Lifetime:   synchronous ioctl; alloc outlives the call
    // 4. Exclusivity: &mut alloc — sole reference
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut alloc)? };
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
        pad: 0,
    };
    let ioctl_nr = drm::drm_iowr_pub(
        DRM_NOUVEAU_CHANNEL_FREE,
        size_of_u32::<NouveauChannelFree>(),
    );
    // SAFETY:
    // 1. Validity:   NouveauChannelFree is #[repr(C)] matching kernel struct (8 bytes)
    // 2. Alignment:  stack-allocated, naturally aligned
    // 3. Lifetime:   synchronous ioctl; free outlives the call
    // 4. Exclusivity: &mut free — sole reference
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut free) }
}

/// Create a nouveau GEM buffer object.
///
/// Returns the GEM handle on success.
///
/// # Errors
///
/// Returns [`DriverError`] on kernel failure.
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

    let ioctl_nr = drm::drm_iowr_pub(DRM_NOUVEAU_GEM_NEW, size_of_u32::<NouveauGemNew>());
    // SAFETY:
    // 1. Validity:   NouveauGemNew is #[repr(C)] matching kernel drm_nouveau_gem_new
    // 2. Alignment:  stack-allocated, naturally aligned
    // 3. Lifetime:   synchronous ioctl; req outlives the call
    // 4. Exclusivity: &mut req — sole reference
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut req)? };
    Ok(req.info.handle)
}

/// Query GEM buffer info (offset/`map_handle`).
pub(crate) fn gem_info(fd: RawFd, handle: u32) -> DriverResult<(u64, u64)> {
    let mut req = NouveauGemNew {
        info: NouveauGemInfo {
            handle,
            ..Default::default()
        },
        ..Default::default()
    };
    let ioctl_nr = drm::drm_iowr_pub(DRM_NOUVEAU_GEM_NEW, size_of_u32::<NouveauGemNew>());
    // SAFETY:
    // 1. Validity:   NouveauGemNew is #[repr(C)] matching kernel struct; kernel fills output
    // 2. Alignment:  stack-allocated, naturally aligned
    // 3. Lifetime:   synchronous ioctl; req outlives the call
    // 4. Exclusivity: &mut req — sole reference
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
    // SAFETY:
    // 1. Validity:   NouveauGemPushbuf is #[repr(C)] matching kernel struct; all pointer
    //                fields (buffers, push) point to valid stack/heap #[repr(C)] structures
    // 2. Alignment:  all structures are naturally aligned
    // 3. Lifetime:   synchronous ioctl; pb, buffers, push all outlive the call
    // 4. Exclusivity: &mut pb — sole reference to the ioctl struct
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut pb) }
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
    // SAFETY:
    // 1. Validity:   NouveauGemCpuPrep is #[repr(C)] matching kernel struct (8 bytes)
    // 2. Alignment:  stack-allocated, naturally aligned
    // 3. Lifetime:   synchronous ioctl (blocks until buffer idle); prep outlives the call
    // 4. Exclusivity: &mut prep — sole reference
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut prep) }
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

// ---------------------------------------------------------------------------
// Diagnostic helpers — instrument EINVAL investigation without guessing
// ---------------------------------------------------------------------------

/// Diagnostic result from a channel allocation attempt.
#[derive(Debug)]
pub struct ChannelAllocDiag {
    /// Human-readable description of the attempt.
    pub description: String,
    /// Result of the attempt.
    pub result: std::result::Result<u32, String>,
}

/// Run a series of diagnostic channel allocation attempts to isolate EINVAL.
///
/// Tries multiple configurations and reports which succeed and which fail.
/// This does NOT leave channels open — successful channels are immediately
/// destroyed.
pub fn diagnose_channel_alloc(fd: RawFd, compute_class: u32) -> Vec<ChannelAllocDiag> {
    let mut results = Vec::new();

    // Attempt 1: bare channel, no subchannels
    {
        let desc = "bare channel (nr_subchan=0, no compute class)".to_string();
        let mut alloc = NouveauChannelAlloc {
            pushbuf_domains: NOUVEAU_GEM_DOMAIN_VRAM | NOUVEAU_GEM_DOMAIN_GART,
            nr_subchan: 0,
            ..Default::default()
        };
        let ioctl_nr = drm::drm_iowr_pub(
            DRM_NOUVEAU_CHANNEL_ALLOC,
            size_of_u32::<NouveauChannelAlloc>(),
        );
        #[expect(clippy::cast_sign_loss, reason = "diagnostic only")]
        let result = match unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut alloc) } {
            Ok(()) => {
                let ch = alloc.channel as u32;
                let _ = destroy_channel(fd, ch);
                Ok(ch)
            }
            Err(e) => Err(format!("{e}")),
        };
        results.push(ChannelAllocDiag {
            description: desc,
            result,
        });
    }

    // Attempt 2: single compute subchannel (the normal path)
    {
        let desc = format!("compute-only (nr_subchan=1, grclass=0x{compute_class:04X})");
        let result = match create_channel(fd, compute_class) {
            Ok(ch) => {
                let _ = destroy_channel(fd, ch);
                Ok(ch)
            }
            Err(e) => Err(format!("{e}")),
        };
        results.push(ChannelAllocDiag {
            description: desc,
            result,
        });
    }

    // Attempt 3: NVK-style multi-engine (2D + copy + compute)
    {
        let desc = format!("NVK-style multi-engine (2D + copy + compute 0x{compute_class:04X})");
        let result = match create_gv100_compute_channel(fd) {
            Ok((ch, _sub)) => {
                let _ = destroy_channel(fd, ch);
                Ok(ch)
            }
            Err(e) => Err(format!("{e}")),
        };
        results.push(ChannelAllocDiag {
            description: desc,
            result,
        });
    }

    // Attempt 4: Volta compute with different classes
    for (name, class) in [
        ("VOLTA_COMPUTE_A", NVIF_CLASS_VOLTA_COMPUTE_A),
        ("TURING_COMPUTE_A", NVIF_CLASS_TURING_COMPUTE_A),
        ("AMPERE_COMPUTE_A", NVIF_CLASS_AMPERE_COMPUTE_A),
    ] {
        if class == compute_class {
            continue; // already tested in attempt 2
        }
        let desc = format!("compute-only ({name}=0x{class:04X})");
        let result = match create_channel(fd, class) {
            Ok(ch) => {
                let _ = destroy_channel(fd, ch);
                Ok(ch)
            }
            Err(e) => Err(format!("{e}")),
        };
        results.push(ChannelAllocDiag {
            description: desc,
            result,
        });
    }

    results
}

/// Log the raw bytes of a `NouveauChannelAlloc` struct for debugging.
#[must_use]
pub fn dump_channel_alloc_hex(compute_class: u32) -> String {
    let mut alloc = NouveauChannelAlloc {
        pushbuf_domains: NOUVEAU_GEM_DOMAIN_VRAM | NOUVEAU_GEM_DOMAIN_GART,
        nr_subchan: 1,
        ..Default::default()
    };
    alloc.subchan[0] = NouveauSubchan {
        handle: 1,
        grclass: compute_class,
    };

    let size = std::mem::size_of::<NouveauChannelAlloc>();
    let ptr: *const u8 = std::ptr::from_ref(&alloc).cast();
    // SAFETY: reading repr(C) struct as bytes for diagnostic hex dump
    let bytes = unsafe { std::slice::from_raw_parts(ptr, size) };

    let mut hex = format!("NouveauChannelAlloc ({size} bytes):\n");
    for (i, chunk) in bytes.chunks(16).enumerate() {
        hex.push_str(&format!("  {:04x}: ", i * 16));
        for b in chunk {
            hex.push_str(&format!("{b:02x} "));
        }
        hex.push('\n');
    }
    hex
}

/// Check for NVIDIA firmware files required by nouveau for compute on Volta+.
///
/// Returns a list of (path, exists) for the firmware files that nouveau
/// typically needs.
#[must_use]
pub fn check_nouveau_firmware(chip: &str) -> Vec<(String, bool)> {
    let base = format!("/lib/firmware/nvidia/{chip}");
    let firmware_files = [
        "acr/bl.bin",
        "acr/ucode_unload.bin",
        "gr/fecs_bl.bin",
        "gr/fecs_inst.bin",
        "gr/fecs_data.bin",
        "gr/gpccs_bl.bin",
        "gr/gpccs_inst.bin",
        "gr/gpccs_data.bin",
        "gr/sw_ctx.bin",
        "gr/sw_nonctx.bin",
        "gr/sw_bundle_init.bin",
        "gr/sw_method_init.bin",
        "nvdec/scrubber.bin",
        "sec2/desc.bin",
        "sec2/image.bin",
        "sec2/sig.bin",
    ];

    firmware_files
        .iter()
        .map(|f| {
            let path = format!("{base}/{f}");
            let exists = std::path::Path::new(&path).exists();
            (path, exists)
        })
        .collect()
}

/// Probe sysfs for the GPU chipset on a nouveau render node.
///
/// Looks for `/sys/class/drm/renderDN/device/` to identify the PCI device.
/// Returns the PCI vendor:device ID pair if readable.
#[must_use]
pub fn probe_gpu_identity(render_node_path: &str) -> Option<GpuIdentity> {
    // /dev/dri/renderD128 → renderD128
    let node_name = render_node_path.rsplit('/').next()?;
    let sysfs_device = format!("/sys/class/drm/{node_name}/device");

    let vendor = std::fs::read_to_string(format!("{sysfs_device}/vendor")).ok()?;
    let device = std::fs::read_to_string(format!("{sysfs_device}/device")).ok()?;

    let vendor_id = u16::from_str_radix(vendor.trim().trim_start_matches("0x"), 16).ok()?;
    let device_id = u16::from_str_radix(device.trim().trim_start_matches("0x"), 16).ok()?;

    Some(GpuIdentity {
        vendor_id,
        device_id,
        sysfs_path: sysfs_device,
    })
}

/// PCI identity of a GPU device.
#[derive(Debug, Clone)]
pub struct GpuIdentity {
    /// PCI vendor ID (0x10DE for NVIDIA).
    pub vendor_id: u16,
    /// PCI device ID (maps to specific GPU model).
    pub device_id: u16,
    /// Sysfs device path.
    pub sysfs_path: String,
}

impl GpuIdentity {
    /// Map a known NVIDIA PCI device ID to an SM architecture version.
    ///
    /// Returns `None` for unrecognized device IDs. This table covers
    /// common consumer and professional GPUs.
    #[must_use]
    pub fn nvidia_sm(&self) -> Option<u32> {
        if self.vendor_id != 0x10DE {
            return None;
        }
        // GV100 (Titan V) — SM70
        // TU102/TU104/TU106/TU116/TU117 — SM75
        // GA102/GA104/GA106/GA107 — SM86
        // GA100 — SM80
        // AD102/AD103/AD104/AD106/AD107 — SM89
        match self.device_id {
            // Volta
            0x1D81 | 0x1DB1 | 0x1DB4 | 0x1DB5 | 0x1DB6 | 0x1DB7 => Some(70),
            // Turing
            0x1E02..=0x1E07
            | 0x1E30..=0x1E3D
            | 0x1E82..=0x1E87
            | 0x1F02..=0x1F15
            | 0x1F82..=0x1F95
            | 0x2182..=0x2191
            | 0x1E89..=0x1E93 => Some(75),
            // Ampere GA100
            0x2080 | 0x20B0..=0x20BF | 0x20F1..=0x20F5 => Some(80),
            // Ampere GA102/GA104/GA106/GA107
            0x2200..=0x2210
            | 0x2216
            | 0x2230..=0x2237
            | 0x2414
            | 0x2460..=0x2489
            | 0x2501..=0x2531
            | 0x2560..=0x2572
            | 0x2580..=0x25AC
            | 0x2684..=0x26B1
            | 0x2700..=0x2730
            | 0x2780..=0x2799
            | 0x2820..=0x2860
            | 0x2880..=0x2899 => Some(86),
            // Ada Lovelace AD102/AD103/AD104/AD106/AD107
            0x2600..=0x2683 => Some(89),
            _ => None,
        }
    }
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
        // NouveauChannelAlloc:
        //   fb_ctxdma_handle: u32 (4)
        //   tt_ctxdma_handle: u32 (4)
        //   channel: i32 (4)
        //   pushbuf_domains: u32 (4)
        //   notifier_handle: u32 (4)
        //   subchan: [NouveauSubchan; 8] = 8 * 8 = 64
        //   nr_subchan: u32 (4)
        //   pad: u32 (4)
        //   Total: 92 bytes (20 header + 64 subchan + 8 trailer)
        assert_eq!(
            std::mem::size_of::<NouveauChannelAlloc>(),
            92,
            "NouveauChannelAlloc must match kernel drm_nouveau_channel_alloc"
        );
    }

    #[test]
    fn channel_free_struct_size() {
        assert_eq!(
            std::mem::size_of::<NouveauChannelFree>(),
            8,
            "NouveauChannelFree must be 8 bytes (kernel ABI)"
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
    fn gpu_identity_nvidia_sm_mapping() {
        let titan_v = GpuIdentity {
            vendor_id: 0x10DE,
            device_id: 0x1D81,
            sysfs_path: String::new(),
        };
        assert_eq!(titan_v.nvidia_sm(), Some(70));

        let non_nvidia = GpuIdentity {
            vendor_id: 0x1002,
            device_id: 0x73BF,
            sysfs_path: String::new(),
        };
        assert_eq!(non_nvidia.nvidia_sm(), None);
    }

    #[test]
    fn dump_channel_alloc_hex_is_nonempty() {
        let hex = dump_channel_alloc_hex(NVIF_CLASS_VOLTA_COMPUTE_A);
        assert!(hex.contains("NouveauChannelAlloc"));
        assert!(hex.contains("bytes"));
    }

    #[test]
    fn firmware_check_returns_entries() {
        let entries = check_nouveau_firmware("gv100");
        assert!(!entries.is_empty());
        for (path, _exists) in &entries {
            assert!(path.contains("gv100"));
        }
    }

    #[test]
    fn ioctl_uses_drm_iowr_pub() {
        use crate::drm;
        let nr = drm::drm_iowr_pub(
            DRM_NOUVEAU_CHANNEL_ALLOC,
            size_of_u32::<NouveauChannelAlloc>(),
        );
        assert!(nr > 0);
        assert_eq!(nr & 0xFF, u64::from(DRM_NOUVEAU_CHANNEL_ALLOC));
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
