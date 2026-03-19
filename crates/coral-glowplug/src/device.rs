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
use std::os::fd::OwnedFd;

/// Holds a `VfioDevice` and its BAR0 mapping for register access.
///
/// Replaces direct `RawVfioDevice` usage — glowplug only needs BAR0
/// register reads, not DMA buffers or compute dispatch.
pub(crate) struct VfioHolder {
    #[expect(dead_code, reason = "kept alive for VFIO fd lifecycle")]
    device: coral_driver::vfio::VfioDevice,
    bar0: coral_driver::vfio::device::MappedBar,
}

/// Comprehensive BAR0 register offsets for NVIDIA GV100 (Titan V / V100).
///
/// Covers PMC, PBUS, PFIFO, PBDMA, PFB, FBHUB, PMU, PCLOCK, GR/FECS/GPCCS,
/// LTC, FBPA, PRAMIN, and thermal domains.
pub const DEFAULT_REGISTER_DUMP_OFFSETS: &[usize] = &[
    // PMC
    0x00_0000, 0x00_0004, 0x00_0200, 0x00_0204,
    // PBUS
    0x00_1C00, 0x00_1C04,
    // PFIFO
    0x00_2004, 0x00_2100, 0x00_2140, 0x00_2200, 0x00_2254,
    0x00_2270, 0x00_2274, 0x00_2280, 0x00_2284, 0x00_228C,
    0x00_2390, 0x00_2394, 0x00_2398, 0x00_239C, 0x00_2504,
    0x00_2508, 0x00_252C, 0x00_2630, 0x00_2634, 0x00_2638,
    0x00_2640, 0x00_2A00, 0x00_2A04,
    // PBDMA idle + PBDMA0
    0x00_3080, 0x00_3084, 0x00_3088, 0x00_308C,
    0x04_0040, 0x04_0044, 0x04_0048, 0x04_004C,
    0x04_0054, 0x04_0060, 0x04_0068, 0x04_0080,
    0x04_0084, 0x04_00A4, 0x04_0100, 0x04_0104,
    0x04_0108, 0x04_010C, 0x04_0110, 0x04_0114, 0x04_0118,
    // PFB / FBHUB
    0x10_0000, 0x10_0200, 0x10_0204, 0x10_0C80, 0x10_0C84,
    0x10_0800, 0x10_0804, 0x10_0808, 0x10_080C, 0x10_0810,
    // BAR1 / BAR2 PRAMIN
    0x10_1000, 0x10_1004, 0x10_1008, 0x10_1714,
    // PMU Falcon
    0x10_A000, 0x10_A040, 0x10_A044, 0x10_A04C,
    0x10_A100, 0x10_A104, 0x10_A108, 0x10_A110,
    0x10_A114, 0x10_A118,
    // PCLOCK
    0x13_7000, 0x13_7050, 0x13_7100,
    // GR (graphics engine)
    0x40_0100, 0x40_0108, 0x40_0110,
    // FECS Falcon
    0x40_9028, 0x40_9030, 0x40_9034, 0x40_9038,
    0x40_9040, 0x40_9044, 0x40_904C, 0x40_9080,
    0x40_9084, 0x40_9100, 0x40_9104, 0x40_9108,
    0x40_9110, 0x40_9210, 0x40_9380,
    // GPCCS Falcon
    0x41_A028, 0x41_A030, 0x41_A034, 0x41_A038,
    0x41_A040, 0x41_A044, 0x41_A04C, 0x41_A080,
    0x41_A084, 0x41_A100, 0x41_A108,
    // MMU Fault buffer
    0x10_0E24, 0x10_0E28, 0x10_0E2C, 0x10_0E30,
    // LTC (L2 cache)
    0x17_E200, 0x17_E204, 0x17_E210,
    // FBPA0
    0x9A_0000, 0x9A_0004, 0x9A_0200,
    // THERM
    0x02_0400, 0x02_0460,
    // NV_PRAMIN window
    0x70_0000, 0x70_0004,
    // PROM
    0x30_0000, 0x30_0004,
];

