// SPDX-License-Identifier: AGPL-3.0-only

use crate::error::DeviceError;
use crate::personality::{Personality, PersonalityRegistry};
use crate::sysfs_ops::SysfsOps;

use super::DeviceSlot;
use super::types::{QUIESCENCE_TIMEOUT, VfioHolder};

impl<S: SysfsOps> DeviceSlot<S> {
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
        if self.sysfs.read_current_driver(&self.bdf).as_deref() == Some("nvidia") {
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
        let client =
            crate::ember::EmberClient::connect().ok_or_else(|| DeviceError::DriverBind {
                bdf: self.bdf.clone(),
                driver: target.into(),
                reason: "ember not available — driver swap requires ember for safe transition"
                    .into(),
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
                let group_id = self.sysfs.read_iommu_group(&self.bdf);
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
                        self.vfio_holder = Some(VfioHolder::new(device, bar0));
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
                let drm = self.sysfs.find_drm_card(&self.bdf);
                self.personality = Personality::Nouveau { drm_card: drm };
            }
            "nvidia" => {
                let drm = self.sysfs.find_drm_card(&self.bdf);
                self.personality = Personality::Nvidia { drm_card: drm };
            }
            "amdgpu" => {
                let drm = self.sysfs.find_drm_card(&self.bdf);
                self.personality = Personality::Amdgpu { drm_card: drm };
            }
            "akida-pcie" | "akida" => {
                self.personality = Personality::Akida;
            }
            "xe" => {
                let drm = self.sysfs.find_drm_card(&self.bdf);
                self.personality = Personality::Xe { drm_card: drm };
            }
            "i915" => {
                let drm = self.sysfs.find_drm_card(&self.bdf);
                self.personality = Personality::I915 { drm_card: drm };
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "available for manual teardown outside test builds"
        )
    )]
    pub(super) fn release(&mut self) -> Result<(), DeviceError> {
        if self.sysfs.has_active_drm_consumers(&self.bdf) {
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
                    self.vfio_holder = Some(VfioHolder::new(device, bar0));
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
            reason:
                "ember not available for reclaim (enable 'no-ember' feature for legacy fallback)"
                    .into(),
        });

        #[cfg(feature = "no-ember")]
        match coral_driver::vfio::VfioDevice::open(&self.bdf) {
            Ok(device) => {
                let bar0 = device.map_bar(0).map_err(|e| DeviceError::VfioOpen {
                    bdf: self.bdf.clone(),
                    reason: format!("BAR0 map: {e}"),
                })?;
                self.vfio_holder = Some(VfioHolder::new(device, bar0));
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
}
