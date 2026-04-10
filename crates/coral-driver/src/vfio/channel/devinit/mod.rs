// SPDX-License-Identifier: AGPL-3.0-or-later
//! GPU Device Initialization (devinit) — sovereign HBM2/GDDR training via VBIOS + PMU.
//!
//! After a GPU enters D3cold and returns to D0, the HBM2 memory controller loses
//! its training state. The GPU's boot ROM normally re-runs the devinit sequence
//! during power-on, but when bound to `vfio-pci` this can fail silently.
//!
//! This module replicates what nouveau's `gm200_devinit_post()` does:
//! 1. Read the VBIOS ROM from sysfs or PRAMIN
//! 2. Parse the BIT (BIOS Information Table) structure
//! 3. Extract the PMU DEVINIT firmware (type 0x04)
//! 4. Upload code+data to the PMU FALCON microcontroller via BAR0
//! 5. Execute the devinit script interpreter on the PMU
//! 6. Wait for completion — at which point HBM2 is trained and VRAM is alive
//!
//! This is the sovereign alternative to binding nouveau just for warmth.
//!
//! # Register map (PMU FALCON at BAR0 + 0x10A000)
//!
//! | Register    | Offset    | Description                      |
//! |-------------|-----------|----------------------------------|
//! | FALCON_CTRL | 0x10A100  | Start/stop PMU execution         |
//! | FALCON_PC   | 0x10A104  | Program counter (init address)   |
//! | FALCON_TRIG | 0x10A10C  | Execution trigger                |
//! | FALCON_MBOX | 0x10A040  | Mailbox / completion signal      |
//! | IMEM_PORT   | 0x10A180  | IMEM access (code upload select) |
//! | IMEM_DATA   | 0x10A184  | IMEM data write                  |
//! | IMEM_TAG    | 0x10A188  | IMEM block tag/address           |
//! | DMEM_PORT   | 0x10A1C0  | DMEM access (data upload select) |
//! | DMEM_DATA   | 0x10A1C4  | DMEM data write                  |

mod pci;
mod pmu;
mod script;
mod vbios;

// Re-exports for public API
pub use crate::error::DevinitError;
pub use pci::{force_pci_d0, pci_power_cycle_devinit};
pub use pmu::{DevinitStatus, FalconDiagnostic, execute_devinit, execute_devinit_with_diagnostics};
pub use script::{
    InterpreterStats, ScriptRegWrite, extract_boot_script_writes, interpret_boot_scripts,
    scan_init_script_writes,
};
pub use vbios::{
    BitEntry, BitTable, PmuFirmware, parse_pmu_table, read_vbios_file, read_vbios_prom,
    read_vbios_sysfs,
};
