// SPDX-License-Identifier: AGPL-3.0-only
//! DeviceSlot — persistent ownership of a PCIe device.
//!
//! Each slot manages one GPU/accelerator from boot to shutdown.
//! It tracks the current driver personality, power state, VRAM
//! health, and provides the VFIO fd for toadStool consumers.

use crate::config::DeviceConfig;
use crate::personality::{Personality, PersonalityRegistry};
use std::collections::BTreeMap;

const PCI_READ_DEAD: u32 = 0xDEAD_DEAD;
const PCI_READ_ALL_ONES: u32 = 0xFFFF_FFFF;
const PCI_FAULT_BADF: u16 = 0xBADF;
const PCI_FAULT_BAD0: u16 = 0xBAD0;
const PCI_FAULT_BAD1: u16 = 0xBAD1;

fn is_faulted_read(val: u32) -> bool {
    val == PCI_READ_DEAD
        || val == PCI_READ_ALL_ONES
        || (val >> 16) as u16 == PCI_FAULT_BADF
        || (val >> 16) as u16 == PCI_FAULT_BAD0
        || (val >> 16) as u16 == PCI_FAULT_BAD1
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PowerState {
    D0,
    D3Hot,
    D3Cold,
    Unknown,
}

impl std::fmt::Display for PowerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::D0 => write!(f, "D0"),
            Self::D3Hot => write!(f, "D3hot"),
            Self::D3Cold => write!(f, "D3cold"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceHealth {
    pub vram_alive: bool,
    pub boot0: u32,
    pub pmc_enable: u32,
    pub power: PowerState,
    pub pci_link_width: Option<u8>,
    pub domains_alive: usize,
    pub domains_faulted: usize,
}

pub struct DeviceSlot {
    pub config: DeviceConfig,
    pub bdf: String,
    pub personality: Personality,
    pub health: DeviceHealth,
    pub vendor_id: u16,
    pub device_id: u16,
    pub chip_name: String,
    vfio_device: Option<coral_driver::nv::RawVfioDevice>,
    register_snapshot: BTreeMap<usize, u32>,
}

impl DeviceSlot {
    pub fn new(config: DeviceConfig) -> Self {
        let bdf = config.bdf.clone();
        let (vendor_id, device_id) = read_pci_ids(&bdf);
        let chip_name = identify_chip(vendor_id, device_id);

        Self {
            config,
            bdf,
            personality: Personality::Unbound,
            health: DeviceHealth {
                vram_alive: false,
                boot0: 0,
                pmc_enable: 0,
                power: PowerState::Unknown,
                pci_link_width: None,
                domains_alive: 0,
                domains_faulted: 0,
            },
            vendor_id,
            device_id,
            chip_name,
            vfio_device: None,
            register_snapshot: BTreeMap::new(),
        }
    }

    pub fn has_vfio(&self) -> bool {
        self.vfio_device.is_some()
    }

    /// Bind device to the configured boot personality and take ownership.
    pub fn activate(&mut self) -> Result<(), String> {
        let target = self.config.boot_personality.clone();
        let registry = PersonalityRegistry::default_linux();
        if !registry.supports(&target) {
            return Err(format!(
                "unknown personality '{target}' (known: {:?})",
                registry.list(),
            ));
        }
        tracing::info!(bdf = %self.bdf, personality = %target, "activating device");

        self.refresh_power_state();

        // Check current driver — if already correct, skip rebind
        let current_driver =
            std::fs::read_link(format!("/sys/bus/pci/devices/{}/driver", self.bdf))
                .ok()
                .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()));

        let trait_personality = registry.create(&target);
        let expected_module = trait_personality
            .as_ref()
            .map(|p| p.driver_module().to_owned());
        let needs_rebind = expected_module
            .as_deref()
            .is_some_and(|module| current_driver.as_deref() != Some(module));

        if needs_rebind {
            // Unbind current driver first
            if current_driver.is_some() {
                tracing::info!(
                    bdf = %self.bdf,
                    current = current_driver.as_deref().unwrap_or("<none>"),
                    target = %target,
                    "unbinding current driver before activation"
                );
                sysfs_write(
                    &format!("/sys/bus/pci/devices/{}/driver/unbind", self.bdf),
                    &self.bdf,
                );
                std::thread::sleep(std::time::Duration::from_millis(500));
                sysfs_write(
                    &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
                    "on",
                );
            }
        }

        match target.as_str() {
            "vfio" => self.bind_vfio()?,
            "nouveau" => {
                if needs_rebind {
                    self.bind_driver("nouveau")?;
                } else {
                    let drm = find_drm_card(&self.bdf);
                    self.personality = Personality::Nouveau { drm_card: drm };
                }
            }
            "amdgpu" => {
                if needs_rebind {
                    self.bind_driver("amdgpu")?;
                } else {
                    let drm = find_drm_card(&self.bdf);
                    self.personality = Personality::Amdgpu { drm_card: drm };
                }
            }
            other => return Err(format!("unknown personality: {other}")),
        }

        self.check_health();
        tracing::info!(
            bdf = %self.bdf,
            personality = %self.personality,
            chip = %self.chip_name,
            vram = self.health.vram_alive,
            power = %self.health.power,
            "device activated"
        );

        Ok(())
    }

    /// Hot-swap to a new driver personality.
    pub fn swap(&mut self, target: &str) -> Result<(), String> {
        tracing::info!(bdf = %self.bdf, from = %self.personality, to = %target, "swapping personality");

        // Step 1: snapshot current state
        if self.has_vfio() {
            self.snapshot_registers();
        }

        // Step 2: release current personality
        self.release()?;

        // Step 3: bind new personality
        match target {
            "vfio" | "vfio-pci" => self.bind_vfio()?,
            "nouveau" => self.bind_driver("nouveau")?,
            "amdgpu" => self.bind_driver("amdgpu")?,
            "unbound" => { /* already unbound from release() */ }
            other => return Err(format!("unknown personality: {other}")),
        }

        // Step 4: verify health
        self.check_health();
        tracing::info!(
            bdf = %self.bdf,
            personality = %self.personality,
            vram = self.health.vram_alive,
            "swap complete"
        );

        Ok(())
    }

    /// Release current personality (unbind driver, drop VFIO fd).
    ///
    /// CRITICAL: we must *close* the VFIO fd (not leak it) before unbind.
    /// Leaking prevents the kernel from releasing the VFIO group, which
    /// blocks sysfs unbind indefinitely. The PM reset from fd close is
    /// accepted — the state vault snapshot preserves register state for
    /// restoration after re-binding.
    fn release(&mut self) -> Result<(), String> {
        // Drop VFIO device — this closes the fd and triggers PM reset,
        // but frees the VFIO group so unbind can proceed.
        drop(self.vfio_device.take());

        // Pin power before unbind to prevent D3 transition
        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );

        // Unbind from current driver
        let drv_path = format!("/sys/bus/pci/devices/{}/driver/unbind", self.bdf);
        sysfs_write(&drv_path, &self.bdf);
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Keep power pinned
        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );
        self.personality = Personality::Unbound;
        Ok(())
    }

    fn bind_vfio(&mut self) -> Result<(), String> {
        let group_id = read_iommu_group(&self.bdf);

        // Bind all devices in the same IOMMU group to vfio-pci
        // (required for group viability — e.g. audio companion device)
        bind_iommu_group_to_vfio(&self.bdf, group_id);

        // Set driver override for the primary device
        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/driver_override", self.bdf),
            "vfio-pci",
        );

        // Bind
        sysfs_write("/sys/bus/pci/drivers/vfio-pci/bind", &self.bdf);
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Pin D0
        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );
        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/d3cold_allowed", self.bdf),
            "0",
        );

        // Open VFIO device
        match coral_driver::nv::RawVfioDevice::open(&self.bdf) {
            Ok(dev) => {
                self.vfio_device = Some(dev);
                self.personality = Personality::Vfio { group_id };
                Ok(())
            }
            Err(e) => {
                self.personality = Personality::Vfio { group_id };
                Err(format!("VFIO open failed: {e}"))
            }
        }
    }

    fn bind_driver(&mut self, driver: &str) -> Result<(), String> {
        // Write newline to clear driver_override (empty string doesn't work via tee)
        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/driver_override", self.bdf),
            "\n",
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
        sysfs_write(&format!("/sys/bus/pci/drivers/{driver}/bind"), &self.bdf);
        std::thread::sleep(std::time::Duration::from_secs(3));

        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );

        let drm_card = find_drm_card(&self.bdf);
        self.personality = match driver {
            "nouveau" => Personality::Nouveau { drm_card },
            "amdgpu" => Personality::Amdgpu { drm_card },
            _ => Personality::Unbound,
        };
        Ok(())
    }

    /// Take a snapshot of key registers (for state preservation across swaps).
    pub fn snapshot_registers(&mut self) {
        let Some(dev) = &self.vfio_device else { return };
        self.register_snapshot.clear();

        let offsets: &[usize] = &[
            0x000000, 0x000200, 0x000204, // BOOT0, PMC_ENABLE, PMC_DEV_ENABLE
            0x002004, 0x002100, 0x002200, // PFIFO
            0x100000, 0x100800, 0x100C80, // PFB, FBHUB, PFB_NISO
            0x10A000, 0x10A040, 0x10A044, // PMU FALCON
            0x137000, 0x137050, 0x137100, // PCLOCK, NVPLL, MEMPLL
            0x9A0000, 0x17E200, 0x300000, // FBPA0, LTC0, PROM
        ];

        for &off in offsets {
            if let Ok(val) = dev.bar0.read_u32(off) {
                self.register_snapshot.insert(off, val);
            }
        }
        tracing::debug!(bdf = %self.bdf, regs = self.register_snapshot.len(), "snapshot taken");

        if let Some(path) = &self.config.oracle_dump {
            let dump: Vec<String> = self
                .register_snapshot
                .iter()
                .map(|(off, val)| format!("{off:#010x} = {val:#010x}"))
                .collect();
            if let Err(e) = std::fs::write(path, dump.join("\n")) {
                tracing::warn!(path, error = %e, "failed to write oracle dump");
            }
        }
    }

    /// Check device health by probing key registers.
    pub fn check_health(&mut self) {
        self.refresh_power_state();

        tracing::debug!(
            bdf = %self.bdf,
            personality = self.personality.name(),
            has_vfio = self.personality.provides_vfio(),
            hbm2_capable = self.personality.supports_hbm2_training(),
            "checking device health"
        );

        let Some(dev) = self.vfio_device.as_ref() else {
            self.health.vram_alive = false;
            self.health.domains_alive = 0;
            self.health.domains_faulted = 0;
            return;
        };
        let r = |off: usize| dev.bar0.read_u32(off).unwrap_or(PCI_READ_DEAD);

        self.health.boot0 = r(0x000000);
        self.health.pmc_enable = r(0x000200);

        let pramin_val = r(0x700000);
        self.health.vram_alive = !is_faulted_read(pramin_val);

        let domains: &[(usize, &str)] = &[
            (0x000200, "PMC"),
            (0x002004, "PFIFO"),
            (0x100000, "PFB"),
            (0x100800, "FBHUB"),
            (0x10A000, "PMU"),
            (0x17E200, "LTC0"),
            (0x9A0000, "FBPA0"),
            (0x137050, "NVPLL"),
            (0x700000, "PRAMIN"),
        ];

        let mut alive = 0;
        let mut faulted = 0;
        for &(off, _) in domains {
            if is_faulted_read(r(off)) {
                faulted += 1;
            } else {
                alive += 1;
            }
        }
        self.health.domains_alive = alive;
        self.health.domains_faulted = faulted;
    }

    /// Resurrect HBM2 by cycling through nouveau.
    ///
    /// Sequence: snapshot → close VFIO fd → unbind → nouveau bind (HBM2 training)
    /// → wait for init → unbind nouveau → rebind VFIO → verify PRAMIN alive.
    ///
    /// Returns Ok(true) if VRAM came back alive, Ok(false) if resurrection
    /// completed but VRAM is still dead, Err if a step failed.
    pub fn resurrect_hbm2(&mut self) -> Result<bool, String> {
        tracing::info!(bdf = %self.bdf, "HBM2 resurrection starting");

        // Step 1: snapshot current state (even if partially dead)
        self.snapshot_registers();
        let snapshot_count = self.register_snapshot.len();
        tracing::info!(bdf = %self.bdf, regs = snapshot_count, "state vault snapshot saved");

        // Step 2: close VFIO fd (triggers PM reset, but frees the group for unbind)
        drop(self.vfio_device.take());

        // Step 3: pin power to prevent D3 during transition
        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );

        // Step 4: unbind from current driver
        let unbind = format!("/sys/bus/pci/devices/{}/driver/unbind", self.bdf);
        sysfs_write(&unbind, &self.bdf);
        std::thread::sleep(std::time::Duration::from_secs(1));
        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );

        // Step 5a: DRM consumer fence check — verify no active DRM consumers
        // before nouveau bind. Active consumers (from a prior DRM session)
        // would race with nouveau's HBM2 training init.
        if has_active_drm_consumers(&self.bdf) {
            tracing::warn!(
                bdf = %self.bdf,
                "active DRM consumers detected — waiting for fence drain"
            );
            std::thread::sleep(std::time::Duration::from_secs(2));
            if has_active_drm_consumers(&self.bdf) {
                tracing::error!(
                    bdf = %self.bdf,
                    "DRM consumers still active — resurrection may conflict"
                );
            }
        }

        // Step 5b: bind to nouveau — this triggers full HBM2 training
        // CRITICAL: clear driver_override BEFORE bind, otherwise the kernel
        // won't match nouveau because override says "vfio-pci"
        tracing::info!(bdf = %self.bdf, "clearing driver_override and binding nouveau...");
        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/driver_override", self.bdf),
            "\n",
        );
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Verify override was cleared
        let override_val =
            std::fs::read_to_string(format!("/sys/bus/pci/devices/{}/driver_override", self.bdf))
                .unwrap_or_default();
        tracing::info!(bdf = %self.bdf, driver_override = ?override_val.trim(), "override after clear");

        sysfs_write("/sys/bus/pci/drivers/nouveau/bind", &self.bdf);

        // Step 6: wait for nouveau to complete init
        // nouveau does: VBIOS parse → PMU → FBPA init → HBM2 PHY training → DRM
        // typically takes 2-5 seconds on GV100
        for attempt in 0..10 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let drv = std::fs::read_link(format!("/sys/bus/pci/devices/{}/driver", self.bdf))
                .ok()
                .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()));

            if drv.as_deref() == Some("nouveau") {
                // Check if DRM is up (indicates full init including HBM2)
                if find_drm_card(&self.bdf).is_some() {
                    tracing::info!(
                        bdf = %self.bdf,
                        attempt,
                        "nouveau init complete (DRM card found)"
                    );
                    break;
                }
            }
            tracing::debug!(bdf = %self.bdf, attempt, driver = ?drv, "waiting for nouveau init...");
        }

        let nouveau_drv = std::fs::read_link(format!("/sys/bus/pci/devices/{}/driver", self.bdf))
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()));

        if nouveau_drv.as_deref() != Some("nouveau") {
            tracing::warn!(bdf = %self.bdf, driver = ?nouveau_drv, "nouveau did not bind — resurrection may fail");
        }

        // Step 7: swap back to VFIO
        tracing::info!(bdf = %self.bdf, "nouveau warm complete, swapping back to vfio-pci...");
        sysfs_write(&unbind, &self.bdf);
        std::thread::sleep(std::time::Duration::from_secs(1));
        sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );

        self.bind_vfio()?;

        // Step 8: verify PRAMIN is alive
        self.check_health();

        let alive = self.health.vram_alive;
        if alive {
            tracing::info!(
                bdf = %self.bdf,
                domains_alive = self.health.domains_alive,
                boot0 = format_args!("{:#010x}", self.health.boot0),
                pmc = format_args!("{:#010x}", self.health.pmc_enable),
                "HBM2 RESURRECTED — VRAM alive"
            );
        } else {
            tracing::warn!(
                bdf = %self.bdf,
                domains_alive = self.health.domains_alive,
                domains_faulted = self.health.domains_faulted,
                "HBM2 resurrection failed — VRAM still dead"
            );
        }

        Ok(alive)
    }

    fn refresh_power_state(&mut self) {
        let path = format!("/sys/bus/pci/devices/{}/power_state", self.bdf);
        self.health.power = match std::fs::read_to_string(&path) {
            Ok(s) => match s.trim() {
                "D0" => PowerState::D0,
                "D3hot" => PowerState::D3Hot,
                "D3cold" => PowerState::D3Cold,
                _ => PowerState::Unknown,
            },
            Err(_) => PowerState::Unknown,
        };

        let link_path = format!("/sys/bus/pci/devices/{}/current_link_width", self.bdf);
        self.health.pci_link_width = std::fs::read_to_string(&link_path)
            .ok()
            .and_then(|s| s.trim().parse().ok());
    }
}

