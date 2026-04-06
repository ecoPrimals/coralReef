// SPDX-License-Identifier: AGPL-3.0-or-later
//! coralctl handlers for mailbox and ring operations.
//!
//! These map CLI subcommands to `mailbox.*` and `ring.*` JSON-RPC methods
//! on the glowplug socket, enabling hotSpring firmware probing from the
//! command line.

use crate::rpc::{check_rpc_error, rpc_call};

pub(crate) fn rpc_mailbox_create(socket: &str, bdf: &str, engine: &str, capacity: usize) {
    let resp = rpc_call(
        socket,
        "mailbox.create",
        serde_json::json!({
            "bdf": bdf,
            "engine": engine,
            "capacity": capacity,
        }),
    );
    check_rpc_error(&resp);
    println!(
        "mailbox created: engine={engine} capacity={capacity} on {bdf}"
    );
}

/// Parameters for `mailbox.post` — avoids exceeding the clippy argument limit.
pub(crate) struct MailboxPostParams<'a> {
    pub socket: &'a str,
    pub bdf: &'a str,
    pub engine: &'a str,
    pub register: u64,
    pub command: u64,
    pub status_register: u64,
    pub expected_status: u64,
    pub status_mask: u64,
    pub timeout_ms: u64,
}

pub(crate) fn rpc_mailbox_post(p: &MailboxPostParams<'_>) {
    let resp = rpc_call(
        p.socket,
        "mailbox.post",
        serde_json::json!({
            "bdf": p.bdf,
            "engine": p.engine,
            "register": p.register,
            "command": p.command,
            "status_register": p.status_register,
            "expected_status": p.expected_status,
            "status_mask": p.status_mask,
            "timeout_ms": p.timeout_ms,
        }),
    );
    check_rpc_error(&resp);
    if let Some(result) = resp.get("result") {
        let seq = result["seq"].as_u64().unwrap_or(0);
        println!("posted: seq={seq} engine={}", p.engine);
    }
}

pub(crate) fn rpc_mailbox_poll(socket: &str, bdf: &str, engine: &str, seq: u64) {
    let resp = rpc_call(
        socket,
        "mailbox.poll",
        serde_json::json!({
            "bdf": bdf,
            "engine": engine,
            "seq": seq,
        }),
    );
    check_rpc_error(&resp);
    if let Some(result) = resp.get("result") {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
    }
}

pub(crate) fn rpc_mailbox_drain(socket: &str, bdf: &str, engine: &str) {
    let resp = rpc_call(
        socket,
        "mailbox.drain",
        serde_json::json!({
            "bdf": bdf,
            "engine": engine,
        }),
    );
    check_rpc_error(&resp);
    if let Some(result) = resp.get("result") {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
    }
}

pub(crate) fn rpc_mailbox_stats(socket: &str, bdf: &str) {
    let resp = rpc_call(
        socket,
        "mailbox.stats",
        serde_json::json!({ "bdf": bdf }),
    );
    check_rpc_error(&resp);
    if let Some(result) = resp.get("result") {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
    }
}

pub(crate) fn rpc_ring_create(socket: &str, bdf: &str, name: &str, capacity: usize) {
    let resp = rpc_call(
        socket,
        "ring.create",
        serde_json::json!({
            "bdf": bdf,
            "name": name,
            "capacity": capacity,
        }),
    );
    check_rpc_error(&resp);
    println!("ring created: name={name} capacity={capacity} on {bdf}");
}

pub(crate) fn rpc_ring_submit(socket: &str, bdf: &str, ring: &str, method: &str, data: &str) {
    let resp = rpc_call(
        socket,
        "ring.submit",
        serde_json::json!({
            "bdf": bdf,
            "ring": ring,
            "method": method,
            "data": data,
        }),
    );
    check_rpc_error(&resp);
    if let Some(result) = resp.get("result") {
        let id = result["id"].as_u64().unwrap_or(0);
        let fence = result["fence"].as_u64().unwrap_or(0);
        println!("submitted: id={id} fence={fence} ring={ring}");
    }
}

pub(crate) fn rpc_ring_consume(socket: &str, bdf: &str, ring: &str) {
    let resp = rpc_call(
        socket,
        "ring.consume",
        serde_json::json!({ "bdf": bdf, "ring": ring }),
    );
    check_rpc_error(&resp);
    if let Some(result) = resp.get("result") {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
    }
}

pub(crate) fn rpc_ring_fence(socket: &str, bdf: &str, ring: &str, fence: u64) {
    let resp = rpc_call(
        socket,
        "ring.fence",
        serde_json::json!({
            "bdf": bdf,
            "ring": ring,
            "fence": fence,
        }),
    );
    check_rpc_error(&resp);
    if let Some(result) = resp.get("result") {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
    }
}

pub(crate) fn rpc_ring_peek(socket: &str, bdf: &str, ring: &str) {
    let resp = rpc_call(
        socket,
        "ring.peek",
        serde_json::json!({ "bdf": bdf, "ring": ring }),
    );
    check_rpc_error(&resp);
    if let Some(result) = resp.get("result") {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
    }
}

pub(crate) fn rpc_ring_stats(socket: &str, bdf: &str) {
    let resp = rpc_call(
        socket,
        "ring.stats",
        serde_json::json!({ "bdf": bdf }),
    );
    check_rpc_error(&resp);
    if let Some(result) = resp.get("result") {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        );
    }
}
