// SPDX-License-Identifier: AGPL-3.0-only
//! Hot-swap integration tests — exercise glowPlug's personality swap,
//! device lend/reclaim, and health monitoring on live hardware.
//!
//! # Prerequisites
//!
//! - `coral-glowplug` daemon running with VFIO-bound GPU(s)
//! - Socket at `$XDG_RUNTIME_DIR/biomeos/coral-glowplug-<family>.sock` (or `CORALREEF_GLOWPLUG_SOCK`)
//! - User has socket and VFIO group permissions
//!
//! Run: `cargo test --test hw_hotswap -p coral-glowplug -- --ignored --test-threads=1`
//!
//! Tests MUST run serially (`--test-threads=1`) because they share
//! the single glowPlug daemon and its VFIO device state.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

fn socket_path() -> String {
    std::env::var("CORALREEF_GLOWPLUG_SOCK").unwrap_or_else(|_| {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        format!("{runtime_dir}/biomeos/coral-glowplug-default.sock")
    })
}

fn connect() -> BufReader<UnixStream> {
    let raw = UnixStream::connect(socket_path()).expect("connect to glowplug socket");
    raw.set_read_timeout(Some(TIMEOUT)).expect("set timeout");
    raw.set_write_timeout(Some(TIMEOUT)).expect("set timeout");
    BufReader::new(raw)
}

fn call(
    stream: &mut BufReader<UnixStream>,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1,
    });
    let mut line = serde_json::to_string(&req).expect("serialize");
    line.push('\n');
    stream.get_mut().write_all(line.as_bytes()).expect("write");

    let mut resp_line = String::new();
    stream.read_line(&mut resp_line).expect("read response");
    let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse response");

    if let Some(err) = resp.get("error") {
        panic!(
            "JSON-RPC error from {method}: {} — {}",
            err["code"], err["message"]
        );
    }

    resp["result"].clone()
}

fn first_vfio_bdf(stream: &mut BufReader<UnixStream>) -> String {
    let list = call(stream, "device.list", serde_json::json!({}));
    let devices = list.as_array().expect("device.list should return array");
    devices
        .iter()
        .find(|d| d["has_vfio_fd"].as_bool() == Some(true))
        .expect("no VFIO device found in glowplug")["bdf"]
        .as_str()
        .expect("bdf string")
        .to_owned()
}

// ── Health and Discovery ───────────────────────────────────

#[test]
#[ignore = "requires running coral-glowplug daemon"]
fn glowplug_health_check() {
    let mut s = connect();
    let result = call(&mut s, "health.check", serde_json::json!({}));
    assert_eq!(result["alive"], true);
    assert_eq!(result["name"], "coral-glowplug");
    let count = result["device_count"].as_u64().expect("device_count");
    assert!(
        count >= 1,
        "expected at least 1 managed device, got {count}"
    );
    eprintln!(
        "glowplug healthy: {count} devices, {} healthy",
        result["healthy_count"]
    );
}

#[test]
#[ignore = "requires running coral-glowplug daemon"]
fn glowplug_device_list() {
    let mut s = connect();
    let list = call(&mut s, "device.list", serde_json::json!({}));
    let devices = list.as_array().expect("array");
    for dev in devices {
        eprintln!(
            "  {} ({}) — {} vram={} domains={}a/{}f",
            dev["bdf"],
            dev["chip"],
            dev["personality"],
            dev["vram_alive"],
            dev["domains_alive"],
            dev["domains_faulted"]
        );
    }
    assert!(!devices.is_empty());
}

#[test]
#[ignore = "requires running coral-glowplug daemon"]
fn glowplug_device_health() {
    let mut s = connect();
    let bdf = first_vfio_bdf(&mut s);
    let health = call(&mut s, "device.health", serde_json::json!({ "bdf": &bdf }));
    let boot0 = health["boot0"].as_u64().unwrap_or(0);
    eprintln!(
        "health for {bdf}: boot0={boot0:#010x} pmc={:#010x} vram={} power={} domains={}a/{}f",
        health["pmc_enable"].as_u64().unwrap_or(0),
        health["vram_alive"],
        health["power"],
        health["domains_alive"],
        health["domains_faulted"]
    );
    // BOOT0 must be a valid chip ID (not faulted). VRAM may be cold on
    // Titan V (GV100) without HBM2 training via nouveau — that's expected.
    assert!(
        boot0 != 0 && boot0 != 0xDEAD_DEAD && boot0 != 0xFFFF_FFFF,
        "BAR0 should be accessible (BOOT0={boot0:#010x})"
    );
}

// ── Lend / Reclaim ─────────────────────────────────────────

#[test]
#[ignore = "requires running coral-glowplug daemon"]
fn glowplug_lend_and_reclaim() {
    let mut s = connect();
    let bdf = first_vfio_bdf(&mut s);

    let lend = call(&mut s, "device.lend", serde_json::json!({ "bdf": &bdf }));
    let group_id = lend["group_id"].as_u64().expect("group_id");
    eprintln!("lent {bdf} — VFIO group {group_id}");

    // Verify the group is now accessible
    let vfio_path = format!("/dev/vfio/{group_id}");
    assert!(
        std::path::Path::new(&vfio_path).exists(),
        "VFIO group device should exist: {vfio_path}"
    );

    // Reclaim
    let reclaim = call(&mut s, "device.reclaim", serde_json::json!({ "bdf": &bdf }));
    assert_eq!(
        reclaim["has_vfio_fd"], true,
        "should have VFIO fd after reclaim"
    );
    eprintln!("reclaimed {bdf} — vram_alive={}", reclaim["vram_alive"]);
}

