// SPDX-License-Identifier: AGPL-3.0-only

use crate::error::DeviceError;
use crate::personality::{Personality, PersonalityRegistry};
use crate::sysfs;
use std::os::fd::OwnedFd;

use super::DeviceSlot;
use super::types::VfioHolder;

impl DeviceSlot {
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
                client
                    .swap_device(&self.bdf, &target)
                    .map_err(|e| DeviceError::DriverBind {
                        bdf: self.bdf.clone(),
                        driver: target.clone(),
                        reason: format!("ember swap during activation: {e}"),
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
                client
                    .swap_device(&self.bdf, &target)
                    .map_err(|e| DeviceError::DriverBind {
                        bdf: self.bdf.clone(),
                        driver: target.clone(),
                        reason: format!("ember swap during activation: {e}"),
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
            "nouveau" | "nvidia" | "amdgpu" | "akida-pcie" => self.bind_driver(&target)?,
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
            &self.bdf, container, group, device_fd,
        )
        .map_err(|e| DeviceError::VfioOpen {
            bdf: self.bdf.clone(),
            reason: format!("ember fds: {e}"),
        })?;

        let bar0 = device.map_bar(0).map_err(|e| DeviceError::VfioOpen {
            bdf: self.bdf.clone(),
            reason: format!("BAR0 map from ember fds: {e}"),
        })?;

        self.vfio_holder = Some(VfioHolder::new(device, bar0));
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
                    self.vfio_holder = Some(VfioHolder::new(device, bar0));
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
                    self.vfio_holder = Some(VfioHolder::new(device, bar0));
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
    #[expect(
        clippy::unnecessary_wraps,
        reason = "returns Result for consistency with fallible bind path"
    )]
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
                let _ =
                    sysfs::sysfs_write(&format!("/sys/bus/pci/drivers/{driver}/bind"), &self.bdf);
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
            "akida-pcie" | "akida" => Personality::Akida,
            _ => Personality::Unbound,
        };
        Ok(())
    }
}
