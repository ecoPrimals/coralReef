// SPDX-License-Identifier: AGPL-3.0-only
//! VFIO device management — open container/group/device, map BARs.
//!
//! `VfioDevice` wraps the full VFIO lifecycle for a single PCIe function:
//! container → group → device fd → BAR mmap. DMA buffers are allocated
//! separately via [`super::DmaBuffer`].

use crate::error::DriverError;
use std::borrow::Cow;
use std::fs::OpenOptions;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};

use super::ioctl;
use super::types::ioctls;
use super::types::{VfioDeviceInfo, VfioGroupStatus, VfioRegionInfo};

/// A mapped BAR region from a VFIO device.
pub struct MappedBar {
    base_ptr: *mut u8,
    size: usize,
}

impl MappedBar {
    /// Read a 32-bit register at the given byte offset.
    ///
    /// # Errors
    ///
    /// Returns error if offset is out of range.
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "BAR offsets are u32-aligned by hardware spec"
    )]
    pub fn read_u32(&self, offset: usize) -> Result<u32, DriverError> {
        if offset + 4 > self.size {
            return Err(DriverError::MmapFailed(Cow::Owned(format!(
                "BAR offset {offset:#x} out of range (size {:#x})",
                self.size
            ))));
        }
        // SAFETY: base_ptr valid from mmap; offset bounds-checked; volatile for MMIO.
        let val = unsafe { std::ptr::read_volatile(self.base_ptr.add(offset).cast::<u32>()) };
        Ok(val)
    }

    /// Write a 32-bit register at the given byte offset.
    ///
    /// # Errors
    ///
    /// Returns error if offset is out of range.
    #[expect(
        clippy::cast_ptr_alignment,
        reason = "BAR offsets are u32-aligned by hardware spec"
    )]
    pub fn write_u32(&self, offset: usize, value: u32) -> Result<(), DriverError> {
        if offset + 4 > self.size {
            return Err(DriverError::MmapFailed(Cow::Owned(format!(
                "BAR offset {offset:#x} out of range (size {:#x})",
                self.size
            ))));
        }
        // SAFETY: base_ptr valid from mmap; offset bounds-checked; volatile for MMIO.
        unsafe {
            std::ptr::write_volatile(self.base_ptr.add(offset).cast::<u32>(), value);
        }
        Ok(())
    }

    /// Size of this BAR region in bytes.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    /// Raw pointer to the BAR base (for callers that need ptr arithmetic).
    #[must_use]
    pub const fn base_ptr(&self) -> *mut u8 {
        self.base_ptr
    }
}

impl Drop for MappedBar {
    fn drop(&mut self) {
        // SAFETY: base_ptr from mmap; size unchanged since mapping.
        unsafe {
            let _ = rustix::mm::munmap(self.base_ptr.cast(), self.size);
        }
    }
}

// SAFETY: MMIO region is process-private; all writes go through volatile ops.
unsafe impl Send for MappedBar {}

// SAFETY: All access to the MMIO region uses volatile ops which are inherently
// atomic at the hardware level; &self methods are read-only (read_volatile).
unsafe impl Sync for MappedBar {}

/// A VFIO-managed PCIe device.
///
/// Holds the container/group/device fd lifecycle. Drop order: fields are
/// dropped in declaration order, so `device` closes before `group` before
/// `container`, matching VFIO's required teardown sequence.
pub struct VfioDevice {
    bdf: String,
    device: OwnedFd,
    num_regions: u32,
    #[expect(dead_code, reason = "kept alive for fd lifecycle")]
    group: std::fs::File,
    container: std::fs::File,
}

impl VfioDevice {
    /// Open a VFIO-bound PCIe device by BDF address (e.g. `"0000:01:00.0"`).
    ///
    /// Performs the full VFIO container/group/device setup:
    /// 1. Open `/dev/vfio/vfio` (container)
    /// 2. Verify API version and IOMMU support
    /// 3. Open `/dev/vfio/{group}` and attach to container
    /// 4. Set IOMMU type
    /// 5. Get device fd from group
    ///
    /// # Prerequisites (provided by toadStool)
    ///
    /// - GPU bound to `vfio-pci`
    /// - IOMMU enabled
    /// - User has permission on `/dev/vfio/*`
    ///
    /// # Errors
    ///
    /// Returns error if any VFIO setup step fails.
    pub fn open(bdf: &str) -> Result<Self, DriverError> {
        let iommu_group = find_iommu_group(bdf)?;
        tracing::info!(bdf, iommu_group, "opening VFIO device");

        let container = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/vfio/vfio")
            .map_err(|e| DriverError::DeviceNotFound(Cow::Owned(format!("/dev/vfio/vfio: {e}"))))?;

        let api_version = ioctl::get_api_version(container.as_fd())?;
        if api_version != ioctls::VFIO_API_VERSION {
            return Err(DriverError::DeviceNotFound(Cow::Owned(format!(
                "VFIO API version mismatch: got {api_version}, expected {}",
                ioctls::VFIO_API_VERSION
            ))));
        }

        let has_type1v2 = ioctl::check_extension(container.as_fd(), ioctls::VFIO_TYPE1V2_IOMMU)?;
        if has_type1v2 != 1 {
            return Err(DriverError::DeviceNotFound(
                "VFIO Type1v2 IOMMU not supported".into(),
            ));
        }

        let group_path = format!("/dev/vfio/{iommu_group}");
        let group = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&group_path)
            .map_err(|e| DriverError::DeviceNotFound(Cow::Owned(format!("{group_path}: {e}"))))?;

