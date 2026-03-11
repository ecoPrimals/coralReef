// SPDX-License-Identifier: AGPL-3.0-only
//! Channel allocation diagnostics — instrument EINVAL investigation.

use std::os::unix::io::RawFd;

use crate::drm;
use crate::error::DriverResult;

use super::{
    DRM_NOUVEAU_CHANNEL_ALLOC, NOUVEAU_GEM_DOMAIN_GART, NOUVEAU_GEM_DOMAIN_VRAM,
    NVIF_CLASS_AMPERE_COMPUTE_A, NVIF_CLASS_TURING_COMPUTE_A, NVIF_CLASS_VOLTA_COMPUTE_A,
    NouveauChannelAlloc, NouveauSubchan, create_channel, create_gv100_compute_channel,
    destroy_channel, size_of_u32,
};

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
#[must_use]
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
        // SAFETY: NouveauChannelAlloc is #[repr(C)] matching kernel struct;
        // stack-allocated, synchronous ioctl, &mut alloc is sole reference
        let result =
            match unsafe { drm::drm_ioctl_named(fd, ioctl_nr, &mut alloc, "diag_channel_alloc") } {
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
    use std::fmt::Write;

    let mut alloc = NouveauChannelAlloc {
        pushbuf_domains: NOUVEAU_GEM_DOMAIN_VRAM | NOUVEAU_GEM_DOMAIN_GART,
        nr_subchan: 1,
        ..Default::default()
    };
    alloc.subchan[0] = NouveauSubchan {
        handle: 1,
        grclass: compute_class,
    };

    let bytes = bytemuck::bytes_of(&alloc);

    let mut hex = format!("NouveauChannelAlloc ({} bytes):\n", bytes.len());
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let _ = write!(hex, "  {:04x}: ", i * 16);
        for b in chunk {
            let _ = write!(hex, "{b:02x} ");
        }
        hex.push('\n');
    }
    hex
}

/// GPU identity probing and firmware inventory.
pub use super::super::identity::{
    FirmwareInventory, FwStatus, GpuIdentity, check_nouveau_firmware, firmware_inventory,
    probe_gpu_identity,
};

/// Try the new UAPI (VM_INIT) to detect kernel support.
///
/// Returns `Ok(())` if VM_INIT is accepted, `Err` otherwise.
/// Used as a probe to decide between new and legacy dispatch paths.
pub fn probe_new_uapi_support(fd: RawFd) -> DriverResult<()> {
    super::new_uapi::vm_init(fd)
}
