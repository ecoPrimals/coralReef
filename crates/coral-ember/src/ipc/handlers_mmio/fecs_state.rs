// SPDX-License-Identifier: AGPL-3.0-only
//! FECS falcon state query handler.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;

use super::super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};
use super::{map_bar0_if_needed, preflight_gate, require_bdf, update_fault_counter};

const FECS_CPUCTL: u32 = 0x409100;
const FECS_BOOTVEC: u32 = 0x409104;
const FECS_OS: u32 = 0x409108;
const FECS_PC: u32 = 0x409110;
const FECS_SCTL: u32 = 0x409240;
const FECS_MAILBOX0: u32 = 0x409040;
const FECS_MAILBOX1: u32 = 0x409044;
const FECS_ENGCTL: u32 = 0x4091E0;

/// `ember.fecs.state` — read FECS falcon status registers via MMIO.
///
/// Params: `{bdf}`
/// Result: `{bdf, cpuctl, bootvec, os, pc, sctl, mailbox0, mailbox1, engctl}`
pub(crate) fn fecs_state(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let Some(dev) = map.get_mut(bdf) else {
        write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf}: not held by ember"),
        )
        .map_err(EmberIpcError::from)?;
        return Ok(());
    };

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf}: BAR0 map failed: {e}"),
        )
        .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_gate(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: {e}"))
            .map_err(EmberIpcError::from);
    }

    let bar0 = dev.bar0.as_ref().unwrap();

    let boot0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
    let cpuctl = bar0.read_u32(FECS_CPUCTL as usize).unwrap_or(0xFFFF_FFFF);
    let bootvec = bar0.read_u32(FECS_BOOTVEC as usize).unwrap_or(0xFFFF_FFFF);
    let os = bar0.read_u32(FECS_OS as usize).unwrap_or(0xFFFF_FFFF);
    let pc = bar0.read_u32(FECS_PC as usize).unwrap_or(0xFFFF_FFFF);
    let sctl = bar0.read_u32(FECS_SCTL as usize).unwrap_or(0xFFFF_FFFF);
    let mailbox0 = bar0.read_u32(FECS_MAILBOX0 as usize).unwrap_or(0xFFFF_FFFF);
    let mailbox1 = bar0.read_u32(FECS_MAILBOX1 as usize).unwrap_or(0xFFFF_FFFF);
    let engctl = bar0.read_u32(FECS_ENGCTL as usize).unwrap_or(0xFFFF_FFFF);

    if let Err(e) = update_fault_counter(dev, boot0) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: {e}"))
            .map_err(EmberIpcError::from);
    }

    let result = serde_json::json!({
        "bdf": bdf,
        "boot0": format!("{boot0:#010x}"),
        "cpuctl": format!("{cpuctl:#010x}"),
        "bootvec": format!("{bootvec:#010x}"),
        "os": format!("{os:#010x}"),
        "pc": format!("{pc:#010x}"),
        "sctl": format!("{sctl:#010x}"),
        "mailbox0": format!("{mailbox0:#010x}"),
        "mailbox1": format!("{mailbox1:#010x}"),
        "engctl": format!("{engctl:#010x}"),
    });

    drop(map);
    write_jsonrpc_ok(stream, id, result).map_err(EmberIpcError::from)
}
