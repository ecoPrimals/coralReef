// SPDX-License-Identifier: AGPL-3.0-only
//! Lightweight ember client for hardware tests.
//!
//! Requests VFIO fds from coral-ember via SCM_RIGHTS so tests can construct
//! `NvVfioComputeDevice` without competing with ember for `/dev/vfio/*`.
//!
//! Backend-aware: handles both legacy (3 fds) and iommufd (2 fds + ioas_id)
//! responses from ember.
#![allow(dead_code)]

use std::io::Write;
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;

use coral_driver::vfio::ReceivedVfioFds;
use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};

fn ember_socket_path() -> String {
    if let Ok(p) = std::env::var("CORALREEF_EMBER_SOCKET") {
        if !p.is_empty() {
            return p;
        }
    }

    // Try production socket first (systemd service path)
    let production = "/run/coralreef/ember.sock";
    if std::path::Path::new(production).exists() {
        return production.to_string();
    }

    // Fall back to XDG runtime convention
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    let family = std::env::var("BIOMEOS_FAMILY_ID")
        .or_else(|_| std::env::var("CORALREEF_FAMILY_ID"))
        .unwrap_or_else(|_| "default".to_string());
    format!("{runtime_dir}/biomeos/coral-ember-{family}.sock")
}

/// Request a PCI device reset from Ember (which runs as root).
///
/// `method` is one of: `"auto"`, `"sbr"`, `"bridge-sbr"`, `"remove-rescan"`.
/// Bridge-SBR resets all devices behind the parent PCI bridge — the only
/// reset mechanism available on GV100 Titan V (no FLR capability).
///
/// After a bridge-SBR, existing BAR mappings become invalid. Callers must
/// re-acquire VFIO fds and re-map BARs.
pub fn device_reset(bdf: &str, method: &str) -> Result<(), String> {
    let socket_path = ember_socket_path();
    let stream = UnixStream::connect(&socket_path).map_err(|e| format!("connect to ember: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "ember.device_reset",
        "params": {"bdf": bdf, "method": method},
        "id": 2
    });
    let req_bytes = format!("{req}\n");
    (&stream)
        .write_all(req_bytes.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = [0u8; 4096];
    let n = std::io::Read::read(&mut &stream, &mut buf).map_err(|e| format!("read: {e}"))?;

    let resp: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("ember.device_reset: {msg}"));
    }

    Ok(())
}

/// Ask ember to safely prepare a device for DMA experiments.
///
/// Ember maps BAR0 server-side, quiesces stale DMA engines (PFIFO reset,
/// scheduler stop, blind PRI ring ACK), masks AER, and enables bus mastering.
/// Call [`cleanup_dma`] when the experiment is done.
pub fn prepare_dma(bdf: &str) -> Result<serde_json::Value, String> {
    let socket_path = ember_socket_path();
    let stream =
        UnixStream::connect(&socket_path).map_err(|e| format!("connect to ember: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "ember.prepare_dma",
        "params": {"bdf": bdf},
        "id": 3
    });
    let req_bytes = format!("{req}\n");
    (&stream)
        .write_all(req_bytes.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = [0u8; 4096];
    let n = std::io::Read::read(&mut &stream, &mut buf).map_err(|e| format!("read: {e}"))?;

    let resp: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("ember.prepare_dma: {msg}"));
    }

    Ok(resp.get("result").cloned().unwrap_or_default())
}

/// Ask ember to clean up after a DMA experiment — disables bus master, restores AER.
pub fn cleanup_dma(bdf: &str) -> Result<(), String> {
    let socket_path = ember_socket_path();
    let stream =
        UnixStream::connect(&socket_path).map_err(|e| format!("connect to ember: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "ember.cleanup_dma",
        "params": {"bdf": bdf},
        "id": 4
    });
    let req_bytes = format!("{req}\n");
    (&stream)
        .write_all(req_bytes.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = [0u8; 4096];
    let n = std::io::Read::read(&mut &stream, &mut buf).map_err(|e| format!("read: {e}"))?;

    let resp: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("ember.cleanup_dma: {msg}"));
    }

    Ok(())
}

// ── MMIO Gateway client functions ────────────────────────────────────

/// Read a single BAR0 register via ember's MMIO gateway (no fd sharing).
pub fn mmio_read(bdf: &str, offset: usize) -> Result<u32, String> {
    let result = ember_rpc(
        "ember.mmio.read",
        serde_json::json!({"bdf": bdf, "offset": offset}),
        10,
    )?;
    result
        .get("value")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| "missing 'value' in response".to_string())
}

