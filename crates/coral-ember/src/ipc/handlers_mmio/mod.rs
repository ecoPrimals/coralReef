// SPDX-License-Identifier: AGPL-3.0-only
//! MMIO Gateway handlers — all BAR0 register and PRAMIN operations run server-side.
//!
//! Experiments no longer receive VFIO fds. Instead, every GPU register read/write
//! routes through ember via these JSON-RPC handlers. Ember validates each operation
//! (poisonous register check, bounds, policy) before touching hardware.
//!
//! Split by layer:
//! - [`low_level`]: single register read/write/batch
//! - [`pramin`]: bulk VRAM operations via PRAMIN window
//! - [`falcon`]: high-level SEC2/falcon experiment RPCs

mod falcon;
mod low_level;
mod pramin;

pub(crate) use falcon::{
    falcon_poll, falcon_start_cpu, falcon_upload_dmem, falcon_upload_imem, sec2_prepare_physical,
};
pub(crate) use low_level::{mmio_batch, mmio_read, mmio_write};
pub(crate) use pramin::{pramin_read, pramin_write};

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;

use super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};

/// Ensure BAR0 is mapped on `dev`, returning an error suitable for JSON-RPC if not.
pub(super) fn map_bar0_if_needed(
    dev: &mut HeldDevice,
) -> Result<(), coral_driver::error::DriverError> {
    if dev.bar0.is_none() {
        dev.bar0 = Some(dev.device.map_bar(0)?);
    }
    Ok(())
}

/// Pre-flight health check: reads BOOT0 (offset 0x0) and verifies the GPU
/// is responsive. Returns `Ok(boot0_value)` on success, `Err(msg)` if the
/// device is faulted or the circuit breaker is open.
///
/// On a healthy read, resets the fault counter to 0. On a faulted read
/// (0xFFFF_FFFF or 0xDEAD_DEAD patterns), increments the counter and
/// trips the circuit breaker after [`crate::hold::MMIO_CIRCUIT_BREAKER_THRESHOLD`]
/// consecutive failures.
pub(super) fn preflight_check(dev: &mut HeldDevice) -> Result<u32, String> {
    if dev.mmio_fault_count >= crate::hold::MMIO_CIRCUIT_BREAKER_THRESHOLD {
        return Err(format!(
            "{}: MMIO circuit breaker OPEN — {} consecutive faulted reads. \
             Device is non-responsive. Manual reset or service restart required.",
            dev.bdf, dev.mmio_fault_count
        ));
    }

    let bar0 = match dev.bar0.as_ref() {
        Some(b) => b,
        None => return Err(format!("{}: BAR0 not mapped", dev.bdf)),
    };

    let boot0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);

    if boot0 == 0xFFFF_FFFF || boot0 == 0xDEAD_DEAD || boot0 == 0 {
        dev.mmio_fault_count += 1;
        tracing::warn!(
            bdf = %dev.bdf,
            boot0 = format_args!("{boot0:#010x}"),
            fault_count = dev.mmio_fault_count,
            threshold = crate::hold::MMIO_CIRCUIT_BREAKER_THRESHOLD,
            "MMIO pre-flight: BOOT0 faulted"
        );
        if dev.mmio_fault_count >= crate::hold::MMIO_CIRCUIT_BREAKER_THRESHOLD {
            tracing::error!(
                bdf = %dev.bdf,
                "MMIO CIRCUIT BREAKER TRIPPED — refusing further MMIO operations"
            );
        }
        return Err(format!(
            "{}: BOOT0 read returned {boot0:#010x} — GPU non-responsive \
             (fault {}/{})",
            dev.bdf,
            dev.mmio_fault_count,
            crate::hold::MMIO_CIRCUIT_BREAKER_THRESHOLD,
        ));
    }

    if dev.mmio_fault_count > 0 {
        tracing::info!(
            bdf = %dev.bdf,
            boot0 = format_args!("{boot0:#010x}"),
            prev_faults = dev.mmio_fault_count,
            "MMIO pre-flight: BOOT0 healthy — resetting fault counter"
        );
    }
    dev.mmio_fault_count = 0;
    Ok(boot0)
}

