// SPDX-License-Identifier: AGPL-3.0-only
//! Ember lifecycle manager — spawn, monitor, kill, and resurrect ember processes.
//!
//! Glowplug is the immortal orchestrator; ember is the sacrificial canary.
//! This module manages ember's entire lifecycle:
//!
//! 1. **Spawn**: start ember via `systemctl start coral-ember`
//! 2. **Monitor**: heartbeat via `ember.status` RPC detects stuck/dead ember
//! 3. **Kill**: graceful SIGTERM → SIGKILL escalation via `systemctl stop`
//! 4. **Resurrect**: stop current ember, restart — ember re-acquires VFIO devices from sysfs
//!
//! Ember is disposable. When it dies (voluntary exit on total fault, SIGTERM,
//! crash), glowplug simply starts a fresh instance. Ember re-acquires its
//! VFIO devices from sysfs on startup — no vault or fd transfer needed.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::fd_vault::FdVault;

/// Ember process states as seen by glowplug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmberState {
    /// Ember is not running (initial state or after confirmed exit).
    Down,
    /// Ember is starting (`systemctl start` issued, waiting for heartbeat).
    Starting,
    /// Ember is running and responding to heartbeats.
    Alive,
    /// Ember has missed heartbeats — assumed stuck.
    Unresponsive,
    /// Ember is being killed (SIGTERM sent, waiting for exit).
    Killing,
}

/// Configuration for the ember lifecycle manager.
#[derive(Debug, Clone)]
pub struct EmberLifecycleConfig {
    /// Interval between heartbeat checks.
    pub heartbeat_interval: Duration,
    /// Number of missed heartbeats before declaring ember unresponsive.
    pub missed_heartbeat_threshold: u32,
    /// Time to wait after SIGTERM before considering the stop failed.
    pub kill_grace_period: Duration,
    /// Time to wait for ember to start (first heartbeat) before giving up.
    pub start_timeout: Duration,
    /// Ember socket path for heartbeat checks.
    pub ember_socket: String,
    /// BDFs of managed compute devices — used for GPU warm cycle during resurrection.
    pub managed_bdfs: Vec<String>,
}

impl Default for EmberLifecycleConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(2),
            missed_heartbeat_threshold: 3,
            kill_grace_period: Duration::from_secs(5),
            start_timeout: Duration::from_secs(30),
            ember_socket: String::new(),
            managed_bdfs: Vec::new(),
        }
    }
}

/// Manages the ember process lifecycle from glowplug's perspective.
pub struct EmberLifecycle {
    state: EmberState,
    config: EmberLifecycleConfig,
    last_heartbeat: Option<Instant>,
    /// When state entered `Starting` — used to enforce `start_timeout`.
    start_entered_at: Option<Instant>,
    /// When state entered `Killing` — used to enforce `kill_grace_period`.
    kill_entered_at: Option<Instant>,
    missed_heartbeats: u32,
    spawn_count: u64,
    resurrect_count: u64,
    /// Backup VFIO fds — keeps GPU bindings alive across ember death.
    vault: Arc<FdVault>,
    /// Last successful fd checkpoint time.
    last_checkpoint: Option<Instant>,
}

impl EmberLifecycle {
    /// Create a new lifecycle manager.
    pub fn new(config: EmberLifecycleConfig) -> Self {
        Self {
            state: EmberState::Down,
            config,
            last_heartbeat: None,
            start_entered_at: None,
            kill_entered_at: None,
            missed_heartbeats: 0,
            spawn_count: 0,
            resurrect_count: 0,
            vault: Arc::new(FdVault::new()),
            last_checkpoint: None,
        }
    }

    /// Shared reference to the fd vault (for health monitor reads).
    pub fn vault(&self) -> &Arc<FdVault> {
        &self.vault
    }

    /// Current ember state.
    pub fn state(&self) -> EmberState {
        self.state
    }

    /// Number of times ember has been spawned.
    pub fn spawn_count(&self) -> u64 {
        self.spawn_count
    }

    /// Number of times ember has been resurrected (subset of spawn_count).
    pub fn resurrect_count(&self) -> u64 {
        self.resurrect_count
    }