/// Write a single BAR0 register via ember's MMIO gateway.
pub fn mmio_write(bdf: &str, offset: usize, value: u32) -> Result<(), String> {
    ember_rpc(
        "ember.mmio.write",
        serde_json::json!({"bdf": bdf, "offset": offset, "value": value}),
        11,
    )?;
    Ok(())
}

/// Batch of register reads/writes in one IPC round-trip.
///
/// `ops` is a vec of `("r"|"w", offset, value)`. For reads, value is ignored.
/// Returns a vec of results: for reads the u32 value, for writes 0.
pub fn mmio_batch(bdf: &str, ops: &[(&str, usize, u32)]) -> Result<Vec<serde_json::Value>, String> {
    let ops_json: Vec<serde_json::Value> = ops
        .iter()
        .map(|(typ, offset, value)| {
            serde_json::json!({"type": typ, "offset": offset, "value": value})
        })
        .collect();

    let result = ember_rpc(
        "ember.mmio.batch",
        serde_json::json!({"bdf": bdf, "ops": ops_json}),
        12,
    )?;
    result
        .get("results")
        .and_then(|v| v.as_array())
        .cloned()
        .ok_or_else(|| "missing 'results' in response".to_string())
}

/// Write bulk data to VRAM via PRAMIN window (server-side, no direct BAR0).
pub fn pramin_write(bdf: &str, vram_addr: u32, data: &[u8]) -> Result<usize, String> {
    let encoded = b64_encode(data);
    let result = ember_rpc_large(
        "ember.pramin.write",
        serde_json::json!({"bdf": bdf, "vram_addr": vram_addr, "data_b64": encoded}),
        13,
    )?;
    result
        .get("bytes_written")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .ok_or_else(|| "missing 'bytes_written' in response".to_string())
}

/// Read bulk data from VRAM via PRAMIN window (server-side).
pub fn pramin_read(bdf: &str, vram_addr: u32, length: usize) -> Result<Vec<u8>, String> {
    let result = ember_rpc_large(
        "ember.pramin.read",
        serde_json::json!({"bdf": bdf, "vram_addr": vram_addr, "length": length}),
        14,
    )?;
    let data_b64 = result
        .get("data_b64")
        .and_then(|v| v.as_str())
        .ok_or("missing 'data_b64' in response")?;
    b64_decode(data_b64).map_err(|e| format!("base64 decode: {e}"))
}

/// Run sec2_prepare_physical_first() server-side in ember.
pub fn sec2_prepare_physical(bdf: &str) -> Result<(bool, Vec<String>), String> {
    let result = ember_rpc(
        "ember.sec2.prepare_physical",
        serde_json::json!({"bdf": bdf}),
        20,
    )?;
    let ok = result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let notes: Vec<String> = result
        .get("notes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Ok((ok, notes))
}

/// Upload code to falcon IMEM via ember.
///
/// When `secure` is true, IMEMC bit 28 is set, marking the IMEM region as
/// accessible only in HS mode. Required for ACR bootloader on fuse-secured
/// falcons (e.g. GV100 SEC2 with SCTL=0x3000).
pub fn falcon_upload_imem(
    bdf: &str,
    base: usize,
    imem_addr: u32,
    code: &[u8],
    start_tag: u32,
    secure: bool,
) -> Result<(), String> {
    let encoded = b64_encode(code);
    ember_rpc_large(
        "ember.falcon.upload_imem",
        serde_json::json!({
            "bdf": bdf, "base": base, "imem_addr": imem_addr,
            "code_b64": encoded, "start_tag": start_tag, "secure": secure,
        }),
        21,
    )?;
    Ok(())
}

/// Upload data to falcon DMEM via ember.
pub fn falcon_upload_dmem(
    bdf: &str,
    base: usize,
    dmem_addr: u32,
    data: &[u8],
) -> Result<(), String> {
    let encoded = b64_encode(data);
    ember_rpc_large(
        "ember.falcon.upload_dmem",
        serde_json::json!({
            "bdf": bdf, "base": base, "dmem_addr": dmem_addr, "data_b64": encoded,
        }),
        22,
    )?;
    Ok(())
}

/// Start falcon CPU and return post-start diagnostics.
pub fn falcon_start_cpu(bdf: &str, base: usize) -> Result<serde_json::Value, String> {
    ember_rpc(
        "ember.falcon.start_cpu",
        serde_json::json!({"bdf": bdf, "base": base}),
        23,
    )
}

/// Server-side falcon polling with stop conditions.
pub fn falcon_poll(
    bdf: &str,
    base: usize,
    timeout_ms: u64,
    mailbox_sentinel: u32,
) -> Result<serde_json::Value, String> {
    let result = ember_rpc(
        "ember.falcon.poll",
        serde_json::json!({
            "bdf": bdf, "base": base,
            "timeout_ms": timeout_ms, "mailbox_sentinel": mailbox_sentinel,
        }),
        24,
    )?;
    Ok(result)
}

// ── Internal helpers ────────────────────────────────────────────────

/// Send a JSON-RPC request to ember and parse the response.
/// Uses a 4KB response buffer (sufficient for most RPCs).
fn ember_rpc(
    method: &str,
    params: serde_json::Value,
    req_id: u64,
) -> Result<serde_json::Value, String> {
    let socket_path = ember_socket_path();
    let stream =
        UnixStream::connect(&socket_path).map_err(|e| format!("connect to ember: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(60)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": req_id,
    });
    let req_bytes = format!("{req}\n");
    (&stream)
        .write_all(req_bytes.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = [0u8; 4096];
    let n = std::io::Read::read(&mut &stream, &mut buf).map_err(|e| format!("read: {e}"))?;
    let resp: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("{method}: {msg}"));
    }
    Ok(resp.get("result").cloned().unwrap_or_default())
}

