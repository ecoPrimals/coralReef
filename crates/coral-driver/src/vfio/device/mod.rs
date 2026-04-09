// SPDX-License-Identifier: AGPL-3.0-or-later
//! VFIO device management — open container/group/device, map BARs.
//!
//! `VfioDevice` wraps the full VFIO lifecycle for a single PCIe function:
//! container → group → device fd → BAR mmap. DMA buffers are allocated
//! separately via [`super::DmaBuffer`].

use crate::error::{DriverError, DriverResult};
use std::borrow::Cow;
use std::ffi::CStr;
use std::fs::OpenOptions;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;

use super::ioctl;
use super::types::ioctls;
use super::types::{
    IommuIoasAlloc, VfioDeviceAttachIommufdPt, VfioDeviceBindIommufd, VfioDeviceInfo,
    VfioGroupStatus, VfioRegionInfo,
};
use crate::gsp::{ApplyError, RegisterAccess};
use crate::mmio_region::MmioRegion;

/// A mapped BAR region from a VFIO device.
///
/// ## Thread safety (`Send` / `Sync`)
///
/// The region wraps a `MmioRegion` whose pointer refers to a `MAP_SHARED` MMIO
/// mapping tied to the VFIO device fd lifetime. Access is performed only through
/// volatile operations (`read_u32` / `write_u32`), which are safe to use from
/// multiple threads for aligned 32-bit MMIO on supported architectures when the
/// mapping is shared read-only or callers coordinate writes. The owning struct is
/// therefore `Send` + `Sync` for the same reasons as other mmap-backed BAR
/// wrappers in this crate.
pub struct MappedBar {
    region: MmioRegion,
}

impl MappedBar {
    /// Read a 32-bit register at the given byte offset.
    ///
    /// # Errors
    ///
    /// Returns error if offset is out of range or not 4-byte aligned.
    pub fn read_u32(&self, offset: usize) -> Result<u32, DriverError> {
        if !offset.is_multiple_of(4) {
            return Err(DriverError::MmapFailed(Cow::Owned(format!(
                "BAR offset {offset:#x} is not 4-byte aligned"
            ))));
        }
        self.region.read_u32(offset)
    }

    /// Write a 32-bit register at the given byte offset.
    ///
    /// # Errors
    ///
    /// Returns error if offset is out of range or not 4-byte aligned.
    pub fn write_u32(&self, offset: usize, value: u32) -> Result<(), DriverError> {
        if !offset.is_multiple_of(4) {
            return Err(DriverError::MmapFailed(Cow::Owned(format!(
                "BAR offset {offset:#x} is not 4-byte aligned"
            ))));
        }
        self.region.write_u32(offset, value)
    }

    /// Size of this BAR region in bytes.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.region.len()
    }

    /// Apply a GR init sequence's BAR0 writes.
    ///
    /// Implements the `RegisterAccess` trait bridge so the GSP applicator
    /// can write directly through the VFIO-mapped BAR0.
    pub fn apply_gr_bar0_writes(&self, writes: &[(u32, u32)]) -> (usize, usize) {
        let mut applied = 0;
        let mut failed = 0;
        for &(offset, value) in writes {
            if self.write_u32(offset as usize, value).is_ok() {
                applied += 1;
            } else {
                failed += 1;
            }
        }
        (applied, failed)
    }

    /// Raw pointer to the BAR base (for callers that need ptr arithmetic).
    #[must_use]
    pub const fn base_ptr(&self) -> *mut u8 {
        self.region.as_ptr()
    }
}

impl RegisterAccess for MappedBar {
    fn read_u32(&self, offset: u32) -> Result<u32, ApplyError> {
        self.read_u32(offset as usize)
            .map_err(|e| ApplyError::MmioFailed {
                offset,
                detail: e.to_string(),
            })
    }

    fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), ApplyError> {
        MappedBar::write_u32(self, offset as usize, value).map_err(|e| ApplyError::MmioFailed {
            offset,
            detail: e.to_string(),
        })
    }
}

// SAFETY: Matches the `Send` / `Sync` rationale in the [`MappedBar`] docs.
unsafe impl Send for MappedBar {}

// SAFETY: Matches the `Send` / `Sync` rationale in the [`MappedBar`] docs.
unsafe impl Sync for MappedBar {}

mod bus_master;
mod dma;
use dma::VfioBackend;
pub use dma::{DmaBackend, ReceivedVfioFds, VfioBackendKind};

