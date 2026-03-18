// SPDX-License-Identifier: AGPL-3.0-only
//! `DeviceSlot` — persistent ownership of a `PCIe` device.
//!
//! Each slot manages one GPU/accelerator from boot to shutdown.
//! It tracks the current driver personality, power state, VRAM
//! health, and provides the VFIO fd for ecosystem consumers.

use crate::config::DeviceConfig;
use crate::error::DeviceError;
use crate::personality::{Personality, PersonalityRegistry};
use crate::sysfs;
use std::collections::BTreeMap;

const PCI_READ_DEAD: u32 = 0xDEAD_DEAD;
const PCI_READ_ALL_ONES: u32 = 0xFFFF_FFFF;
const PCI_FAULT_BADF: u16 = 0xBADF;
const PCI_FAULT_BAD0: u16 = 0xBAD0;
const PCI_FAULT_BAD1: u16 = 0xBAD1;

#[must_use]
const fn is_faulted_read(val: u32) -> bool {
    val == PCI_READ_DEAD
        || val == PCI_READ_ALL_ONES
        || (val >> 16) as u16 == PCI_FAULT_BADF
        || (val >> 16) as u16 == PCI_FAULT_BAD0
        || (val >> 16) as u16 == PCI_FAULT_BAD1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        let (vendor_id, device_id) = sysfs::read_pci_ids(&bdf);
        let chip_name = sysfs::identify_chip(vendor_id, device_id);

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

    #[must_use]
    pub const fn has_vfio(&self) -> bool {
        self.vfio_device.is_some()
    }

    /// Bind device to the configured boot personality and take ownership.
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::UnknownPersonality` if the configured boot personality
    /// is not supported. Returns `DeviceError::VfioOpen` or propagates driver bind
    /// errors when binding fails.
    pub fn activate(&mut self) -> Result<(), DeviceError> {
        let target = self.config.boot_personality.clone();
        let registry = PersonalityRegistry::default_linux();
        if !registry.supports(&target) {
            return Err(DeviceError::UnknownPersonality {
                bdf: self.bdf.clone(),
                personality: target,
                known: registry.list().to_vec(),
            });
        }
        tracing::info!(bdf = %self.bdf, personality = %target, "activating device");

        self.refresh_power_state();

        // Check current driver — if already correct, skip rebind
        let current_driver = sysfs::read_current_driver(&self.bdf);

        let trait_personality = registry.create(&target);
        let expected_module = trait_personality
            .as_ref()
            .map(|p| (p.name(), p.driver_module().to_owned()));
        let needs_rebind = expected_module
            .as_ref()
            .is_some_and(|(_, module)| current_driver.as_deref() != Some(module.as_str()));

        if let Some(ref p) = trait_personality {
            tracing::debug!(
                has_vfio = p.provides_vfio(),
                drm_card = ?p.drm_card(),
                hbm2_training = p.supports_hbm2_training(),
                "personality capabilities"
            );
        }

        if needs_rebind {
            if sysfs::has_active_drm_consumers(&self.bdf) {
                tracing::error!(
                    bdf = %self.bdf,
                    current = current_driver.as_deref().unwrap_or("<none>"),
                    target = %target,
                    "REFUSING to unbind — active DRM consumers detected. \
                     Unbinding a driver with active display/render clients causes kernel panic."
                );
                return Err(DeviceError::ActiveDrmConsumers {
                    bdf: self.bdf.clone(),
                });
            }

            if current_driver.is_some() {
                tracing::info!(
                    bdf = %self.bdf,
                    current = current_driver.as_deref().unwrap_or("<none>"),
                    target = %target,
                    "unbinding current driver before activation"
                );
                sysfs::sysfs_write(
                    &format!("/sys/bus/pci/devices/{}/driver/unbind", self.bdf),
                    &self.bdf,
                );
                std::thread::sleep(std::time::Duration::from_millis(500));
                sysfs::sysfs_write(
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
                    let drm = sysfs::find_drm_card(&self.bdf);
                    self.personality = Personality::Nouveau { drm_card: drm };
                }
            }
            "amdgpu" => {
                if needs_rebind {
                    self.bind_driver("amdgpu")?;
                } else {
                    let drm = sysfs::find_drm_card(&self.bdf);
                    self.personality = Personality::Amdgpu { drm_card: drm };
                }
            }
            other => {
                return Err(DeviceError::UnknownPersonality {
                    bdf: self.bdf.clone(),
                    personality: other.to_string(),
                    known: registry.list().to_vec(),
                });
            }
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
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::UnknownPersonality` if the target personality is not
    /// supported. Propagates `DeviceError::VfioOpen` or driver bind errors when
    /// binding fails.
    pub fn swap(&mut self, target: &str) -> Result<(), DeviceError> {
        if std::path::Path::new("/sys/module/nvidia").exists() {
            tracing::error!(
                bdf = %self.bdf,
                "REFUSING swap — nvidia kernel modules are loaded. \
                 Driver swaps with nvidia loaded corrupt device state and panic the kernel."
            );
            return Err(DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: target.into(),
                reason: "nvidia kernel modules loaded — driver swap would panic the kernel".into(),
            });
        }

        let registry = PersonalityRegistry::default_linux();
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
            other => {
                return Err(DeviceError::UnknownPersonality {
                    bdf: self.bdf.clone(),
                    personality: other.to_string(),
                    known: registry.list().to_vec(),
                });
            }
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
    fn release(&mut self) -> Result<(), DeviceError> {
        if sysfs::has_active_drm_consumers(&self.bdf) {
            tracing::error!(
                bdf = %self.bdf,
                personality = %self.personality,
                "REFUSING to release — active DRM consumers detected. \
                 Unbinding a driver with active display/render clients causes kernel panic."
            );
            return Err(DeviceError::ActiveDrmConsumers {
                bdf: self.bdf.clone(),
            });
        }

        // Drop VFIO device — this closes the fd and triggers PM reset,
        // but frees the VFIO group so unbind can proceed.
        drop(self.vfio_device.take());

        // Pin power before unbind to prevent D3 transition
        sysfs::sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );

        // Unbind from current driver
        let drv_path = format!("/sys/bus/pci/devices/{}/driver/unbind", self.bdf);
        sysfs::sysfs_write(&drv_path, &self.bdf);
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Keep power pinned
        sysfs::sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );
        self.personality = Personality::Unbound;
        Ok(())
    }

    fn bind_vfio(&mut self) -> Result<(), DeviceError> {
        let group_id = sysfs::read_iommu_group(&self.bdf);

        // Bind all devices in the same IOMMU group to vfio-pci
        // (required for group viability — e.g. audio companion device)
        sysfs::bind_iommu_group_to_vfio(&self.bdf, group_id);

        // Set driver override for the primary device
        sysfs::sysfs_write(
            &format!("/sys/bus/pci/devices/{}/driver_override", self.bdf),
            "vfio-pci",
        );

        // Bind
        sysfs::sysfs_write("/sys/bus/pci/drivers/vfio-pci/bind", &self.bdf);
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Pin D0
        sysfs::sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );
        sysfs::sysfs_write(
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
                Err(DeviceError::VfioOpen {
                    bdf: self.bdf.clone(),
                    reason: e.to_string(),
                })
            }
        }
    }

    fn bind_driver(&mut self, driver: &str) -> Result<(), DeviceError> {
        // Write newline to clear driver_override (empty string doesn't work via tee)
        sysfs::sysfs_write(
            &format!("/sys/bus/pci/devices/{}/driver_override", self.bdf),
            "\n",
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
        sysfs::sysfs_write(&format!("/sys/bus/pci/drivers/{driver}/bind"), &self.bdf);
        std::thread::sleep(std::time::Duration::from_secs(3));

        sysfs::sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );

        let drm_card = sysfs::find_drm_card(&self.bdf);
        self.personality = match driver {
            "nouveau" => Personality::Nouveau { drm_card },
            "amdgpu" => Personality::Amdgpu { drm_card },
            _ => Personality::Unbound,
        };
        Ok(())
    }

    /// Lend the VFIO fd to an external consumer (e.g. a hardware test).
    ///
    /// Drops the internal VFIO fd so another process can open the VFIO group,
    /// but keeps `vfio-pci` bound so the consumer doesn't need to rebind.
    /// Returns the IOMMU group id for the consumer to open `/dev/vfio/{group}`.
    ///
    /// The device transitions to a "lent" state where health checks are
    /// suspended (no VFIO fd to probe). Call [`reclaim`](Self::reclaim) after
    /// the consumer finishes.
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::DriverBind` if the device is not currently in
    /// VFIO personality (nothing to lend).
    pub fn lend(&mut self) -> Result<u32, DeviceError> {
        let group_id = match self.personality {
            Personality::Vfio { group_id } => group_id,
            _ => {
                return Err(DeviceError::DriverBind {
                    bdf: self.bdf.clone(),
                    driver: "vfio-pci".into(),
                    reason: format!("device is {}, not VFIO — nothing to lend", self.personality),
                });
            }
        };

        if self.vfio_device.is_none() {
            return Err(DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: "vfio-pci".into(),
                reason: "VFIO fd already lent or not open".into(),
            });
        }

        self.snapshot_registers();
        drop(self.vfio_device.take());
        tracing::info!(bdf = %self.bdf, group_id, "VFIO fd lent — group available for external consumer");

        Ok(group_id)
    }

    /// Reclaim a previously lent VFIO fd.
    ///
    /// Re-opens the VFIO group fd and verifies device health.
    /// Must be called after the external consumer has dropped its fd.
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::VfioOpen` if the VFIO group fd cannot be
    /// re-opened (e.g. the external consumer still holds it).
    pub fn reclaim(&mut self) -> Result<(), DeviceError> {
        let group_id = match self.personality {
            Personality::Vfio { group_id } => group_id,
            _ => {
                return Err(DeviceError::DriverBind {
                    bdf: self.bdf.clone(),
                    driver: "vfio-pci".into(),
                    reason: format!(
                        "device is {}, not VFIO — nothing to reclaim",
                        self.personality
                    ),
                });
            }
        };

        if self.vfio_device.is_some() {
            tracing::warn!(bdf = %self.bdf, "VFIO fd already held — reclaim is a no-op");
            return Ok(());
        }

        match coral_driver::nv::RawVfioDevice::open(&self.bdf) {
            Ok(dev) => {
                self.vfio_device = Some(dev);
                self.check_health();
                tracing::info!(
                    bdf = %self.bdf,
                    group_id,
                    vram = self.health.vram_alive,
                    "VFIO fd reclaimed"
                );
                Ok(())
            }
            Err(e) => Err(DeviceError::VfioOpen {
                bdf: self.bdf.clone(),
                reason: e.to_string(),
            }),
        }
    }

    /// Take a snapshot of key registers (for state preservation across swaps).
    pub fn snapshot_registers(&mut self) {
        let Some(dev) = &self.vfio_device else { return };
        self.register_snapshot.clear();

        let offsets: &[usize] = &[
            0x00_0000, 0x00_0200, 0x00_0204, // BOOT0, PMC_ENABLE, PMC_DEV_ENABLE
            0x00_2004, 0x00_2100, 0x00_2200, // PFIFO
            0x10_0000, 0x10_0800, 0x10_0C80, // PFB, FBHUB, PFB_NISO
            0x10_A000, 0x10_A040, 0x10_A044, // PMU FALCON
            0x13_7000, 0x13_7050, 0x13_7100, // PCLOCK, NVPLL, MEMPLL
            0x9A_0000, 0x17_E200, 0x30_0000, // FBPA0, LTC0, PROM
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

        self.health.boot0 = r(0x00_0000);
        self.health.pmc_enable = r(0x00_0200);

        let pramin_val = r(0x70_0000);
        self.health.vram_alive = !is_faulted_read(pramin_val);

        let domains: &[(usize, &str)] = &[
            (0x00_0200, "PMC"),
            (0x00_2004, "PFIFO"),
            (0x10_0000, "PFB"),
            (0x10_0800, "FBHUB"),
            (0x10_A000, "PMU"),
            (0x17_E200, "LTC0"),
            (0x9A_0000, "FBPA0"),
            (0x13_7050, "NVPLL"),
            (0x70_0000, "PRAMIN"),
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
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::VfioOpen` if rebinding to VFIO after the nouveau
    /// HBM2 training cycle fails.
    pub fn resurrect_hbm2(&mut self) -> Result<bool, DeviceError> {
        if std::path::Path::new("/sys/module/nvidia").exists() {
            tracing::error!(
                bdf = %self.bdf,
                "REFUSING HBM2 resurrection — nvidia kernel modules are loaded. \
                 They corrupt GV100 device state during probe and make driver swaps \
                 unsafe (kernel panic). Unload nvidia modules first."
            );
            return Err(DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: "nouveau".into(),
                reason: "nvidia kernel modules loaded — driver swap would panic the kernel".into(),
            });
        }

        tracing::info!(bdf = %self.bdf, "HBM2 resurrection starting");

        // Step 1: snapshot current state (even if partially dead)
        self.snapshot_registers();
        let snapshot_count = self.register_snapshot.len();
        tracing::info!(bdf = %self.bdf, regs = snapshot_count, "state vault snapshot saved");

        // Step 2: close VFIO fd (triggers PM reset, but frees the group for unbind)
        drop(self.vfio_device.take());

        // Step 3: pin power to prevent D3 during transition
        sysfs::sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );

        // Step 4: unbind from current driver
        let unbind = format!("/sys/bus/pci/devices/{}/driver/unbind", self.bdf);
        sysfs::sysfs_write(&unbind, &self.bdf);
        std::thread::sleep(std::time::Duration::from_secs(1));
        sysfs::sysfs_write(
            &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
            "on",
        );

        // Step 5a: DRM consumer fence check — verify no active DRM consumers
        // before nouveau bind. Active consumers (from a prior DRM session)
        // would race with nouveau's HBM2 training init.
        if sysfs::has_active_drm_consumers(&self.bdf) {
            tracing::warn!(
                bdf = %self.bdf,
                "active DRM consumers detected — waiting for fence drain"
            );
            std::thread::sleep(std::time::Duration::from_secs(2));
            if sysfs::has_active_drm_consumers(&self.bdf) {
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
        sysfs::sysfs_write(
            &format!("/sys/bus/pci/devices/{}/driver_override", self.bdf),
            "\n",
        );
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Verify override was cleared
        let override_val =
            std::fs::read_to_string(format!("/sys/bus/pci/devices/{}/driver_override", self.bdf))
                .unwrap_or_default();
        tracing::info!(bdf = %self.bdf, driver_override = ?override_val.trim(), "override after clear");

        sysfs::sysfs_write("/sys/bus/pci/drivers/nouveau/bind", &self.bdf);

        // Step 6: wait for nouveau to complete init
        // nouveau does: VBIOS parse → PMU → FBPA init → HBM2 PHY training → DRM
        // typically takes 2-5 seconds on GV100
        for attempt in 0..10 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let drv = sysfs::read_current_driver(&self.bdf);

            if drv.as_deref() == Some("nouveau") {
                // Check if DRM is up (indicates full init including HBM2)
                if sysfs::find_drm_card(&self.bdf).is_some() {
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

        let nouveau_drv = sysfs::read_current_driver(&self.bdf);

        if nouveau_drv.as_deref() != Some("nouveau") {
            tracing::warn!(bdf = %self.bdf, driver = ?nouveau_drv, "nouveau did not bind — resurrection may fail");
        }

        // Step 7: swap back to VFIO
        tracing::info!(bdf = %self.bdf, "nouveau warm complete, swapping back to vfio-pci...");
        sysfs::sysfs_write(&unbind, &self.bdf);
        std::thread::sleep(std::time::Duration::from_secs(1));
        sysfs::sysfs_write(
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

    pub(crate) fn refresh_power_state(&mut self) {
        self.health.power = sysfs::read_power_state(&self.bdf);
        self.health.pci_link_width = sysfs::read_link_width(&self.bdf);
    }
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
    fn test_device_slot_has_vfio_initially_false() {
        let config = DeviceConfig {
            bdf: "0000:99:00.0".into(),
            name: None,
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            role: None,
            oracle_dump: None,
        };
        let slot = DeviceSlot::new(config);
        assert!(!slot.has_vfio());
    }

    #[test]
    fn test_device_health_struct() {
        let health = DeviceHealth {
            vram_alive: true,
            boot0: 0x1234_5678,
            pmc_enable: 0x9abc_def0,
            power: PowerState::D0,
            pci_link_width: Some(16),
            domains_alive: 9,
            domains_faulted: 0,
        };
        assert!(health.vram_alive);
        assert_eq!(health.boot0, 0x1234_5678);
        assert_eq!(health.pmc_enable, 0x9abc_def0);
        assert_eq!(health.power, PowerState::D0);
        assert_eq!(health.pci_link_width, Some(16));
        assert_eq!(health.domains_alive, 9);
        assert_eq!(health.domains_faulted, 0);
    }

    #[test]
    fn test_power_state_equality() {
        assert_eq!(PowerState::D0, PowerState::D0);
        assert_ne!(PowerState::D0, PowerState::D3Hot);
    }

    #[test]
    fn test_activate_nonexistent_bdf_with_drm_check_does_not_panic() {
        let config = DeviceConfig {
            bdf: "0000:ff:00.0".into(),
            name: None,
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            role: None,
            oracle_dump: None,
        };
        let mut slot = DeviceSlot::new(config);
        // activate on a nonexistent device won't have DRM consumers,
        // so it should proceed (and fail at the bind stage, not the guard)
        let result = slot.activate();
        // Either succeeds (unlikely) or fails at bind — but must NOT panic
        drop(result);
    }

    #[test]
    fn test_release_nonexistent_bdf_does_not_panic() {
        let config = DeviceConfig {
            bdf: "0000:ff:00.0".into(),
            name: None,
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            role: None,
            oracle_dump: None,
        };
        let mut slot = DeviceSlot::new(config);
        let result = slot.release();
        assert!(
            result.is_ok(),
            "release on nonexistent should succeed (no DRM consumers)"
        );
        assert_eq!(slot.personality, Personality::Unbound);
    }

    #[test]
    fn test_swap_nonexistent_bdf_does_not_panic() {
        let config = DeviceConfig {
            bdf: "0000:ff:00.0".into(),
            name: None,
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            role: None,
            oracle_dump: None,
        };
        let mut slot = DeviceSlot::new(config);
        let result = slot.swap("nouveau");
        // Should not panic — guard passes (no DRM), bind may fail
        drop(result);
    }

    #[test]
    fn test_lend_requires_vfio_personality() {
        let config = DeviceConfig {
            bdf: "0000:ff:00.0".into(),
            name: None,
            boot_personality: "nouveau".into(),
            power_policy: "always_on".into(),
            role: None,
            oracle_dump: None,
        };
        let mut slot = DeviceSlot::new(config);
        slot.personality = Personality::Nouveau { drm_card: None };
        let result = slot.lend();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not VFIO"));
    }

    #[test]
    fn test_lend_returns_error_when_no_fd() {
        let config = DeviceConfig {
            bdf: "0000:ff:00.0".into(),
            name: None,
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            role: None,
            oracle_dump: None,
        };
        let mut slot = DeviceSlot::new(config);
        slot.personality = Personality::Vfio { group_id: 42 };
        // No vfio_device set — lend should fail
        let result = slot.lend();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already lent"));
    }

    #[test]
    fn test_reclaim_requires_vfio_personality() {
        let config = DeviceConfig {
            bdf: "0000:ff:00.0".into(),
            name: None,
            boot_personality: "nouveau".into(),
            power_policy: "always_on".into(),
            role: None,
            oracle_dump: None,
        };
        let mut slot = DeviceSlot::new(config);
        slot.personality = Personality::Unbound;
        let result = slot.reclaim();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not VFIO"));
    }

    #[test]
    fn test_active_drm_consumers_error_display() {
        let err = super::DeviceError::ActiveDrmConsumers {
            bdf: "0000:03:00.0".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("active DRM consumers"));
        assert!(msg.contains("0000:03:00.0"));
        assert!(msg.contains("crash the kernel"));
    }
}