/// `ember.mmio.circuit_breaker` — query or reset the MMIO circuit breaker.
///
/// Params: `{bdf, action: "status"|"reset"}`
/// Result: `{bdf, fault_count, threshold, tripped, action}`
pub(crate) fn circuit_breaker(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;
    let action = params
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("status");

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = require_held_mut(&mut map, bdf, stream, &id)?;

    let threshold = crate::hold::MMIO_CIRCUIT_BREAKER_THRESHOLD;
    if action == "reset" {
        tracing::info!(bdf, prev_faults = dev.mmio_fault_count, "MMIO circuit breaker reset via RPC");
        dev.mmio_fault_count = 0;
    }

    let fault_count = dev.mmio_fault_count;
    let tripped = fault_count >= threshold;
    drop(map);

    write_jsonrpc_ok(
        stream,
        id,
        serde_json::json!({
            "bdf": bdf, "fault_count": fault_count,
            "threshold": threshold, "tripped": tripped, "action": action,
        }),
    )
    .map_err(EmberIpcError::from)
}

// ── Shared helpers ──────────────────────────────────────────────────

pub(super) fn require_bdf(params: &serde_json::Value) -> Result<&str, EmberIpcError> {
    params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf' parameter"))
}

pub(super) fn require_offset(params: &serde_json::Value) -> Result<usize, EmberIpcError> {
    params
        .get("offset")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .ok_or(EmberIpcError::InvalidRequest("missing 'offset' parameter"))
}

pub(super) fn require_u64(
    params: &serde_json::Value,
    key: &'static str,
) -> Result<u64, EmberIpcError> {
    params
        .get(key)
        .and_then(|v| v.as_u64())
        .ok_or(EmberIpcError::InvalidRequest(key))
}

pub(super) fn decode_b64_param(
    params: &serde_json::Value,
    key: &str,
) -> Result<Vec<u8>, EmberIpcError> {
    let encoded = params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest(Box::leak(
            format!("missing '{key}'").into_boxed_str(),
        )))?;
    base64_decode(encoded).map_err(|e| {
        EmberIpcError::InvalidRequest(Box::leak(
            format!("base64 decode '{key}': {e}").into_boxed_str(),
        ))
    })
}

pub(super) fn require_held_mut<'a>(
    map: &'a mut HashMap<String, HeldDevice>,
    bdf: &str,
    stream: &mut impl Write,
    id: &serde_json::Value,
) -> Result<&'a mut HeldDevice, EmberIpcError> {
    if map.contains_key(bdf) {
        Ok(map.get_mut(bdf).unwrap())
    } else {
        write_jsonrpc_error(stream, id.clone(), -32000, &format!("{bdf}: not held by ember"))
            .map_err(EmberIpcError::from)?;
        Err(EmberIpcError::InvalidRequest("device not held"))
    }
}

pub(super) fn le_bytes_to_u32(bytes: &[u8]) -> u32 {
    match bytes.len() {
        4 => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        3 => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]),
        2 => u32::from_le_bytes([bytes[0], bytes[1], 0, 0]),
        1 => u32::from_le_bytes([bytes[0], 0, 0, 0]),
        _ => 0,
    }
}

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(super) fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(B64_CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub(super) fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let input = input.trim_end_matches('=');
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for ch in input.bytes() {
        let val = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'\n' | b'\r' | b' ' => continue,
            _ => return Err(format!("invalid base64 character: {ch:#04x}")),
        };
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        let data = b"Hello, World!";
        let encoded = base64_encode(data);
        let decoded = base64_decode(&encoded).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn base64_empty() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_decode("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn base64_binary() {
        let data: Vec<u8> = (0..=255).collect();
        let encoded = base64_encode(&data);
        let decoded = base64_decode(&encoded).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn le_bytes_various_lengths() {
        assert_eq!(le_bytes_to_u32(&[0x01, 0x02, 0x03, 0x04]), 0x04030201);
        assert_eq!(le_bytes_to_u32(&[0x01, 0x02, 0x03]), 0x00030201);
        assert_eq!(le_bytes_to_u32(&[0x01, 0x02]), 0x00000201);
        assert_eq!(le_bytes_to_u32(&[0x01]), 0x00000001);
        assert_eq!(le_bytes_to_u32(&[]), 0);
    }
}
