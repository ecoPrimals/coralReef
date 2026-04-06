// SPDX-License-Identifier: AGPL-3.0-or-later

//! IOVA layout for system-memory ACR boot.
//!
//! Placed at `0x40000+` to avoid channel infrastructure (`0x1000..0xB000`)
//! and FECS init push buffer (`0x100000`). All within first 2 MiB for
//! single PT0 coverage.
//!
//! The LOW_CATCH region (0x0000..0x40000) provides IOMMU-valid backing for
//! VAs the ACR firmware accesses below our named buffers. Without this,
//! identity-mapped VA 0x26000 (etc.) translates to IOVA 0x26000 but has
//! no IOMMU mapping, causing IO_PAGE_FAULT and SEC2 DMA trap.

/// Catch-all backing for low VA range (256 KiB at IOVA 0).
/// Covers 0x0000..0x40000 to prevent IOMMU faults on ACR internal DMA.
pub const LOW_CATCH: u64 = 0x0;
/// Size of the low catch-all region.
pub const LOW_CATCH_SIZE: usize = 0x4_0000;

/// SEC2 instance block (4 KiB).
pub const INST: u64 = 0x4_0000;
/// Page directory level 3 (4 KiB).
pub const PD3: u64 = 0x4_1000;
/// Page directory level 2 (4 KiB).
pub const PD2: u64 = 0x4_2000;
/// Page directory level 1 (4 KiB).
pub const PD1: u64 = 0x4_3000;
/// Page directory level 0 (4 KiB).
pub const PD0: u64 = 0x4_4000;
/// Page table level 0 (4 KiB).
pub const PT0: u64 = 0x4_5000;
/// ACR payload (up to 32 KiB).
pub const ACR: u64 = 0x4_6000;
/// Shadow WPR (up to 128 KiB) — separate from WPR for ACR verification.
pub const SHADOW: u64 = 0x6_0000;
/// WPR region (up to 128 KiB) — at 0x70000 to match VRAM-style addressing.
pub const WPR: u64 = 0x7_0000;
