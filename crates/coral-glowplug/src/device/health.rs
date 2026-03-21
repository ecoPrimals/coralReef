// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::BTreeMap;

use crate::error::DeviceError;
use crate::personality::Personality;
use crate::sysfs_ops::SysfsOps;

use super::DeviceSlot;
use super::types::{
    DEFAULT_REGISTER_DUMP_OFFSETS, PCI_READ_DEAD, QUIESCENCE_POLL_MS, VfioHolder, is_faulted_read,
};

impl<S: SysfsOps> DeviceSlot<S> {
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
        let offsets = if offsets.is_empty() {
            DEFAULT_REGISTER_DUMP_OFFSETS
        } else {
            offsets
        };
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
        if self.sysfs.read_current_driver(&self.bdf).as_deref() == Some("nvidia") {
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

        let warm_driver =
            crate::pci_ids::hbm2_training_driver(self.vendor_id).ok_or_else(|| {
                DeviceError::DriverBind {
                    bdf: self.bdf.clone(),
                    driver: "unknown".into(),
                    reason: format!(
                        "no HBM2 training driver known for vendor {:#06x}",
                        self.vendor_id
                    ),
                }
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
                    reason: format!("ember fds after resurrection: {e}"),
                })?;
                let bar0 = device.map_bar(0).map_err(|e| DeviceError::VfioOpen {
                    bdf: self.bdf.clone(),
                    reason: format!("BAR0 map after resurrection: {e}"),
                })?;
                self.vfio_holder = Some(VfioHolder::new(device, bar0));
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

    /// Check if the GPU is quiescent (no in-flight work on PFIFO/PBDMA).
    ///
    /// Reads GV100 status registers to detect pending work. Conservative:
    /// returns false if any register indicates possible activity.
    fn check_quiescence(&self) -> bool {
        #[cfg(test)]
        if let Some(q) = self.test_quiescence_override {
            return q;
        }
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

        let cold_silicon = is_cold_pattern(pfifo_sched) || is_cold_pattern(pbdma0);

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
    pub(super) fn wait_quiescence(&self, timeout: std::time::Duration) -> bool {
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
        self.health.power = self.sysfs.read_power_state(&self.bdf);
        self.health.pci_link_width = self.sysfs.read_link_width(&self.bdf);
    }
}