    /// Record a successful heartbeat from ember.
    pub fn record_heartbeat(&mut self) {
        self.last_heartbeat = Some(Instant::now());
        self.missed_heartbeats = 0;
        if self.state == EmberState::Starting || self.state == EmberState::Down {
            self.state = EmberState::Alive;
            self.start_entered_at = None;
            tracing::info!(
                spawn_count = self.spawn_count,
                "ember lifecycle: heartbeat received — state=Alive"
            );
        }
    }

    /// Check heartbeat status. Call this periodically from the health loop.
    ///
    /// Returns `true` if ember is unresponsive and needs intervention.
    pub fn check_heartbeat(&mut self) -> bool {
        if self.state != EmberState::Alive {
            return false;
        }

        let deadline = self.config.heartbeat_interval * self.config.missed_heartbeat_threshold;

        match self.last_heartbeat {
            Some(last) if last.elapsed() > deadline => {
                self.missed_heartbeats += 1;
                if self.missed_heartbeats >= self.config.missed_heartbeat_threshold {
                    tracing::error!(
                        missed = self.missed_heartbeats,
                        "ember lifecycle: UNRESPONSIVE — missed {} heartbeats",
                        self.missed_heartbeats
                    );
                    self.state = EmberState::Unresponsive;
                    return true;
                }
                tracing::warn!(
                    missed = self.missed_heartbeats,
                    threshold = self.config.missed_heartbeat_threshold,
                    "ember lifecycle: heartbeat missed"
                );
                false
            }
            _ => false,
        }
    }

