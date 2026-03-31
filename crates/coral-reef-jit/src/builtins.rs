// SPDX-License-Identifier: AGPL-3.0-only
//! System register → JIT function parameter mapping.
//!
//! NVIDIA compute shaders use `S2R` (system register read) and `CS2R` instructions
//! to access built-in values like `global_invocation_id`, `workgroup_id`, etc.
//! This module maps system register indices to the corresponding JIT function
//! parameter positions.

/// System register indices (NVIDIA convention, used by `CoralIR` after `naga_translate`).
pub mod sys_regs {
    /// Thread ID X within workgroup (`local_invocation_id.x`).
    pub const SR_TID_X: u8 = 0x21;
    /// Thread ID Y within workgroup (`local_invocation_id.y`).
    pub const SR_TID_Y: u8 = 0x22;
    /// Thread ID Z within workgroup (`local_invocation_id.z`).
    pub const SR_TID_Z: u8 = 0x23;
    /// CTA (workgroup) ID X.
    pub const SR_CTAID_X: u8 = 0x25;
    /// CTA (workgroup) ID Y.
    pub const SR_CTAID_Y: u8 = 0x26;
    /// CTA (workgroup) ID Z.
    pub const SR_CTAID_Z: u8 = 0x27;
    /// Number of threads per workgroup X (`workgroup_size.x`).
    pub const SR_NTID_X: u8 = 0x29;
    /// Number of threads per workgroup Y (`workgroup_size.y`).
    pub const SR_NTID_Y: u8 = 0x2a;
    /// Number of threads per workgroup Z (`workgroup_size.z`).
    pub const SR_NTID_Z: u8 = 0x2b;
    /// Number of CTAs (workgroups) X.
    pub const SR_NCTAID_X: u8 = 0x2d;
    /// Number of CTAs (workgroups) Y.
    pub const SR_NCTAID_Y: u8 = 0x2e;
    /// Number of CTAs (workgroups) Z.
    pub const SR_NCTAID_Z: u8 = 0x2f;
    /// `LaneId` (thread lane within warp) — always 0 for CPU execution.
    pub const SR_LANEID: u8 = 0x00;
    /// Clock counter low bits.
    pub const SR_CLOCK_LO: u8 = 0x50;
}

/// Parameter indices into the JIT function signature.
///
/// The JIT'd kernel function has signature:
/// ```text
/// fn kernel(
///     bindings_ptr: *mut *mut u8,    // [0] pointer to array of binding buffer pointers
///     global_id_x: u32,              // [1]
///     global_id_y: u32,              // [2]
///     global_id_z: u32,              // [3]
///     workgroup_id_x: u32,           // [4]
///     workgroup_id_y: u32,           // [5]
///     workgroup_id_z: u32,           // [6]
///     local_id_x: u32,              // [7]
///     local_id_y: u32,              // [8]
///     local_id_z: u32,              // [9]
///     num_workgroups_x: u32,         // [10]
///     num_workgroups_y: u32,         // [11]
///     num_workgroups_z: u32,         // [12]
///     workgroup_size_x: u32,         // [13]
///     workgroup_size_y: u32,         // [14]
///     workgroup_size_z: u32,         // [15]
/// ) -> ()
/// ```
pub mod params {
    /// Pointer to the binding buffer pointer array.
    pub const BINDINGS_PTR: usize = 0;
    /// Global invocation ID X.
    pub const GLOBAL_ID_X: usize = 1;
    /// Global invocation ID Y.
    pub const GLOBAL_ID_Y: usize = 2;
    /// Global invocation ID Z.
    pub const GLOBAL_ID_Z: usize = 3;
    /// Workgroup ID X.
    pub const WORKGROUP_ID_X: usize = 4;
    /// Workgroup ID Y.
    pub const WORKGROUP_ID_Y: usize = 5;
    /// Workgroup ID Z.
    pub const WORKGROUP_ID_Z: usize = 6;
    /// Local invocation ID X.
    pub const LOCAL_ID_X: usize = 7;
    /// Local invocation ID Y.
    pub const LOCAL_ID_Y: usize = 8;
    /// Local invocation ID Z.
    pub const LOCAL_ID_Z: usize = 9;
    /// Number of workgroups X.
    pub const NUM_WORKGROUPS_X: usize = 10;
    /// Number of workgroups Y.
    pub const NUM_WORKGROUPS_Y: usize = 11;
    /// Number of workgroups Z.
    pub const NUM_WORKGROUPS_Z: usize = 12;
    /// Workgroup size X.
    pub const WORKGROUP_SIZE_X: usize = 13;
    /// Workgroup size Y.
    pub const WORKGROUP_SIZE_Y: usize = 14;
    /// Workgroup size Z.
    pub const WORKGROUP_SIZE_Z: usize = 15;
    /// Total number of kernel function parameters.
    pub const PARAM_COUNT: usize = 16;
}

/// Map a system register index to a JIT function parameter index.
///
/// Returns `None` for unsupported/irrelevant system registers (e.g. `SR_LANEID`
/// returns `Some` with value 0 since CPU execution is single-lane).
#[must_use]
pub const fn sys_reg_to_param(sr_idx: u8) -> Option<SysRegMapping> {
    match sr_idx {
        sys_regs::SR_TID_X => Some(SysRegMapping::Param(params::LOCAL_ID_X)),
        sys_regs::SR_TID_Y => Some(SysRegMapping::Param(params::LOCAL_ID_Y)),
        sys_regs::SR_TID_Z => Some(SysRegMapping::Param(params::LOCAL_ID_Z)),
        sys_regs::SR_CTAID_X => Some(SysRegMapping::Param(params::WORKGROUP_ID_X)),
        sys_regs::SR_CTAID_Y => Some(SysRegMapping::Param(params::WORKGROUP_ID_Y)),
        sys_regs::SR_CTAID_Z => Some(SysRegMapping::Param(params::WORKGROUP_ID_Z)),
        sys_regs::SR_NTID_X => Some(SysRegMapping::Param(params::WORKGROUP_SIZE_X)),
        sys_regs::SR_NTID_Y => Some(SysRegMapping::Param(params::WORKGROUP_SIZE_Y)),
        sys_regs::SR_NTID_Z => Some(SysRegMapping::Param(params::WORKGROUP_SIZE_Z)),
        sys_regs::SR_NCTAID_X => Some(SysRegMapping::Param(params::NUM_WORKGROUPS_X)),
        sys_regs::SR_NCTAID_Y => Some(SysRegMapping::Param(params::NUM_WORKGROUPS_Y)),
        sys_regs::SR_NCTAID_Z => Some(SysRegMapping::Param(params::NUM_WORKGROUPS_Z)),
        sys_regs::SR_LANEID | sys_regs::SR_CLOCK_LO => Some(SysRegMapping::Constant(0)),
        _ => None,
    }
}

/// How a system register maps to the JIT execution context.
#[derive(Debug, Clone, Copy)]
pub enum SysRegMapping {
    /// Index into the kernel function parameters.
    Param(usize),
    /// Fixed constant value (e.g. `LaneId` = 0 for CPU).
    Constant(u32),
}
