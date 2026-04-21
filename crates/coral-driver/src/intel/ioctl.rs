// SPDX-License-Identifier: AGPL-3.0-or-later
//! Intel DRM ioctl definitions for i915 and xe kernel drivers.
//!
//! These constants and structures mirror the kernel UAPI headers:
//! - `include/uapi/drm/i915_drm.h` (legacy i915 driver)
//! - `include/uapi/drm/xe_drm.h` (new xe driver, Gen 12.50+)
//!
//! # Usage
//!
//! The `IntelDevice` will detect which kernel driver is in use (i915 vs xe)
//! and dispatch to the appropriate ioctl set. Both drivers support GEM
//! buffer objects and batch buffer submission for compute.
//!
//! # Safety
//!
//! All ioctl wrappers are unsafe — they perform raw kernel calls. Callers
//! must ensure valid file descriptors and properly initialized structs.

#![allow(dead_code)]

// ============================================================================
// DRM ioctl base definitions
// ============================================================================

const DRM_IOCTL_BASE: u8 = b'd';
const DRM_COMMAND_BASE: u32 = 0x40;

/// Construct a DRM ioctl number for a driver-specific command.
const fn drm_iowr(nr: u32, size: usize) -> u64 {
    let dir: u64 = 3; // _IOC_READ | _IOC_WRITE
    let ty = DRM_IOCTL_BASE as u64;
    let nr = (DRM_COMMAND_BASE + nr) as u64;
    (dir << 30) | ((size as u64 & 0x3FFF) << 16) | (ty << 8) | nr
}

const fn drm_iow(nr: u32, size: usize) -> u64 {
    let dir: u64 = 1; // _IOC_WRITE
    let ty = DRM_IOCTL_BASE as u64;
    let nr = (DRM_COMMAND_BASE + nr) as u64;
    (dir << 30) | ((size as u64 & 0x3FFF) << 16) | (ty << 8) | nr
}

const fn drm_ior(nr: u32, size: usize) -> u64 {
    let dir: u64 = 2; // _IOC_READ
    let ty = DRM_IOCTL_BASE as u64;
    let nr = (DRM_COMMAND_BASE + nr) as u64;
    (dir << 30) | ((size as u64 & 0x3FFF) << 16) | (ty << 8) | nr
}

// ============================================================================
// i915 driver ioctls
// ============================================================================

pub mod i915 {
    use super::*;

    pub const I915_PARAM_CHIPSET_ID: u32 = 4;
    pub const I915_PARAM_REVISION: u32 = 32;
    pub const I915_PARAM_SUBSLICE_TOTAL: u32 = 33;
    pub const I915_PARAM_EU_TOTAL: u32 = 34;
    pub const I915_PARAM_HAS_EXEC_COMPUTE: u32 = 53;
    pub const I915_PARAM_CS_TIMESTAMP_FREQUENCY: u32 = 51;

    /// `DRM_IOCTL_I915_GETPARAM` — query hardware parameters.
    pub const GETPARAM: u64 = drm_iowr(0x06, size_of::<GetParam>());

    /// `DRM_IOCTL_I915_GEM_CREATE` — allocate a GEM buffer object.
    pub const GEM_CREATE: u64 = drm_iowr(0x0B, size_of::<GemCreate>());

    /// `DRM_IOCTL_I915_GEM_MMAP_OFFSET` — get an mmap offset for a GEM BO.
    pub const GEM_MMAP_OFFSET: u64 = drm_iowr(0x3C, size_of::<GemMmapOffset>());

    /// `DRM_IOCTL_I915_GEM_CLOSE` — close a GEM buffer object handle.
    pub const GEM_CLOSE: u64 = drm_iow(0x09, size_of::<GemClose>());

    /// `DRM_IOCTL_I915_GEM_EXECBUFFER2` — submit a batch buffer for execution.
    pub const GEM_EXECBUFFER2: u64 = drm_iowr(0x29, size_of::<GemExecbuffer2>());

