// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON-RPC 2.0 IPC handler and SCM_RIGHTS fd passing.

mod fd;
mod jsonrpc;

#[cfg(target_os = "linux")]
mod dispatch;
#[cfg(target_os = "linux")]
mod handlers_device;
#[cfg(target_os = "linux")]
mod handlers_devinit;
#[cfg(target_os = "linux")]
mod handlers_journal;
#[cfg(target_os = "linux")]
mod handlers_kmod;
#[cfg(target_os = "linux")]
mod handlers_mmio;
#[cfg(target_os = "linux")]
mod handlers_sovereign;
#[cfg(target_os = "linux")]
mod helpers;

#[cfg(all(test, target_os = "linux"))]
mod tests;

pub use fd::send_with_fds;
pub use jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};

#[cfg(target_os = "linux")]
pub use dispatch::{handle_client, handle_client_tcp};
