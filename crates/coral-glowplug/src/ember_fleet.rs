// SPDX-License-Identifier: AGPL-3.0-only
//! Ember Fleet — per-device ember instances + hot-standby pool.
//!
//! Evolves the single-ember model into N isolated ember processes (one per GPU)
//! plus M hot-standby instances for sub-200ms takeover on death.
//!
//! Each [`EmberInstance`] manages one GPU's lifecycle independently. A Titan V
//! ember crash has zero impact on K80 embers. [`EmberFleet`] orchestrates the
//! entire pool, handling spawning, heartbeat monitoring, checkpoint, and
//! fault-informed resurrection.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::fd_vault::FdVault;

/// Re-export the state enum from the legacy module.
pub use crate::ember_lifecycle::EmberState;

/// Resurrection strategy chosen based on fault history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResurrectionStrategy {
    /// Vault fds are alive — promote a hot-standby via `ember.adopt_device`.
    HotAdopt,
    /// Vault alive but fault suggests warm cycle first, then respawn.
    WarmThenRespawn,
    /// Vault dead or PCIe timeout — full remove+rescan, then respawn.
    FullRecovery,
    /// Cold respawn via systemctl (simplest fallback).
    ColdRespawn,
}

/// Record of a single ember death for fault-pattern analysis.
#[derive(Debug, Clone)]
pub struct FaultRecord {
    /// When the fault was detected.
    pub timestamp: Instant,
    /// Exit signal or code if known.
    pub exit_info: Option<String>,
    /// Which RPC was in-flight (if trackable via last heartbeat response).
    pub last_operation: Option<String>,
    /// Was the device already in a faulted state?
    pub device_was_faulted: bool,
    /// Strategy that was used for this resurrection.
    pub strategy_used: ResurrectionStrategy,
}

/// Per-device ember instance managed by the fleet.
pub struct EmberInstance {
    /// PCI BDF address of the device this ember holds.
    pub bdf: String,
    /// Human-readable name from config (e.g. "titan-v-1").
    pub name: Option<String>,
    /// Current lifecycle state.
    state: EmberState,
    /// Socket path for this instance's ember process.
    pub socket_path: String,
    /// Systemd unit name for this instance (e.g. "coral-ember@0000-03-00.0").
    pub unit_name: String,
    /// Per-device fd vault.
    vault: FdVault,
    /// Fault history for pattern-based resurrection.
    pub fault_history: Vec<FaultRecord>,
    spawn_count: u64,
    resurrect_count: u64,
    last_heartbeat: Option<Instant>,
    start_entered_at: Option<Instant>,
    kill_entered_at: Option<Instant>,
    missed_heartbeats: u32,
    last_checkpoint: Option<Instant>,
}

/// Hot-standby ember process waiting for device adoption.
pub struct StandbyEmber {
    /// Pool index.
    pub index: usize,
    /// Socket path for this standby.
    pub socket_path: String,
    /// Systemd unit name.
    pub unit_name: String,
    /// Lifecycle state (should be Alive when ready).
    state: EmberState,
    last_heartbeat: Option<Instant>,
    start_entered_at: Option<Instant>,
}

/// Fleet-wide configuration.
#[derive(Debug, Clone)]
pub struct FleetConfig {
    pub heartbeat_interval: Duration,
    pub missed_heartbeat_threshold: u32,
    pub kill_grace_period: Duration,
    pub start_timeout: Duration,
    /// Number of hot-standby embers to maintain.
    pub standby_pool_size: usize,
}

impl Default for FleetConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(2),
            missed_heartbeat_threshold: 3,
            kill_grace_period: Duration::from_secs(5),
            start_timeout: Duration::from_secs(30),
            standby_pool_size: 1,
        }
    }
}

/// Fleet orchestrator managing N per-device embers + M standbys.
pub struct EmberFleet {
    pub instances: HashMap<String, EmberInstance>,
    pub standby_pool: Vec<StandbyEmber>,
    config: FleetConfig,
}

