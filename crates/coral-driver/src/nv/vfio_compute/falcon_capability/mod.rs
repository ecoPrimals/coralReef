// SPDX-License-Identifier: AGPL-3.0-or-later
//! Falcon capability discovery — runtime bit solver for PIO register layouts.
//!
//! Instead of hardcoding bit positions (which vary across falcon versions and
//! caused the IMEMC BIT(24) vs BIT(6) bug), this module probes actual hardware
//! to discover the correct register format for each falcon instance.
//!
//! Each falcon self-describes: version, PIO protocol, CPUCTL layout, security
//! state, and memory sizes are all discovered at runtime. No global tables of
//! "GV100 uses this, Blackwell uses that" — the hardware tells us.

mod pio;
mod probe;
mod types;

pub use pio::FalconPio;
pub use probe::{probe_all_falcons, probe_falcon};
pub use types::{
    CpuCtlLayout, FalconCapabilities, FalconVersion, PioCtrl, PioLayout, SecurityMode,
};

pub(super) const DEAD_READ: u32 = 0xDEAD_DEAD;
