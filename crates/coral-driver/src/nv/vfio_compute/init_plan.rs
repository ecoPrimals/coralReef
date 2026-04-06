// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure plans and snapshots for GR init and warm falcon restart (`init.rs` I/O).

use std::borrow::Cow;

use crate::gsp::{GrRegWrite, RegCategory};
use crate::vfio::channel::registers::falcon;
use crate::vfio::device::Bar0;

fn read_at(reads: &[(usize, u32)], addr: usize) -> u32 {
    reads
        .iter()
        .find(|(a, _)| *a == addr)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

/// BAR0 dynamic GR init values derived from hardware reads (see `apply_dynamic_gr_plan`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DynamicGrInitPlan {
    /// `0x418880` — GPC MMU config from `0x100c80`.
    pub gpc_mmu_cfg: u32,
    /// `0x4188b4` — FB MMU write target.
    pub fb_mmu_write: u32,
    /// `0x4188b8` — FB MMU read target.
    pub fb_mmu_read: u32,
    /// `0x4188b0` — FB MMU base.
    pub fb_mmu_base: u32,
    /// `0x4188ac` — active LTCs (from `0x100800`).
    pub active_ltcs: u32,
    /// `0x41833c` — secondary LTC topology (from `0x100804`).
    pub active_ltcs_aux: u32,
    /// ROP active FBP count (low nibble of `0x12006c`).
    pub fbp_count: u32,
    /// Current `0x408850` before mask.
    pub cur_408850: u32,
    /// Current `0x408958` before mask.
    pub cur_408958: u32,
    /// Current `0x40584c` before OR.
    pub cur_40584c: u32,
}

impl DynamicGrInitPlan {
    /// Pure: compute masks and targets from `(BAR0 offset, value)` read pairs.
    #[must_use]
    pub fn from_reads(reads: &[(usize, u32)]) -> Self {
        let gpc_mmu_cfg = read_at(reads, 0x100c80) & 0xf000_1fff;
        let fbp_count = read_at(reads, 0x12006c) & 0xf;
        Self {
            gpc_mmu_cfg,
            fb_mmu_write: read_at(reads, 0x100cc8),
            fb_mmu_read: read_at(reads, 0x100ccc),
            fb_mmu_base: read_at(reads, 0x100cc4),
            active_ltcs: read_at(reads, 0x100800),
            active_ltcs_aux: read_at(reads, 0x100804),
            fbp_count,
            cur_408850: read_at(reads, 0x408850),
            cur_408958: read_at(reads, 0x408958),
            cur_40584c: read_at(reads, 0x40584c),
        }
    }
}

/// Apply [`DynamicGrInitPlan`] via BAR0 MMIO (nouveau `gf100_gr_init` dynamic path).
pub(crate) fn apply_dynamic_gr_plan(plan: &DynamicGrInitPlan, bar0: &dyn Bar0) {
    let _ = bar0.write_u32(0x418880, plan.gpc_mmu_cfg);
    let _ = bar0.write_u32(0x418890, 0);
    let _ = bar0.write_u32(0x418894, 0);
    let _ = bar0.write_u32(0x4188b4, plan.fb_mmu_write);
    let _ = bar0.write_u32(0x4188b8, plan.fb_mmu_read);
    let _ = bar0.write_u32(0x4188b0, plan.fb_mmu_base);
    let _ = bar0.write_u32(0x4188ac, plan.active_ltcs);
    let _ = bar0.write_u32(0x41833c, plan.active_ltcs_aux);
    let _ = bar0.write_u32(0x408850, (plan.cur_408850 & !0xf) | plan.fbp_count);
    let _ = bar0.write_u32(0x408958, (plan.cur_408958 & !0xf) | plan.fbp_count);
    let _ = bar0.write_u32(0x40802c, 1);
    let _ = bar0.write_u32(0x400100, 0xffff_ffff);
    let _ = bar0.write_u32(0x40013c, 0xffff_ffff);
    let _ = bar0.write_u32(0x400124, 0x0000_0002);
    let _ = bar0.write_u32(0x404000, 0xc000_0000);
    let _ = bar0.write_u32(0x404600, 0xc000_0000);
    let _ = bar0.write_u32(0x408030, 0xc000_0000);
    let _ = bar0.write_u32(0x406018, 0xc000_0000);
    let _ = bar0.write_u32(0x404490, 0xc000_0000);
    let _ = bar0.write_u32(0x405840, 0xc000_0000);
    let _ = bar0.write_u32(0x405844, 0x00ff_ffff);
    let _ = bar0.write_u32(0x405848, 0xc000_0000);
    let _ = bar0.write_u32(0x40584c, plan.cur_40584c | 1);
    let _ = bar0.write_u32(0x407020, 0x4000_0000);
    let _ = bar0.write_u32(0x400108, 0xffff_ffff);
    let _ = bar0.write_u32(0x400138, 0xffff_ffff);
    let _ = bar0.write_u32(0x400118, 0xffff_ffff);
    let _ = bar0.write_u32(0x400130, 0xffff_ffff);
    let _ = bar0.write_u32(0x40011c, 0xffff_ffff);
    let _ = bar0.write_u32(0x400134, 0xffff_ffff);
    let _ = bar0.write_u32(0x409c24, 0x000e_0002);
    let _ = bar0.write_u32(0x400500, 0x0001_0001);
}

