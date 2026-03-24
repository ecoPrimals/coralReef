// SPDX-License-Identifier: AGPL-3.0-only

//! IOVA layout for system-memory ACR boot.
//!
//! Placed at `0x40000+` to avoid channel infrastructure (`0x1000..0xB000`)
//! and FECS init push buffer (`0x100000`). All within first 2 MiB for
//! single PT0 coverage.

/// SEC2 instance block (4 KiB).
pub const INST: u64 = 0x4_0000;
pub const PD3: u64 = 0x4_1000;
pub const PD2: u64 = 0x4_2000;
pub const PD1: u64 = 0x4_3000;
pub const PD0: u64 = 0x4_4000;
pub const PT0: u64 = 0x4_5000;
/// ACR payload (up to 32 KiB).
pub const ACR: u64 = 0x4_6000;
/// WPR region (up to 128 KiB).
pub const WPR: u64 = 0x4_E000;