    /// Spawn ember via systemctl.
    ///
    /// Returns `Ok(())` if the start command was issued successfully.
    pub fn spawn_ember(&mut self) -> Result<(), String> {
        tracing::info!("ember lifecycle: spawning ember");
        self.state = EmberState::Starting;
        self.start_entered_at = Some(Instant::now());
        self.spawn_count += 1;

        let output = std::process::Command::new("systemctl")
            .args(["start", "coral-ember"])
            .output()
            .map_err(|e| format!("systemctl start: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            self.state = EmberState::Down;
            self.start_entered_at = None;
            return Err(format!("systemctl start failed: {stderr}"));
        }

        tracing::info!(
            spawn_count = self.spawn_count,
            "ember lifecycle: start command issued"
        );
        Ok(())
    }

    /// Kill the current ember process.
    ///
    /// Issues `systemctl stop coral-ember`. Ember's SIGTERM handler will
    /// disable bus master on all devices before exiting. If ember doesn't
    /// exit within `kill_grace_period`, systemd escalates to SIGKILL.
    pub fn kill_ember(&mut self) -> Result<(), String> {
        tracing::info!("ember lifecycle: killing ember");
        self.state = EmberState::Killing;
        self.kill_entered_at = Some(Instant::now());

        let output = std::process::Command::new("systemctl")
            .args(["stop", "coral-ember"])
            .output()
            .map_err(|e| format!("systemctl stop: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(stderr = %stderr, "systemctl stop returned non-zero");
        }

        self.state = EmberState::Down;
        self.last_heartbeat = None;
        self.missed_heartbeats = 0;
        self.kill_entered_at = None;

        tracing::info!("ember lifecycle: ember stopped");
        Ok(())
    }

    /// Checkpoint VFIO fds from a live ember into the vault.
    ///
    /// Should be called periodically while ember is `Alive`. The vaulted fds
    /// keep VFIO bindings alive in the kernel even if ember dies — preventing
    /// the PM reset that would cold-down the GPU.
    pub fn checkpoint_fds(&mut self) {
        if self.state != EmberState::Alive || self.config.ember_socket.is_empty() {
            return;
        }

        // Don't checkpoint too frequently — every 30s is enough
        if let Some(last) = self.last_checkpoint {
            if last.elapsed() < Duration::from_secs(30) {
                return;
            }
        }

        match self.vault.checkpoint_from_ember(&self.config.ember_socket) {
            Ok(count) => {
                tracing::info!(devices = count, "fd vault: checkpoint OK");
                self.last_checkpoint = Some(Instant::now());
            }
            Err(e) => {
                tracing::warn!(error = %e, "fd vault: checkpoint failed (non-fatal)");
            }
        }
    }

    /// Resurrect ember: stop current instance, conditionally warm-cycle GPUs,
    /// then start fresh.
    ///
    /// If the fd vault holds backup fds, the VFIO binding stayed alive through
    /// ember's death — no warm cycle needed (GPU is still warm). If the vault
    /// is empty (first start, or fds were lost), do a full nouveau warm cycle.
    pub fn resurrect_ember(&mut self) -> Result<(), String> {
        let vault_live = self.vault.device_count() > 0;
        tracing::info!(
            vault_devices = self.vault.device_count(),
            warm_cycle_needed = !vault_live,
            "ember lifecycle: RESURRECTING"
        );

        if self.state != EmberState::Down {
            self.kill_ember()?;
        }

        if vault_live {
            // Vault kept VFIO binding alive — GPU is still warm. Clear the
            // vault so the new ember re-opens fresh fds (old ones stay valid
            // in kernel until the OwnedFds here are dropped).
            tracing::info!("resurrection: vault has live fds — skipping warm cycle");
            self.vault.clear();
        } else {
            // No vaulted fds — GPU went through PM reset and is cold. Warm
            // cycle via nouveau to retrain the memory controller.
            for bdf in &self.config.managed_bdfs {
                match sysfs_warm_cycle(bdf) {
                    Ok(()) => tracing::info!(bdf, "resurrection warm cycle: SUCCESS"),
                    Err(e) => tracing::warn!(bdf, error = %e, "resurrection warm cycle: FAILED (ember may still recover)"),
                }
            }
        }

        self.resurrect_count += 1;
        self.last_checkpoint = None;

        self.spawn_ember()
    }

    /// Perform the heartbeat check via IPC. Sends `ember.status` RPC
    /// and returns `true` if ember responded.
    pub fn probe_heartbeat(&self) -> bool {
        let socket_path = &self.config.ember_socket;
        if socket_path.is_empty() {
            return false;
        }

        match std::os::unix::net::UnixStream::connect(socket_path) {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let req = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "ember.status",
                    "params": {},
                    "id": 1,
                });
                if std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes()).is_err() {
                    return false;
                }
                let mut buf = [0u8; 1024];
                matches!(std::io::Read::read(&mut &stream, &mut buf), Ok(n) if n > 0)
            }
            Err(_) => false,
        }
    }

    /// Run one tick of the heartbeat monitor. Call from the health loop.
    ///
    /// Returns the current state after processing.
    pub fn tick(&mut self) -> EmberState {
        match self.state {
            EmberState::Alive => {
                if self.probe_heartbeat() {
                    self.record_heartbeat();
                    self.checkpoint_fds();
                } else {
                    self.check_heartbeat();
                }
            }
            EmberState::Starting => {
                if self.probe_heartbeat() {
                    self.record_heartbeat();
                } else if let Some(entered) = self.start_entered_at {
                    if entered.elapsed() > self.config.start_timeout {
                        tracing::error!(
                            timeout_secs = self.config.start_timeout.as_secs(),
                            "ember lifecycle: start timeout exceeded — giving up"
                        );
                        self.state = EmberState::Down;
                        self.start_entered_at = None;
                    }
                }
            }
            EmberState::Unresponsive => {
                tracing::warn!(
                    "ember lifecycle tick: ember unresponsive — initiating resurrection"
                );
                if let Err(e) = self.resurrect_ember() {
                    tracing::error!(error = %e, "resurrection failed");
                    self.state = EmberState::Down;
                }
            }
            EmberState::Down => {
                if self.probe_heartbeat() {
                    tracing::info!("ember lifecycle: ember already running (detected from Down state)");
                    self.record_heartbeat();
                }
            }
            EmberState::Killing => {
                if let Some(entered) = self.kill_entered_at {
                    if entered.elapsed() > self.config.kill_grace_period {
                        tracing::warn!(
                            "ember lifecycle: kill grace period exceeded — forcing state to Down"
                        );
                        self.state = EmberState::Down;
                        self.kill_entered_at = None;
                    }
                }
            }
        }
        self.state
    }

    /// Trigger an automatic nouveau warm cycle for a cold device.
    ///
    /// Sequence: release from ember → bind to nouveau → wait → rebind to vfio → reacquire in ember.
    pub fn auto_warm_device(&self, bdf: &str, ember_socket: &str) -> Result<(), String> {
        tracing::info!(bdf, "auto-warm: initiating nouveau warm cycle");

        let steps = [
            ("ember.release", serde_json::json!({"bdf": bdf})),
        ];

        for (method, params) in &steps {
            let stream = std::os::unix::net::UnixStream::connect(ember_socket)
                .map_err(|e| format!("auto-warm: connect to ember: {e}"))?;
            let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
            let req = serde_json::json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
                "id": 1,
            });
            std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())
                .map_err(|e| format!("auto-warm: write {method}: {e}"))?;
            let mut buf = [0u8; 4096];
            let _ = std::io::Read::read(&mut &stream, &mut buf);
        }

        let bind_result = std::process::Command::new("sudo")
            .args(["tee", &format!("/sys/bus/pci/devices/{bdf}/driver_override")])
            .stdin(std::process::Stdio::piped())
            .output();
        if let Ok(output) = bind_result {
            if !output.status.success() {
                tracing::warn!(bdf, "auto-warm: driver_override write failed (non-fatal)");
            }
        }

        std::thread::sleep(Duration::from_secs(3));

        let _ = std::process::Command::new("sudo")
            .args(["sh", "-c", &format!(
                "echo > /sys/bus/pci/devices/{bdf}/driver_override && \
                 echo {bdf} > /sys/bus/pci/drivers/vfio-pci/bind"
            )])
            .output();

        let stream = std::os::unix::net::UnixStream::connect(ember_socket)
            .map_err(|e| format!("auto-warm: reconnect to ember: {e}"))?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "ember.reacquire",
            "params": {"bdf": bdf},
            "id": 1,
        });
        std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())
            .map_err(|e| format!("auto-warm: reacquire write: {e}"))?;
        let mut buf = [0u8; 4096];
        let _ = std::io::Read::read(&mut &stream, &mut buf);

        tracing::info!(bdf, "auto-warm: warm cycle complete");
        Ok(())
    }

    /// Check VRAM liveness for a device via ember RPC.
    ///
    /// Returns `true` if VRAM is warm (accessible), `false` if cold.
    pub fn check_vram_liveness(&self, bdf: &str, ember_socket: &str) -> bool {
        let stream = match std::os::unix::net::UnixStream::connect(ember_socket) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "ember.pramin.read",
            "params": {"bdf": bdf, "vram_addr": 0x10000, "length": 4},
            "id": 1,
        });
        if std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes()).is_err() {
            return false;
        }
        let mut buf = [0u8; 4096];
        let n = match std::io::Read::read(&mut &stream, &mut buf) {
            Ok(n) if n > 0 => n,
            _ => return false,
        };

        let resp: serde_json::Value = match serde_json::from_slice(&buf[..n]) {
            Ok(v) => v,
            Err(_) => return false,
        };

        if resp.get("error").is_some() {
            return false;
        }

        if let Some(data) = resp.pointer("/result/data") {
            if let Some(arr) = data.as_array() {
                if arr.len() >= 4 {
                    let val = arr[0].as_u64().unwrap_or(0)
                        | (arr[1].as_u64().unwrap_or(0) << 8)
                        | (arr[2].as_u64().unwrap_or(0) << 16)
                        | (arr[3].as_u64().unwrap_or(0) << 24);
                    return val != 0xbad0_ac01 && val != 0xFFFF_FFFF;
                }
            }
        }

        false
    }

    /// JSON-serializable status for RPC responses.
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({
            "state": format!("{:?}", self.state),
            "spawn_count": self.spawn_count,
            "resurrect_count": self.resurrect_count,
            "missed_heartbeats": self.missed_heartbeats,
            "last_heartbeat_ago_ms": self.last_heartbeat.map(|t| t.elapsed().as_millis() as u64),
        })
    }
}