/// FECS/GPCCS falcon registers + master clock enable snapshot for warm restart.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WarmFalconSnapshot {
    /// FECS `CPUCTL`.
    pub fecs_cpuctl: u32,
    /// FECS `SCTL`.
    pub fecs_sctl: u32,
    /// FECS program counter.
    pub fecs_pc: u32,
    /// FECS exception info.
    pub fecs_exci: u32,
    /// FECS `MAILBOX0`.
    pub fecs_mb0: u32,
    /// GPCCS `CPUCTL`.
    pub gpccs_cpuctl: u32,
    /// GPCCS program counter.
    pub gpccs_pc: u32,
    /// GPCCS exception info.
    pub gpccs_exci: u32,
    /// GR engine enable (`0x400500`).
    pub gr_enable: u32,
    /// `NV_PMC_ENABLE` — engine clock / enable bitmask (master control).
    pub mc_status: u32,
}

impl WarmFalconSnapshot {
    /// Read falcon + MC state from BAR0.
    #[must_use]
    pub fn capture(bar0: &dyn Bar0) -> Self {
        let r = |a: usize| bar0.read_u32(a).unwrap_or(0xDEAD_DEAD);
        Self {
            fecs_cpuctl: r(falcon::FECS_BASE + falcon::CPUCTL),
            fecs_sctl: r(falcon::FECS_BASE + falcon::SCTL),
            fecs_pc: r(falcon::FECS_BASE + falcon::PC),
            fecs_exci: r(falcon::FECS_BASE + falcon::EXCI),
            fecs_mb0: r(falcon::FECS_BASE + falcon::MAILBOX0),
            gpccs_cpuctl: r(falcon::GPCCS_BASE + falcon::CPUCTL),
            gpccs_pc: r(falcon::GPCCS_BASE + falcon::PC),
            gpccs_exci: r(falcon::GPCCS_BASE + falcon::EXCI),
            gr_enable: r(0x400500),
            mc_status: r(crate::vfio::channel::registers::misc::PMC_ENABLE),
        }
    }

    fn fecs_dead(self) -> bool {
        self.fecs_cpuctl == 0xDEAD_DEAD || self.fecs_cpuctl & 0xBADF_0000 == 0xBADF_0000
    }

    fn fecs_halted(self) -> bool {
        self.fecs_cpuctl & falcon::CPUCTL_HALTED != 0
    }

    fn fecs_hreset(self) -> bool {
        self.fecs_cpuctl & falcon::CPUCTL_HRESET != 0
    }
}

/// Pure warm-restart policy from [`WarmFalconSnapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WarmRestartDecision {
    /// PRI timeout — treat as cold GPU.
    FecsUnreachable,
    /// Continue: re-apply GR enables and optional GR context setup.
    Proceed {
        /// Log livepatch / HRESET advisory.
        warn_hreset: bool,
        /// FECS halted but not in HRESET — method interface may work.
        log_halted_not_hreset: bool,
    },
}

impl WarmRestartDecision {
    /// Determine restart actions from a snapshot (no I/O).
    #[must_use]
    pub fn from_snapshot(snap: &WarmFalconSnapshot) -> Self {
        if snap.fecs_dead() {
            return Self::FecsUnreachable;
        }
        let warn_hreset = snap.fecs_hreset();
        let log_halted_not_hreset = snap.fecs_halted() && !snap.fecs_hreset();
        Self::Proceed {
            warn_hreset,
            log_halted_not_hreset,
        }
    }
}

/// One FECS method `(class method offset, argument)` for GPFIFO submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FecsMethod {
    /// Method offset (often BAR0 method index for GR class).
    pub offset: u32,
    /// Method data.
    pub value: u32,
}

