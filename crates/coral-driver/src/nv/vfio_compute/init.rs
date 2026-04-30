// SPDX-License-Identifier: AGPL-3.0-or-later
//! VFIO GR init — thin re-export facade.
//!
//! The actual implementation lives in focused sub-modules:
//! - [`super::gr_bar0`] — firmware-blob-driven BAR0 register writes
//! - [`super::warm_channel`] — warm falcon restart and FECS channel init
//! - [`super::kepler_cold`] — Kepler cold-boot (PRI ring → clocks → FECS)
//! - [`super::kepler_warm`] — warm Kepler GR init
//! - [`super::kepler_recovery`] — cold recovery after bus reset
//! - [`super::kepler_fecs_boot`] — FECS/GPCCS firmware upload and boot
//! - [`super::pmu`] — PMU falcon firmware boot
//! - [`super::pgob`] — PGOB power gating control
//! - [`super::pri`] — PRI ring management
//! - [`super::quiesce`] — engine quiesce before teardown
//! - [`super::vbios_devinit`] — VBIOS DEVINIT script interpreter

pub(crate) use super::kepler_cold::kepler_cold_init;
pub(crate) use super::kepler_warm::kepler_warm_gr_init;
