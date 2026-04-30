// SPDX-License-Identifier: AGPL-3.0-or-later
//! sysfs GPC0 cross-check and PRI idle before warm ladder stages.

use super::super::hardware_guard::GuardedBar;

pub(super) fn warm_preflight(guard: &GuardedBar<'_>, bdf: &str) {
    let r = guard.read_fn();
    let w = |reg: u32, val: u32| {
        if let Err(refusal) = guard.write_u32(reg, val) {
            tracing::error!(%refusal, "kepler_warm_gr_init: hardware guard refused write");
        }
    };

    // Cross-check: read GPC0 via sysfs resource0 to verify if GPCs are truly
    // alive independently of the VFIO BAR mapping.
    {
        let vfio_gpc0 = r(0x50_2608);
        let vfio_pmc = r(0x200);
        let sysfs_result = super::super::pri::sysfs_bar0_read_gpc0(bdf);
        tracing::info!(
            vfio_pmc = format_args!("{vfio_pmc:#010x}"),
            vfio_gpc0 = format_args!("{vfio_gpc0:#010x}"),
            sysfs_gpc0 = format_args!("{:#010x}", sysfs_result.unwrap_or(0xDEAD_DEAD)),
            "GPC0 cross-check: VFIO BAR vs sysfs resource0"
        );
    }

    // PRI ring faults accumulate during nouveau unbind + vfio-pci rebind.
    // Must clear them before any GR/FECS register access or reads return 0xbadfXXXX.
    super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

    let pri_status = r(0x120058);
    if pri_status != 0 {
        tracing::warn!(
            pri_status = format_args!("{pri_status:#010x}"),
            "PRI ring faults persist — re-initializing ring master"
        );
        let _ = super::super::pri::vbios_pri_ring_init(&r, &w);
        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
    }
}