/// GPU quiescence timeout for pre-swap drain.
const QUIESCENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// Polling interval during quiescence wait.
const QUIESCENCE_POLL_MS: u64 = 50;

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
    vfio_holder: Option<VfioHolder>,
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
            vfio_holder: None,
            register_snapshot: BTreeMap::new(),
        }
    }

    #[must_use]
    pub const fn has_vfio(&self) -> bool {
        self.vfio_holder.is_some()
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

            // Delegate driver swap to ember — all sysfs unbind/bind happens there
            let client = crate::ember::EmberClient::connect();

            #[cfg(not(feature = "no-ember"))]
            let client = client.ok_or_else(|| DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: target.clone(),
                reason: "ember not available — driver swap requires ember (enable 'no-ember' feature for legacy sysfs fallback)".into(),
            })?;

            #[cfg(not(feature = "no-ember"))]
            {
                tracing::info!(
                    bdf = %self.bdf,
                    current = current_driver.as_deref().unwrap_or("<none>"),
                    target = %target,
                    "delegating activation rebind to ember"
                );
                client.swap_device(&self.bdf, &target).map_err(|e| {
                    DeviceError::DriverBind {
                        bdf: self.bdf.clone(),
                        driver: target.clone(),
                        reason: format!("ember swap during activation: {e}"),
                    }
                })?;
            }

            #[cfg(feature = "no-ember")]
            if let Some(client) = client {
                tracing::info!(
                    bdf = %self.bdf,
                    current = current_driver.as_deref().unwrap_or("<none>"),
                    target = %target,
                    "delegating activation rebind to ember"
                );
                client.swap_device(&self.bdf, &target).map_err(|e| {
                    DeviceError::DriverBind {
                        bdf: self.bdf.clone(),
                        driver: target.clone(),
                        reason: format!("ember swap during activation: {e}"),
                    }
                })?;
            } else {
                tracing::warn!(
                    bdf = %self.bdf,
                    "ember not available — using legacy direct sysfs activation (no-ember mode)"
                );
                if current_driver.is_some() {
                    let _ = sysfs::sysfs_write(
                        &format!("/sys/bus/pci/devices/{}/driver/unbind", self.bdf),
                        &self.bdf,
                    );
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let _ = sysfs::sysfs_write(
                        &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
                        "on",
                    );
                }
            }
        }

        match target.as_str() {
            "vfio" => self.bind_vfio()?,
            "nouveau" | "nvidia" | "amdgpu" => self.bind_driver(&target)?,
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

    /// Hot-swap to a new driver personality via ember.
    ///
    /// Delegates all sysfs `driver/unbind` and `drivers/*/bind` operations to
    /// the immortal ember process. Glowplug only drops its local VFIO fds and
    /// updates personality state after ember confirms the swap.
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::DriverBind` if ember is not available or the swap
    /// fails. Returns `DeviceError::VfioOpen` if post-swap fd acquisition fails.
    pub fn swap(&mut self, target: &str) -> Result<(), DeviceError> {
        if crate::sysfs::read_current_driver(&self.bdf).as_deref() == Some("nvidia") {
            tracing::error!(
                bdf = %self.bdf,
                "REFUSING swap — nvidia is bound to this device. \
                 Unbind nvidia from this BDF before swapping."
            );
            return Err(DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: target.into(),
                reason: "nvidia is bound to this device — unbind before swapping".into(),
            });
        }

        let registry = PersonalityRegistry::default_linux();
        tracing::info!(bdf = %self.bdf, from = %self.personality, to = %target, "swapping personality");

        if self.has_vfio() && !self.wait_quiescence(QUIESCENCE_TIMEOUT) {
            tracing::warn!(
                bdf = %self.bdf,
                "proceeding with swap despite quiescence timeout — GPU may have in-flight work"
            );
        }

        if self.has_vfio() {
            self.snapshot_registers();
        }

        // Drop local VFIO holder before asking ember to swap
        drop(self.vfio_holder.take());

        // Delegate the entire driver swap to ember
        let client = crate::ember::EmberClient::connect().ok_or_else(|| {
            DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: target.into(),
                reason: "ember not available — driver swap requires ember for safe transition"
                    .into(),
            }
        })?;

        client
            .swap_device(&self.bdf, target)
            .map_err(|e| DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: target.into(),
                reason: format!("ember swap_device: {e}"),
            })?;

        // Update local personality state after successful ember swap
        match target {
            "vfio" | "vfio-pci" => {
                let group_id = sysfs::read_iommu_group(&self.bdf);
                match client.request_fds(&self.bdf) {
                    Ok(fds) => {
                        let device = coral_driver::vfio::VfioDevice::from_received_fds(
                            &self.bdf,
                            fds.container,
                            fds.group,
                            fds.device,
                        )
                        .map_err(|e| DeviceError::VfioOpen {
                            bdf: self.bdf.clone(),
                            reason: format!("ember fds after swap: {e}"),
                        })?;
                        let bar0 = device.map_bar(0).map_err(|e| DeviceError::VfioOpen {
                            bdf: self.bdf.clone(),
                            reason: format!("BAR0 map after swap: {e}"),
                        })?;
                        self.vfio_holder = Some(VfioHolder { device, bar0 });
                        self.personality = Personality::Vfio { group_id };
                    }
                    Err(e) => {
                        return Err(DeviceError::VfioOpen {
                            bdf: self.bdf.clone(),
                            reason: format!("ember fds after swap: {e}"),
                        });
                    }
                }
            }
            "nouveau" => {
                let drm = sysfs::find_drm_card(&self.bdf);
                self.personality = Personality::Nouveau { drm_card: drm };
            }
            "nvidia" => {
                let drm = sysfs::find_drm_card(&self.bdf);
                self.personality = Personality::Nvidia { drm_card: drm };
            }
            "amdgpu" => {
                let drm = sysfs::find_drm_card(&self.bdf);
                self.personality = Personality::Amdgpu { drm_card: drm };
            }
            "unbound" => {
                self.personality = Personality::Unbound;
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
            vram = self.health.vram_alive,
            "swap complete"
        );

        Ok(())
    }

    /// Release local VFIO holder only — no sysfs writes.
    ///
    /// All sysfs `driver/unbind` operations are delegated to ember via
    /// `swap_device` RPC. This method only drops the dup'd VFIO fds held
    /// locally by glowplug.
    #[allow(dead_code, reason = "used in tests and available for manual teardown")]
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

        drop(self.vfio_holder.take());
        self.personality = Personality::Unbound;
        Ok(())
    }

    /// Acquire VFIO fds for this device.
    ///
    /// Primary path: get dup'd fds from ember via `SCM_RIGHTS`.
    /// With `no-ember` feature: falls back to direct `VfioDevice::open` with legacy sysfs bind.
    fn bind_vfio(&mut self) -> Result<(), DeviceError> {
        let group_id = sysfs::read_iommu_group(&self.bdf);

        if let Some(client) = crate::ember::EmberClient::connect() {
            match client.request_fds(&self.bdf) {
                Ok(fds) => {
                    let device = coral_driver::vfio::VfioDevice::from_received_fds(
                        &self.bdf,
                        fds.container,
                        fds.group,
                        fds.device,
                    )
                    .map_err(|e| DeviceError::VfioOpen {
                        bdf: self.bdf.clone(),
                        reason: format!("ember fds: {e}"),
                    })?;
                    let bar0 = device.map_bar(0).map_err(|e| DeviceError::VfioOpen {
                        bdf: self.bdf.clone(),
                        reason: format!("BAR0 map from ember: {e}"),
                    })?;
                    self.vfio_holder = Some(VfioHolder { device, bar0 });
                    self.personality = Personality::Vfio { group_id };
                    tracing::info!(bdf = %self.bdf, "VFIO fds acquired from ember");
                    return Ok(());
                }
                Err(e) => {
                    #[cfg(not(feature = "no-ember"))]
                    return Err(DeviceError::VfioOpen {
                        bdf: self.bdf.clone(),
                        reason: format!(
                            "ember fds failed: {e} (enable 'no-ember' feature for legacy fallback)"
                        ),
                    });

                    #[cfg(feature = "no-ember")]
                    tracing::warn!(
                        bdf = %self.bdf, error = %e,
                        "ember fds unavailable, falling back to direct open (no-ember mode)"
                    );
                }
            }
        }

        #[cfg(not(feature = "no-ember"))]
        return Err(DeviceError::VfioOpen {
            bdf: self.bdf.clone(),
            reason: "ember not available — VFIO bind requires ember (enable 'no-ember' feature for legacy fallback)".into(),
        });

        #[cfg(feature = "no-ember")]
        {
            tracing::warn!(
                bdf = %self.bdf,
                "legacy VFIO bind without ember (no-ember mode)"
            );
            sysfs::bind_iommu_group_to_vfio(&self.bdf, group_id);
            let _ = sysfs::sysfs_write(
                &format!("/sys/bus/pci/devices/{}/driver_override", self.bdf),
                "vfio-pci",
            );
            let _ = sysfs::sysfs_write("/sys/bus/pci/drivers/vfio-pci/bind", &self.bdf);
            std::thread::sleep(std::time::Duration::from_millis(500));
            let _ = sysfs::sysfs_write(
                &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
                "on",
            );
            let _ = sysfs::sysfs_write(
                &format!("/sys/bus/pci/devices/{}/d3cold_allowed", self.bdf),
                "0",
            );

            match coral_driver::vfio::VfioDevice::open(&self.bdf) {
                Ok(device) => {
                    let bar0 = device.map_bar(0).map_err(|e| DeviceError::VfioOpen {
                        bdf: self.bdf.clone(),
                        reason: format!("BAR0 map: {e}"),
                    })?;
                    self.vfio_holder = Some(VfioHolder { device, bar0 });
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
    }

    /// Update personality state after a driver bind.
    ///
    /// Checks if the driver is already bound (ember did it via `swap_device`).
    /// With `no-ember` feature: falls back to legacy sysfs bind when the driver is not yet active.
    #[allow(clippy::unnecessary_wraps)]
    fn bind_driver(&mut self, driver: &str) -> Result<(), DeviceError> {
        let current = sysfs::read_current_driver(&self.bdf);
        if current.as_deref() != Some(driver) {
            #[cfg(not(feature = "no-ember"))]
            {
                tracing::warn!(
                    bdf = %self.bdf,
                    driver,
                    current = ?current,
                    "expected ember to have already bound {driver} — driver mismatch"
                );
            }

            #[cfg(feature = "no-ember")]
            {
                tracing::warn!(
                    bdf = %self.bdf,
                    driver,
                    current = ?current,
                    "legacy sysfs bind (no-ember mode)"
                );
                let _ = sysfs::sysfs_write(
                    &format!("/sys/bus/pci/devices/{}/driver_override", self.bdf),
                    "\n",
                );
                std::thread::sleep(std::time::Duration::from_millis(200));
                let _ = sysfs::sysfs_write(
                    &format!("/sys/bus/pci/drivers/{driver}/bind"),
                    &self.bdf,
                );
                std::thread::sleep(std::time::Duration::from_secs(3));
                let _ = sysfs::sysfs_write(
                    &format!("/sys/bus/pci/devices/{}/power/control", self.bdf),
                    "on",
                );
            }
        }

        let drm_card = sysfs::find_drm_card(&self.bdf);
        self.personality = match driver {
            "nouveau" => Personality::Nouveau { drm_card },
            "nvidia" => Personality::Nvidia { drm_card },
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
        let Personality::Vfio { group_id } = self.personality else {
            return Err(DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: "vfio-pci".into(),
                reason: format!("device is {}, not VFIO — nothing to lend", self.personality),
            });
        };

        if self.vfio_holder.is_none() {
            return Err(DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: "vfio-pci".into(),
                reason: "VFIO fd already lent or not open".into(),
            });
        }

        self.snapshot_registers();
        drop(self.vfio_holder.take());
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
        let Personality::Vfio { group_id } = self.personality else {
            return Err(DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: "vfio-pci".into(),
                reason: format!(
                    "device is {}, not VFIO — nothing to reclaim",
                    self.personality
                ),
            });
        };

        if self.vfio_holder.is_some() {
            tracing::warn!(bdf = %self.bdf, "VFIO fd already held — reclaim is a no-op");
            return Ok(());
        }

        if let Some(client) = crate::ember::EmberClient::connect() {
            match client.request_fds(&self.bdf) {
                Ok(fds) => {
                    let device = coral_driver::vfio::VfioDevice::from_received_fds(
                        &self.bdf,
                        fds.container,
                        fds.group,
                        fds.device,
                    )
                    .map_err(|e| DeviceError::VfioOpen {
                        bdf: self.bdf.clone(),
                        reason: format!("ember fds: {e}"),
                    })?;
                    let bar0 = device.map_bar(0).map_err(|e| DeviceError::VfioOpen {
                        bdf: self.bdf.clone(),
                        reason: format!("BAR0 map from ember: {e}"),
                    })?;
                    self.vfio_holder = Some(VfioHolder { device, bar0 });
                    self.check_health();
                    tracing::info!(
                        bdf = %self.bdf,
                        group_id,
                        vram = self.health.vram_alive,
                        "VFIO fd reclaimed via ember"
                    );
                    return Ok(());
                }
                Err(e) => {
                    #[cfg(not(feature = "no-ember"))]
                    return Err(DeviceError::VfioOpen {
                        bdf: self.bdf.clone(),
                        reason: format!("ember fds failed during reclaim: {e}"),
                    });

                    #[cfg(feature = "no-ember")]
                    tracing::warn!(bdf = %self.bdf, error = %e, "ember fds unavailable for reclaim, using direct open (no-ember mode)");
                }
            }
        }

        #[cfg(not(feature = "no-ember"))]
        return Err(DeviceError::VfioOpen {
            bdf: self.bdf.clone(),
            reason: "ember not available for reclaim (enable 'no-ember' feature for legacy fallback)".into(),
        });

        #[cfg(feature = "no-ember")]
        match coral_driver::vfio::VfioDevice::open(&self.bdf) {
            Ok(device) => {
                let bar0 = device.map_bar(0).map_err(|e| DeviceError::VfioOpen {
                    bdf: self.bdf.clone(),
                    reason: format!("BAR0 map: {e}"),
                })?;
                self.vfio_holder = Some(VfioHolder { device, bar0 });
                self.check_health();
                tracing::info!(
                    bdf = %self.bdf,
                    group_id,
                    vram = self.health.vram_alive,
                    "VFIO fd reclaimed (direct open, no-ember mode)"
                );
                Ok(())
            }
            Err(e) => Err(DeviceError::VfioOpen {
                bdf: self.bdf.clone(),
                reason: e.to_string(),
            }),
        }
    }

    /// Read a single BAR0 register via the VFIO holder.
    ///
    /// Returns `None` if no VFIO holder is active or if the offset is
    /// out of the BAR0 mapping range.
    #[must_use]
    pub fn read_register(&self, offset: usize) -> Option<u32> {
        self.vfio_holder.as_ref()?.bar0.read_u32(offset).ok()
    }

    /// Dump a set of BAR0 registers, returning offset → value pairs.
    ///
    /// If `offsets` is empty, uses the default comprehensive register set
    /// covering PMC, PBUS, PFIFO, PBDMA, PFB, FBHUB, PMU, PCLOCK, GR, FECS,
    /// GPCCS, LTC, FBPA, PRAMIN, and thermal domains.
    #[must_use]
    pub fn dump_registers(&self, offsets: &[usize]) -> BTreeMap<usize, u32> {
        let offsets = if offsets.is_empty() { DEFAULT_REGISTER_DUMP_OFFSETS } else { offsets };
        let mut result = BTreeMap::new();
        if let Some(holder) = &self.vfio_holder {
            for &off in offsets {
                if let Ok(val) = holder.bar0.read_u32(off) {
                    result.insert(off, val);
                }
            }
        }
        result
    }

    /// Returns the most recent register snapshot taken during state preservation.
    #[must_use]
    pub fn last_snapshot(&self) -> &BTreeMap<usize, u32> {
        &self.register_snapshot
    }

    /// Take a snapshot of key registers (for state preservation across swaps).
    pub fn snapshot_registers(&mut self) {
        let Some(holder) = &self.vfio_holder else {
            return;
        };
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
            if let Ok(val) = holder.bar0.read_u32(off) {
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

        let Some(holder) = self.vfio_holder.as_ref() else {
            self.health.vram_alive = false;
            self.health.domains_alive = 0;
            self.health.domains_faulted = 0;
            return;
        };
        let r = |off: usize| holder.bar0.read_u32(off).unwrap_or(PCI_READ_DEAD);

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

    /// Resurrect HBM2 by cycling through nouveau via ember.
    ///
    /// Delegates all driver transitions to ember's `swap_device` RPC:
    /// snapshot → ember swap to nouveau (HBM2 training) → ember swap
    /// back to vfio → acquire fds → verify PRAMIN alive.
    ///
    /// Returns `Ok(true)` if VRAM came back alive, `Ok(false)` if resurrection
    /// completed but VRAM is still dead, `Err` if a step failed.
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::DriverBind` if ember is not available or a swap
    /// fails. Returns `DeviceError::VfioOpen` if post-swap fd acquisition fails.
    pub fn resurrect_hbm2(&mut self) -> Result<bool, DeviceError> {
        if crate::sysfs::read_current_driver(&self.bdf).as_deref() == Some("nvidia") {
            tracing::error!(
                bdf = %self.bdf,
                "REFUSING HBM2 resurrection — nvidia is bound to this device. \
                 Unbind nvidia from this BDF before resurrection."
            );
            return Err(DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: "nouveau".into(),
                reason: "nvidia is bound to this device — unbind before resurrection".into(),
            });
        }

        let warm_driver = crate::pci_ids::hbm2_training_driver(self.vendor_id)
            .ok_or_else(|| DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: "unknown".into(),
                reason: format!(
                    "no HBM2 training driver known for vendor {:#06x}",
                    self.vendor_id
                ),
            })?;

        tracing::info!(bdf = %self.bdf, warm_driver, "HBM2 resurrection starting via ember");

        self.snapshot_registers();
        tracing::info!(
            bdf = %self.bdf,
            regs = self.register_snapshot.len(),
            "state vault snapshot saved"
        );

        // Drop local VFIO holder
        drop(self.vfio_holder.take());

        // Ember required for resurrection
        let client =
            crate::ember::EmberClient::connect().ok_or_else(|| DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: warm_driver.into(),
                reason: "ember not available — resurrection requires ember for safe transition"
                    .into(),
            })?;

        // Step 1: swap to warm driver (ember handles unbind + bind + HBM2 training wait)
        client
            .swap_device(&self.bdf, warm_driver)
            .map_err(|e| DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: warm_driver.into(),
                reason: format!("ember swap to {warm_driver}: {e}"),
            })?;
        tracing::info!(bdf = %self.bdf, warm_driver, "HBM2 warm complete via ember");

        // Step 2: swap back to VFIO (ember handles unbind warm driver + bind vfio + reacquire)
        client
            .swap_device(&self.bdf, "vfio")
            .map_err(|e| DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: "vfio".into(),
                reason: format!("ember swap back to vfio after {warm_driver}: {e}"),
            })?;

        // Step 3: acquire VFIO fds from ember
        let group_id = sysfs::read_iommu_group(&self.bdf);
        match client.request_fds(&self.bdf) {
            Ok(fds) => {
                let device = coral_driver::vfio::VfioDevice::from_received_fds(
                    &self.bdf,
                    fds.container,
                    fds.group,
                    fds.device,
                )
                .map_err(|e| DeviceError::VfioOpen {
                    bdf: self.bdf.clone(),
                    reason: format!("ember fds after resurrection: {e}"),
                })?;
                let bar0 = device.map_bar(0).map_err(|e| DeviceError::VfioOpen {
                    bdf: self.bdf.clone(),
                    reason: format!("BAR0 map after resurrection: {e}"),
                })?;
                self.vfio_holder = Some(VfioHolder { device, bar0 });
                self.personality = Personality::Vfio { group_id };
            }
            Err(e) => {
                return Err(DeviceError::VfioOpen {
                    bdf: self.bdf.clone(),
                    reason: format!("ember fds after resurrection: {e}"),
                });
            }
        }

        // Step 4: verify PRAMIN is alive
        self.check_health();
        let alive = self.health.vram_alive;
        if alive {
            tracing::info!(
                bdf = %self.bdf,
                domains_alive = self.health.domains_alive,
                boot0 = format_args!("{:#010x}", self.health.boot0),
                pmc = format_args!("{:#010x}", self.health.pmc_enable),
                "HBM2 RESURRECTED via ember — VRAM alive"
            );
        } else {
            tracing::warn!(
                bdf = %self.bdf,
                domains_alive = self.health.domains_alive,
                domains_faulted = self.health.domains_faulted,
                "HBM2 resurrection via ember failed — VRAM still dead"
            );
        }

        Ok(alive)
    }

    /// Bind VFIO using fds received from the coral-ember process.
    ///
    /// The ember holds the original fds; these are dup'd copies received
    /// via `SCM_RIGHTS`. Dropping this `DeviceSlot` closes the dup'd fds
    /// but the ember's originals keep the VFIO binding alive.
    pub fn activate_from_ember(
        &mut self,
        container: OwnedFd,
        group: OwnedFd,
        device_fd: OwnedFd,
    ) -> Result<(), DeviceError> {
        let group_id = sysfs::read_iommu_group(&self.bdf);

        tracing::info!(
            bdf = %self.bdf,
            group_id,
            "activating device from ember fds"
        );

        let device = coral_driver::vfio::VfioDevice::from_received_fds(
            &self.bdf,
            container,
            group,
            device_fd,
        )
        .map_err(|e| DeviceError::VfioOpen {
            bdf: self.bdf.clone(),
            reason: format!("ember fds: {e}"),
        })?;

        let bar0 = device.map_bar(0).map_err(|e| DeviceError::VfioOpen {
            bdf: self.bdf.clone(),
            reason: format!("BAR0 map from ember fds: {e}"),
        })?;

        self.vfio_holder = Some(VfioHolder { device, bar0 });
        self.personality = Personality::Vfio { group_id };
        self.check_health();

        tracing::info!(
            bdf = %self.bdf,
            personality = %self.personality,
            chip = %self.chip_name,
            vram = self.health.vram_alive,
            power = %self.health.power,
            "device activated from ember"
        );

        Ok(())
    }

    /// Check if the GPU is quiescent (no in-flight work on PFIFO/PBDMA).
    ///
    /// Reads GV100 status registers to detect pending work. Conservative:
    /// returns false if any register indicates possible activity.
    fn check_quiescence(&self) -> bool {
        let Some(holder) = &self.vfio_holder else {
            return true;
        };
        let r = |off: usize| holder.bar0.read_u32(off).unwrap_or(0xFFFF_FFFF);

        // PFIFO_INTR_0 (0x002100): non-zero means pending interrupts
        let pfifo_intr = r(0x00_2100);
        // PFIFO (0x002504): scheduler/engine status
        let pfifo_sched = r(0x00_2504);
        // PBDMA0 (0x040108): channel status
        let pbdma0 = r(0x04_0108);

        // Cold silicon: uninitialized registers contain 0xbadf**** or 0xbad0****
        // patterns. These are NOT in-flight work — the GPU has never been initialized.
        let is_cold_pattern =
            |v: u32| (v & 0xFFFF_0000) == 0xBADF_0000 || (v & 0xFFF0_0000) == 0xBAD0_0000;

        let cold_silicon =
            is_cold_pattern(pfifo_sched) || is_cold_pattern(pbdma0);

        let quiescent = cold_silicon || (pfifo_intr == 0 && pfifo_sched == 0 && pbdma0 == 0);

        tracing::debug!(
            bdf = %self.bdf,
            pfifo_intr = format_args!("{pfifo_intr:#010x}"),
            pfifo_sched = format_args!("{pfifo_sched:#010x}"),
            pbdma0 = format_args!("{pbdma0:#010x}"),
            cold_silicon,
            quiescent,
            "GPU quiescence check"
        );

        quiescent
    }

    /// Wait for GPU quiescence with timeout. Returns true if quiescent.
    fn wait_quiescence(&self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut attempt = 0u32;

        while std::time::Instant::now() < deadline {
            if self.check_quiescence() {
                tracing::info!(bdf = %self.bdf, attempt, "GPU quiescent");
                return true;
            }
            attempt += 1;
            std::thread::sleep(std::time::Duration::from_millis(QUIESCENCE_POLL_MS));
        }

        tracing::warn!(bdf = %self.bdf, attempts = attempt, "GPU quiescence timeout");
        false
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
