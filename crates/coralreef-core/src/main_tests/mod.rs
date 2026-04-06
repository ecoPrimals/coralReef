// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

mod cli;
mod cmd_compile;
mod cmd_doctor;
mod cmd_server;
mod discovery;
mod exit_unibin;
mod shutdown;

#[cfg(unix)]
mod cmd_server_process;
