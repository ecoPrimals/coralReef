// SPDX-License-Identifier: AGPL-3.0-or-later
//! BAR mapping, reset, and PCI power management for VFIO devices.

use crate::error::DriverError;
use std::borrow::Cow;
use std::os::fd::{AsFd, AsRawFd};

use super::super::ioctl;
use super::super::types::{VfioPciHotReset, VfioRegionInfo};
use super::VfioDevice;
use super::dma::VfioBackend;
use super::mapped_bar::MappedBar;

impl VfioDevice {
    /// Map a BAR region into the process address space.
    ///
    /// `bar_index` selects which BAR (0 for BAR0, etc.).
    ///
    /// # Errors
    ///
    /// Returns error if the region index is out of range, has size 0, or mmap fails.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "struct argsz always fits u32"
    )]
    pub fn map_bar(&self, bar_index: u32) -> Result<MappedBar, DriverError> {
        if bar_index >= self.num_regions {
            return Err(DriverError::DeviceNotFound(Cow::Owned(format!(
                "BAR{bar_index} out of range (device has {} regions)",
                self.num_regions
            ))));
        }

        let mut region_info = VfioRegionInfo {
            argsz: std::mem::size_of::<VfioRegionInfo>() as u32,
            index: bar_index,
            ..Default::default()
        };
        ioctl::device_get_region_info(self.device.as_fd(), &mut region_info)?;

        if region_info.size == 0 {
            return Err(DriverError::MmapFailed(Cow::Owned(format!(
                "BAR{bar_index} region has size 0"
            ))));
        }

        let region_size = region_info.size as usize;

        // SAFETY: device fd valid; region offset from kernel; size verified non-zero;
        // MAP_SHARED for MMIO semantics; ProtFlags R|W for register access.
        let raw_ptr = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                region_size,
                rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
                rustix::mm::MapFlags::SHARED,
                &self.device,
                region_info.offset,
            )
            .map_err(|e| {
                DriverError::MmapFailed(Cow::Owned(format!("BAR{bar_index} mmap failed: {e}")))
            })?
        };
        // Defensive null check: Linux mmap returns MAP_FAILED on error (handled above);
        // on success the pointer is non-null. Check for robustness across platforms.
        if raw_ptr.is_null() {
            return Err(DriverError::MmapFailed(Cow::Owned(format!(
                "BAR{bar_index} mmap returned null"
            ))));
        }
        let base_ptr = raw_ptr.cast::<u8>();

        tracing::info!(
            bdf = %self.bdf,
            bar = bar_index,
            size = format_args!("{region_size:#x}"),
            "VFIO BAR mapped"
        );

        // SAFETY: `base_ptr`/`region_size` come from the successful `mmap` above.
        let region = unsafe { crate::mmio_region::MmioRegion::new(base_ptr, region_size) };

        Ok(MappedBar { region })
    }

    /// Reset the device via VFIO (FLR).
    ///
    /// # Errors
    ///
    /// Returns error if the reset ioctl fails (e.g. no FLR capability).
    pub fn reset(&self) -> Result<(), DriverError> {
        ioctl::device_reset(self.device.as_fd())?;
        tracing::info!(bdf = %self.bdf, "VFIO device reset");
        Ok(())
    }

    /// PCI Secondary Bus Reset via `VFIO_DEVICE_PCI_HOT_RESET`.
    ///
    /// The kernel walks up to the upstream PCIe bridge and asserts SBR on
    /// the secondary bus, fully resetting all GPU engines including falcons
    /// stuck in secure mode. This works on GV100 Titan V which lacks FLR.
    ///
    /// For legacy VFIO, the group fd is passed to authorize the reset.
    /// For iommufd (kernel 6.7+), `count=0` may work if all affected
    /// devices share the same iommufd context.
    ///
    /// After SBR, PCI config space (including bus master) is cleared.
    /// Call `enable_bus_master` or re-open the device to restore it.
    ///
    /// # Errors
    ///
    /// Returns error if the hot reset ioctl fails.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "struct argsz always fits u32"
    )]
    pub fn pci_hot_reset(&self) -> Result<(), DriverError> {
        match &self.backend {
            VfioBackend::LegacyGroup { group, .. } => {
                let mut reset = VfioPciHotReset {
                    argsz: (std::mem::size_of::<u32>() * 3 + std::mem::size_of::<i32>()) as u32,
                    flags: 0,
                    count: 1,
                    group_fds: [group.as_raw_fd(), 0, 0, 0],
                };
                ioctl::device_pci_hot_reset(self.device.as_fd(), &mut reset)?;
            }
            VfioBackend::Iommufd { .. } => {
                let mut reset = VfioPciHotReset {
                    argsz: (std::mem::size_of::<u32>() * 3) as u32,
                    flags: 0,
                    count: 0,
                    group_fds: [0; 4],
                };
                ioctl::device_pci_hot_reset(self.device.as_fd(), &mut reset)?;
            }
        }

        tracing::info!(bdf = %self.bdf, "VFIO PCI hot reset (SBR) completed");

        // SBR clears bus master — re-enable it.
        std::thread::sleep(std::time::Duration::from_millis(100));
        self.enable_bus_master()?;

        Ok(())
    }

    /// Perform a PCI D3→D0 power state transition via PCI PM capability.
    ///
    /// This puts the GPU into D3 sleep state and brings it back to D0,
    /// which should reset all GPU engines to their power-on state.
    /// Returns the PM capability offset and power state before/after.
    pub fn pci_power_cycle(&self) -> Result<(u32, u32), DriverError> {
        const VFIO_PCI_CONFIG_REGION_INDEX: u32 = 7;

        let mut region_info = VfioRegionInfo {
            argsz: std::mem::size_of::<VfioRegionInfo>() as u32,
            index: VFIO_PCI_CONFIG_REGION_INDEX,
            ..Default::default()
        };
        ioctl::device_get_region_info(self.device.as_fd(), &mut region_info)?;
        let cfg = region_info.offset;

        // Walk PCI capabilities chain to find Power Management capability (ID=0x01)
        let mut cap_ptr_buf = [0u8; 1];
        rustix::io::pread(self.device.as_fd(), &mut cap_ptr_buf, cfg + 0x34)
            .map_err(|e| DriverError::SubmitFailed(format!("PM cap ptr read: {e}").into()))?;
        let mut cap_off = cap_ptr_buf[0] as u64;

        let mut pm_offset = 0u64;
        while cap_off != 0 && cap_off < 256 {
            let mut cap_hdr = [0u8; 2];
            rustix::io::pread(self.device.as_fd(), &mut cap_hdr, cfg + cap_off).map_err(|e| {
                DriverError::SubmitFailed(format!("cap read at {cap_off:#x}: {e}").into())
            })?;
            let cap_id = cap_hdr[0];
            let next = cap_hdr[1] as u64;
            if cap_id == 0x01 {
                pm_offset = cap_off;
                break;
            }
            cap_off = next;
        }

        if pm_offset == 0 {
            return Err(DriverError::DeviceNotFound(
                "PCI PM capability not found".into(),
            ));
        }

        // PM Control/Status Register is at PM capability + 4
        let pmcsr_off = pm_offset + 4;
        let mut pmcsr_buf = [0u8; 2];
        rustix::io::pread(self.device.as_fd(), &mut pmcsr_buf, cfg + pmcsr_off)
            .map_err(|e| DriverError::SubmitFailed(format!("PMCSR read: {e}").into()))?;
        let pmcsr_before = u16::from_le_bytes(pmcsr_buf);
        let state_before = pmcsr_before & 0x3;

        tracing::info!(
            pm_offset = format_args!("{pm_offset:#x}"),
            pmcsr = format_args!("{pmcsr_before:#06x}"),
            power_state = state_before,
            "PCI PM: current state (0=D0, 3=D3hot)"
        );

        // Set D3hot (bits [1:0] = 3)
        let d3_val = (pmcsr_before & !0x3) | 0x3;
        rustix::io::pwrite(self.device.as_fd(), &d3_val.to_le_bytes(), cfg + pmcsr_off)
            .map_err(|e| DriverError::SubmitFailed(format!("PMCSR D3 write: {e}").into()))?;

        // Wait for D3 to take effect (PCI spec: 10ms minimum)
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Return to D0 (bits [1:0] = 0)
        let d0_val = pmcsr_before & !0x3;
        rustix::io::pwrite(self.device.as_fd(), &d0_val.to_le_bytes(), cfg + pmcsr_off)
            .map_err(|e| DriverError::SubmitFailed(format!("PMCSR D0 write: {e}").into()))?;

        // Wait for D0 transition (PCI spec: 10ms for D3hot→D0)
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Verify power state
        rustix::io::pread(self.device.as_fd(), &mut pmcsr_buf, cfg + pmcsr_off)
            .map_err(|e| DriverError::SubmitFailed(format!("PMCSR verify: {e}").into()))?;
        let pmcsr_after = u16::from_le_bytes(pmcsr_buf);
        let state_after = pmcsr_after & 0x3;

        tracing::info!(
            pmcsr_after = format_args!("{pmcsr_after:#06x}"),
            power_state = state_after,
            "PCI PM: after D3→D0 cycle"
        );

        Ok((state_before as u32, state_after as u32))
    }
}
