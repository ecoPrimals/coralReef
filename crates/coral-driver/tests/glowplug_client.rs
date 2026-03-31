// SPDX-License-Identifier: AGPL-3.0-only
//! Lightweight JSON-RPC 2.0 client for `coral-glowplug` socket.
//!
//! Used by hardware tests to borrow VFIO devices from the running
//! glowPlug daemon via `device.lend` / `device.reclaim`.
//!
//! Included via `#[path]` by multiple test crates — not all items
//! are used by every consumer.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

fn default_glowplug_socket_path() -> String {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    format!("{runtime_dir}/biomeos/coral-glowplug-default.sock")
}

pub struct GlowPlugClient {
    stream: BufReader<UnixStream>,
    next_id: u64,
}

impl GlowPlugClient {
    pub fn connect() -> Result<Self, String> {
        let path = std::env::var("CORALREEF_GLOWPLUG_SOCK")
            .unwrap_or_else(|_| default_glowplug_socket_path());
        let raw = UnixStream::connect(&path)
            .map_err(|e| format!("connect to glowplug at {path}: {e}"))?;
        raw.set_read_timeout(Some(TIMEOUT))
            .map_err(|e| format!("set read timeout: {e}"))?;
        raw.set_write_timeout(Some(TIMEOUT))
            .map_err(|e| format!("set write timeout: {e}"))?;
        Ok(Self {
            stream: BufReader::new(raw),
            next_id: 1,
        })
    }

    fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": id,
        });
        let mut line = serde_json::to_string(&req).map_err(|e| format!("serialize: {e}"))?;
        line.push('\n');
        self.stream
            .get_mut()
            .write_all(line.as_bytes())
            .map_err(|e| format!("write: {e}"))?;

        let mut resp_line = String::new();
        self.stream
            .read_line(&mut resp_line)
            .map_err(|e| format!("read: {e}"))?;

        let resp: serde_json::Value =
            serde_json::from_str(&resp_line).map_err(|e| format!("parse response: {e}"))?;

        if let Some(err) = resp.get("error") {
            return Err(format!(
                "JSON-RPC error {}: {}",
                err["code"], err["message"]
            ));
        }

        resp.get("result")
            .cloned()
            .ok_or_else(|| "response has no result".into())
    }

    /// Ask glowPlug to lend the VFIO fd for the given BDF.
    /// Returns the IOMMU group id.
    pub fn lend(&mut self, bdf: &str) -> Result<u32, String> {
        let result = self.call("device.lend", serde_json::json!({ "bdf": bdf }))?;
        result["group_id"]
            .as_u64()
            .map(|g| g as u32)
            .ok_or_else(|| "lend response missing group_id".into())
    }

    /// Ask glowPlug to reclaim the VFIO fd for the given BDF.
    pub fn reclaim(&mut self, bdf: &str) -> Result<(), String> {
        let result = self.call("device.reclaim", serde_json::json!({ "bdf": bdf }))?;
        if result["has_vfio_fd"].as_bool() == Some(true) {
            Ok(())
        } else {
            Err(format!("reclaim succeeded but has_vfio_fd=false: {result}"))
        }
    }

    /// Swap device to a different driver target (e.g. "nouveau", "vfio-pci").
    pub fn swap(&mut self, bdf: &str, target: &str) -> Result<serde_json::Value, String> {
        self.call(
            "device.swap",
            serde_json::json!({ "bdf": bdf, "target": target }),
        )
    }

    /// Check daemon health.
    pub fn health_check(&mut self) -> Result<serde_json::Value, String> {
        self.call("health.check", serde_json::json!({}))
    }

    /// Trigger a PCI device reset via GlowPlug → Ember.
    /// Methods: "flr", "sbr", "bridge-sbr", "remove-rescan", "auto"
    pub fn reset(&mut self, bdf: &str, method: &str) -> Result<serde_json::Value, String> {
        self.call(
            "device.reset",
            serde_json::json!({ "bdf": bdf, "method": method }),
        )
    }

    /// Orchestrate a warm handoff via GlowPlug → Ember.
    ///
    /// Swaps device to `driver` (typically "nouveau"), waits for FECS to boot,
    /// swaps back to `vfio-pci`, and returns the handoff summary. This is the
    /// programmatic equivalent of `coralctl warm-fecs`.
    pub fn warm_handoff(
        &mut self,
        bdf: &str,
        driver: &str,
        settle_ms: u64,
        poll_fecs: bool,
        poll_timeout_ms: u64,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "device.warm_handoff",
            serde_json::json!({
                "bdf": bdf,
                "driver": driver,
                "settle_ms": settle_ms,
                "poll_fecs": poll_fecs,
                "poll_timeout_ms": poll_timeout_ms,
            }),
        )
    }

    /// Write a single BAR0 register via glowplug.
    pub fn write_register(
        &mut self,
        bdf: &str,
        offset: u64,
        value: u32,
        allow_dangerous: bool,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "device.write_register",
            serde_json::json!({
                "bdf": bdf,
                "offset": offset,
                "value": value,
                "allow_dangerous": allow_dangerous,
            }),
        )
    }

    // ── Device discovery / info ──

    /// List all managed devices (returns array of DeviceInfo).
    pub fn list(&mut self) -> Result<serde_json::Value, String> {
        self.call("device.list", serde_json::json!({}))
    }

    /// Get info for a single device by BDF.
    pub fn get(&mut self, bdf: &str) -> Result<serde_json::Value, String> {
        self.call("device.get", serde_json::json!({"bdf": bdf}))
    }

    /// Full BAR0 health probe for a device (boot0, PMC, VRAM, FECS, domains).
    pub fn health(&mut self, bdf: &str) -> Result<serde_json::Value, String> {
        self.call("device.health", serde_json::json!({"bdf": bdf}))
    }

    // ── Register / BAR0 access ──

    /// Read arbitrary BAR0 registers by offset list.
    pub fn register_dump(
        &mut self,
        bdf: &str,
        offsets: &[u64],
    ) -> Result<serde_json::Value, String> {
        self.call(
            "device.register_dump",
            serde_json::json!({"bdf": bdf, "offsets": offsets}),
        )
    }

    /// Bulk BAR0 read: `count` consecutive u32s starting at `offset`.
    pub fn read_bar0_range(
        &mut self,
        bdf: &str,
        offset: u64,
        count: u64,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "device.read_bar0_range",
            serde_json::json!({"bdf": bdf, "offset": offset, "count": count}),
        )
    }

    // ── PRAMIN / VRAM access ──

    /// Read `count` u32s from VRAM via the PRAMIN window.
    pub fn pramin_read(
        &mut self,
        bdf: &str,
        vram_offset: u64,
        count: u64,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "device.pramin_read",
            serde_json::json!({"bdf": bdf, "vram_offset": vram_offset, "count": count}),
        )
    }

    /// Write u32 values to VRAM via the PRAMIN window.
    pub fn pramin_write(
        &mut self,
        bdf: &str,
        vram_offset: u64,
        values: &[u32],
    ) -> Result<serde_json::Value, String> {
        self.call(
            "device.pramin_write",
            serde_json::json!({"bdf": bdf, "vram_offset": vram_offset, "values": values}),
        )
    }

    // ── Daemon / health ──

    /// Daemon uptime and device counts.
    pub fn daemon_status(&mut self) -> Result<serde_json::Value, String> {
        self.call("daemon.status", serde_json::json!({}))
    }

    /// Readiness probe: all devices healthy?
    pub fn health_readiness(&mut self) -> Result<serde_json::Value, String> {
        self.call("health.readiness", serde_json::json!({}))
    }

    // ── Recovery / advanced ──

    /// Attempt VRAM/domain health recovery for a device.
    pub fn resurrect(&mut self, bdf: &str) -> Result<serde_json::Value, String> {
        self.call("device.resurrect", serde_json::json!({"bdf": bdf}))
    }

    /// Capture MMU page tables for oracle analysis.
    pub fn oracle_capture(
        &mut self,
        bdf: &str,
        max_channels: u64,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "device.oracle_capture",
            serde_json::json!({"bdf": bdf, "max_channels": max_channels}),
        )
    }

    /// Remote compute dispatch through glowplug.
    pub fn dispatch(
        &mut self,
        bdf: &str,
        shader_b64: &str,
        dims: &[u64],
        inputs_b64: &[String],
        output_sizes: &[u64],
    ) -> Result<serde_json::Value, String> {
        self.call(
            "device.dispatch",
            serde_json::json!({
                "bdf": bdf,
                "shader": shader_b64,
                "dims": dims,
                "inputs": inputs_b64,
                "output_sizes": output_sizes,
            }),
        )
    }

    // ── Mailbox (falcon firmware interaction) ──

    pub fn mailbox_create(
        &mut self,
        bdf: &str,
        engine: &str,
        capacity: u64,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "mailbox.create",
            serde_json::json!({"bdf": bdf, "engine": engine, "capacity": capacity}),
        )
    }

    pub fn mailbox_post(
        &mut self,
        bdf: &str,
        engine: &str,
        register: u32,
        command: u32,
        status_register: u32,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "mailbox.post",
            serde_json::json!({
                "bdf": bdf,
                "engine": engine,
                "register": register,
                "command": command,
                "status_register": status_register,
            }),
        )
    }

    pub fn mailbox_poll(
        &mut self,
        bdf: &str,
        engine: &str,
        seq: u64,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "mailbox.poll",
            serde_json::json!({"bdf": bdf, "engine": engine, "seq": seq}),
        )
    }

    pub fn mailbox_complete(
        &mut self,
        bdf: &str,
        engine: &str,
        seq: u64,
        status_val: u32,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "mailbox.complete",
            serde_json::json!({"bdf": bdf, "engine": engine, "seq": seq, "status": status_val}),
        )
    }

    pub fn mailbox_drain(
        &mut self,
        bdf: &str,
        engine: &str,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "mailbox.drain",
            serde_json::json!({"bdf": bdf, "engine": engine}),
        )
    }

    // ── Ring (work submission) ──

    pub fn ring_create(
        &mut self,
        bdf: &str,
        name: &str,
        capacity: u64,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "ring.create",
            serde_json::json!({"bdf": bdf, "name": name, "capacity": capacity}),
        )
    }

    pub fn ring_submit(
        &mut self,
        bdf: &str,
        ring: &str,
        method: &str,
        data: &str,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "ring.submit",
            serde_json::json!({"bdf": bdf, "ring": ring, "method": method, "data": data}),
        )
    }

    pub fn ring_consume(
        &mut self,
        bdf: &str,
        ring: &str,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "ring.consume",
            serde_json::json!({"bdf": bdf, "ring": ring}),
        )
    }

    pub fn ring_fence(
        &mut self,
        bdf: &str,
        ring: &str,
        fence: u64,
    ) -> Result<serde_json::Value, String> {
        self.call(
            "ring.fence",
            serde_json::json!({"bdf": bdf, "ring": ring, "fence": fence}),
        )
    }
}

/// RAII guard that lends a VFIO device from glowPlug and reclaims on drop.
pub struct VfioLease {
    client: GlowPlugClient,
    bdf: String,
    pub group_id: u32,
}

impl VfioLease {
    pub fn acquire(bdf: &str) -> Result<Self, String> {
        let mut client = GlowPlugClient::connect()?;
        let group_id = client.lend(bdf)?;
        eprintln!("glowplug: lent {bdf} (VFIO group {group_id})");
        Ok(Self {
            client,
            bdf: bdf.to_owned(),
            group_id,
        })
    }
}

impl Drop for VfioLease {
    fn drop(&mut self) {
        eprintln!("glowplug: reclaiming {}...", self.bdf);
        match self.client.reclaim(&self.bdf) {
            Ok(()) => eprintln!("glowplug: {} reclaimed", self.bdf),
            Err(e) => eprintln!("glowplug: RECLAIM FAILED for {}: {e}", self.bdf),
        }
    }
}
