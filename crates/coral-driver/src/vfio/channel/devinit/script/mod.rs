// SPDX-License-Identifier: AGPL-3.0-or-later
//! VBIOS init script parsing and host-side interpreter.
//!
//! Reference: nouveau nvkm/subdev/bios/init.c (Ben Skeggs, Red Hat)

mod interpreter;
mod scan;

pub use interpreter::{InterpreterStats, interpret_boot_scripts};
pub use scan::{ScriptRegWrite, extract_boot_script_writes, scan_init_script_writes};