    /// `DRM_IOCTL_I915_GEM_WAIT` — wait for a GEM BO to become idle.
    pub const GEM_WAIT: u64 = drm_iowr(0x2C, size_of::<GemWait>());

    /// `DRM_IOCTL_I915_GEM_CONTEXT_CREATE_EXT` — create an execution context.
    pub const GEM_CONTEXT_CREATE: u64 = drm_iowr(0x2D, size_of::<GemContextCreate>());

    /// `DRM_IOCTL_I915_GEM_CONTEXT_DESTROY` — destroy an execution context.
    pub const GEM_CONTEXT_DESTROY: u64 = drm_iow(0x2E, size_of::<GemContextDestroy>());

    /// Engine class for compute (CCS).
    pub const ENGINE_CLASS_COMPUTE: u16 = 4;
    /// Engine class for render (RCS).
    pub const ENGINE_CLASS_RENDER: u16 = 0;

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct GetParam {
        pub param: i32,
        pub _pad: i32,
        pub value: *mut i32,
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct GemCreate {
        pub size: u64,
        pub handle: u32,
        pub _pad: u32,
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct GemMmapOffset {
        pub handle: u32,
        pub _pad: u32,
        pub offset: u64,
        pub flags: u64,
        pub extensions: u64,
    }

    /// Mmap type: write-combining (for CPU-written staging buffers).
    pub const MMAP_OFFSET_WC: u64 = 1;
    /// Mmap type: write-back (for GPU-local, cacheable access).
    pub const MMAP_OFFSET_WB: u64 = 2;
    /// Mmap type: fixed (xe-style, for specific VRAM placement).
    pub const MMAP_OFFSET_FIXED: u64 = 4;

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct GemClose {
        pub handle: u32,
        pub _pad: u32,
    }

    /// Execbuffer2 flags.
    pub const EXEC_DEFAULT: u64 = 0;
    pub const EXEC_RENDER: u64 = 1 << 0;
    pub const EXEC_COMPUTE: u64 = 4 << 0;
    pub const EXEC_FENCE_OUT: u64 = 1 << 17;

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct ExecObject2 {
        pub handle: u32,
        pub relocation_count: u32,
        pub relocs_ptr: u64,
        pub alignment: u64,
        pub offset: u64,
        pub flags: u64,
        pub rsvd1: u64,
        pub rsvd2: u64,
    }

    pub const EXEC_OBJECT_PINNED: u64 = 1 << 4;

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct GemExecbuffer2 {
        pub buffers_ptr: u64,
        pub buffer_count: u32,
        pub batch_start_offset: u32,
        pub batch_len: u32,
        pub dr1: u32,
        pub dr4: u32,
        pub num_cliprects: u32,
        pub cliprects_ptr: u64,
        pub flags: u64,
        pub rsvd1: u64,
        pub rsvd2: u64,
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct GemWait {
        pub bo_handle: u32,
        pub flags: u32,
        pub timeout_ns: i64,
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct GemContextCreate {
        pub ctx_id: u32,
        pub flags: u32,
        pub extensions: u64,
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct GemContextDestroy {
        pub ctx_id: u32,
        pub _pad: u32,
    }

    use std::mem::size_of;
}

// ============================================================================
// xe driver ioctls (Gen 12.50+ / DG2 / Battlemage)
// ============================================================================

pub mod xe {
    use super::*;

    pub const XE_DEVICE_QUERY: u64 = drm_iowr(0x00, size_of::<DeviceQuery>());
    pub const XE_GEM_CREATE: u64 = drm_iowr(0x01, size_of::<GemCreate>());
    pub const XE_GEM_MMAP_OFFSET: u64 = drm_iowr(0x02, size_of::<GemMmapOffset>());
    pub const XE_VM_CREATE: u64 = drm_iowr(0x03, size_of::<VmCreate>());
    pub const XE_VM_DESTROY: u64 = drm_iow(0x04, size_of::<VmDestroy>());
    pub const XE_VM_BIND: u64 = drm_iowr(0x05, size_of::<VmBind>());
    pub const XE_EXEC_QUEUE_CREATE: u64 = drm_iowr(0x06, size_of::<ExecQueueCreate>());
    pub const XE_EXEC_QUEUE_DESTROY: u64 = drm_iow(0x07, size_of::<ExecQueueDestroy>());
    pub const XE_EXEC: u64 = drm_iowr(0x08, size_of::<Exec>());
    pub const XE_WAIT_USER_FENCE: u64 = drm_iowr(0x09, size_of::<WaitUserFence>());
    pub const XE_GEM_CLOSE: u64 = drm_iow(0x0A, size_of::<GemClose>());

    pub const ENGINE_CLASS_COMPUTE: u16 = 1;
    pub const ENGINE_CLASS_RENDER: u16 = 0;

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct DeviceQuery {
        pub extensions: u64,
        pub query: u32,
        pub size: u32,
        pub data: u64,
    }

    pub const QUERY_ENGINES: u32 = 0;
    pub const QUERY_MEM_REGIONS: u32 = 1;
    pub const QUERY_CONFIG: u32 = 2;
    pub const QUERY_GT_LIST: u32 = 3;
    pub const QUERY_HWCONFIG: u32 = 4;
    pub const QUERY_GT_TOPOLOGY: u32 = 5;
    pub const QUERY_ENGINE_CYCLES: u32 = 6;
    pub const QUERY_UC_FW_VERSION: u32 = 7;

    /// GEM memory placement flags.
    pub const MEM_REGION_VRAM: u32 = 1;
    pub const MEM_REGION_SMEM: u32 = 0;

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct GemCreate {
        pub extensions: u64,
        pub size: u64,
        pub placement: u32,
        pub flags: u32,
        pub vm_id: u32,
        pub handle: u32,
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct GemMmapOffset {
        pub extensions: u64,
        pub handle: u32,
        pub flags: u32,
        pub offset: u64,
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct GemClose {
        pub handle: u32,
        pub _pad: u32,
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct VmCreate {
        pub extensions: u64,
        pub flags: u64,
        pub vm_id: u32,
        pub _pad: u32,
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct VmDestroy {
        pub vm_id: u32,
        pub _pad: u32,
    }

    /// VM bind operation type.
    pub const VM_BIND_OP_MAP: u32 = 0;
    pub const VM_BIND_OP_UNMAP: u32 = 1;
    pub const VM_BIND_OP_MAP_USERPTR: u32 = 2;
    pub const VM_BIND_OP_PREFETCH: u32 = 3;

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct VmBindOp {
        pub extensions: u64,
        pub obj: u32,
        pub _pad: u32,
        pub obj_offset: u64,
        pub range: u64,
        pub addr: u64,
        pub gt_mask: u64,
        pub op: u32,
        pub flags: u32,
        pub prefetch_mem_region_instance: u32,
        pub _pad2: u32,
        pub reserved: [u64; 2],
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct VmBind {
        pub extensions: u64,
        pub vm_id: u32,
        pub exec_queue_id: u32,
        pub num_binds: u32,
        pub _pad: u32,
        pub bind: u64,
        pub _pad2: [u64; 2],
        pub num_syncs: u32,
        pub _pad3: u32,
        pub syncs: u64,
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct ExecQueueCreate {
        pub extensions: u64,
        pub width: u16,
        pub num_placements: u16,
        pub vm_id: u32,
        pub flags: u32,
        pub exec_queue_id: u32,
        pub instances: u64,
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct ExecQueueDestroy {
        pub exec_queue_id: u32,
        pub _pad: u32,
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct Exec {
        pub extensions: u64,
        pub exec_queue_id: u32,
        pub num_syncs: u32,
        pub syncs: u64,
        pub address: u64,
        pub num_batch_buffer: u16,
        pub _pad: [u16; 3],
    }

    #[repr(C)]
    #[derive(Debug, Default)]
    pub struct WaitUserFence {
        pub extensions: u64,
        pub addr: u64,
        pub op: u16,
        pub flags: u16,
        pub _pad: u32,
        pub value: u64,
        pub mask: u64,
        pub timeout: i64,
        pub exec_queue_id: u32,
        pub _pad2: u32,
    }

    pub const UFENCE_WAIT_OP_EQ: u16 = 0;
    pub const UFENCE_WAIT_OP_NEQ: u16 = 1;
    pub const UFENCE_WAIT_OP_GT: u16 = 2;
    pub const UFENCE_WAIT_OP_GTE: u16 = 3;
    pub const UFENCE_WAIT_OP_LT: u16 = 4;
    pub const UFENCE_WAIT_OP_LTE: u16 = 5;

    use std::mem::size_of;
}

// ============================================================================
// Compute command encoding (GPGPU_WALKER / COMPUTE_WALKER)
// ============================================================================

/// Intel EU compute dispatch command (GPGPU_WALKER for Gen 9-12,
/// COMPUTE_WALKER for Gen 12.50+).
///
/// These are the batch buffer commands that configure and launch compute
/// threads on the EU array.
pub mod compute_cmd {
    /// Bits needed for GPGPU_WALKER (Gen 9-12.0).
    pub const GPGPU_WALKER_OPCODE: u32 = 0x7105;
    /// Length field for GPGPU_WALKER (15 DWORDs total, so length = 13).
    pub const GPGPU_WALKER_LENGTH: u32 = 13;

    /// COMPUTE_WALKER opcode (Gen 12.50+, i.e. xe driver).
    pub const COMPUTE_WALKER_OPCODE: u32 = 0x7229;

    /// INTERFACE_DESCRIPTOR_DATA (IDD) encodes kernel metadata per EU thread
    /// dispatch: shader offset, binding table pointer, sampler state, etc.
    pub const IDD_SIZE_DWORDS: usize = 8;

    /// MEDIA_INTERFACE_DESCRIPTOR_LOAD opcode.
    pub const MI_LOAD_IDD_OPCODE: u32 = 0x7002;

    /// PIPE_CONTROL — flush/invalidate caches and signal completion.
    pub const PIPE_CONTROL_OPCODE: u32 = 0x7A04;
    pub const PC_FLUSH_ENABLE: u32 = 1 << 7;
    pub const PC_DC_FLUSH: u32 = 1 << 5;
    pub const PC_CS_STALL: u32 = 1 << 20;
    pub const PC_POST_SYNC_WRITE_IMM: u32 = 1 << 14;

    /// MI_BATCH_BUFFER_END — terminate a batch buffer.
    pub const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;

    /// Encode a minimal GPGPU_WALKER command for a 1D dispatch.
    ///
    /// Returns the command words. The caller writes these into a GEM batch
    /// buffer before exec submission.
    #[must_use]
    pub fn encode_gpgpu_walker(
        idd_offset: u32,
        group_count: [u32; 3],
        local_size: [u32; 3],
    ) -> Vec<u32> {
        let simd_size = if local_size[0] >= 32 { 2 } else if local_size[0] >= 16 { 1 } else { 0 };

        let thread_width_x = (local_size[0].max(1) + (1 << (simd_size + 3)) - 1) >> (simd_size + 3);
        let thread_height_y = local_size[1].max(1);
        let thread_depth_z = local_size[2].max(1);

        let mut cmd = vec![0u32; 15];
        cmd[0] = (GPGPU_WALKER_OPCODE << 16) | (GPGPU_WALKER_LENGTH & 0xFF);
        cmd[1] = idd_offset;
        cmd[2] = simd_size;
        cmd[3] = thread_width_x - 1;
        cmd[4] = 0; // group start X
        cmd[5] = 0;
        cmd[6] = group_count[0];
        cmd[7] = thread_height_y - 1;
        cmd[8] = 0; // group start Y
        cmd[9] = 0;
        cmd[10] = group_count[1];
        cmd[11] = thread_depth_z - 1;
        cmd[12] = 0; // group start Z
        cmd[13] = 0;
        cmd[14] = group_count[2];
        cmd
    }

    /// Encode a PIPE_CONTROL command for compute completion fencing.
    #[must_use]
    pub fn encode_pipe_control_fence(fence_addr: u64, fence_value: u32) -> [u32; 6] {
        let flags = PC_CS_STALL | PC_DC_FLUSH | PC_FLUSH_ENABLE | PC_POST_SYNC_WRITE_IMM;
        [
            (PIPE_CONTROL_OPCODE << 16) | 4, // length = 4 (6 DW total)
            flags,
            (fence_addr & 0xFFFF_FFFC) as u32,
            (fence_addr >> 32) as u32,
            fence_value,
            0, // immediate data high (unused for 32-bit write)
        ]
    }
}

// ============================================================================
// Detection: which Intel DRM driver is loaded?
// ============================================================================

/// Detected Intel DRM driver backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntelDriver {
    /// Legacy i915 driver (Gen 9 through Gen 12.0 / DG2 with i915).
    I915,
    /// New xe driver (Gen 12.50+ / DG2 rebind / Battlemage).
    Xe,
}

/// Attempt to detect which Intel DRM driver is active.
///
/// Checks for `/dev/dri/renderD*` nodes and reads the driver name via
/// `DRM_IOCTL_VERSION`. Returns `None` if no Intel GPU is found.
pub fn detect_driver() -> Option<IntelDriver> {
    for i in 128..136 {
        let path = format!("/dev/dri/renderD{i}");
        let driver = detect_driver_for_node(&path);
        if driver.is_some() {
            return driver;
        }
    }
    None
}

fn detect_driver_for_node(path: &str) -> Option<IntelDriver> {
    use crate::drm;
    use std::os::unix::io::AsRawFd;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .ok()?;
    let (_ver, name) = drm::drm_version(file.as_raw_fd()).ok()?;

    match name.as_str() {
        "i915" => Some(IntelDriver::I915),
        "xe" => Some(IntelDriver::Xe),
        _ => None,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i915_ioctl_numbers_non_zero() {
        assert_ne!(i915::GETPARAM, 0);
        assert_ne!(i915::GEM_CREATE, 0);
        assert_ne!(i915::GEM_EXECBUFFER2, 0);
        assert_ne!(i915::GEM_WAIT, 0);
        assert_ne!(i915::GEM_CONTEXT_CREATE, 0);
    }

    #[test]
    fn xe_ioctl_numbers_non_zero() {
        assert_ne!(xe::XE_DEVICE_QUERY, 0);
        assert_ne!(xe::XE_GEM_CREATE, 0);
        assert_ne!(xe::XE_VM_CREATE, 0);
        assert_ne!(xe::XE_EXEC, 0);
        assert_ne!(xe::XE_WAIT_USER_FENCE, 0);
    }

    #[test]
    fn i915_and_xe_ioctl_distinct() {
        assert_ne!(i915::GEM_CREATE, xe::XE_GEM_CREATE);
    }

    #[test]
    fn gpgpu_walker_encodes_correct_length() {
        let cmd = compute_cmd::encode_gpgpu_walker(0, [4, 1, 1], [64, 1, 1]);
        assert_eq!(cmd.len(), 15);
        assert_eq!(cmd[0] >> 16, compute_cmd::GPGPU_WALKER_OPCODE);
        assert_eq!(cmd[6], 4); // group_count_x
    }

    #[test]
    fn pipe_control_fence_encodes_address() {
        let fence = compute_cmd::encode_pipe_control_fence(0x1000_DEAD_BEEF_0000, 0x42);
        assert_eq!(fence.len(), 6);
        assert_eq!(fence[4], 0x42);
    }

    #[test]
    fn detect_driver_graceful_without_hardware() {
        let result = detect_driver();
        // On a machine without Intel GPU, this returns None.
        // On a machine with one, it returns Some(I915) or Some(Xe).
        match result {
            Some(IntelDriver::I915) | Some(IntelDriver::Xe) | None => {}
        }
    }
}