/// Like `ember_rpc` but handles large request payloads (base64 data) and
/// large responses (PRAMIN read). Uses streaming read for responses.
fn ember_rpc_large(
    method: &str,
    params: serde_json::Value,
    req_id: u64,
) -> Result<serde_json::Value, String> {
    let socket_path = ember_socket_path();
    let stream =
        UnixStream::connect(&socket_path).map_err(|e| format!("connect to ember: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(120)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": req_id,
    });
    let req_bytes = format!("{req}\n");
    (&stream)
        .write_all(req_bytes.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    // Read response in chunks until newline or EOF
    let mut buf = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    loop {
        let n = std::io::Read::read(&mut &stream, &mut chunk).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.contains(&b'\n') || buf.len() > 4 * 1024 * 1024 {
            break;
        }
    }

    let resp: serde_json::Value =
        serde_json::from_slice(&buf).map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(format!("{method}: {msg}"));
    }
    Ok(resp.get("result").cloned().unwrap_or_default())
}

// Minimal base64 encode/decode matching the server-side implementation.

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
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

fn b64_decode(input: &str) -> Result<Vec<u8>, String> {
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

// ── Legacy fd-sharing (kept for glowplug compatibility) ─────────────

pub fn request_fds(bdf: &str) -> Result<ReceivedVfioFds, String> {
    let socket_path = ember_socket_path();
    let stream = UnixStream::connect(&socket_path).map_err(|e| format!("connect to ember: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "ember.vfio_fds",
        "params": {"bdf": bdf},
        "id": 1
    });
    let req_bytes = format!("{req}\n");
    (&stream)
        .write_all(req_bytes.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let mut buf = [0u8; 4096];
    let mut cmsg_space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(3))];
    let mut cmsg_buffer = RecvAncillaryBuffer::new(&mut cmsg_space);

    let result = recvmsg(
        &stream,
        &mut [rustix::io::IoSliceMut::new(&mut buf)],
        &mut cmsg_buffer,
        RecvFlags::empty(),
    )
    .map_err(|e| format!("recvmsg: {e}"))?;

    let n = result.bytes;

    let resp: serde_json::Value =
        serde_json::from_slice(&buf[..n]).map_err(|e| format!("parse: {e}"))?;

    if resp.get("error").is_some() {
        let err = resp["error"]["message"].as_str().unwrap_or("unknown");
        return Err(format!("ember: {err}"));
    }

    let mut fds: Vec<OwnedFd> = Vec::new();
    for msg in cmsg_buffer.drain() {
        if let RecvAncillaryMessage::ScmRights(rights) = msg {
            fds.extend(rights);
        }
    }

    let backend = resp
        .get("result")
        .and_then(|r| r.get("backend"))
        .and_then(|b| b.as_str())
        .unwrap_or("legacy");

    match backend {
        "iommufd" => {
            if fds.len() < 2 {
                return Err(format!("iommufd: need 2 fds, got {}", fds.len()));
            }
            let ioas_id = resp
                .get("result")
                .and_then(|r| r.get("ioas_id"))
                .and_then(|v| v.as_u64())
                .ok_or("iommufd response missing ioas_id")? as u32;
            let mut it = fds.into_iter();
            Ok(ReceivedVfioFds::Iommufd {
                iommufd: it.next().expect("checked len >= 2"),
                device: it.next().expect("checked len >= 2"),
                ioas_id,
            })
        }
        _ => {
            if fds.len() < 3 {
                return Err(format!("legacy: need 3 fds, got {}", fds.len()));
            }
            let mut it = fds.into_iter();
            Ok(ReceivedVfioFds::Legacy {
                container: it.next().expect("checked len >= 3"),
                group: it.next().expect("checked len >= 3"),
                device: it.next().expect("checked len >= 3"),
            })
        }
    }
}
