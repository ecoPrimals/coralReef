// SPDX-License-Identifier: AGPL-3.0-only
//! Safe Rust wrappers over VFIO kernel ioctls.
//!
//! Each function encapsulates one `unsafe` ioctl call with documented safety
//! invariants. Callers pass valid `BorrowedFd` handles from VFIO opens.

use crate::error::DriverError;
use rustix::io::Result as IoResult;
use rustix::ioctl::{Ioctl, IoctlOutput, Opcode};
use std::borrow::Cow;
use std::os::fd::BorrowedFd;

use super::types::ioctls;
use super::types::iommufd as iommufd_ops;
use super::types::{
    IommuIoasAlloc, IommuIoasMap, IommuIoasUnmap, VfioDeviceAttachIommufdPt, VfioDeviceBindIommufd,
    VfioDeviceInfo, VfioDmaMap, VfioDmaUnmap, VfioGroupStatus, VfioPciHotReset, VfioRegionInfo,
};

/// Ioctl adapter for VFIO commands that return an i32 (no-arg or integer-arg).
pub(crate) struct VfioIoctlReturn<const OP: Opcode> {
    arg: usize,
}

// SAFETY: opcode is a compile-time VFIO constant; as_ptr returns arg cast to
// *mut c_void (integer-arg ioctl); output_from_ptr wraps the kernel return value.
unsafe impl<const OP: Opcode> Ioctl for VfioIoctlReturn<OP> {
    type Output = i32;
    const IS_MUTATING: bool = false;

    fn opcode(&self) -> Opcode {
        OP
    }

    fn as_ptr(&mut self) -> *mut std::ffi::c_void {
        self.arg as *mut std::ffi::c_void
    }

    unsafe fn output_from_ptr(
        out: IoctlOutput,
        _extract_output: *mut std::ffi::c_void,
    ) -> IoResult<Self::Output> {
        // SAFETY: Integer-return VFIO ioctls encode the result in `IoctlOutput`; no
        // structured output at `_extract_output` for this adapter.
        Ok(out)
    }
}

/// Ioctl adapter for VFIO commands that read/write a kernel ABI struct.
pub(crate) struct VfioIoctlPtr<const OP: Opcode, T> {
    ptr: *mut T,
}

// SAFETY: opcode is compile-time constant; T is repr(C) matching kernel ABI;
// IS_MUTATING=true because kernel writes back into the struct.
unsafe impl<const OP: Opcode, T> Ioctl for VfioIoctlPtr<OP, T> {
    type Output = ();
    const IS_MUTATING: bool = true;

    fn opcode(&self) -> Opcode {
        OP
    }

    fn as_ptr(&mut self) -> *mut std::ffi::c_void {
        self.ptr.cast()
    }

    unsafe fn output_from_ptr(
        _out: IoctlOutput,
        _extract_output: *mut std::ffi::c_void,
    ) -> IoResult<Self::Output> {
        // SAFETY: Struct-pointer VFIO ioctls mutate `T` in place; kernel result is
        // not passed separately via `IoctlOutput` for this adapter.
        Ok(())
    }
}

fn vfio_err(op: &str, e: rustix::io::Errno) -> DriverError {
    DriverError::DeviceNotFound(Cow::Owned(format!("VFIO {op}: {e}")))
}

#[inline]
pub(crate) fn get_api_version(fd: BorrowedFd<'_>) -> Result<i32, DriverError> {
    // SAFETY: no-arg VFIO ioctl; fd is valid from caller.
    let ioctl = VfioIoctlReturn::<{ ioctls::OP_GET_API_VERSION }> { arg: 0 };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("GET_API_VERSION", e))
}

#[inline]
pub(crate) fn check_extension(fd: BorrowedFd<'_>, arg: u32) -> Result<i32, DriverError> {
    // SAFETY: integer-arg VFIO ioctl; fd valid; arg is extension id.
    let ioctl = VfioIoctlReturn::<{ ioctls::OP_CHECK_EXTENSION }> { arg: arg as usize };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("CHECK_EXTENSION", e))
}

#[inline]
pub(crate) fn set_iommu(fd: BorrowedFd<'_>, arg: u32) -> Result<i32, DriverError> {
    // SAFETY: integer-arg VFIO ioctl; fd valid; arg is IOMMU type.
    let ioctl = VfioIoctlReturn::<{ ioctls::OP_SET_IOMMU }> { arg: arg as usize };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("SET_IOMMU", e))
}

