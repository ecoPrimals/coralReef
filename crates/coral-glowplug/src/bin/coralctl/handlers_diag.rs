// SPDX-License-Identifier: AGPL-3.0-or-later
//! RPC handlers: register probes, VRAM, MMIO, and snapshots.

use std::collections::HashMap;

use crate::rpc::{check_rpc_error, rpc_call};

use serde_json::json;

pub(crate) fn rpc_probe(socket: &str, bdf: &str) {
    let response = rpc_call(socket, "device.register_dump", json!({ "bdf": bdf }));
    check_rpc_error(&response);

    let result = match response.get("result") {
        Some(r) => r,
        None => {
            tracing::error!("no result in RPC response");
            std::process::exit(1);
        }
    };

    let regs = result
        .get("registers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let count = result
        .get("register_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    println!("=== Register Probe: {bdf} ({count} registers) ===");
    for reg in &regs {
        let offset = reg.get("offset").and_then(|v| v.as_str()).unwrap_or("?");
        let value = reg.get("value").and_then(|v| v.as_str()).unwrap_or("?");
        println!("  {offset} = {value}");
    }
}

pub(crate) fn rpc_vram_probe(socket: &str, bdf: &str) {
    println!("=== VRAM Probe: {bdf} ===");

    let regions: &[(u64, &str)] = &[
        (0x0000, "VRAM base"),
        (0x0100, "VRAM +0x100"),
        (0x1_0000, "VRAM +64K"),
    ];

    let mut alive = true;
    for &(offset, label) in regions {
        let response = rpc_call(
            socket,
            "device.pramin_read",
            json!({
                "bdf": bdf,
                "vram_offset": offset,
                "count": 8,
            }),
        );
        check_rpc_error(&response);

        let result = response.get("result").unwrap_or(&serde_json::Value::Null);
        let values: Vec<u32> = result
            .get("values")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u32))
                    .collect()
            })
            .unwrap_or_default();

        let bad_count = values
            .iter()
            .filter(|&&v| (v >> 16) == 0xBAD0 || v == 0xDEAD_DEAD || v == 0xFFFF_FFFF)
            .count();

        if bad_count > values.len() / 2 {
            alive = false;
            print!("  {label} ({offset:#06x}): DEAD");
        } else {
            print!("  {label} ({offset:#06x}): ok  ");
        }
        for (i, val) in values.iter().enumerate() {
            if i < 4 {
                print!(" {val:#010x}");
            }
        }
        println!();
    }

    // Write-readback test at VRAM +0x100
    let test_val: u64 = 0xDEAD_BEEF;
    let read_before = rpc_call(
        socket,
        "device.pramin_read",
        json!({ "bdf": bdf, "vram_offset": 0x100_u64, "count": 1 }),
    );
    check_rpc_error(&read_before);
    let before_val = read_before
        .get("result")
        .and_then(|r| r.get("values"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let write_resp = rpc_call(
        socket,
        "device.pramin_write",
        json!({ "bdf": bdf, "vram_offset": 0x100_u64, "values": [test_val] }),
    );
    check_rpc_error(&write_resp);

    let read_after = rpc_call(
        socket,
        "device.pramin_read",
        json!({ "bdf": bdf, "vram_offset": 0x100_u64, "count": 1 }),
    );
    check_rpc_error(&read_after);
    let after_val = read_after
        .get("result")
        .and_then(|r| r.get("values"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let readback_ok = after_val == test_val as u32;
    println!(
        "\n  Write-readback: before={before_val:#010x} wrote={test_val:#010x} read={after_val:#010x} {}",
        if readback_ok { "OK" } else { "FAILED" }
    );
    if !readback_ok {
        alive = false;
    }

    let status = if alive { "ALIVE" } else { "DEAD (0xbad0acXX)" };
    println!("\n=== HBM2: {status} ===");
}

pub(crate) fn rpc_mmio_read(socket: &str, bdf: &str, offset: usize) {
    let response = rpc_call(
        socket,
        "device.register_dump",
        json!({ "bdf": bdf, "offsets": [offset] }),
    );
    check_rpc_error(&response);

    let result = response.get("result").unwrap_or(&serde_json::Value::Null);
    let regs = result
        .get("registers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if let Some(reg) = regs.first() {
        let off_str = reg.get("offset").and_then(|v| v.as_str()).unwrap_or("?");
        let val_str = reg.get("value").and_then(|v| v.as_str()).unwrap_or("?");
        println!("{off_str} = {val_str}");
    } else {
        tracing::error!("no value returned for offset {offset:#010x}");
        std::process::exit(1);
    }
}

pub(crate) fn rpc_mmio_write(
    socket: &str,
    bdf: &str,
    offset: usize,
    value: u32,
    allow_dangerous: bool,
) {
    let response = rpc_call(
        socket,
        "device.write_register",
        json!({
            "bdf": bdf,
            "offset": offset,
            "value": value as u64,
            "allow_dangerous": allow_dangerous,
        }),
    );
    check_rpc_error(&response);
    println!("{:#010x} <- {:#010x}  ok", offset, value);
}

pub(crate) fn rpc_snapshot_save(socket: &str, bdf: &str, file: Option<String>) {
    let response = rpc_call(socket, "device.register_dump", json!({ "bdf": bdf }));
    check_rpc_error(&response);

    let result = match response.get("result") {
        Some(r) => r,
        None => {
            tracing::error!("no result in RPC response");
            std::process::exit(1);
        }
    };

    let filename = file.unwrap_or_else(|| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let safe_bdf = bdf.replace(':', "-");
        format!("{safe_bdf}_snapshot_{ts}.json")
    });

    let snapshot = json!({
        "bdf": bdf,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        "registers": result.get("registers"),
    });

    let json = serde_json::to_string_pretty(&snapshot).expect("serialization");
    match std::fs::write(&filename, &json) {
        Ok(()) => {
            let count = result
                .get("register_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            println!("saved {count} registers to {filename}");
        }
        Err(e) => {
            tracing::error!(path = %filename, error = %e, "failed to write snapshot");
            std::process::exit(1);
        }
    }
}

pub(crate) fn rpc_snapshot_diff(socket: &str, bdf: &str, file: &str) {
    let saved_json = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(path = %file, error = %e, "failed to read snapshot");
            std::process::exit(1);
        }
    };
    let saved: serde_json::Value = match serde_json::from_str(&saved_json) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(path = %file, error = %e, "invalid JSON in snapshot");
            std::process::exit(1);
        }
    };

    let saved_regs = saved
        .get("registers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let offsets: Vec<u64> = saved_regs
        .iter()
        .filter_map(|r| r.get("raw_offset").and_then(|v| v.as_u64()))
        .collect();

    let response = rpc_call(
        socket,
        "device.register_dump",
        json!({ "bdf": bdf, "offsets": offsets }),
    );
    check_rpc_error(&response);

    let current_regs = response
        .get("result")
        .and_then(|r| r.get("registers"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let current_map: HashMap<u64, u64> = current_regs
        .iter()
        .filter_map(|r| {
            let off = r.get("raw_offset").and_then(|v| v.as_u64())?;
            let val = r.get("raw_value").and_then(|v| v.as_u64())?;
            Some((off, val))
        })
        .collect();

    let mut changed = 0;
    let mut total = 0;
    println!("=== Snapshot Diff: {bdf} vs {file} ===");
    println!("{:<14} {:<14} {:<14} STATUS", "OFFSET", "SAVED", "CURRENT");
    let sep = "-".repeat(56);
    println!("{sep}");

    for reg in &saved_regs {
        let off = match reg.get("raw_offset").and_then(|v| v.as_u64()) {
            Some(o) => o,
            None => continue,
        };
        let saved_val = reg.get("raw_value").and_then(|v| v.as_u64()).unwrap_or(0);
        let current_val = current_map.get(&off).copied().unwrap_or(0xDEAD_DEAD);

        total += 1;
        let status = if saved_val == current_val {
            "="
        } else {
            changed += 1;
            "CHANGED"
        };
        if saved_val != current_val {
            println!(
                "{:#012x}   {:#012x}   {:#012x}   {status}",
                off, saved_val, current_val
            );
        }
    }
    println!("\n{changed}/{total} registers changed");
}
