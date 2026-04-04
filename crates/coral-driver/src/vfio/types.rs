// SPDX-License-Identifier: AGPL-3.0-only
//! VFIO kernel ABI types and ioctl opcodes.
//!
//! `repr(C)` structures matching the Linux VFIO ioctl interface, plus
//! ioctl opcodes derived from `_IO(';', base + offset)`.

/// VFIO ioctl opcodes and constants from `<linux/vfio.h>`.
#[expect(
    dead_code,
    reason = "kernel ABI definitions — full surface kept for correctness"
)]
pub(crate) mod ioctls {
    use rustix::ioctl::{Opcode, opcode};

    const VFIO_TYPE: u8 = b';';
    const VFIO_BASE: u8 = 100;

    pub const OP_GET_API_VERSION: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE);
    pub const OP_CHECK_EXTENSION: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 1);
    pub const OP_SET_IOMMU: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 2);

    pub const OP_GROUP_GET_STATUS: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 3);
    pub const OP_GROUP_SET_CONTAINER: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 4);
    pub const OP_GROUP_GET_DEVICE_FD: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 6);

    pub const OP_DEVICE_GET_INFO: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 7);
    pub const OP_DEVICE_GET_REGION_INFO: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 8);
    pub const OP_DEVICE_GET_IRQ_INFO: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 9);
    pub const OP_DEVICE_SET_IRQS: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 10);
    pub const OP_DEVICE_RESET: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 11);

    /// `VFIO_DEVICE_GET_PCI_HOT_RESET_INFO` — query affected groups for hot reset.
    pub const OP_DEVICE_GET_PCI_HOT_RESET_INFO: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 12);
    /// `VFIO_DEVICE_PCI_HOT_RESET` — trigger PCI SBR via upstream bridge.
    /// Same opcode as `OP_IOMMU_MAP_DMA` (VFIO_BASE+13) — kernel dispatches by fd type.
    pub const OP_DEVICE_PCI_HOT_RESET: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 13);

    pub const OP_IOMMU_MAP_DMA: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 13);
    pub const OP_IOMMU_UNMAP_DMA: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 14);

    pub const VFIO_API_VERSION: i32 = 0;

    pub const VFIO_TYPE1V2_IOMMU: u32 = 3;

    pub const VFIO_GROUP_FLAGS_VIABLE: u32 = 1 << 0;

    pub const VFIO_DMA_MAP_FLAG_READ: u32 = 1 << 0;
    pub const VFIO_DMA_MAP_FLAG_WRITE: u32 = 1 << 1;

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "used by NvVfioComputeDevice BAR0 region access when wired in"
        )
    )]
    pub const BAR0_REGION_INDEX: u32 = 0;

    // --- VFIO device-level ioctls for iommufd binding (kernel 6.2+) ---

    /// `VFIO_DEVICE_BIND_IOMMUFD` — bind a cdev device fd to an iommufd.
    pub const OP_DEVICE_BIND_IOMMUFD: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 18);
    /// `VFIO_DEVICE_ATTACH_IOMMUFD_PT` — attach device to an IOAS or hwpt.
    pub const OP_DEVICE_ATTACH_IOMMUFD_PT: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 19);
    /// `VFIO_DEVICE_DETACH_IOMMUFD_PT` — detach device from IOAS.
    #[expect(
        dead_code,
        reason = "reserved for explicit detach; fd close also detaches"
    )]
    pub const OP_DEVICE_DETACH_IOMMUFD_PT: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 20);
}

/// Opcodes and constants for the iommufd subsystem (`/dev/iommu`, kernel 6.2+).
///
/// These mirror `<linux/iommufd.h>`. The iommufd type byte is `';'` (same as
/// VFIO) but command numbers live in the `0x80+` range.
pub(crate) mod iommufd {
    use rustix::ioctl::{Opcode, opcode};

    const IOMMUFD_TYPE: u8 = b';';

    #[expect(dead_code, reason = "iommufd opcode — not yet used")]
    pub const OP_DESTROY: Opcode = opcode::none(IOMMUFD_TYPE, 0x80);
    pub const OP_IOAS_ALLOC: Opcode = opcode::none(IOMMUFD_TYPE, 0x81);
    pub const OP_IOAS_MAP: Opcode = opcode::none(IOMMUFD_TYPE, 0x85);
    pub const OP_IOAS_UNMAP: Opcode = opcode::none(IOMMUFD_TYPE, 0x86);

    pub const IOAS_MAP_FIXED_IOVA: u32 = 1 << 0;
    pub const IOAS_MAP_WRITEABLE: u32 = 1 << 1;
    pub const IOAS_MAP_READABLE: u32 = 1 << 2;
}

// ---------------------------------------------------------------------------
// VFIO device-level structs for iommufd binding (kernel 6.2+, `<linux/vfio.h>`)
// ---------------------------------------------------------------------------

/// `struct vfio_device_bind_iommufd` — bind a cdev device fd to an iommufd.
#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct VfioDeviceBindIommufd {
    pub argsz: u32,
    pub flags: u32,
    /// Input: raw fd of the opened `/dev/iommu`.
    pub iommufd: i32,
    /// Output: device id within the iommufd.
    pub out_devid: u32,
}

