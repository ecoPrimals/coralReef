// SPDX-License-Identifier: AGPL-3.0-only

//! PCIe Advanced Error Reporting (AER) masking for VFIO devices.
//!
//! During GPU experiments (ACR boot, SEC2 firmware upload), stray PCIe errors
//! can trigger the kernel's AER handler which cascades through the root complex
//! into a D-state lockup.  Masking AER for the duration of an experiment
//! prevents these cascades while still allowing the experiment to observe and
//! handle errors explicitly.

use std::borrow::Cow;
use std::os::fd::AsFd;

use super::super::ioctl;
use super::super::types::VfioRegionInfo;
use crate::error::DriverError;

use super::VfioDevice;

const VFIO_PCI_CONFIG_REGION_INDEX: u32 = 7;

/// Saved AER mask state for later restoration.
#[derive(Debug, Clone)]
pub struct AerMaskState {
    /// Offset of the AER extended capability in PCI config space.
    pub aer_cap_offset: u64,
    /// Original uncorrectable error mask before masking.
    pub original_ue_mask: u32,
    /// Original correctable error mask before masking.
    pub original_ce_mask: u32,
}

impl VfioDevice {
    /// Find the AER extended capability offset by walking the PCIe extended
    /// capability list starting at offset 0x100.
    fn find_aer_cap(&self) -> Result<u64, DriverError> {
        let config_offset = self.config_region_offset()?;

        let mut offset: u64 = 0x100;
        for _ in 0..48 {
            let mut buf = [0u8; 4];
            let n = rustix::io::pread(self.device.as_fd(), &mut buf, config_offset + offset)
                .map_err(|e| {
                    DriverError::SubmitFailed(Cow::Owned(format!("AER cap walk read: {e}")))
                })?;
            if n != 4 {
                break;
            }
            let header = u32::from_le_bytes(buf);
            let cap_id = header & 0xFFFF;
            let next = (header >> 20) & 0xFFC;

            if cap_id == 0x0001 {
                return Ok(offset);
            }
            if next == 0 || next <= offset as u32 {
                break;
            }
            offset = next as u64;
        }

        Err(DriverError::SubmitFailed(
            "AER extended capability not found in config space".into(),
        ))
    }

    fn config_region_offset(&self) -> Result<u64, DriverError> {
        let mut region_info = VfioRegionInfo {
            argsz: std::mem::size_of::<VfioRegionInfo>() as u32,
            index: VFIO_PCI_CONFIG_REGION_INDEX,
            ..Default::default()
        };
        ioctl::device_get_region_info(self.device.as_fd(), &mut region_info)?;
        Ok(region_info.offset)
    }

    fn pci_config_read32(&self, config_offset: u64, reg: u64) -> Result<u32, DriverError> {
        let mut buf = [0u8; 4];
        let n = rustix::io::pread(self.device.as_fd(), &mut buf, config_offset + reg)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCI read {reg:#x}: {e}"))))?;
        if n != 4 {
            return Err(DriverError::SubmitFailed(
                Cow::Owned(format!("PCI read {reg:#x}: short read ({n})")),
            ));
        }
        Ok(u32::from_le_bytes(buf))
    }

    fn pci_config_write32(&self, config_offset: u64, reg: u64, val: u32) -> Result<(), DriverError> {
        let buf = val.to_le_bytes();
        let n = rustix::io::pwrite(self.device.as_fd(), &buf, config_offset + reg)
            .map_err(|e| {
                DriverError::SubmitFailed(Cow::Owned(format!("PCI write {reg:#x}: {e}")))
            })?;
        if n != 4 {
            return Err(DriverError::SubmitFailed(
                Cow::Owned(format!("PCI write {reg:#x}: short write ({n})")),
            ));
        }
        Ok(())
    }

    /// Mask all AER errors (correctable + uncorrectable) for this device.
    ///
    /// Returns the saved state needed to restore masks via [`unmask_aer`].
    /// Call this before GPU experiments to prevent kernel AER cascades.
    pub fn mask_aer(&self) -> Result<AerMaskState, DriverError> {
        let aer_cap_offset = self.find_aer_cap()?;
        let config_offset = self.config_region_offset()?;

        // AER capability layout (relative to cap base):
        //   +0x08: Uncorrectable Error Mask
        //   +0x14: Correctable Error Mask
        let ue_mask_reg = aer_cap_offset + 0x08;
        let ce_mask_reg = aer_cap_offset + 0x14;

        let original_ue_mask = self.pci_config_read32(config_offset, ue_mask_reg)?;
        let original_ce_mask = self.pci_config_read32(config_offset, ce_mask_reg)?;

        // Mask all errors
        self.pci_config_write32(config_offset, ue_mask_reg, 0xFFFF_FFFF)?;
        self.pci_config_write32(config_offset, ce_mask_reg, 0xFFFF_FFFF)?;

        let verify_ue = self.pci_config_read32(config_offset, ue_mask_reg)?;
        let verify_ce = self.pci_config_read32(config_offset, ce_mask_reg)?;

        tracing::info!(
            aer_cap = format_args!("{aer_cap_offset:#x}"),
            ue_mask = format_args!("{original_ue_mask:#010x} → {verify_ue:#010x}"),
            ce_mask = format_args!("{original_ce_mask:#010x} → {verify_ce:#010x}"),
            "AER errors MASKED — kernel handler suppressed"
        );

        Ok(AerMaskState {
            aer_cap_offset,
            original_ue_mask,
            original_ce_mask,
        })
    }

    /// Restore AER masks to their pre-experiment values.
    pub fn unmask_aer(&self, state: &AerMaskState) -> Result<(), DriverError> {
        let config_offset = self.config_region_offset()?;

        let ue_mask_reg = state.aer_cap_offset + 0x08;
        let ce_mask_reg = state.aer_cap_offset + 0x14;

        self.pci_config_write32(config_offset, ue_mask_reg, state.original_ue_mask)?;
        self.pci_config_write32(config_offset, ce_mask_reg, state.original_ce_mask)?;

        tracing::info!(
            ue_mask = format_args!("{:#010x}", state.original_ue_mask),
            ce_mask = format_args!("{:#010x}", state.original_ce_mask),
            "AER errors UNMASKED — original masks restored"
        );

        Ok(())
    }
}