/// Pure list of FECS channel methods from firmware-derived GR writes.
///
/// `warm`: when `true`, returns an empty list (firmware already owns the channel).
/// `golden`: when `true`, sorts methods by offset for stable submission order;
/// when `false`, preserves the firmware sequence order.
#[must_use]
pub(crate) fn fecs_init_methods(
    fecs_writes: &[&GrRegWrite],
    golden: bool,
    warm: bool,
) -> Vec<FecsMethod> {
    if warm {
        return Vec::new();
    }
    let mut out = Vec::new();
    for &w in fecs_writes {
        if matches!(
            w.category,
            RegCategory::BundleInit | RegCategory::MethodInit
        ) {
            out.push(FecsMethod {
                offset: w.offset,
                value: w.value,
            });
        }
    }
    if golden {
        out.sort_by_key(|m| m.offset);
    }
    out
}

/// Error token for unreachable FECS during warm restart.
#[must_use]
pub(crate) fn warm_fecs_unreachable_err() -> Cow<'static, str> {
    Cow::Borrowed("FECS unreachable (PRI timeout) — GPU is cold")
}

#[cfg(test)]
mod tests {
    use crate::gsp::{GrRegWrite, RegCategory};
    use crate::vfio::channel::registers::falcon;

    use super::{
        DynamicGrInitPlan, FecsMethod, WarmFalconSnapshot, WarmRestartDecision, fecs_init_methods,
    };

    #[test]
    fn dynamic_gr_init_plan_from_reads_masks_gpc_mmu_and_fbp() {
        let reads = [
            (0x100c80, 0xFFFF_FABC),
            (0x100cc8, 0x1111),
            (0x100ccc, 0x2222),
            (0x100cc4, 0x3333),
            (0x100800, 0x4444),
            (0x100804, 0x5555),
            (0x12006c, 0xFFFF_FFF3),
            (0x408850, 0xAAAA_A00C),
            (0x408958, 0xBBBB_B00D),
            (0x40584c, 0xCCCC_C000),
        ];
        let plan = DynamicGrInitPlan::from_reads(&reads);
        assert_eq!(plan.gpc_mmu_cfg, 0xF000_1ABC);
        assert_eq!(plan.fb_mmu_write, 0x1111);
        assert_eq!(plan.fb_mmu_read, 0x2222);
        assert_eq!(plan.fb_mmu_base, 0x3333);
        assert_eq!(plan.active_ltcs, 0x4444);
        assert_eq!(plan.active_ltcs_aux, 0x5555);
        assert_eq!(plan.fbp_count, 3);
        assert_eq!(plan.cur_408850, 0xAAAA_A00C);
        assert_eq!(plan.cur_408958, 0xBBBB_B00D);
        assert_eq!(plan.cur_40584c, 0xCCCC_C000);
    }

    #[test]
    fn dynamic_gr_init_plan_empty_reads_all_zero() {
        let plan = DynamicGrInitPlan::from_reads(&[]);
        assert_eq!(plan.gpc_mmu_cfg, 0);
        assert_eq!(plan.fbp_count, 0);
    }

    #[test]
    fn dynamic_gr_init_plan_reads_without_extra_masked_bits() {
        let reads = [
            (0x100c80, 0x0000_0123),
            (0x12006c, 0x4),
            (0x408850, 0xFFFF_FFF4),
            (0x408958, 0xFFFF_FFF4),
        ];
        let plan = DynamicGrInitPlan::from_reads(&reads);
        assert_eq!(plan.gpc_mmu_cfg, 0x0000_0123);
        assert_eq!(plan.fbp_count, 4);
    }

    #[test]
    fn warm_restart_fecs_unreachable_dead_read() {
        let snap = WarmFalconSnapshot {
            fecs_cpuctl: 0xDEAD_DEAD,
            fecs_sctl: 0,
            fecs_pc: 0,
            fecs_exci: 0,
            fecs_mb0: 0,
            gpccs_cpuctl: 0,
            gpccs_pc: 0,
            gpccs_exci: 0,
            gr_enable: 1,
            mc_status: 1,
        };
        assert_eq!(
            WarmRestartDecision::from_snapshot(&snap),
            WarmRestartDecision::FecsUnreachable
        );
    }

    #[test]
    fn warm_restart_fecs_unreachable_badf_prefix() {
        let snap = WarmFalconSnapshot {
            fecs_cpuctl: 0xBADF_0000,
            fecs_sctl: 0,
            fecs_pc: 0x1000,
            fecs_exci: 0,
            fecs_mb0: 0,
            gpccs_cpuctl: 0,
            gpccs_pc: 0,
            gpccs_exci: 0,
            gr_enable: 1,
            mc_status: 1,
        };
        assert_eq!(
            WarmRestartDecision::from_snapshot(&snap),
            WarmRestartDecision::FecsUnreachable
        );
    }

