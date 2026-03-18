// SPDX-License-Identifier: AGPL-3.0-only
//! GPU probing, BAR0 init, and open diagnostics for nouveau.

use crate::drm::DrmDevice;
use crate::gsp::{self, GrFirmwareBlobs, GrInitSequence};

use super::bar0;
use super::ioctl;
use super::pushbuf;

/// Syncobj wait timeout in nanoseconds (5 seconds).
///
/// Applied to both FECS init and compute dispatch syncobj waits.
const SYNCOBJ_TIMEOUT_NS: i64 = 5_000_000_000;

/// Compute a monotonic deadline `SYNCOBJ_TIMEOUT_NS` from now.
pub fn syncobj_deadline() -> i64 {
    let tp = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    tp.tv_sec * 1_000_000_000 + tp.tv_nsec as i64 + SYNCOBJ_TIMEOUT_NS
}

/// Select the compute engine class for a GPU architecture.
///
/// Returns the DRM class ID that the kernel needs to instantiate a compute
/// engine on this GPU generation.
pub const fn compute_class_for_sm(sm: u32) -> u32 {
    match sm {
        75 => pushbuf::class::TURING_COMPUTE_A,
        80..=89 => pushbuf::class::AMPERE_COMPUTE_A,
        _ => pushbuf::class::VOLTA_COMPUTE_A,
    }
}

/// Map SM architecture version to the chip codename used by firmware paths.
///
/// Delegates to [`super::identity::chip_name`] — single source of truth.
#[must_use]
pub const fn sm_to_chip(sm: u32) -> &'static str {
    super::identity::chip_name(sm)
}

/// Sovereign BAR0 GR initialization — Phase 0 of device open.
///
/// Opens the GPU's BAR0 MMIO window via sysfs and writes the PGRAPH
/// register init sequence parsed from NVIDIA firmware blobs. This replaces
/// the PMU firmware that nouveau lacks on Volta and supplements GSP on
/// Ampere where the kernel's init path may be incomplete.
///
/// Gracefully falls back if BAR0 access is unavailable (no root, no sysfs).
/// When it succeeds, subsequent channel creation should find a valid GR
/// context, resolving the CTXNOTVALID error.
#[cfg(feature = "nouveau")]
pub fn try_bar0_gr_init(render_node_path: &str, sm: u32) {
    let chip = sm_to_chip(sm);
    let blobs = match GrFirmwareBlobs::parse(chip) {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!(chip, error = %e, "firmware not available — skipping BAR0 GR init");
            return;
        }
    };

    let seq = GrInitSequence::for_gv100(&blobs);
    let (bar0_entries, fecs_entries) = gsp::split_for_application(&seq);

    tracing::info!(
        chip,
        bar0_writes = bar0_entries.len(),
        fecs_entries = fecs_entries.len(),
        total = seq.len(),
        "sovereign GR init: {} BAR0 register writes to apply",
        bar0_entries.len()
    );

    if bar0_entries.len() <= 2 {
        tracing::debug!(
            chip,
            "only pre-init entries — no PGRAPH registers to write via BAR0"
        );
        return;
    }

    let mut bar0 = match bar0::Bar0Access::from_render_node(render_node_path) {
        Ok(b) => b,
        Err(e) => {
            tracing::info!(
                chip,
                error = %e,
                "BAR0 access not available (needs root) — falling back to kernel GR init"
            );
            return;
        }
    };

    let boot_id = bar0.read_boot_id().unwrap_or(0);
    tracing::info!(
        chip,
        boot_id = format_args!("{boot_id:#010x}"),
        bar0_size_mib = bar0.size() / (1024 * 1024),
        "BAR0 open — applying sovereign GR init sequence"
    );

    let result = gsp::apply_bar0(&seq, &mut bar0);

    if result.success() {
        tracing::info!(
            chip,
            bar0_writes = result.bar0_writes,
            fecs_remaining = result.fecs_entries,
            "sovereign BAR0 GR init complete — PGRAPH registers written"
        );
    } else {
        tracing::warn!(
            chip,
            bar0_writes = result.bar0_writes,
            errors = result.errors.len(),
            "sovereign BAR0 GR init had errors: {:?}",
            result.errors
        );
    }

    let verify_errors = gsp::verify_pre_init(&bar0);
    if verify_errors.is_empty() {
        tracing::info!(chip, "BAR0 pre-init verification passed");
    } else {
        tracing::warn!(chip, errors = ?verify_errors, "BAR0 pre-init verification issues");
    }
}

/// Run diagnostic probes when channel creation fails.
#[cfg(feature = "nouveau")]
pub fn run_open_diagnostics(drm: &DrmDevice, sm: u32, compute_class: u32) {
    let diags = ioctl::diagnose_channel_alloc(drm.fd(), compute_class);
    for diag in &diags {
        match &diag.result {
            Ok(ch) => tracing::info!(
                description = %diag.description,
                channel = ch,
                "diagnostic: PASS"
            ),
            Err(err) => tracing::warn!(
                description = %diag.description,
                error = %err,
                "diagnostic: FAIL"
            ),
        }
    }
    let chip = sm_to_chip(sm);
    let fw = ioctl::check_nouveau_firmware(chip);
    let missing: Vec<_> = fw.iter().filter(|(_, exists)| !*exists).collect();
    if !missing.is_empty() {
        tracing::warn!(
            chip,
            missing_count = missing.len(),
            "nouveau firmware files missing — compute may not be available"
        );
    }
    if let Some(id) = ioctl::probe_gpu_identity(&drm.path) {
        tracing::info!(
            vendor = format_args!("0x{:04X}", id.vendor_id),
            device = format_args!("0x{:04X}", id.device_id),
            detected_sm = ?id.nvidia_sm(),
            "GPU identity from sysfs"
        );
    }
}