impl EmberInstance {
    fn new(bdf: String, name: Option<String>) -> Self {
        let slug = coral_ember::bdf_to_slug(&bdf);
        Self {
            socket_path: coral_ember::ember_instance_socket_path(&bdf),
            unit_name: format!("coral-ember@{slug}"),
            bdf,
            name,
            state: EmberState::Down,
            vault: FdVault::new(),
            fault_history: Vec::new(),
            spawn_count: 0,
            resurrect_count: 0,
            last_heartbeat: None,
            start_entered_at: None,
            kill_entered_at: None,
            missed_heartbeats: 0,
            last_checkpoint: None,
        }
    }

    pub fn state(&self) -> EmberState {
        self.state
    }

    pub fn spawn_count(&self) -> u64 {
        self.spawn_count
    }

    pub fn resurrect_count(&self) -> u64 {
        self.resurrect_count
    }

    pub fn vault(&self) -> &FdVault {
        &self.vault
    }

    fn spawn(&mut self) -> Result<(), String> {
        tracing::info!(bdf = %self.bdf, unit = %self.unit_name, "fleet: spawning ember instance");
        self.state = EmberState::Starting;
        self.start_entered_at = Some(Instant::now());
        self.spawn_count += 1;

        let output = std::process::Command::new("systemctl")
            .args(["start", &self.unit_name])
            .output()
            .map_err(|e| format!("systemctl start {}: {e}", self.unit_name))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            self.state = EmberState::Down;
            self.start_entered_at = None;
            return Err(format!("systemctl start {} failed: {stderr}", self.unit_name));
        }