        let mut group_status = VfioGroupStatus {
            argsz: std::mem::size_of::<VfioGroupStatus>() as u32,
            flags: 0,
        };
        ioctl::group_status(group.as_fd(), &mut group_status)?;

        if (group_status.flags & ioctls::VFIO_GROUP_FLAGS_VIABLE) == 0 {
            return Err(DriverError::DeviceNotFound(
                "VFIO group not viable — all devices must be bound to vfio-pci".into(),
            ));
        }

        let container_fd = container.as_raw_fd();
        ioctl::group_set_container(group.as_fd(), std::ptr::from_ref(&container_fd).cast())?;

        ioctl::set_iommu(container.as_fd(), ioctls::VFIO_TYPE1V2_IOMMU)?;

        let bdf_cstr = std::ffi::CString::new(bdf)
            .map_err(|e| DriverError::DeviceNotFound(Cow::Owned(format!("Invalid BDF: {e}"))))?;
        let device_fd = ioctl::group_get_device_fd(group.as_fd(), bdf_cstr.as_ptr().cast())?;
        // SAFETY: kernel returns a valid fd on success.
        let device = unsafe { OwnedFd::from_raw_fd(device_fd) };

        let mut dev_info = VfioDeviceInfo {
            argsz: std::mem::size_of::<VfioDeviceInfo>() as u32,
            ..Default::default()
        };
        ioctl::device_info(device.as_fd(), &mut dev_info)?;

        tracing::info!(
            bdf,
            num_regions = dev_info.num_regions,
            num_irqs = dev_info.num_irqs,
            "VFIO device opened"
        );

        Ok(Self {
            bdf: bdf.to_string(),
            device,
            num_regions: dev_info.num_regions,
            group,
            container,
        })
    }

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
        let base_ptr = unsafe {
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
        }
        .cast::<u8>();

        tracing::info!(
            bdf = %self.bdf,
            bar = bar_index,
            size = format_args!("{region_size:#x}"),
            "VFIO BAR mapped"
        );

        Ok(MappedBar {
            base_ptr,
            size: region_size,
        })
    }

    /// Reset the device via VFIO.
    ///
    /// # Errors
    ///
    /// Returns error if the reset ioctl fails.
    pub fn reset(&self) -> Result<(), DriverError> {
        ioctl::device_reset(self.device.as_fd())?;
        tracing::info!(bdf = %self.bdf, "VFIO device reset");
        Ok(())
    }

    /// Raw fd of the VFIO container (for DMA buffer allocation).
    #[must_use]
    pub fn container_fd(&self) -> RawFd {
        self.container.as_raw_fd()
    }

    /// PCIe BDF address.
    #[must_use]
    pub fn bdf(&self) -> &str {
        &self.bdf
    }

    /// Number of BAR regions reported by the device.
    #[must_use]
    pub const fn num_regions(&self) -> u32 {
        self.num_regions
    }
}

impl std::fmt::Debug for VfioDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VfioDevice")
            .field("bdf", &self.bdf)
            .field("num_regions", &self.num_regions)
            .finish_non_exhaustive()
    }
}

fn find_iommu_group(bdf: &str) -> Result<u32, DriverError> {
    let path = format!("/sys/bus/pci/devices/{bdf}/iommu_group");
    let link = std::fs::read_link(&path).map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!(
            "Cannot read IOMMU group for {bdf}: {e}. \
             Is IOMMU enabled and GPU bound to vfio-pci?"
        )))
    })?;

    let group_str = link
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| DriverError::DeviceNotFound("Invalid IOMMU group path".into()))?;

    group_str.parse::<u32>().map_err(|e| {
        DriverError::DeviceNotFound(Cow::Owned(format!("Invalid IOMMU group number: {e}")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_iommu_group_nonexistent() {
        let result = find_iommu_group("9999:99:99.9");
        assert!(result.is_err());
    }

    #[test]
    fn open_nonexistent_bdf() {
        let result = VfioDevice::open("9999:99:99.9");
        assert!(result.is_err());
    }
}
