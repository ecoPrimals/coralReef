// SPDX-License-Identifier: AGPL-3.0-or-later
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
use std::path::PathBuf;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

fn default_glowplug_socket_path() -> String {
    let base =
        std::env::var("XDG_RUNTIME_DIR").map_or_else(|_| std::env::temp_dir(), PathBuf::from);
    let ns = std::env::var("BIOMEOS_ECOSYSTEM_NAMESPACE").unwrap_or_else(|_| "biomeos".into());
    let family = std::env::var("BIOMEOS_FAMILY_ID").unwrap_or_else(|_| "default".into());
    base.join(ns)
        .join(format!("coral-glowplug-{family}.sock"))
        .display()
        .to_string()
}

pub struct GlowPlugClient {
    stream: BufReader<UnixStream>,
    next_id: u64,
}

impl GlowPlugClient {
    pub fn connect() -> Result<Self, String> {
        let path = std::env::var("CORALREEF_GLOWPLUG_SOCK")
            .or_else(|_| std::env::var("CORALREEF_GLOWPLUG_SOCKET"))
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