// ── Sysfs Helpers ────────────────────────────────────────────────────────

/// Write to a sysfs path using direct filesystem access.
///
/// Requires the process to have `CAP_SYS_ADMIN` capability or appropriate
/// udev rules for the target path. Does NOT fall back to `sudo` — the
/// binary should be deployed with the correct capabilities via systemd
/// `AmbientCapabilities=CAP_SYS_ADMIN` or `setcap`.
///
/// If the write fails, logs the error with guidance on capability setup.
pub(crate) fn sysfs_write(path: &str, value: &str) {
    if let Err(e) = std::fs::write(path, value) {
        tracing::warn!(
            path,
            error = %e,
            "sysfs write failed — ensure CAP_SYS_ADMIN or udev rules grant access"
        );
    }
}

fn read_pci_ids(bdf: &str) -> (u16, u16) {
    let vendor = std::fs::read_to_string(format!("/sys/bus/pci/devices/{bdf}/vendor"))
        .ok()
        .and_then(|s| u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .unwrap_or(0);
    let device = std::fs::read_to_string(format!("/sys/bus/pci/devices/{bdf}/device"))
        .ok()
        .and_then(|s| u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
        .unwrap_or(0);
    (vendor, device)
}

fn read_iommu_group(bdf: &str) -> u32 {
    std::fs::read_link(format!("/sys/bus/pci/devices/{bdf}/iommu_group"))
        .ok()
        .and_then(|p| p.file_name()?.to_str()?.parse().ok())
        .unwrap_or(0)
}

fn identify_chip(vendor: u16, device: u16) -> String {
    match (vendor, device) {
        (0x10de, 0x1d81) => "GV100 (Titan V)".into(),
        (0x10de, 0x1db1) => "GV100GL (V100)".into(),
        (0x10de, 0x2204) => "GA102 (RTX 3090)".into(),
        (0x10de, 0x2d05) => "GB206 (RTX 5060)".into(),
        (0x1002, 0x66a0) => "Vega 20 (MI50)".into(),
        (0x1002, 0x66a1) => "Vega 20 (MI60)".into(),
        (v, d) => format!("{v:#06x}:{d:#06x}"),
    }
}

/// Ensure all devices in the same IOMMU group are bound to vfio-pci.
/// VFIO requires group viability: every device in the group must use vfio-pci.
fn bind_iommu_group_to_vfio(primary_bdf: &str, group_id: u32) {
    let group_path = format!("/sys/kernel/iommu_groups/{group_id}/devices");
    let Ok(entries) = std::fs::read_dir(&group_path) else {
        return;
    };

    for entry in entries.flatten() {
        let peer_bdf = entry.file_name().to_string_lossy().to_string();
        if peer_bdf == primary_bdf {
            continue;
        }

        let driver = std::fs::read_link(format!("/sys/bus/pci/devices/{peer_bdf}/driver"))
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()));

        if driver.as_deref() == Some("vfio-pci") {
            continue;
        }

        tracing::info!(
            peer = %peer_bdf,
            driver = driver.as_deref().unwrap_or("none"),
            group = group_id,
            "binding IOMMU group peer to vfio-pci"
        );

        // Unbind from current driver
        if driver.is_some() {
            sysfs_write(
                &format!("/sys/bus/pci/devices/{peer_bdf}/driver/unbind"),
                &peer_bdf,
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
        }

        // Override and bind to vfio-pci
        sysfs_write(
            &format!("/sys/bus/pci/devices/{peer_bdf}/driver_override"),
            "vfio-pci",
        );
        sysfs_write("/sys/bus/pci/drivers/vfio-pci/bind", &peer_bdf);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Check whether a DRM device has active consumer fds (open file handles).
///
/// Scans `/proc/*/fd` for symlinks pointing to the device's DRM render node.
/// Returns `true` if any non-self process holds a DRM fd open.
fn has_active_drm_consumers(bdf: &str) -> bool {
    let drm_dir = format!("/sys/bus/pci/devices/{bdf}/drm");
    let Ok(entries) = std::fs::read_dir(&drm_dir) else {
        return false;
    };

    let drm_paths: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("card") || name.starts_with("renderD") {
                Some(format!("/dev/dri/{name}"))
            } else {
                None
            }
        })
        .collect();

    if drm_paths.is_empty() {
        return false;
    }

    let self_pid = std::process::id();
    let Ok(proc_entries) = std::fs::read_dir("/proc") else {
        return false;
    };

    for entry in proc_entries.flatten() {
        let pid_str = entry.file_name().to_string_lossy().to_string();
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        if pid == self_pid {
            continue;
        }

        let fd_dir = format!("/proc/{pid}/fd");
        let Ok(fds) = std::fs::read_dir(&fd_dir) else {
            continue;
        };

        for fd_entry in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd_entry.path()) {
                let target_str = target.to_string_lossy();
                if drm_paths.iter().any(|p| target_str.as_ref() == p.as_str()) {
                    tracing::debug!(
                        pid,
                        fd = ?fd_entry.file_name(),
                        target = %target_str,
                        "active DRM consumer found"
                    );
                    return true;
                }
            }
        }
    }

    false
}