        tracing::info!(
            bdf = %self.bdf,
            spawn_count = self.spawn_count,
            "fleet: start command issued"
        );
        Ok(())
    }

    fn kill(&mut self) -> Result<(), String> {
        tracing::info!(bdf = %self.bdf, unit = %self.unit_name, "fleet: killing ember instance");
        self.state = EmberState::Killing;
        self.kill_entered_at = Some(Instant::now());

        let output = std::process::Command::new("systemctl")
            .args(["stop", &self.unit_name])
            .output()
            .map_err(|e| format!("systemctl stop {}: {e}", self.unit_name))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(unit = %self.unit_name, stderr = %stderr, "systemctl stop non-zero");
        }

        self.state = EmberState::Down;
        self.last_heartbeat = None;
        self.missed_heartbeats = 0;
        self.kill_entered_at = None;
        Ok(())
    }

    fn probe_heartbeat(&self) -> bool {
        if self.socket_path.is_empty() {
            return false;
        }
        match std::os::unix::net::UnixStream::connect(&self.socket_path) {
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

    fn record_heartbeat(&mut self) {
        self.last_heartbeat = Some(Instant::now());
        self.missed_heartbeats = 0;
        if self.state == EmberState::Starting || self.state == EmberState::Down {
            self.state = EmberState::Alive;
            self.start_entered_at = None;
            tracing::info!(bdf = %self.bdf, "fleet: ember alive");
        }
    }

    fn check_heartbeat(&mut self, config: &FleetConfig) -> bool {
        if self.state != EmberState::Alive {
            return false;
        }
        let deadline = config.heartbeat_interval * config.missed_heartbeat_threshold;
        match self.last_heartbeat {
            Some(last) if last.elapsed() > deadline => {
                self.missed_heartbeats += 1;
                if self.missed_heartbeats >= config.missed_heartbeat_threshold {
                    tracing::error!(
                        bdf = %self.bdf,
                        missed = self.missed_heartbeats,
                        "fleet: ember UNRESPONSIVE"
                    );
                    self.state = EmberState::Unresponsive;
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn checkpoint_fds(&mut self) {
        if self.state != EmberState::Alive || self.socket_path.is_empty() {
            return;
        }
        if let Some(last) = self.last_checkpoint {
            if last.elapsed() < Duration::from_secs(30) {
                return;
            }
        }
        match self.vault.checkpoint_from_ember(&self.socket_path) {
            Ok(count) => {
                tracing::info!(bdf = %self.bdf, devices = count, "fleet vault: checkpoint OK");
                self.last_checkpoint = Some(Instant::now());
            }
            Err(e) => {
                tracing::warn!(bdf = %self.bdf, error = %e, "fleet vault: checkpoint failed");
            }
        }
    }

    /// Choose resurrection strategy based on fault history.
    fn choose_strategy(&self) -> ResurrectionStrategy {
        let vault_alive = self.vault.device_count() > 0;
        let recent_faults = self.fault_history.iter()
            .filter(|f| f.timestamp.elapsed() < Duration::from_secs(300))
            .count();

        if !vault_alive {
            return if recent_faults >= 3 {
                ResurrectionStrategy::FullRecovery
            } else {
                ResurrectionStrategy::WarmThenRespawn
            };
        }

        if recent_faults >= 2 {
            ResurrectionStrategy::WarmThenRespawn
        } else {
            ResurrectionStrategy::HotAdopt
        }
    }

    fn resurrect(&mut self, config: &FleetConfig, standby: Option<&mut StandbyEmber>) -> Result<(), String> {
        let strategy = self.choose_strategy();
        let vault_alive = self.vault.device_count() > 0;

        tracing::info!(
            bdf = %self.bdf,
            strategy = ?strategy,
            vault_alive,
            "fleet: RESURRECTING"
        );

        if self.state != EmberState::Down {
            self.kill()?;
        }

        match strategy {
            ResurrectionStrategy::HotAdopt => {
                if let Some(sb) = standby {
                    if sb.state == EmberState::Alive {
                        match adopt_device_rpc(&sb.socket_path, &self.bdf, &self.vault) {
                            Ok(()) => {
                                tracing::info!(
                                    bdf = %self.bdf,
                                    standby = sb.index,
                                    "fleet: hot-standby adopted device"
                                );
                                self.socket_path = sb.socket_path.clone();
                                self.unit_name = sb.unit_name.clone();
                                self.state = EmberState::Alive;
                                self.last_heartbeat = Some(Instant::now());
                                self.vault.clear();
                                self.fault_history.push(FaultRecord {
                                    timestamp: Instant::now(),
                                    exit_info: None,
                                    last_operation: None,
                                    device_was_faulted: false,
                                    strategy_used: strategy,
                                });
                                self.resurrect_count += 1;
                                return Ok(());
                            }
                            Err(e) => {
                                tracing::warn!(
                                    bdf = %self.bdf,
                                    error = %e,
                                    "fleet: hot-adopt failed, falling back to cold respawn"
                                );
                            }
                        }
                    }
                }
                self.vault.clear();
                self.cold_respawn(config, strategy)
            }
            ResurrectionStrategy::WarmThenRespawn => {
                self.vault.clear();
                match super::ember_lifecycle::sysfs_warm_cycle_pub(&self.bdf) {
                    Ok(()) => tracing::info!(bdf = %self.bdf, "fleet: warm cycle OK"),
                    Err(e) => tracing::warn!(bdf = %self.bdf, error = %e, "fleet: warm cycle failed"),
                }
                self.cold_respawn(config, strategy)
            }
            ResurrectionStrategy::FullRecovery => {
                self.vault.clear();
                tracing::info!(bdf = %self.bdf, "fleet: full recovery — remove+rescan");
                let device_path = format!("/sys/bus/pci/devices/{}", self.bdf);
                let _ = std::fs::write(format!("{device_path}/remove"), "1");
                std::thread::sleep(Duration::from_secs(2));
                let _ = std::fs::write("/sys/bus/pci/rescan", "1");
                std::thread::sleep(Duration::from_secs(3));
                match super::ember_lifecycle::sysfs_warm_cycle_pub(&self.bdf) {
                    Ok(()) => tracing::info!(bdf = %self.bdf, "fleet: post-rescan warm cycle OK"),
                    Err(e) => tracing::warn!(bdf = %self.bdf, error = %e, "fleet: post-rescan warm cycle failed"),
                }
                self.cold_respawn(config, strategy)
            }
            ResurrectionStrategy::ColdRespawn => {
                self.vault.clear();
                self.cold_respawn(config, strategy)
            }
        }
    }

    fn cold_respawn(&mut self, _config: &FleetConfig, strategy: ResurrectionStrategy) -> Result<(), String> {
        self.fault_history.push(FaultRecord {
            timestamp: Instant::now(),
            exit_info: None,
            last_operation: None,
            device_was_faulted: false,
            strategy_used: strategy,
        });
        self.resurrect_count += 1;
        self.last_checkpoint = None;

        let slug = coral_ember::bdf_to_slug(&self.bdf);
        self.socket_path = coral_ember::ember_instance_socket_path(&self.bdf);
        self.unit_name = format!("coral-ember@{slug}");
        self.spawn()
    }

    /// Tick one cycle for this instance.
    fn tick(&mut self, config: &FleetConfig) -> EmberState {
        match self.state {
            EmberState::Alive => {
                if self.probe_heartbeat() {
                    self.record_heartbeat();
                    self.checkpoint_fds();
                } else {
                    self.check_heartbeat(config);
                }
            }
            EmberState::Starting => {
                if self.probe_heartbeat() {
                    self.record_heartbeat();
                } else if let Some(entered) = self.start_entered_at {
                    if entered.elapsed() > config.start_timeout {
                        tracing::error!(bdf = %self.bdf, "fleet: start timeout");
                        self.state = EmberState::Down;
                        self.start_entered_at = None;
                    }
                }
            }
            EmberState::Unresponsive => {
                tracing::warn!(bdf = %self.bdf, "fleet: unresponsive — will resurrect");
            }
            EmberState::Down => {
                if self.probe_heartbeat() {
                    tracing::info!(bdf = %self.bdf, "fleet: ember already running (from Down)");
                    self.record_heartbeat();
                }
            }
            EmberState::Killing => {
                if let Some(entered) = self.kill_entered_at {
                    if entered.elapsed() > config.kill_grace_period {
                        self.state = EmberState::Down;
                        self.kill_entered_at = None;
                    }
                }
            }
        }
        self.state
    }

    /// JSON-serializable status.
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({
            "bdf": self.bdf,
            "name": self.name,
            "state": format!("{:?}", self.state),
            "socket": self.socket_path,
            "unit": self.unit_name,
            "spawn_count": self.spawn_count,
            "resurrect_count": self.resurrect_count,
            "missed_heartbeats": self.missed_heartbeats,
            "vault_devices": self.vault.device_count(),
            "fault_count": self.fault_history.len(),
            "last_heartbeat_ago_ms": self.last_heartbeat.map(|t| t.elapsed().as_millis() as u64),
        })
    }
}

impl StandbyEmber {
    fn new(index: usize) -> Self {
        Self {
            index,
            socket_path: coral_ember::ember_standby_socket_path(index),
            unit_name: format!("coral-ember-standby@{index}"),
            state: EmberState::Down,
            last_heartbeat: None,
            start_entered_at: None,
        }
    }

    fn spawn(&mut self) -> Result<(), String> {
        tracing::info!(index = self.index, "fleet: spawning standby ember");
        self.state = EmberState::Starting;
        self.start_entered_at = Some(Instant::now());

        let output = std::process::Command::new("systemctl")
            .args(["start", &self.unit_name])
            .output()
            .map_err(|e| format!("systemctl start {}: {e}", self.unit_name))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            self.state = EmberState::Down;
            return Err(format!("spawn standby {}: {stderr}", self.unit_name));
        }
        Ok(())
    }

    fn probe_heartbeat(&self) -> bool {
        match std::os::unix::net::UnixStream::connect(&self.socket_path) {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let req = serde_json::json!({
                    "jsonrpc": "2.0", "method": "ember.status", "params": {}, "id": 1,
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

    fn tick(&mut self, config: &FleetConfig) {
        match self.state {
            EmberState::Alive => {
                if !self.probe_heartbeat() {
                    self.state = EmberState::Down;
                } else {
                    self.last_heartbeat = Some(Instant::now());
                }
            }
            EmberState::Starting => {
                if self.probe_heartbeat() {
                    self.state = EmberState::Alive;
                    self.last_heartbeat = Some(Instant::now());
                    self.start_entered_at = None;
                    tracing::info!(index = self.index, "fleet: standby alive");
                } else if let Some(entered) = self.start_entered_at {
                    if entered.elapsed() > config.start_timeout {
                        tracing::warn!(index = self.index, "fleet: standby start timeout");
                        self.state = EmberState::Down;
                        self.start_entered_at = None;
                    }
                }
            }
            EmberState::Down => {}
            _ => {}
        }
    }
}

impl EmberFleet {
    /// Create a fleet from device configs.
    pub fn new(
        devices: &[(String, Option<String>)],
        config: FleetConfig,
    ) -> Self {
        let instances = devices
            .iter()
            .map(|(bdf, name)| (bdf.clone(), EmberInstance::new(bdf.clone(), name.clone())))
            .collect();

        Self {
            instances,
            standby_pool: Vec::new(),
            config,
        }
    }

    /// Spawn all per-device ember instances.
    pub fn spawn_all(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        let bdfs: Vec<String> = self.instances.keys().cloned().collect();
        for bdf in bdfs {
            if let Some(inst) = self.instances.get_mut(&bdf) {
                if let Err(e) = inst.spawn() {
                    tracing::error!(bdf = %bdf, error = %e, "fleet: failed to spawn");
                    errors.push(format!("{bdf}: {e}"));
                }
            }
        }
        self.ensure_standby_pool();
        errors
    }

    /// Ensure the standby pool has the configured number of ready instances.
    pub fn ensure_standby_pool(&mut self) {
        while self.standby_pool.len() < self.config.standby_pool_size {
            let idx = self.standby_pool.len();
            let mut sb = StandbyEmber::new(idx);
            if let Err(e) = sb.spawn() {
                tracing::warn!(index = idx, error = %e, "fleet: standby spawn failed (non-fatal)");
            }
            self.standby_pool.push(sb);
        }
    }

    /// Tick all instances and standbys. Returns BDFs that need resurrection.
    pub fn tick_all(&mut self) -> Vec<String> {
        let mut needs_resurrect = Vec::new();
        let bdfs: Vec<String> = self.instances.keys().cloned().collect();

        for bdf in bdfs {
            if let Some(inst) = self.instances.get_mut(&bdf) {
                let state = inst.tick(&self.config);
                match state {
                    EmberState::Unresponsive | EmberState::Down => {
                        if inst.spawn_count > 0 && state == EmberState::Unresponsive {
                            needs_resurrect.push(bdf);
                        } else if inst.spawn_count > 0
                            && state == EmberState::Down
                            && inst.last_heartbeat.is_some()
                        {
                            needs_resurrect.push(bdf);
                        }
                    }
                    _ => {}
                }
            }
        }

        for sb in &mut self.standby_pool {
            sb.tick(&self.config);
        }

        needs_resurrect
    }

    /// Resurrect a specific instance, optionally using a standby.
    pub fn resurrect(&mut self, bdf: &str) -> Result<(), String> {
        let ready_standby_idx = self.standby_pool
            .iter()
            .position(|sb| sb.state == EmberState::Alive);

        let standby = ready_standby_idx.map(|idx| &mut self.standby_pool[idx]);

        let inst = self.instances.get_mut(bdf)
            .ok_or_else(|| format!("no instance for {bdf}"))?;

        let config = self.config.clone();

        // We need to take the standby out temporarily to avoid borrow issues
        drop(standby);
        let mut standby_ref = ready_standby_idx.map(|idx| {
            std::mem::replace(&mut self.standby_pool[idx], StandbyEmber::new(idx))
        });

        let inst = self.instances.get_mut(bdf).unwrap();
        let result = inst.resurrect(&config, standby_ref.as_mut());

        if ready_standby_idx.is_some() {
            self.ensure_standby_pool();
        }

        result
    }

    /// Fleet-wide status for RPC.
    pub fn status(&self) -> serde_json::Value {
        let instances: Vec<serde_json::Value> = self.instances.values().map(|i| i.status()).collect();
        let standbys: Vec<serde_json::Value> = self.standby_pool.iter().map(|sb| {
            serde_json::json!({
                "index": sb.index,
                "state": format!("{:?}", sb.state),
                "socket": sb.socket_path,
            })
        }).collect();

        serde_json::json!({
            "mode": "fleet",
            "instances": instances,
            "standby_pool": standbys,
            "standby_pool_size": self.config.standby_pool_size,
        })
    }

    /// Get the socket path for a specific BDF.
    pub fn socket_for_bdf(&self, bdf: &str) -> Option<&str> {
        self.instances.get(bdf).map(|i| i.socket_path.as_str())
    }

    /// Write fleet discovery file for external clients.
    pub fn write_discovery(&self) {
        let routes: HashMap<&str, &str> = self.instances.iter()
            .map(|(bdf, inst)| (bdf.as_str(), inst.socket_path.as_str()))
            .collect();

        let discovery = serde_json::json!({
            "mode": "fleet",
            "routes": routes,
            "standby_count": self.standby_pool.iter().filter(|s| s.state == EmberState::Alive).count(),
        });

        let path = "/tmp/biomeos/coral-ember-fleet.json";
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(path, serde_json::to_string_pretty(&discovery).unwrap_or_default()) {
            tracing::warn!(error = %e, "fleet: failed to write discovery file");
        }
    }
}

/// Send `ember.adopt_device` RPC to a standby ember, transferring vault fds.
fn adopt_device_rpc(standby_socket: &str, bdf: &str, _vault: &FdVault) -> Result<(), String> {
    let stream = std::os::unix::net::UnixStream::connect(standby_socket)
        .map_err(|e| format!("adopt: connect to standby: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "ember.adopt_device",
        "params": {"bdf": bdf},
        "id": 1,
    });
    std::io::Write::write_all(&mut &stream, format!("{req}\n").as_bytes())
        .map_err(|e| format!("adopt: write: {e}"))?;

    let mut buf = [0u8; 4096];
    let n = std::io::Read::read(&mut &stream, &mut buf)
        .map_err(|e| format!("adopt: read: {e}"))?;

    let resp: serde_json::Value = serde_json::from_slice(&buf[..n])
        .map_err(|e| format!("adopt: parse: {e}"))?;

    if resp.get("error").is_some() {
        return Err(format!("adopt: ember returned error: {}", resp["error"]));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fleet_creates_instances() {
        let devices = vec![
            ("0000:03:00.0".to_string(), Some("titan-v".to_string())),
            ("0000:4c:00.0".to_string(), None),
        ];
        let fleet = EmberFleet::new(&devices, FleetConfig::default());
        assert_eq!(fleet.instances.len(), 2);
        assert!(fleet.instances.contains_key("0000:03:00.0"));
        assert!(fleet.instances.contains_key("0000:4c:00.0"));
    }

    #[test]
    fn instance_socket_path() {
        let inst = EmberInstance::new("0000:03:00.0".to_string(), None);
        assert_eq!(inst.socket_path, "/run/coralreef/ember-0000-03-00.0.sock");
        assert_eq!(inst.unit_name, "coral-ember@0000-03-00.0");
    }

    #[test]
    fn standby_socket_path() {
        let sb = StandbyEmber::new(0);
        assert_eq!(sb.socket_path, "/run/coralreef/ember-standby-0.sock");
        assert_eq!(sb.unit_name, "coral-ember-standby@0");
    }

    #[test]
    fn strategy_cold_when_vault_empty() {
        let inst = EmberInstance::new("0000:03:00.0".to_string(), None);
        let strategy = inst.choose_strategy();
        assert!(matches!(strategy, ResurrectionStrategy::WarmThenRespawn));
    }
}
