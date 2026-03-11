// SPDX-License-Identifier: AGPL-3.0-only
//! New nouveau UAPI (kernel 6.6+) — VM_INIT / VM_BIND / EXEC pipeline.
//!
//! hotSpring Exp 051 confirmed: on kernel 6.17+ with Volta (GV100),
//! legacy `CHANNEL_ALLOC` returns EINVAL unless `VM_INIT` is called first.
//! NVK (Mesa 25.1+) uses this path: `VM_INIT` → `CHANNEL_ALLOC` → `VM_BIND` → `EXEC`.

use crate::drm;
use crate::error::DriverResult;
use std::os::unix::io::RawFd;

use super::super::NV_KERNEL_MANAGED_ADDR;
use super::{DRM_NOUVEAU_EXEC, DRM_NOUVEAU_VM_BIND, DRM_NOUVEAU_VM_INIT, size_of_u32};

/// VA space size for kernel-managed allocations (from NVK ioctl trace).
const NV_KERNEL_MANAGED_SIZE: u64 = 0x80_0000_0000;

// ---------------------------------------------------------------------------
// Ioctl structures (must match kernel `nouveau_drm.h` layout, kernel 6.6+)
// ---------------------------------------------------------------------------

/// Initialize kernel-managed VA space. Must be called before VM_BIND.
/// NVK uses `kernel_managed_addr = 0x80_0000_0000`, `kernel_managed_size = 0x80_0000_0000`.
#[repr(C)]
#[derive(Default)]
struct NouveauVmInit {
    kernel_managed_addr: u64,
    kernel_managed_size: u64,
    unmanaged_addr: u64,
    unmanaged_size: u64,
}

/// Bind operation type for VM_BIND.
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
enum NouveauVmBindOp {
    Map = 0,
    Unmap = 1,
}

/// Single bind operation within a VM_BIND request.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NouveauVmBindEntry {
    op: u32,
    flags: u32,
    handle: u32,
    pad: u32,
    addr: u64,
    bo_offset: u64,
    range: u64,
}

/// VM_BIND request — maps/unmaps GEM objects to GPU virtual addresses.
#[repr(C)]
#[derive(Default)]
struct NouveauVmBind {
    op_count: u32,
    flags: u32,
    op_ptr: u64,
    wait_count: u32,
    sig_count: u32,
    wait_ptr: u64,
    sig_ptr: u64,
}

/// Async VM_BIND flag.
#[expect(dead_code, reason = "available for async VM_BIND operations")]
const NOUVEAU_VM_BIND_RUN_ASYNC: u32 = 1;

/// Push buffer entry for EXEC.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NouveauExecPush {
    va: u64,
    va_len: u32,
    flags: u32,
}

/// EXEC request — submits push buffer for GPU execution.
#[repr(C)]
#[derive(Default)]
struct NouveauExec {
    channel: u32,
    push_count: u32,
    wait_count: u32,
    sig_count: u32,
    push_ptr: u64,
    wait_ptr: u64,
    sig_ptr: u64,
}

/// EXEC push buffer flag: no wait (fire-and-forget).
#[expect(dead_code, reason = "available for fire-and-forget dispatch")]
const NOUVEAU_EXEC_PUSH_NO_WAIT: u32 = 1;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize a kernel-managed VA space for the new nouveau UAPI.
///
/// Must be called before `vm_bind`. Uses the same VA base as NVK:
/// `kernel_managed_addr = 0x80_0000_0000`, `kernel_managed_size = 0x80_0000_0000`.
///
/// # Errors
///
/// Returns [`crate::error::DriverError`] if the kernel rejects the request
/// (e.g. old kernel without new UAPI, or VA space already initialized).
pub fn vm_init(fd: RawFd) -> DriverResult<()> {
    let mut req = NouveauVmInit {
        kernel_managed_addr: NV_KERNEL_MANAGED_ADDR,
        kernel_managed_size: NV_KERNEL_MANAGED_SIZE,
        ..Default::default()
    };
    let ioctl_nr = drm::drm_iowr_pub(DRM_NOUVEAU_VM_INIT, size_of_u32::<NouveauVmInit>());
    // SAFETY:
    // 1. Validity:   NouveauVmInit is #[repr(C)] matching kernel drm_nouveau_vm_init
    // 2. Alignment:  stack-allocated, naturally aligned
    // 3. Lifetime:   synchronous ioctl; req outlives the call
    // 4. Exclusivity: &mut req — sole reference
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut req) }
}

