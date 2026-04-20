// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![allow(
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::unreadable_literal,
    clippy::redundant_closure_for_method_calls,
    clippy::missing_const_for_fn,
    clippy::option_if_let_else,
    clippy::map_unwrap_or,
    clippy::single_match_else,
    clippy::manual_let_else,
    clippy::bool_to_int_with_if,
    clippy::needless_pass_by_value,
    clippy::match_same_arms,
    clippy::redundant_pub_crate,
    clippy::branches_sharing_code,
    clippy::uninlined_format_args,
    clippy::significant_drop_tightening,
    clippy::or_fun_call,
    clippy::semicolon_if_nothing_returned,
    clippy::items_after_statements
)]
//! coral-glowplug — Sovereign `PCIe` device lifecycle broker.
//!
//! Starts at boot, binds GPUs, holds VFIO fds open forever,
//! and exposes a Unix socket for ecosystem consumers (capability-based discovery).
//!
//! Usage:
//!   coral-glowplug --config `$XDG_CONFIG_HOME`/coralreef/glowplug.toml
//!   coral-glowplug --bdf 0000:4a:00.0              # single device, defaults
//!   coral-glowplug --bdf 0000:4a:00.0 --bdf 0000:03:00.0:nouveau

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("coral-glowplug: Linux-only GPU lifecycle daemon (unsupported on this target).");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
include!("glowplug_main_linux.rs");