/// A VFIO-managed PCIe device.
///
/// Holds the device fd lifecycle and the backend-specific state for DMA.
/// Drop order: `device` closes before `backend` fields, matching VFIO's
/// required teardown sequence.
pub struct VfioDevice {
    bdf: String,
    pub(super) device: OwnedFd,
    num_regions: u32,
    backend: VfioBackend,
}

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

        let bdf_cstr = std::ffi::CString::new(bdf)
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
        let region = unsafe { MmioRegion::new(base_ptr, region_size) };

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
        use super::types::VfioPciHotReset;

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

    /// DMA mapping backend for this device. Pass this to [`super::DmaBuffer`],
    /// [`super::VfioChannel`], and other code that needs to create IOMMU mappings.
    #[must_use]
    pub fn dma_backend(&self) -> DmaBackend {
        match &self.backend {
            VfioBackend::LegacyGroup { container, .. } => {
                DmaBackend::LegacyContainer(Arc::clone(container))
            }
            VfioBackend::Iommufd { iommufd, ioas_id } => DmaBackend::Iommufd {
                fd: Arc::clone(iommufd),
                ioas_id: *ioas_id,
            },
        }
    }

    /// Which backend this device is using. Callers (ember, glowplug) use this
    /// to select the right IPC fd-passing strategy without panicking.
    #[must_use]
    pub fn backend_kind(&self) -> VfioBackendKind {
        match &self.backend {
            VfioBackend::LegacyGroup { .. } => VfioBackendKind::Legacy,
            VfioBackend::Iommufd { ioas_id, .. } => VfioBackendKind::Iommufd { ioas_id: *ioas_id },
        }
    }

    /// File descriptors to pass via `SCM_RIGHTS` for this device.
    ///
    /// - **Legacy**: `[container, group, device]` (3 fds)
    /// - **Iommufd**: `[iommufd, device]` (2 fds)
    ///
    /// The receiver must also know the [`backend_kind`](Self::backend_kind) to
    /// reconstruct the device on the other side.
    #[must_use]
    pub fn sendable_fds(&self) -> Vec<BorrowedFd<'_>> {
        match &self.backend {
            VfioBackend::LegacyGroup {
                container, group, ..
            } => vec![container.as_fd(), group.as_fd(), self.device.as_fd()],
            VfioBackend::Iommufd { iommufd, .. } => {
                vec![iommufd.as_fd(), self.device.as_fd()]
            }
        }
    }

    /// Raw fd of the VFIO container (legacy path only, for ember `SCM_RIGHTS`).
    ///
    /// Prefer [`sendable_fds`](Self::sendable_fds) for backend-agnostic fd passing.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::Unsupported`] on an iommufd-backed device (no legacy container fd).
    pub fn container_fd(&self) -> DriverResult<RawFd> {
        match &self.backend {
            VfioBackend::LegacyGroup { container, .. } => Ok(container.as_raw_fd()),
            VfioBackend::Iommufd { .. } => Err(DriverError::Unsupported(
                "container_fd() not available on iommufd backend".into(),
            )),
        }
    }

    /// Borrowed handle to the VFIO container fd (legacy path only).
    ///
    /// Prefer [`sendable_fds`](Self::sendable_fds) for backend-agnostic fd passing.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::Unsupported`] on an iommufd-backed device (no legacy container fd).
    pub fn container_as_fd(&self) -> DriverResult<BorrowedFd<'_>> {
        match &self.backend {
            VfioBackend::LegacyGroup { container, .. } => Ok(container.as_fd()),
            VfioBackend::Iommufd { .. } => Err(DriverError::Unsupported(
                "container_as_fd() not available on iommufd backend".into(),
            )),
        }
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

    /// Leak the VFIO file descriptors so they are NOT closed on drop.
    ///
    /// This prevents the kernel from performing a PM reset on the GPU,
    /// preserving HBM2 training state across process exits. Call this
    /// when you want to keep the GPU warm between test runs.
    ///
    /// The fds become unreachable but the kernel keeps them alive until
    /// the process exits, at which point the kernel will reset the device.
    pub fn leak(self) {
        std::mem::forget(self);
    }

    /// Raw fd of the VFIO device (for SCM\_RIGHTS fd passing to/from coral-ember).
    #[must_use]
    pub fn device_fd(&self) -> RawFd {
        self.device.as_raw_fd()
    }

    /// Borrowed handle to the VFIO device fd (for `SCM_RIGHTS` / [`AsFd`] APIs).
    #[must_use]
    pub fn device_as_fd(&self) -> BorrowedFd<'_> {
        self.device.as_fd()
    }

    /// Raw fd of the VFIO group (legacy path only, for ember `SCM_RIGHTS`).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::Unsupported`] on an iommufd-backed device (no VFIO group fd).
    pub fn group_fd(&self) -> DriverResult<RawFd> {
        match &self.backend {
            VfioBackend::LegacyGroup { group, .. } => Ok(group.as_raw_fd()),
            VfioBackend::Iommufd { .. } => Err(DriverError::Unsupported(
                "group_fd() not available on iommufd backend".into(),
            )),
        }
    }

    /// Borrowed handle to the VFIO group fd (legacy path only).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::Unsupported`] on an iommufd-backed device (no VFIO group fd).
    pub fn group_as_fd(&self) -> DriverResult<BorrowedFd<'_>> {
        match &self.backend {
            VfioBackend::LegacyGroup { group, .. } => Ok(group.as_fd()),
            VfioBackend::Iommufd { .. } => Err(DriverError::Unsupported(
                "group_as_fd() not available on iommufd backend".into(),
            )),
        }
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

impl std::fmt::Debug for VfioDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VfioDevice")
            .field("bdf", &self.bdf)
            .field("num_regions", &self.num_regions)
            .finish_non_exhaustive()
    }
}

/// `VFIO_GROUP_GET_DEVICE_FD` returns a new owning fd; centralizes the only
/// `OwnedFd::from_raw_fd` needed for this path.
fn vfio_group_open_device_fd(group: BorrowedFd<'_>, bdf: &CStr) -> Result<OwnedFd, DriverError> {
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
