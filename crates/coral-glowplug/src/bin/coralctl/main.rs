// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::unreadable_literal,
    clippy::redundant_closure_for_method_calls,
    clippy::missing_const_for_fn,
    clippy::option_if_let_else,
    clippy::map_unwrap_or,
    clippy::single_match_else,
    clippy::manual_let_else,
    clippy::needless_pass_by_value,
    clippy::match_same_arms,
    clippy::redundant_pub_crate,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::items_after_statements,
    clippy::uninlined_format_args,
    clippy::significant_drop_tightening,
    clippy::or_fun_call,
    clippy::cast_precision_loss,
    clippy::semicolon_if_nothing_returned,
    clippy::implicit_clone
)]
//! coralctl — CLI companion for coral-glowplug and coral-ember.
//!
//! All device management commands go through glowplug's JSON-RPC socket.
//! No privilege escalation needed — the user just needs to be in the
//! `coralreef` group (socket is `root:coralreef 0660`).
//!
//! Subcommands:
//!   status        List all managed devices
//!   swap          Hot-swap a device to a new driver personality
//!   health        Query device health registers
//!   probe         Dump all BAR0 registers for a device
//!   vram-probe    Check HBM2/VRAM accessibility via PRAMIN
//!   mmio          Read or write a single BAR0 register
//!   snapshot      Save or diff register snapshots
//!   deploy-udev   Generate /dev/vfio/* udev rules from glowplug.toml
#![forbid(unsafe_code)]

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("coralctl: Linux-only — this CLI manages coral-glowplug on Linux.");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
include!("coralctl_main_linux.rs");
