// SPDX-License-Identifier: AGPL-3.0-only

//! PCI Command / bus mastering and D0 transition for VFIO devices.

use std::borrow::Cow;
use std::os::fd::AsFd;

use super::super::ioctl;
use super::super::types::VfioRegionInfo;
use crate::error::DriverError;

use super::VfioDevice;

impl VfioDevice {
    /// Disable PCI Bus Master — prevent the GPU from initiating DMA.
    ///
    /// After a driver swap (e.g. nouveau → vfio), the GPU's DMA engines
    /// may be in stale states. With bus mastering enabled, those engines can
    /// fire DMA requests to invalid IOMMU mappings, causing IOMMU faults that
    /// cascade through PCIe AER to hard-lock the system.
    ///
    /// Call this immediately after opening a post-swap VFIO device, then
    /// re-enable with [`enable_bus_master`] only when the GPU's DMA engines
    /// have been explicitly set up (e.g. after SEC2 falcon binding).
    pub fn disable_bus_master(&self) -> Result<(), DriverError> {
        const VFIO_PCI_CONFIG_REGION_INDEX: u32 = 7;
        const PCI_COMMAND: u64 = 0x04;
        const PCI_COMMAND_BUS_MASTER: u16 = 0x0004;

        let mut region_info = VfioRegionInfo {
            argsz: std::mem::size_of::<VfioRegionInfo>() as u32,
            index: VFIO_PCI_CONFIG_REGION_INDEX,
            ..Default::default()
        };
        ioctl::device_get_region_info(self.device.as_fd(), &mut region_info)?;
        let config_offset = region_info.offset;

        let mut cmd_buf = [0u8; 2];
        let n = rustix::io::pread(
            self.device.as_fd(),
            &mut cmd_buf,
            config_offset + PCI_COMMAND,
        )
        .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCI config read: {e}"))))?;
        if n != 2 {
            return Err(DriverError::SubmitFailed("PCI config read short".into()));
        }
        let cmd = u16::from_le_bytes(cmd_buf);
        let new_cmd = cmd & !PCI_COMMAND_BUS_MASTER;
        let new_buf = new_cmd.to_le_bytes();
        let n = rustix::io::pwrite(self.device.as_fd(), &new_buf, config_offset + PCI_COMMAND)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCI config write: {e}"))))?;
        if n != 2 {
            return Err(DriverError::SubmitFailed("PCI config write short".into()));
        }

        tracing::info!(
            pci_command_before = format_args!("{cmd:#06x}"),
            pci_command_after = format_args!("{new_cmd:#06x}"),
            "PCI Bus Master DISABLED — GPU DMA suppressed"
        );

        Ok(())
    }