fn find_drm_card(bdf: &str) -> Option<String> {
    let drm_dir = format!("/sys/bus/pci/devices/{bdf}/drm");
    let entries = std::fs::read_dir(&drm_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("card") {
            return Some(format!("/dev/dri/{name}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DeviceConfig;

    #[test]
    fn test_is_faulted_read_pci_dead() {
        assert!(is_faulted_read(0xDEAD_DEAD));
    }

    #[test]
    fn test_is_faulted_read_all_ones() {
        assert!(is_faulted_read(0xFFFF_FFFF));
    }

    #[test]
    fn test_is_faulted_read_badf() {
        assert!(is_faulted_read((PCI_FAULT_BADF as u32) << 16));
    }

    #[test]
    fn test_is_faulted_read_bad0() {
        assert!(is_faulted_read((PCI_FAULT_BAD0 as u32) << 16));
    }

    #[test]
    fn test_is_faulted_read_bad1() {
        assert!(is_faulted_read((PCI_FAULT_BAD1 as u32) << 16));
    }

    #[test]
    fn test_is_faulted_read_valid() {
        assert!(!is_faulted_read(0x0000_0000));
        assert!(!is_faulted_read(0x1234_5678));
        assert!(!is_faulted_read(0x0001_0000));
    }

    #[test]
    fn test_pci_constants() {
        assert_eq!(PCI_READ_DEAD, 0xDEAD_DEAD);
        assert_eq!(PCI_READ_ALL_ONES, 0xFFFF_FFFF);
        assert_eq!(PCI_FAULT_BADF, 0xBADF);
        assert_eq!(PCI_FAULT_BAD0, 0xBAD0);
        assert_eq!(PCI_FAULT_BAD1, 0xBAD1);
    }

    #[test]
    fn test_power_state_display() {
        assert_eq!(PowerState::D0.to_string(), "D0");
        assert_eq!(PowerState::D3Hot.to_string(), "D3hot");
        assert_eq!(PowerState::D3Cold.to_string(), "D3cold");
        assert_eq!(PowerState::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_device_health_defaults_in_slot() {
        let config = DeviceConfig {
            bdf: "0000:99:00.0".into(),
            name: None,
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            role: None,
            oracle_dump: None,
        };
        let slot = DeviceSlot::new(config);
        assert!(!slot.health.vram_alive);
        assert_eq!(slot.health.boot0, 0);
        assert_eq!(slot.health.pmc_enable, 0);
        assert_eq!(slot.health.power, PowerState::Unknown);
        assert!(slot.health.pci_link_width.is_none());
        assert_eq!(slot.health.domains_alive, 0);
        assert_eq!(slot.health.domains_faulted, 0);
    }

    #[test]
    fn test_device_slot_new_with_mock_config() {
        let config = DeviceConfig {
            bdf: "0000:99:00.0".into(),
            name: Some("Test GPU".into()),
            boot_personality: "nouveau".into(),
            power_policy: "power_save".into(),
            role: Some("compute".into()),
            oracle_dump: Some("/tmp/dump.txt".into()),
        };
        let slot = DeviceSlot::new(config.clone());
        assert_eq!(slot.bdf, "0000:99:00.0");
        assert_eq!(slot.config.name.as_deref(), Some("Test GPU"));
        assert_eq!(slot.config.boot_personality, "nouveau");
        assert_eq!(slot.config.power_policy, "power_save");
        assert_eq!(slot.config.role.as_deref(), Some("compute"));
        assert_eq!(slot.config.oracle_dump.as_deref(), Some("/tmp/dump.txt"));
        assert_eq!(slot.personality, Personality::Unbound);
        assert!(!slot.has_vfio());
    }

    #[test]
    fn test_identify_chip_known_devices() {
        assert_eq!(identify_chip(0x10de, 0x1d81), "GV100 (Titan V)");
        assert_eq!(identify_chip(0x10de, 0x1db1), "GV100GL (V100)");
        assert_eq!(identify_chip(0x10de, 0x2204), "GA102 (RTX 3090)");
        assert_eq!(identify_chip(0x10de, 0x2d05), "GB206 (RTX 5060)");
        assert_eq!(identify_chip(0x1002, 0x66a0), "Vega 20 (MI50)");
        assert_eq!(identify_chip(0x1002, 0x66a1), "Vega 20 (MI60)");
    }

    #[test]
    fn test_identify_chip_unknown() {
        let name = identify_chip(0x1234, 0x5678);
        assert!(name.contains("0x1234"));
        assert!(name.contains("0x5678"));
    }

    #[test]
    fn test_personality_registry_validation() {
        let registry = crate::personality::PersonalityRegistry::default_linux();
        assert!(registry.supports("vfio"));
        assert!(registry.supports("nouveau"));
        assert!(registry.supports("amdgpu"));
        assert!(registry.supports("unbound"));
        assert!(!registry.supports("nvidia-proprietary"));
        assert!(!registry.supports("unknown"));
    }
}
