// SPDX-License-Identifier: AGPL-3.0-or-later

//! Boot strategies: mailbox command, direct FECS, HRESET, EMEM, nouveau-style SEC2.

mod bootvec;
mod direct_falcon_upload;
mod direct_fecs;
mod direct_hreset;
mod emem;
mod mailbox_command;
mod nouveau;
mod physical_first;

pub use bootvec::FalconBootvecOffsets;
pub use direct_falcon_upload::attempt_direct_falcon_upload;
pub use direct_fecs::attempt_direct_fecs_boot;
pub use direct_hreset::attempt_direct_hreset;
pub use emem::attempt_emem_boot;
pub use mailbox_command::attempt_acr_mailbox_command;
pub use nouveau::attempt_nouveau_boot;
pub use physical_first::attempt_physical_first_boot;
