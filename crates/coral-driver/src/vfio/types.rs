// SPDX-License-Identifier: AGPL-3.0-only
//! VFIO kernel ABI types and ioctl opcodes.
//!
//! `repr(C)` structures matching the Linux VFIO ioctl interface, plus
//! ioctl opcodes derived from `_IO(';', base + offset)`.

/// VFIO ioctl opcodes and constants from `<linux/vfio.h>`.
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
    #[expect(dead_code, reason = "VFIO ioctl opcode; reserved for IRQ support")]
    pub const OP_DEVICE_GET_IRQ_INFO: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 9);
    #[expect(dead_code, reason = "VFIO ioctl opcode; reserved for IRQ support")]
    pub const OP_DEVICE_SET_IRQS: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 10);
    pub const OP_DEVICE_RESET: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 11);

    pub const OP_IOMMU_MAP_DMA: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 13);
    pub const OP_IOMMU_UNMAP_DMA: Opcode = opcode::none(VFIO_TYPE, VFIO_BASE + 14);

    pub const VFIO_API_VERSION: i32 = 0;

    pub const VFIO_TYPE1V2_IOMMU: u32 = 3;

    pub const VFIO_GROUP_FLAGS_VIABLE: u32 = 1 << 0;

    pub const VFIO_DMA_MAP_FLAG_READ: u32 = 1 << 0;
    pub const VFIO_DMA_MAP_FLAG_WRITE: u32 = 1 << 1;

    #[expect(dead_code, reason = "used by NvVfioComputeDevice in step 2")]
    pub const BAR0_REGION_INDEX: u32 = 0;
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
}
