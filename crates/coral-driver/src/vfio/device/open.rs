// SPDX-License-Identifier: AGPL-3.0-or-later
//! VFIO device open paths and reconstruction from passed file descriptors.

use crate::error::DriverError;
use std::borrow::Cow;
use std::ffi::{CStr, CString};
use std::fs::OpenOptions;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;

use super::super::ioctl;
use super::super::types::ioctls;
use super::super::types::{
    IommuIoasAlloc, VfioDeviceAttachIommufdPt, VfioDeviceBindIommufd, VfioDeviceInfo,
    VfioGroupStatus,
};
use super::VfioDevice;
use super::dma::{ReceivedVfioFds, VfioBackend};

impl VfioDevice {
    /// Open a VFIO-bound PCIe device by BDF address (e.g. `"0000:01:00.0"`).
    ///
    /// Automatically selects the best available backend:
    /// 1. **iommufd/cdev** (kernel 6.2+) — opens `/dev/iommu` and the device's
    ///    cdev node under `/dev/vfio/devices/`. No IOMMU group dance needed.
    /// 2. **Legacy container/group** — falls back to the traditional
    ///    `/dev/vfio/vfio` + `/dev/vfio/{group}` path for older kernels.
    ///
    /// # Prerequisites (provided by ecosystem hardware setup)
    ///
    /// - GPU bound to `vfio-pci`
    /// - IOMMU enabled
    /// - User has permission on `/dev/vfio/*` (and `/dev/iommu` for cdev path)
    ///
    /// # Errors
    ///
    /// Returns error if both backends fail.
    pub fn open(bdf: &str) -> Result<Self, DriverError> {
        match Self::open_iommufd(bdf) {
            Ok(dev) => {
                tracing::info!(bdf, "VFIO device opened via iommufd/cdev");
                return Ok(dev);
            }
            Err(e) => {
                tracing::debug!(bdf, err = %e, "iommufd/cdev unavailable, trying legacy group");
            }
        }
        Self::open_legacy_group(bdf)
    }

