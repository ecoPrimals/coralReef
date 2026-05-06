// SPDX-License-Identifier: AGPL-3.0-or-later

//! Warm Kepler GR initialization — IMEM-preserved restart or cold boot fallback.
//!
//! Sub-shards preserve HW MMIO ordering; façade sequences them with early-exit
//! `bool` handshakes identical to nested `return` in the legacy monolithic file.

mod early_pmu_pmc;
mod fecs_engctl_warm;
mod gr_hub_load;
mod post_done_firmware;
mod preflight;

/// Warm Kepler GR initialization: try IMEM-preserved restart, else cold boot.
///
/// After nouveau POST + vfio-pci rebind, PLLs and PRI ring are alive.
/// On kernel 6.17, nouveau does NOT initialize GR for GK110B — FECS is
/// left in HRESET with empty IMEM. The warm restart path detects this and
/// falls back to `kepler_load_and_boot_fecs` which uploads the correct
/// GK110 desktop firmware (extracted from nouveau.ko's internal blobs).
///
/// If FECS IMEM does contain firmware (e.g. a future kernel initializes GR),
/// the warm path attempts a fast restart first.
pub fn kepler_warm_gr_init(guard: &super::hardware_guard::GuardedBar<'_>, bdf: &str) {
    preflight::warm_preflight(guard, bdf);
    if post_done_firmware::maybe_post_done_early_boot(guard, bdf) {
        return;
    }
    if early_pmu_pmc::warm_early_pmu_and_pmc_recovery(guard) {
        return;
    }
    if gr_hub_load::maybe_gr_hub_firmware_prep_and_upload(guard) {
        return;
    }
    let _ = fecs_engctl_warm::warm_fecs_engctl_restart_or_fallback(guard);
}
