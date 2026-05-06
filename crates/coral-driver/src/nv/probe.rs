// SPDX-License-Identifier: AGPL-3.0-or-later
//! GPU probing, BAR0 init, and open diagnostics for nouveau.

use crate::drm::DrmDevice;
use crate::gsp::{self, GrFirmwareBlobs, GrInitSequence};

use super::bar0;
use super::ioctl;

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
/// Delegates to [`super::generation::profile_for_sm`] — the authoritative
/// per-generation registry.
pub fn compute_class_for_sm(sm: u32) -> u32 {
    super::generation::profile_for_sm(sm).compute_class
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
    let profile = crate::nv::generation::profile_for_sm(sm);
    if crate::nv::generation::is_kepler(profile) {
        tracing::info!(sm, "Kepler GPU — skipping BAR0 GR init (nouveau handles natively)");
        return;
    }

    let chip = sm_to_chip(sm);
    let blobs = match GrFirmwareBlobs::parse(chip) {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!(chip, error = %e, "firmware not available — skipping BAR0 GR init");
            return;
        }
    };

    let profile = crate::nv::generation::profile_for_sm(sm);
    let seq = GrInitSequence::for_profile(&blobs, profile);
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

/// Boot FECS/GPCCS on Kepler GPUs where nouveau initialized hardware but
/// didn't load falcon firmware (headless compute cards like Tesla K80).
///
/// Checks if FECS is in HRESET and, if so, uploads firmware via PIO and
/// starts the falcons. Requires BAR0 write access (root).
#[cfg(feature = "nouveau")]
pub fn try_kepler_fecs_boot(render_node_path: &str, sm: u32) {
    let profile = crate::nv::generation::profile_for_sm(sm);
    if !crate::nv::generation::is_kepler(profile) {
        return;
    }

    let mut bar0 = match bar0::Bar0Access::from_render_node(render_node_path) {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!(error = %e, "BAR0 not available — cannot boot FECS");
            return;
        }
    };

    use crate::gsp::RegisterAccess;

    let fecs_cpuctl = bar0.read_u32(0x409100).unwrap_or(0xDEAD_DEAD);
    let fecs_running = fecs_cpuctl & 0x20 == 0 && fecs_cpuctl & 0x02 != 0;
    if fecs_running {
        tracing::info!(fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"), "FECS already running");
        return;
    }

    let fecs_hreset = fecs_cpuctl & 0x10 != 0;
    if !fecs_hreset {
        tracing::warn!(
            fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
            "FECS not in HRESET — cannot safely upload firmware"
        );
        return;
    }

    tracing::info!("Kepler FECS in HRESET — uploading firmware via BAR0 PIO");

    let fw_dir = format!(
        "{}/firmware/gk210",
        env!("CARGO_MANIFEST_DIR")
    );
    let load = |name: &str| -> Option<Vec<u8>> {
        let path = format!("{fw_dir}/{name}");
        match std::fs::read(&path) {
            Ok(data) => Some(data),
            Err(e) => {
                tracing::warn!(path, error = %e, "firmware file missing");
                None
            }
        }
    };

    let Some(fecs_code) = load("gk210_fecs_code.bin") else { return };
    let Some(fecs_data) = load("gk210_fecs_data.bin") else { return };
    let Some(gpccs_code) = load("gk210_gpccs_code.bin") else { return };
    let Some(gpccs_data) = load("gk210_gpccs_data.bin") else { return };

    tracing::info!(
        fecs_code = fecs_code.len(),
        fecs_data = fecs_data.len(),
        gpccs_code = gpccs_code.len(),
        gpccs_data = gpccs_data.len(),
        "Kepler firmware loaded"
    );

    let fecs_base: u32 = 0x409000;

    // Enable ITFEN (PIO transfers) on FECS
    let _ = bar0.write_u32(fecs_base + 0x048, 0x03);

    upload_falcon_dmem(&mut bar0, fecs_base, &fecs_data);
    upload_falcon_imem(&mut bar0, fecs_base, &fecs_code);

    let dmem_ok = verify_falcon_dmem(&mut bar0, fecs_base, &fecs_data);
    tracing::info!(dmem_ok, "FECS DMEM verification");

    // Upload GPCCS per-GPC
    for gpc in 0..8u32 {
        let gpccs_base = 0x50_0000 + gpc * 0x8000 + 0x2000;
        let cpuctl = bar0.read_u32(gpccs_base + 0x100).unwrap_or(0xDEAD_DEAD);
        if cpuctl == 0xDEAD_DEAD || cpuctl & 0xBAD0_0000 == 0xBAD0_0000 {
            continue;
        }
        let _ = bar0.write_u32(gpccs_base + 0x048, 0x03);
        upload_falcon_dmem(&mut bar0, gpccs_base, &gpccs_data);
        upload_falcon_imem(&mut bar0, gpccs_base, &gpccs_code);
    }

    // Boot FECS: set BOOTVEC and STARTCPU
    let _ = bar0.write_u32(fecs_base + 0x104, 0);
    let _ = bar0.write_u32(fecs_base + 0x100, 0x02);

    for i in 0..40u32 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let cpuctl = bar0.read_u32(fecs_base + 0x100).unwrap_or(0);
        let mailbox = bar0.read_u32(fecs_base + 0x800).unwrap_or(0);
        if mailbox & 0x8000_0000 != 0 || cpuctl & 0x20 != 0 {
            tracing::info!(
                poll = i,
                cpuctl = format_args!("{cpuctl:#010x}"),
                mailbox = format_args!("{mailbox:#010x}"),
                "FECS boot complete"
            );
            return;
        }
        if i % 10 == 0 {
            tracing::info!(
                poll = i,
                cpuctl = format_args!("{cpuctl:#010x}"),
                mailbox = format_args!("{mailbox:#010x}"),
                "FECS boot polling..."
            );
        }
    }
    tracing::warn!("FECS boot timed out (2s)");
}