    /// Enable PCI Bus Master and transition to D0 power state.
    ///
    /// After VFIO FLR the GPU's bus master bit is cleared and the device
    /// may be in D3 power state. Without bus mastering, the GPU cannot
    /// issue DMA reads/writes — the PFIFO will fault immediately when
    /// trying to read the instance block from system memory.
    ///
    /// PCI config space is accessed via pread/pwrite on the VFIO device fd
    /// at region index 7 (VFIO_PCI_CONFIG_REGION_INDEX).
    pub fn enable_bus_master(&self) -> Result<(), DriverError> {
        const VFIO_PCI_CONFIG_REGION_INDEX: u32 = 7;
        const PCI_COMMAND: u64 = 0x04;
        const PCI_COMMAND_BUS_MASTER: u16 = 0x0004;
        const PCI_COMMAND_MEMORY: u16 = 0x0002;
        const PCI_COMMAND_IO: u16 = 0x0001;

        // Get config region offset.
        let mut region_info = VfioRegionInfo {
            argsz: std::mem::size_of::<VfioRegionInfo>() as u32,
            index: VFIO_PCI_CONFIG_REGION_INDEX,
            ..Default::default()
        };
        ioctl::device_get_region_info(self.device.as_fd(), &mut region_info)?;

        let config_offset = region_info.offset;

        // Read current PCI Command register (2 bytes at offset 0x04).
        let mut cmd_buf = [0u8; 2];
        let n = rustix::io::pread(
            self.device.as_fd(),
            &mut cmd_buf,
            config_offset + PCI_COMMAND,
        )
        .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCI config read: {e}"))))?;
        if n != 2 {
            return Err(DriverError::SubmitFailed("PCI config read short".into()));
        }
        let cmd = u16::from_le_bytes(cmd_buf);

        tracing::info!(
            pci_command = format_args!("{cmd:#06x}"),
            bus_master = cmd & PCI_COMMAND_BUS_MASTER != 0,
            "PCI Command register (before)"
        );

        // Set Bus Master + Memory + I/O enable.
        let new_cmd = cmd | PCI_COMMAND_BUS_MASTER | PCI_COMMAND_MEMORY | PCI_COMMAND_IO;
        let new_buf = new_cmd.to_le_bytes();
        let n = rustix::io::pwrite(self.device.as_fd(), &new_buf, config_offset + PCI_COMMAND)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCI config write: {e}"))))?;
        if n != 2 {
            return Err(DriverError::SubmitFailed("PCI config write short".into()));
        }

        // Verify.
        let mut verify_buf = [0u8; 2];
        rustix::io::pread(
            self.device.as_fd(),
            &mut verify_buf,
            config_offset + PCI_COMMAND,
        )
        .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCI config verify: {e}"))))?;
        let verify_cmd = u16::from_le_bytes(verify_buf);

        tracing::info!(
            pci_command = format_args!("{verify_cmd:#06x}"),
            bus_master = verify_cmd & PCI_COMMAND_BUS_MASTER != 0,
            "PCI Command register (after)"
        );

        if verify_cmd & PCI_COMMAND_BUS_MASTER == 0 {
            return Err(DriverError::SubmitFailed(
                "Failed to enable PCI Bus Master".into(),
            ));
        }

        // Transition to D0 power state if device is in D3.
        // PM capability is at PCI config offset 0x60 for NVIDIA GPUs;
        // PM Control/Status register at capability + 4 = 0x64.
        // Bits [1:0] of PMCSR = PowerState (00=D0, 11=D3hot).
        const PCI_PM_CTRL: u64 = 0x64;
        let mut pm_buf = [0u8; 2];
        let _ = rustix::io::pread(
            self.device.as_fd(),
            &mut pm_buf,
            config_offset + PCI_PM_CTRL,
        );
        let pmcsr = u16::from_le_bytes(pm_buf);
        let power_state = pmcsr & 0x3;

        if power_state != 0 {
            tracing::info!(
                power_state,
                pmcsr = format_args!("{pmcsr:#06x}"),
                "Transitioning GPU from D{power_state} to D0"
            );

            let new_pmcsr = pmcsr & !0x3; // D0 = power state 0
            let pm_new_buf = new_pmcsr.to_le_bytes();
            let _ = rustix::io::pwrite(
                self.device.as_fd(),
                &pm_new_buf,
                config_offset + PCI_PM_CTRL,
            );

            // D3→D0 transition requires at least 10ms per PCI spec.
            std::thread::sleep(std::time::Duration::from_millis(20));

            // Re-enable bus master (D3→D0 transition may clear it).
            let new_buf2 = new_cmd.to_le_bytes();
            let _ = rustix::io::pwrite(self.device.as_fd(), &new_buf2, config_offset + PCI_COMMAND);
        }

        // Verify final state.
        let mut final_cmd_buf = [0u8; 2];
        let _ = rustix::io::pread(
            self.device.as_fd(),
            &mut final_cmd_buf,
            config_offset + PCI_COMMAND,
        );
        let mut final_pm_buf = [0u8; 2];
        let _ = rustix::io::pread(
            self.device.as_fd(),
            &mut final_pm_buf,
            config_offset + PCI_PM_CTRL,
        );
        let final_cmd = u16::from_le_bytes(final_cmd_buf);
        let final_pm = u16::from_le_bytes(final_pm_buf) & 0x3;

        tracing::info!(
            pci_command = format!("{final_cmd:#06x}"),
            bus_master = final_cmd & PCI_COMMAND_BUS_MASTER != 0,
            power_state = final_pm,
            "PCI: CMD/BusMaster/PowerState verified"
        );

        Ok(())
    }
}