    /// Modern iommufd/cdev open path (kernel 6.2+).
    ///
    /// 1. Open `/dev/iommu`
    /// 2. Discover cdev via `{sysfs}/bus/pci/devices/{bdf}/vfio-dev/`
    /// 3. Open `/dev/vfio/devices/{cdev}`
    /// 4. `VFIO_DEVICE_BIND_IOMMUFD`
    /// 5. `IOMMU_IOAS_ALLOC`
    /// 6. `VFIO_DEVICE_ATTACH_IOMMUFD_PT`
    #[expect(
        clippy::cast_possible_truncation,
        reason = "struct argsz always fits u32"
    )]
    fn open_iommufd(bdf: &str) -> Result<Self, DriverError> {
        let iommufd = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/iommu")
            .map_err(|e| DriverError::DeviceNotFound(Cow::Owned(format!("/dev/iommu: {e}"))))?;

        let cdev_name = crate::linux_paths::sysfs_vfio_cdev_name(bdf).ok_or_else(|| {
            DriverError::DeviceNotFound(Cow::Owned(format!(
                "No VFIO cdev for {bdf} (vfio-dev/ not found in sysfs)"
            )))
        })?;

        let cdev_path = format!("/dev/vfio/devices/{cdev_name}");
        let device_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&cdev_path)
            .map_err(|e| DriverError::DeviceNotFound(Cow::Owned(format!("{cdev_path}: {e}"))))?;
        let device = OwnedFd::from(device_file);

        let mut bind = VfioDeviceBindIommufd {
            argsz: std::mem::size_of::<VfioDeviceBindIommufd>() as u32,
            flags: 0,
            iommufd: iommufd.as_raw_fd(),
            out_devid: 0,
        };
        ioctl::device_bind_iommufd(device.as_fd(), &mut bind)?;
        tracing::debug!(bdf, devid = bind.out_devid, "bound device to iommufd");

        let iommufd_fd = OwnedFd::from(iommufd);
        let mut ioas_alloc = IommuIoasAlloc {
            size: std::mem::size_of::<IommuIoasAlloc>() as u32,
            flags: 0,
            out_ioas_id: 0,
        };
        ioctl::iommufd_ioas_alloc(iommufd_fd.as_fd(), &mut ioas_alloc)?;
        let ioas_id = ioas_alloc.out_ioas_id;
        tracing::debug!(bdf, ioas_id, "allocated IOAS");

        let mut attach = VfioDeviceAttachIommufdPt {
            argsz: std::mem::size_of::<VfioDeviceAttachIommufdPt>() as u32,
            flags: 0,
            pt_id: ioas_id,
        };
        ioctl::device_attach_iommufd_pt(device.as_fd(), &mut attach)?;
        tracing::debug!(bdf, ioas_id, "attached device to IOAS");

        let mut dev_info = VfioDeviceInfo {
            argsz: std::mem::size_of::<VfioDeviceInfo>() as u32,
            ..Default::default()
        };
        ioctl::device_info(device.as_fd(), &mut dev_info)?;

        tracing::info!(
            bdf,
            num_regions = dev_info.num_regions,
            num_irqs = dev_info.num_irqs,
            cdev = %cdev_name,
            "VFIO device opened (iommufd)"
        );

        let dev = Self {
            bdf: bdf.to_string(),
            device,
            num_regions: dev_info.num_regions,
            backend: VfioBackend::Iommufd {
                iommufd: Arc::new(iommufd_fd),
                ioas_id,
            },
        };

        dev.enable_bus_master()?;

        Ok(dev)
    }

    /// Legacy container/group open path (kernel < 6.2).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "struct argsz always fits u32"
    )]
    fn open_legacy_group(bdf: &str) -> Result<Self, DriverError> {
        let iommu_group = find_iommu_group(bdf)?;
        tracing::info!(bdf, iommu_group, "opening VFIO device (legacy group)");

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

        let container_raw_fd = container.as_raw_fd();
        ioctl::group_set_container(group.as_fd(), std::ptr::from_ref(&container_raw_fd).cast())?;

        ioctl::set_iommu(container.as_fd(), ioctls::VFIO_TYPE1V2_IOMMU)?;

        let bdf_cstr = CString::new(bdf)
            .map_err(|e| DriverError::DeviceNotFound(Cow::Owned(format!("Invalid BDF: {e}"))))?;
        let device = vfio_group_open_device_fd(group.as_fd(), bdf_cstr.as_c_str())?;

        let mut dev_info = VfioDeviceInfo {
            argsz: std::mem::size_of::<VfioDeviceInfo>() as u32,
            ..Default::default()
        };
        ioctl::device_info(device.as_fd(), &mut dev_info)?;

        tracing::info!(
            bdf,
            num_regions = dev_info.num_regions,
            num_irqs = dev_info.num_irqs,
            "VFIO device opened (legacy group)"
        );

        let dev = Self {
            bdf: bdf.to_string(),
            device,
            num_regions: dev_info.num_regions,
            backend: VfioBackend::LegacyGroup {
                container: Arc::new(OwnedFd::from(container)),
                group,
            },
        };

        dev.enable_bus_master()?;

        Ok(dev)
    }

    /// Reconstruct from fds received via `SCM_RIGHTS` from coral-ember.
    ///
    /// Handles both backend shapes:
    /// - **Legacy**: container + group + device (3 fds) — older kernels
    /// - **Iommufd**: iommufd + device (2 fds) + ioas_id metadata — kernel 6.2+
    ///
    /// The ember holds the original fds; these are dup'd copies. When this
    /// `VfioDevice` is dropped, the dup'd fds close but the ember's originals
    /// keep the VFIO binding alive (no PM reset on GV100).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "struct argsz always fits u32"
    )]
    pub fn from_received(bdf: &str, fds: ReceivedVfioFds) -> Result<Self, DriverError> {
        let (device, backend) = match fds {
            ReceivedVfioFds::Legacy {
                container,
                group,
                device,
            } => (
                device,
                VfioBackend::LegacyGroup {
                    container: Arc::new(container),
                    group: std::fs::File::from(group),
                },
            ),
            ReceivedVfioFds::Iommufd {
                iommufd,
                device,
                ioas_id,
            } => (
                device,
                VfioBackend::Iommufd {
                    iommufd: Arc::new(iommufd),
                    ioas_id,
                },
            ),
        };

        let backend_label = match &backend {
            VfioBackend::LegacyGroup { .. } => "legacy",
            VfioBackend::Iommufd { .. } => "iommufd",
        };

        let mut dev_info = VfioDeviceInfo {
            argsz: std::mem::size_of::<VfioDeviceInfo>() as u32,
            ..Default::default()
        };
        ioctl::device_info(device.as_fd(), &mut dev_info)?;

        tracing::info!(
            bdf,
            backend = backend_label,
            num_regions = dev_info.num_regions,
            num_irqs = dev_info.num_irqs,
            "VFIO device reconstructed from ember fds"
        );

        let dev = Self {
            bdf: bdf.to_string(),
            device,
            num_regions: dev_info.num_regions,
            backend,
        };

        dev.enable_bus_master()?;

        Ok(dev)
    }
}

/// `VFIO_GROUP_GET_DEVICE_FD` returns a new owning fd; centralizes the only
/// `OwnedFd::from_raw_fd` needed for this path.
fn vfio_group_open_device_fd(
    group: std::os::fd::BorrowedFd<'_>,
    bdf: &CStr,
) -> Result<OwnedFd, DriverError> {
    let raw_fd = ioctl::group_get_device_fd(group, bdf.as_ptr().cast())?;
    if raw_fd < 0 {
        return Err(DriverError::DeviceNotFound(Cow::Owned(format!(
            "VFIO GROUP_GET_DEVICE_FD: invalid fd {raw_fd}"
        ))));
    }
    // SAFETY: On success the kernel returns a new owning device fd.
    Ok(unsafe { OwnedFd::from_raw_fd(raw_fd) })
}

fn find_iommu_group(bdf: &str) -> Result<u32, DriverError> {
    let path = crate::linux_paths::sysfs_pci_device_file(bdf, "iommu_group");
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