    #[test]
    fn warm_restart_fecs_halted_proceed_advisory() {
        let snap = WarmFalconSnapshot {
            fecs_cpuctl: falcon::CPUCTL_HALTED,
            fecs_sctl: 0,
            fecs_pc: 0x400,
            fecs_exci: 0,
            fecs_mb0: 0,
            gpccs_cpuctl: 0,
            gpccs_pc: 0,
            gpccs_exci: 0,
            gr_enable: 1,
            mc_status: 1,
        };
        assert_eq!(
            WarmRestartDecision::from_snapshot(&snap),
            WarmRestartDecision::Proceed {
                warn_hreset: false,
                log_halted_not_hreset: true,
            }
        );
    }

    #[test]
    fn warm_restart_fecs_running_not_halted_proceed() {
        let snap = WarmFalconSnapshot {
            fecs_cpuctl: falcon::CPUCTL_STARTCPU,
            fecs_sctl: 0,
            fecs_pc: 0x200,
            fecs_exci: 0,
            fecs_mb0: 0,
            gpccs_cpuctl: falcon::CPUCTL_HALTED,
            gpccs_pc: 0x500,
            gpccs_exci: 0,
            gr_enable: 1,
            mc_status: 1,
        };
        assert_eq!(
            WarmRestartDecision::from_snapshot(&snap),
            WarmRestartDecision::Proceed {
                warn_hreset: false,
                log_halted_not_hreset: false,
            }
        );
    }

    #[test]
    fn warm_restart_fecs_hreset_warn() {
        let snap = WarmFalconSnapshot {
            fecs_cpuctl: falcon::CPUCTL_HALTED | falcon::CPUCTL_HRESET,
            fecs_sctl: 0,
            fecs_pc: 0,
            fecs_exci: 0,
            fecs_mb0: 0,
            gpccs_cpuctl: falcon::CPUCTL_HALTED,
            gpccs_pc: 0,
            gpccs_exci: 0,
            gr_enable: 1,
            mc_status: 1,
        };
        assert_eq!(
            WarmRestartDecision::from_snapshot(&snap),
            WarmRestartDecision::Proceed {
                warn_hreset: true,
                log_halted_not_hreset: false,
            }
        );
    }

    fn w(off: u32, v: u32, cat: RegCategory) -> GrRegWrite {
        GrRegWrite {
            offset: off,
            value: v,
            category: cat,
            delay_us: 0,
        }
    }

    #[test]
    fn fecs_init_methods_warm_empty() {
        let a = w(10, 1, RegCategory::BundleInit);
        let seq = [&a];
        assert!(fecs_init_methods(&seq, true, true).is_empty());
    }

    #[test]
    fn fecs_init_methods_golden_sorts_offsets() {
        let a = w(0x30, 1, RegCategory::MethodInit);
        let b = w(0x10, 2, RegCategory::BundleInit);
        let c = w(0x20, 3, RegCategory::BundleInit);
        let seq = [&a, &b, &c];
        let got = fecs_init_methods(&seq, true, false);
        assert_eq!(
            got,
            vec![
                FecsMethod {
                    offset: 0x10,
                    value: 2
                },
                FecsMethod {
                    offset: 0x20,
                    value: 3
                },
                FecsMethod {
                    offset: 0x30,
                    value: 1
                },
            ]
        );
    }

    #[test]
    fn fecs_init_methods_bundle_only_preserves_order_when_not_golden() {
        let a = w(0x40, 1, RegCategory::BundleInit);
        let b = w(0x10, 2, RegCategory::BundleInit);
        let seq = [&a, &b];
        let got = fecs_init_methods(&seq, false, false);
        assert_eq!(
            got,
            vec![
                FecsMethod {
                    offset: 0x40,
                    value: 1
                },
                FecsMethod {
                    offset: 0x10,
                    value: 2
                },
            ]
        );
    }

    #[test]
    fn fecs_init_methods_method_only_sequence() {
        let a = w(0x100, 9, RegCategory::MethodInit);
        let b = w(0x200, 8, RegCategory::MethodInit);
        let seq = [&a, &b];
        let got = fecs_init_methods(&seq, false, false);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].offset, 0x100);
        assert_eq!(got[1].offset, 0x200);
    }

    #[test]
    fn fecs_init_methods_skips_non_init_categories() {
        let a = w(0x1, 1, RegCategory::MasterControl);
        let b = w(0x2, 2, RegCategory::BundleInit);
        let seq = [&a, &b];
        let got = fecs_init_methods(&seq, false, false);
        assert_eq!(
            got,
            vec![FecsMethod {
                offset: 0x2,
                value: 2
            }]
        );
    }
}