#[test]
#[ignore = "requires running coral-glowplug daemon"]
fn glowplug_lend_open_device_reclaim() {
    let mut s = connect();
    let bdf = first_vfio_bdf(&mut s);

    let lend = call(&mut s, "device.lend", serde_json::json!({ "bdf": &bdf }));
    let _group_id = lend["group_id"].as_u64().expect("group_id");

    // Actually open the VFIO device as a test consumer
    {
        let dev = coral_driver::nv::RawVfioDevice::open(&bdf);
        match dev {
            Ok(raw) => {
                let boot0 = raw.bar0.read_u32(0x0).unwrap_or(0xDEAD_DEAD);
                eprintln!("opened {bdf} as test consumer — BOOT0={boot0:#010x}");
                // Device drops here, releasing the VFIO group fd
            }
            Err(e) => {
                eprintln!("could not open VFIO device {bdf}: {e}");
                // Still reclaim even if open fails
            }
        }
    }

    std::thread::sleep(Duration::from_millis(200));

    let reclaim = call(&mut s, "device.reclaim", serde_json::json!({ "bdf": &bdf }));
    assert_eq!(reclaim["has_vfio_fd"], true);
    eprintln!(
        "reclaimed {bdf} after test consumer — vram_alive={}",
        reclaim["vram_alive"]
    );
}

// ── Hot-Swap Stress ────────────────────────────────────────

#[test]
#[ignore = "requires running coral-glowplug daemon"]
fn glowplug_lend_reclaim_cycle_10x() {
    let mut s = connect();
    let bdf = first_vfio_bdf(&mut s);

    for i in 0..10 {
        let lend = call(&mut s, "device.lend", serde_json::json!({ "bdf": &bdf }));
        assert!(
            lend["group_id"].is_number(),
            "cycle {i}: lend should return group_id"
        );

        std::thread::sleep(Duration::from_millis(50));

        let reclaim = call(&mut s, "device.reclaim", serde_json::json!({ "bdf": &bdf }));
        assert_eq!(
            reclaim["has_vfio_fd"], true,
            "cycle {i}: should have fd after reclaim"
        );

        std::thread::sleep(Duration::from_millis(50));
    }
    eprintln!("10 lend/reclaim cycles completed for {bdf}");

    // Final health check — BAR0 must still be accessible
    let health = call(&mut s, "device.health", serde_json::json!({ "bdf": &bdf }));
    let boot0 = health["boot0"].as_u64().unwrap_or(0);
    assert!(
        boot0 != 0 && boot0 != 0xDEAD_DEAD && boot0 != 0xFFFF_FFFF,
        "BAR0 should survive stress (BOOT0={boot0:#010x})"
    );
}

#[test]
#[ignore = "requires running coral-glowplug daemon"]
fn glowplug_health_check_during_lend() {
    let mut s = connect();
    let bdf = first_vfio_bdf(&mut s);

    call(&mut s, "device.lend", serde_json::json!({ "bdf": &bdf }));

    // Health check while lent — should report not alive (no fd to probe)
    let health = call(&mut s, "device.health", serde_json::json!({ "bdf": &bdf }));
    eprintln!("health during lend: vram_alive={}", health["vram_alive"]);
    assert_eq!(
        health["vram_alive"], false,
        "VRAM should report dead when fd is lent out"
    );

    // Daemon health should still work
    let daemon = call(&mut s, "health.check", serde_json::json!({}));
    assert_eq!(daemon["alive"], true);

    call(&mut s, "device.reclaim", serde_json::json!({ "bdf": &bdf }));
}

#[test]
#[ignore = "requires running coral-glowplug daemon"]
fn glowplug_double_lend_fails() {
    let mut s = connect();
    let bdf = first_vfio_bdf(&mut s);

    call(&mut s, "device.lend", serde_json::json!({ "bdf": &bdf }));

    // Second lend should fail
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "device.lend",
        "params": { "bdf": &bdf },
        "id": 2,
    });
    let mut line = serde_json::to_string(&req).expect("serialize");
    line.push('\n');
    s.get_mut().write_all(line.as_bytes()).expect("write");

    let mut resp_line = String::new();
    s.read_line(&mut resp_line).expect("read");
    let resp: serde_json::Value = serde_json::from_str(&resp_line).expect("parse");
    assert!(
        resp["error"].is_object(),
        "double lend should return error: {resp}"
    );
    eprintln!(
        "double lend correctly rejected: {}",
        resp["error"]["message"]
    );

    call(&mut s, "device.reclaim", serde_json::json!({ "bdf": &bdf }));
}

#[test]
#[ignore = "requires running coral-glowplug daemon"]
fn glowplug_reclaim_without_lend_is_noop() {
    let mut s = connect();
    let bdf = first_vfio_bdf(&mut s);

    // Reclaim when already holding fd should be a no-op
    let reclaim = call(&mut s, "device.reclaim", serde_json::json!({ "bdf": &bdf }));
    assert_eq!(reclaim["has_vfio_fd"], true);
}