#[inline]
pub(crate) fn group_status(
    fd: BorrowedFd<'_>,
    arg: &mut VfioGroupStatus,
) -> Result<(), DriverError> {
    // SAFETY: struct ioctl; fd valid; arg has kernel layout with correct argsz.
    let ioctl = VfioIoctlPtr::<{ ioctls::OP_GROUP_GET_STATUS }, _> {
        ptr: std::ptr::from_mut(arg),
    };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("GROUP_GET_STATUS", e))
}

#[inline]
pub(crate) fn group_set_container(
    fd: BorrowedFd<'_>,
    arg: *const std::ffi::c_void,
) -> Result<i32, DriverError> {
    // SAFETY: pointer-arg ioctl; fd valid; arg points to container fd int.
    let ioctl = VfioIoctlReturn::<{ ioctls::OP_GROUP_SET_CONTAINER }> { arg: arg as usize };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("GROUP_SET_CONTAINER", e))
}

#[inline]
pub(crate) fn group_get_device_fd(
    fd: BorrowedFd<'_>,
    arg: *const std::ffi::c_void,
) -> Result<i32, DriverError> {
    // SAFETY: pointer-arg ioctl; fd valid; arg is C string (PCIe BDF).
    let ioctl = VfioIoctlReturn::<{ ioctls::OP_GROUP_GET_DEVICE_FD }> { arg: arg as usize };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("GROUP_GET_DEVICE_FD", e))
}

#[inline]
pub(crate) fn device_info(fd: BorrowedFd<'_>, arg: &mut VfioDeviceInfo) -> Result<(), DriverError> {
    // SAFETY: struct ioctl; fd valid; arg has kernel layout.
    let ioctl = VfioIoctlPtr::<{ ioctls::OP_DEVICE_GET_INFO }, _> {
        ptr: std::ptr::from_mut(arg),
    };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("DEVICE_GET_INFO", e))
}

#[inline]
pub(crate) fn device_get_region_info(
    fd: BorrowedFd<'_>,
    arg: &mut VfioRegionInfo,
) -> Result<(), DriverError> {
    // SAFETY: struct ioctl; fd valid; arg has kernel layout with argsz and index set.
    let ioctl = VfioIoctlPtr::<{ ioctls::OP_DEVICE_GET_REGION_INFO }, _> {
        ptr: std::ptr::from_mut(arg),
    };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("DEVICE_GET_REGION_INFO", e))
}

#[inline]
pub(crate) fn device_reset(fd: BorrowedFd<'_>) -> Result<i32, DriverError> {
    // SAFETY: no-arg VFIO ioctl; fd valid.
    let ioctl = VfioIoctlReturn::<{ ioctls::OP_DEVICE_RESET }> { arg: 0 };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("DEVICE_RESET", e))
}

/// `VFIO_DEVICE_PCI_HOT_RESET` — PCI SBR via the upstream bridge.
///
/// The kernel walks up to the PCIe bridge and asserts Secondary Bus Reset,
/// resetting ALL devices behind the bridge. All IOMMU groups containing
/// affected devices must be represented by open group fds.
#[inline]
pub(crate) fn device_pci_hot_reset(
    fd: BorrowedFd<'_>,
    arg: &mut VfioPciHotReset,
) -> Result<(), DriverError> {
    let ioctl = VfioIoctlPtr::<{ ioctls::OP_DEVICE_PCI_HOT_RESET }, _> {
        ptr: std::ptr::from_mut(arg),
    };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("DEVICE_PCI_HOT_RESET", e))
}

#[inline]
pub(crate) fn dma_map(fd: BorrowedFd<'_>, arg: &VfioDmaMap) -> Result<(), DriverError> {
    // SAFETY: struct ioctl; fd valid (container); arg has kernel layout.
    let ioctl = VfioIoctlPtr::<{ ioctls::OP_IOMMU_MAP_DMA }, VfioDmaMap> {
        ptr: std::ptr::from_ref(arg).cast_mut(),
    };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("IOMMU_MAP_DMA", e))
}

#[inline]
pub(crate) fn dma_unmap(fd: BorrowedFd<'_>, arg: &VfioDmaUnmap) -> Result<(), DriverError> {
    // SAFETY: struct ioctl; fd valid (container); arg has kernel layout.
    let ioctl = VfioIoctlPtr::<{ ioctls::OP_IOMMU_UNMAP_DMA }, VfioDmaUnmap> {
        ptr: std::ptr::from_ref(arg).cast_mut(),
    };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("IOMMU_UNMAP_DMA", e))
}

// ---------------------------------------------------------------------------
// iommufd / VFIO cdev ioctls (kernel 6.2+)
// ---------------------------------------------------------------------------