/// Bind a GEM buffer object into the GPU virtual address space.
///
/// Maps `range` bytes from `bo_offset` within `gem_handle` at GPU address `va`.
///
/// # Errors
///
/// Returns [`crate::error::DriverError`] on kernel failure.
pub fn vm_bind_map(
    fd: RawFd,
    gem_handle: u32,
    va: u64,
    bo_offset: u64,
    range: u64,
) -> DriverResult<()> {
    let mut entry = NouveauVmBindEntry {
        op: NouveauVmBindOp::Map as u32,
        handle: gem_handle,
        addr: va,
        bo_offset,
        range,
        ..Default::default()
    };
    let mut req = NouveauVmBind {
        op_count: 1,
        op_ptr: std::ptr::from_mut(&mut entry) as u64,
        ..Default::default()
    };
    let ioctl_nr = drm::drm_iowr_pub(DRM_NOUVEAU_VM_BIND, size_of_u32::<NouveauVmBind>());
    // SAFETY:
    // 1. Validity:   NouveauVmBind + NouveauVmBindEntry are #[repr(C)] matching kernel structs
    // 2. Alignment:  stack-allocated, naturally aligned
    // 3. Lifetime:   synchronous ioctl; req + entry outlive the call
    // 4. Exclusivity: &mut req — sole reference; entry pointer valid for ioctl duration
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut req) }
}

/// Unmap a GPU virtual address range.
///
/// # Errors
///
/// Returns [`crate::error::DriverError`] on kernel failure.
pub fn vm_bind_unmap(fd: RawFd, va: u64, range: u64) -> DriverResult<()> {
    let mut entry = NouveauVmBindEntry {
        op: NouveauVmBindOp::Unmap as u32,
        addr: va,
        range,
        ..Default::default()
    };
    let mut req = NouveauVmBind {
        op_count: 1,
        op_ptr: std::ptr::from_mut(&mut entry) as u64,
        ..Default::default()
    };
    let ioctl_nr = drm::drm_iowr_pub(DRM_NOUVEAU_VM_BIND, size_of_u32::<NouveauVmBind>());
    // SAFETY: same as vm_bind_map — #[repr(C)] structs, synchronous ioctl
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut req) }
}

/// Submit a push buffer for GPU execution via the new UAPI.
///
/// `push_va` is the GPU virtual address of the push buffer data.
/// `push_len` is the byte length of the push data.
///
/// # Errors
///
/// Returns [`crate::error::DriverError`] on kernel failure.
pub fn exec_submit(fd: RawFd, channel: u32, push_va: u64, push_len: u32) -> DriverResult<()> {
    let mut push = [NouveauExecPush {
        va: push_va,
        va_len: push_len,
        ..Default::default()
    }];
    let mut req = NouveauExec {
        channel,
        push_count: 1,
        push_ptr: push.as_mut_ptr() as u64,
        ..Default::default()
    };
    let ioctl_nr = drm::drm_iowr_pub(DRM_NOUVEAU_EXEC, size_of_u32::<NouveauExec>());
    // SAFETY:
    // 1. Validity:   NouveauExec + NouveauExecPush are #[repr(C)] matching kernel structs
    // 2. Alignment:  stack-allocated, naturally aligned
    // 3. Lifetime:   synchronous ioctl; req + push outlive the call
    // 4. Exclusivity: &mut req — sole reference; push pointer valid for ioctl duration
    unsafe { drm::drm_ioctl_typed(fd, ioctl_nr, &mut req) }
}