/// `struct vfio_device_attach_iommufd_pt` — attach device to an IOAS.
#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct VfioDeviceAttachIommufdPt {
    pub argsz: u32,
    pub flags: u32,
    /// Input: IOAS id (from `IOMMU_IOAS_ALLOC`).
    pub pt_id: u32,
}

// ---------------------------------------------------------------------------
// iommufd structs (`<linux/iommufd.h>`, kernel 6.2+)
// ---------------------------------------------------------------------------

/// `struct iommu_ioas_alloc` — allocate an IO Address Space.
#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct IommuIoasAlloc {
    pub size: u32,
    pub flags: u32,
    pub out_ioas_id: u32,
}

/// `struct iommu_ioas_map` — map user VA into an IOAS at a fixed IOVA.
#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct IommuIoasMap {
    pub size: u32,
    pub flags: u32,
    pub ioas_id: u32,
    pub __reserved: u32,
    pub user_va: u64,
    pub length: u64,
    pub iova: u64,
}

/// `struct iommu_ioas_unmap` — unmap an IOVA range from an IOAS.
#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct IommuIoasUnmap {
    pub size: u32,
    pub ioas_id: u32,
    pub iova: u64,
    pub length: u64,
}

/// `struct vfio_pci_hot_reset` — PCI Secondary Bus Reset via VFIO.
///
/// Variable-length: the kernel struct uses a flexible array `__s32 group_fds[]`.
/// This fixed-size variant supports up to 4 groups (sufficient for all known topologies).
#[repr(C)]
#[derive(Debug)]
pub(crate) struct VfioPciHotReset {
    pub argsz: u32,
    pub flags: u32,
    pub count: u32,
    pub group_fds: [i32; 4],
}

/// VFIO device info (kernel ABI).
#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct VfioDeviceInfo {
    pub argsz: u32,
    pub flags: u32,
    pub num_regions: u32,
    pub num_irqs: u32,
}

/// VFIO region info (kernel ABI).
#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct VfioRegionInfo {
    pub argsz: u32,
    pub flags: u32,
    pub index: u32,
    pub cap_offset: u32,
    pub size: u64,
    pub offset: u64,
}

/// VFIO group status (kernel ABI).
#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct VfioGroupStatus {
    pub argsz: u32,
    pub flags: u32,
}

/// VFIO DMA map request (kernel ABI).
#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct VfioDmaMap {
    pub argsz: u32,
    pub flags: u32,
    pub vaddr: u64,
    pub iova: u64,
    pub size: u64,
}

/// VFIO DMA unmap request (kernel ABI).
#[repr(C)]
#[derive(Debug, Default)]
pub(crate) struct VfioDmaUnmap {
    pub argsz: u32,
    pub flags: u32,
    pub iova: u64,
    pub size: u64,
}