/// `VFIO_DEVICE_BIND_IOMMUFD` on a cdev device fd.
#[inline]
pub(crate) fn device_bind_iommufd(
    fd: BorrowedFd<'_>,
    arg: &mut VfioDeviceBindIommufd,
) -> Result<(), DriverError> {
    // SAFETY: struct ioctl; fd is an open cdev device fd; arg has kernel layout.
    let ioctl = VfioIoctlPtr::<{ ioctls::OP_DEVICE_BIND_IOMMUFD }, _> {
        ptr: std::ptr::from_mut(arg),
    };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("DEVICE_BIND_IOMMUFD", e))
}

/// `VFIO_DEVICE_ATTACH_IOMMUFD_PT` on a cdev device fd.
#[inline]
pub(crate) fn device_attach_iommufd_pt(
    fd: BorrowedFd<'_>,
    arg: &mut VfioDeviceAttachIommufdPt,
) -> Result<(), DriverError> {
    // SAFETY: struct ioctl; fd is an open cdev device fd; arg has kernel layout.
    let ioctl = VfioIoctlPtr::<{ ioctls::OP_DEVICE_ATTACH_IOMMUFD_PT }, _> {
        ptr: std::ptr::from_mut(arg),
    };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("DEVICE_ATTACH_IOMMUFD_PT", e))
}

/// `IOMMU_IOAS_ALLOC` on an iommufd.
#[inline]
pub(crate) fn iommufd_ioas_alloc(
    fd: BorrowedFd<'_>,
    arg: &mut IommuIoasAlloc,
) -> Result<(), DriverError> {
    // SAFETY: struct ioctl; fd is /dev/iommu; arg has kernel layout.
    let ioctl = VfioIoctlPtr::<{ iommufd_ops::OP_IOAS_ALLOC }, _> {
        ptr: std::ptr::from_mut(arg),
    };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("IOMMU_IOAS_ALLOC", e))
}

/// `IOMMU_IOAS_MAP` on an iommufd.
#[inline]
pub(crate) fn iommufd_ioas_map(
    fd: BorrowedFd<'_>,
    arg: &mut IommuIoasMap,
) -> Result<(), DriverError> {
    // SAFETY: struct ioctl; fd is /dev/iommu; arg has kernel layout.
    let ioctl = VfioIoctlPtr::<{ iommufd_ops::OP_IOAS_MAP }, _> {
        ptr: std::ptr::from_mut(arg),
    };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("IOMMU_IOAS_MAP", e))
}

/// `IOMMU_IOAS_UNMAP` on an iommufd.
#[inline]
pub(crate) fn iommufd_ioas_unmap(
    fd: BorrowedFd<'_>,
    arg: &mut IommuIoasUnmap,
) -> Result<(), DriverError> {
    // SAFETY: struct ioctl; fd is /dev/iommu; arg has kernel layout.
    let ioctl = VfioIoctlPtr::<{ iommufd_ops::OP_IOAS_UNMAP }, _> {
        ptr: std::ptr::from_mut(arg),
    };
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("IOMMU_IOAS_UNMAP", e))
}

// ---------------------------------------------------------------------------
// VFIO IRQ ioctls
// ---------------------------------------------------------------------------

/// `VFIO_DEVICE_GET_IRQ_INFO` — query IRQ capabilities for an index.
#[inline]
pub(crate) fn device_get_irq_info<T>(fd: BorrowedFd<'_>, arg: &mut T) -> Result<(), DriverError> {
    let ioctl = VfioIoctlPtr::<{ ioctls::OP_DEVICE_GET_IRQ_INFO }, _> {
        ptr: std::ptr::from_mut(arg),
    };
    // SAFETY: `fd` is a valid VFIO device fd; `arg` is a mutable reference to a repr(C) struct
    // matching the kernel's vfio_irq_info layout. The kernel reads/writes within the struct bounds.
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("DEVICE_GET_IRQ_INFO", e))
}

/// `VFIO_DEVICE_SET_IRQS` — configure IRQ trigger/masking for a device.
#[inline]
pub(crate) fn device_set_irqs<T>(fd: BorrowedFd<'_>, arg: &mut T) -> Result<(), DriverError> {
    let ioctl = VfioIoctlPtr::<{ ioctls::OP_DEVICE_SET_IRQS }, _> {
        ptr: std::ptr::from_mut(arg),
    };
    // SAFETY: `fd` is a valid VFIO device fd; `arg` is a mutable reference to a repr(C) struct
    // matching the kernel's vfio_irq_set layout. The kernel reads within the struct bounds.
    unsafe { rustix::ioctl::ioctl(fd, ioctl) }.map_err(|e| vfio_err("DEVICE_SET_IRQS", e))
}
