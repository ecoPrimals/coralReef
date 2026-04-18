// SPDX-License-Identifier: AGPL-3.0-or-later

//! SEC2 falcon probing, EMEM/IMEM/DMEM helpers, and engine reset.

mod boot_prepare;
mod diagnostics;
mod emem;
mod falcon_cpu;
mod falcon_mem_upload;
mod falcon_reset;
mod pmc;
mod probe;

pub(crate) use falcon_mem_upload::sec2_dmem_read;
pub use falcon_mem_upload::{falcon_dmem_upload, falcon_imem_upload_nouveau};

pub use boot_prepare::{sec2_prepare_direct_boot, sec2_prepare_physical_first};
pub use diagnostics::{sec2_exit_diagnostics, sec2_tracepc_dump};
pub use emem::{sec2_emem_read, sec2_emem_verify, sec2_emem_write};
pub(crate) use falcon_cpu::falcon_prepare_physical_dma;
pub use falcon_cpu::{falcon_configure_fbif_all_sysmem, falcon_pio_scrub_imem, falcon_start_cpu};
pub use falcon_reset::{falcon_engine_reset, reset_sec2};
pub(crate) use pmc::{find_sec2_pmc_bit, pmc_enable_sec2};
pub use probe::{Sec2Probe, Sec2State};