/// Parameters for polling a BAR register until a condition is met.
#[derive(Clone, Copy)]
#[expect(
    dead_code,
    reason = "used by NvVfioComputeDevice BAR0 polling in step 2"
)]
pub(crate) struct PollConfig<'a> {
    pub reg_offset: usize,
    pub done_mask: u32,
    pub error_mask: u32,
    pub max_polls: u32,
    pub yield_interval: u32,
    pub timeout_msg: &'a str,
    pub error_msg: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_info_default() {
        let info = VfioDeviceInfo::default();
        assert_eq!(info.argsz, 0);
        assert_eq!(info.num_regions, 0);
        assert_eq!(info.num_irqs, 0);
    }

    #[test]
    fn group_status_default() {
        let s = VfioGroupStatus::default();
        assert_eq!(s.argsz, 0);
        assert_eq!(s.flags, 0);
    }

    #[test]
    fn region_info_default() {
        let r = VfioRegionInfo::default();
        assert_eq!(r.size, 0);
        assert_eq!(r.offset, 0);
    }

    #[test]
    fn dma_map_default() {
        let m = VfioDmaMap::default();
        assert_eq!(m.vaddr, 0);
        assert_eq!(m.iova, 0);
        assert_eq!(m.size, 0);
    }

    #[test]
    fn dma_unmap_default() {
        let u = VfioDmaUnmap::default();
        assert_eq!(u.iova, 0);
        assert_eq!(u.size, 0);
    }

    #[test]
    fn dma_map_flags_combined() {
        let flags = ioctls::VFIO_DMA_MAP_FLAG_READ | ioctls::VFIO_DMA_MAP_FLAG_WRITE;
        assert_eq!(flags, 3);
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "size_of::<T>() as u32 will never overflow for VFIO structs"
    )]
    fn dma_map_struct_layout() {
        let m = VfioDmaMap {
            argsz: std::mem::size_of::<VfioDmaMap>() as u32,
            flags: ioctls::VFIO_DMA_MAP_FLAG_READ | ioctls::VFIO_DMA_MAP_FLAG_WRITE,
            vaddr: 0x1000_0000,
            iova: 0x2000_0000,
            size: 4096,
        };
        assert_eq!(m.argsz, std::mem::size_of::<VfioDmaMap>() as u32);
        assert_eq!(m.iova, 0x2000_0000);
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "size_of::<T>() as u32 will never overflow for VFIO structs"
    )]
    fn dma_unmap_struct_layout() {
        let u = VfioDmaUnmap {
            argsz: std::mem::size_of::<VfioDmaUnmap>() as u32,
            flags: 0,
            iova: 0x1000_0000,
            size: 8192,
        };
        assert_eq!(u.iova, 0x1000_0000);
        assert_eq!(u.size, 8192);
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "size_of::<T>() as u32 will never overflow for VFIO structs"
    )]
    fn device_info_argsz() {
        let info = VfioDeviceInfo {
            argsz: std::mem::size_of::<VfioDeviceInfo>() as u32,
            flags: 0,
            num_regions: 6,
            num_irqs: 3,
        };
        assert!(info.argsz >= 16);
        assert_eq!(info.num_regions, 6);
    }

    #[test]
    fn dma_map_argsz_abi() {
        assert!(std::mem::size_of::<VfioDmaMap>() >= 32);
    }

    #[test]
    fn dma_unmap_argsz_abi() {
        assert!(std::mem::size_of::<VfioDmaUnmap>() >= 24);
    }

    #[test]
    fn region_info_abi_size() {
        assert!(std::mem::size_of::<VfioRegionInfo>() >= 32);
    }

    #[test]
    fn poll_config_lifetime() {
        let cfg = PollConfig {
            reg_offset: 0,
            done_mask: 1,
            error_mask: 2,
            max_polls: 100,
            yield_interval: 10,
            timeout_msg: "timeout",
            error_msg: "error",
        };
        assert_eq!(cfg.max_polls, 100);
    }

    #[test]
    fn aligned_size_4096() {
        let aligned = 4096usize.div_ceil(4096) * 4096;
        assert_eq!(aligned, 4096);
    }

    #[test]
    fn aligned_size_1_byte() {
        let aligned = 1usize.div_ceil(4096) * 4096;
        assert_eq!(aligned, 4096);
    }

    #[test]
    fn aligned_size_4097() {
        let aligned = 4097usize.div_ceil(4096) * 4096;
        assert_eq!(aligned, 8192);
    }

    #[test]
    fn iommufd_bind_struct_layout() {
        assert!(std::mem::size_of::<VfioDeviceBindIommufd>() >= 16);
    }

    #[test]
    fn iommufd_attach_struct_layout() {
        assert!(std::mem::size_of::<VfioDeviceAttachIommufdPt>() >= 12);
    }

    #[test]
    fn iommu_ioas_alloc_layout() {
        assert!(std::mem::size_of::<IommuIoasAlloc>() >= 12);
    }

    #[test]
    fn iommu_ioas_map_layout() {
        assert!(std::mem::size_of::<IommuIoasMap>() >= 40);
    }

    #[test]
    fn iommu_ioas_unmap_layout() {
        assert!(std::mem::size_of::<IommuIoasUnmap>() >= 24);
    }

    #[test]
    fn iommufd_map_flags() {
        let flags =
            iommufd::IOAS_MAP_FIXED_IOVA | iommufd::IOAS_MAP_WRITEABLE | iommufd::IOAS_MAP_READABLE;
        assert_eq!(flags, 7);
    }

    #[test]
    fn vfio_kernel_abi_constants() {
        assert_eq!(ioctls::VFIO_API_VERSION, 0);
        assert_eq!(ioctls::VFIO_TYPE1V2_IOMMU, 3);
        assert_eq!(ioctls::BAR0_REGION_INDEX, 0);
        assert_eq!(ioctls::VFIO_GROUP_FLAGS_VIABLE, 1);
    }

    #[test]
    fn vfio_dma_flag_bits() {
        assert_eq!(ioctls::VFIO_DMA_MAP_FLAG_READ, 1);
        assert_eq!(ioctls::VFIO_DMA_MAP_FLAG_WRITE, 2);
    }

    #[test]
    fn repr_c_struct_sizes_and_alignment() {
        assert!(std::mem::size_of::<VfioDeviceInfo>() >= 16);
        assert!(std::mem::size_of::<VfioRegionInfo>() >= 32);
        assert!(std::mem::align_of::<VfioDmaMap>() <= 8);
        assert!(std::mem::align_of::<IommuIoasMap>() <= 8);
        assert_eq!(std::mem::size_of::<VfioDeviceBindIommufd>(), 16);
        assert_eq!(std::mem::size_of::<VfioDeviceAttachIommufdPt>(), 12);
    }

    #[test]
    fn iommu_ioas_alloc_roundtrip_fields() {
        let mut a = IommuIoasAlloc {
            size: 12,
            flags: 0,
            out_ioas_id: 99,
        };
        assert_eq!(a.size, 12);
        a.out_ioas_id = 0;
        assert_eq!(a.out_ioas_id, 0);
    }
}