/// Perform a GPU warm cycle via sysfs: bind to nouveau, wait, unbind.
///
/// This retrains the GPU's memory controller and clears PRAMIN degradation.
/// Called by glowplug during resurrection when ember is dead (no RPC possible).
///
/// Sequence:
/// 1. Ensure vfio-pci unbinds the device (ember is already dead)
/// 2. Set driver_override to nouveau, trigger probe
/// 3. Wait for nouveau to initialize (trains HBM2)
/// 4. Unbind nouveau, set driver_override back to vfio-pci
/// 5. Let ember re-acquire on startup
/// Public wrapper for fleet module access.
pub fn sysfs_warm_cycle_pub(bdf: &str) -> Result<(), String> {
    sysfs_warm_cycle(bdf)
}

fn sysfs_warm_cycle(bdf: &str) -> Result<(), String> {
    let device_path = format!("/sys/bus/pci/devices/{bdf}");
    let override_path = format!("{device_path}/driver_override");
    let driver_path = format!("{device_path}/driver");

    // Step 1: Unbind current driver (if any)
    if let Ok(link) = std::fs::read_link(&driver_path) {
        if let Some(driver_name) = link.file_name().and_then(|n| n.to_str()) {
            let unbind_path = format!("/sys/bus/pci/drivers/{driver_name}/unbind");
            let _ = std::fs::write(&unbind_path, bdf);
            std::thread::sleep(Duration::from_millis(500));
            tracing::info!(bdf, driver = driver_name, "warm cycle: unbound current driver");
        }
    }

    // Step 2: Ensure nouveau module is loaded (may be blacklisted at boot)
    let modprobe_ok = std::process::Command::new("modprobe")
        .arg("nouveau")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !modprobe_ok {
        tracing::warn!(bdf, "warm cycle: modprobe nouveau non-zero (may already be loaded)");
    }
    std::thread::sleep(Duration::from_millis(200));

    // Step 3: Set driver_override to nouveau and trigger probe
    std::fs::write(&override_path, "nouveau")
        .map_err(|e| format!("set driver_override to nouveau: {e}"))?;
    let _ = std::fs::write("/sys/bus/pci/drivers_probe", bdf);
    tracing::info!(bdf, "warm cycle: nouveau probe issued");

    // Step 4: Wait for nouveau initialization (memory controller retrain)
    std::thread::sleep(Duration::from_secs(4));

    // Verify nouveau actually bound
    let bound = std::fs::read_link(&driver_path)
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
        .unwrap_or_default();
    if bound != "nouveau" {
        tracing::warn!(bdf, bound_driver = %bound, "warm cycle: nouveau did not bind via probe, trying direct");
        let _ = std::fs::write("/sys/bus/pci/drivers/nouveau/bind", bdf);
        std::thread::sleep(Duration::from_secs(3));
    }

    // Step 5: Unbind nouveau
    let _ = std::fs::write("/sys/bus/pci/drivers/nouveau/unbind", bdf);
    std::thread::sleep(Duration::from_millis(500));
    tracing::info!(bdf, "warm cycle: nouveau unbound");

    // Step 6: Set driver_override back to vfio-pci and trigger probe
    std::fs::write(&override_path, "vfio-pci")
        .map_err(|e| format!("set driver_override to vfio-pci: {e}"))?;
    let _ = std::fs::write("/sys/bus/pci/drivers_probe", bdf);
    std::thread::sleep(Duration::from_secs(1));
    tracing::info!(bdf, "warm cycle: vfio-pci override + probe done");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> EmberLifecycleConfig {
        EmberLifecycleConfig {
            heartbeat_interval: Duration::from_millis(100),
            missed_heartbeat_threshold: 3,
            kill_grace_period: Duration::from_secs(1),
            start_timeout: Duration::from_secs(5),
            ember_socket: String::new(),
            managed_bdfs: Vec::new(),
        }
    }

    #[test]
    fn initial_state_is_down() {
        let lc = EmberLifecycle::new(test_config());
        assert_eq!(lc.state(), EmberState::Down);
        assert_eq!(lc.spawn_count(), 0);
        assert_eq!(lc.resurrect_count(), 0);
    }

    #[test]
    fn heartbeat_transitions_starting_to_alive() {
        let mut lc = EmberLifecycle::new(test_config());
        lc.state = EmberState::Starting;
        lc.record_heartbeat();
        assert_eq!(lc.state(), EmberState::Alive);
    }

    #[test]
    fn heartbeat_transitions_down_to_alive() {
        let mut lc = EmberLifecycle::new(test_config());
        assert_eq!(lc.state(), EmberState::Down);
        lc.record_heartbeat();
        assert_eq!(lc.state(), EmberState::Alive);
    }

    #[test]
    fn check_heartbeat_false_when_not_alive() {
        let mut lc = EmberLifecycle::new(test_config());
        assert!(!lc.check_heartbeat());
    }

    #[test]
    fn status_is_valid_json() {
        let lc = EmberLifecycle::new(test_config());
        let s = lc.status();
        assert_eq!(s["state"], "Down");
        assert_eq!(s["spawn_count"], 0);
    }
}