/// Upload DMEM via manual word-by-word PIO addressing.
///
/// GK110B (K80) falcons do not support AINCW (auto-increment write) from
/// userspace mmap — each word must be individually addressed via DMEMC.
#[cfg(feature = "nouveau")]
fn upload_falcon_dmem(bar0: &mut bar0::Bar0Access, base: u32, data: &[u8]) {
    use crate::gsp::RegisterAccess;
    let words = (data.len() + 3) / 4;
    for i in 0..words {
        let off = i * 4;
        let val = u32::from_le_bytes([
            data.get(off).copied().unwrap_or(0),
            data.get(off + 1).copied().unwrap_or(0),
            data.get(off + 2).copied().unwrap_or(0),
            data.get(off + 3).copied().unwrap_or(0),
        ]);
        let addr = u32::try_from(off).unwrap_or(0);
        let _ = bar0.write_u32(base + 0x1C0, addr);
        let _ = bar0.write_u32(base + 0x1C4, val);
    }
}

/// Upload IMEM via manual word-by-word PIO addressing with 256-byte block tags.
#[cfg(feature = "nouveau")]
fn upload_falcon_imem(bar0: &mut bar0::Bar0Access, base: u32, data: &[u8]) {
    use crate::gsp::RegisterAccess;
    let words = (data.len() + 3) / 4;
    for i in 0..words {
        let off = i * 4;
        let val = u32::from_le_bytes([
            data.get(off).copied().unwrap_or(0),
            data.get(off + 1).copied().unwrap_or(0),
            data.get(off + 2).copied().unwrap_or(0),
            data.get(off + 3).copied().unwrap_or(0),
        ]);
        let addr = u32::try_from(off).unwrap_or(0);
        let _ = bar0.write_u32(base + 0x180, addr);
        if addr % 256 == 0 {
            let _ = bar0.write_u32(base + 0x188, addr / 256);
        }
        let _ = bar0.write_u32(base + 0x184, val);
    }
}

/// Verify first N words of DMEM match expected data (manual addressing).
#[cfg(feature = "nouveau")]
fn verify_falcon_dmem(bar0: &mut bar0::Bar0Access, base: u32, data: &[u8]) -> bool {
    use crate::gsp::RegisterAccess;
    let check_words = (data.len() / 4).min(16);
    for i in 0..check_words {
        let off = i * 4;
        let expected = u32::from_le_bytes([
            data[off],
            data.get(off + 1).copied().unwrap_or(0),
            data.get(off + 2).copied().unwrap_or(0),
            data.get(off + 3).copied().unwrap_or(0),
        ]);
        let addr = u32::try_from(off).unwrap_or(0);
        let _ = bar0.write_u32(base + 0x1C0, addr);
        let actual = bar0.read_u32(base + 0x1C4).unwrap_or(0);
        if actual != expected {
            tracing::warn!(
                word = i,
                expected = format_args!("{expected:#010x}"),
                actual = format_args!("{actual:#010x}"),
                "DMEM mismatch"
            );
            return false;
        }
    }
    true
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
