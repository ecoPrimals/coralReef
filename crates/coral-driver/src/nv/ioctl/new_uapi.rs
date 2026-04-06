// SPDX-License-Identifier: AGPL-3.0-or-later
//! New nouveau UAPI (kernel 6.6+) — `VM_INIT` / `VM_BIND` / `EXEC` pipeline.
//!
//! Ecosystem experiment Exp-051 confirmed: on kernel 6.17+ with Volta (GV100),
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

/// Initialize kernel-managed VA space. Must be called before `VM_BIND`.
/// NVK uses `kernel_managed_addr = 0x80_0000_0000`, `kernel_managed_size = 0x80_0000_0000`.
///
/// Must match `struct drm_nouveau_vm_init` from kernel UAPI (16 bytes, 2 fields).
/// Previous 4-field (32 byte) version caused EINVAL: ioctl number encodes struct
/// size, so the kernel rejected the mismatched ioctl.
#[repr(C)]
#[derive(Default)]
struct NouveauVmInit {
    kernel_managed_addr: u64,
    kernel_managed_size: u64,
}

/// Bind operation type for `VM_BIND`.
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
enum NouveauVmBindOp {
    Map = 0,
    Unmap = 1,
}

/// Single bind operation within a `VM_BIND` request.
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

/// `VM_BIND` request — maps/unmaps GEM objects to GPU virtual addresses.
///
/// Field order must match kernel `drm_nouveau_vm_bind`:
/// `op_count`, `flags`, `wait_count`, `sig_count`, `wait_ptr`, `sig_ptr`, `op_ptr`.
#[repr(C)]
#[derive(Default)]
struct NouveauVmBind {
    op_count: u32,
    flags: u32,
    wait_count: u32,
    sig_count: u32,
    wait_ptr: u64,
    sig_ptr: u64,
    op_ptr: u64,
}

/// Async `VM_BIND` flag.
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

/// `EXEC` request — submits push buffer for GPU execution.
///
/// Field order must match kernel `drm_nouveau_exec`: `wait_ptr`, `sig_ptr`, `push_ptr`.
#[repr(C)]
#[derive(Default)]
struct NouveauExec {
    channel: u32,
    push_count: u32,
    wait_count: u32,
    sig_count: u32,
    wait_ptr: u64,
    sig_ptr: u64,
    push_ptr: u64,
}

/// `EXEC` push buffer flag: no wait (fire-and-forget).
#[expect(dead_code, reason = "available for fire-and-forget dispatch")]
const NOUVEAU_EXEC_PUSH_NO_WAIT: u32 = 1;

// DRM syncobj ioctl numbers (from linux/drm.h)
const DRM_IOCTL_SYNCOBJ_CREATE: u64 = drm::drm_iowr_pub(0xBF, size_of_u32::<DrmSyncobjCreate>());
const DRM_IOCTL_SYNCOBJ_DESTROY: u64 = drm::drm_iowr_pub(0xC0, size_of_u32::<DrmSyncobjDestroy>());
const DRM_IOCTL_SYNCOBJ_WAIT: u64 = drm::drm_iowr_pub(0xC3, size_of_u32::<DrmSyncobjWait>());

/// DRM syncobj sync entry for nouveau EXEC.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NouveauSync {
    flags: u32,
    handle: u32,
    timeline_value: u64,
}

/// Create a DRM syncobj.
#[repr(C)]
#[derive(Default)]
struct DrmSyncobjCreate {
    handle: u32,
    flags: u32,
}

/// Destroy a DRM syncobj.
#[repr(C)]
#[derive(Default)]
struct DrmSyncobjDestroy {
    handle: u32,
    pad: u32,
}

/// Wait on DRM syncobjs.
#[repr(C)]
#[derive(Default)]
struct DrmSyncobjWait {
    handles: u64,
    timeout_nsec: i64,
    count_handles: u32,
    flags: u32,
    first_signaled: u32,
    pad: u32,
}

const DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL: u32 = 1;

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
    };
    let ioctl_nr = drm::drm_iowr_pub(DRM_NOUVEAU_VM_INIT, size_of_u32::<NouveauVmInit>());
    drm::drm_ioctl_named(fd, ioctl_nr, &mut req, "nouveau_vm_init")
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
    drm::drm_ioctl_named(fd, ioctl_nr, &mut req, "nouveau_vm_bind_map")
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
    drm::drm_ioctl_named(fd, ioctl_nr, &mut req, "nouveau_vm_bind_unmap")
}

/// Create a DRM syncobj for GPU completion signaling.
///
/// Returns the syncobj handle.
///
/// # Errors
///
/// Returns [`crate::error::DriverError`] on kernel failure.
pub fn syncobj_create(fd: RawFd) -> DriverResult<u32> {
    let mut req = DrmSyncobjCreate::default();
    drm::drm_ioctl_named(fd, DRM_IOCTL_SYNCOBJ_CREATE, &mut req, "syncobj_create")?;
    Ok(req.handle)
}

/// Destroy a DRM syncobj.
///
/// # Errors
///
/// Returns [`crate::error::DriverError`] on kernel failure.
pub fn syncobj_destroy(fd: RawFd, handle: u32) -> DriverResult<()> {
    let mut req = DrmSyncobjDestroy {
        handle,
        ..Default::default()
    };
    drm::drm_ioctl_named(fd, DRM_IOCTL_SYNCOBJ_DESTROY, &mut req, "syncobj_destroy")
}

/// Wait on a DRM syncobj with a timeout.
///
/// `timeout_nsec` is an absolute timeout in nanoseconds (`CLOCK_MONOTONIC`).
///
/// # Errors
///
/// Returns [`crate::error::DriverError`] on timeout or kernel failure.
pub fn syncobj_wait(fd: RawFd, handle: u32, timeout_nsec: i64) -> DriverResult<()> {
    let handles = [handle];
    let mut req = DrmSyncobjWait {
        handles: handles.as_ptr() as u64,
        timeout_nsec,
        count_handles: 1,
        flags: DRM_SYNCOBJ_WAIT_FLAGS_WAIT_ALL,
        ..Default::default()
    };
    drm::drm_ioctl_named(fd, DRM_IOCTL_SYNCOBJ_WAIT, &mut req, "syncobj_wait")
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
    drm::drm_ioctl_named(fd, ioctl_nr, &mut req, "nouveau_exec")
}

/// Submit a push buffer with a signaling syncobj for completion tracking.
///
/// Like `exec_submit`, but also signals the given syncobj when the GPU
/// finishes processing the push buffer.
///
/// # Errors
///
/// Returns [`crate::error::DriverError`] on kernel failure.
pub fn exec_submit_with_signal(
    fd: RawFd,
    channel: u32,
    push_va: u64,
    push_len: u32,
    signal_syncobj: u32,
) -> DriverResult<()> {
    let mut push = [NouveauExecPush {
        va: push_va,
        va_len: push_len,
        ..Default::default()
    }];
    let mut sig = [NouveauSync {
        flags: 0,
        handle: signal_syncobj,
        timeline_value: 0,
    }];
    let mut req = NouveauExec {
        channel,
        push_count: 1,
        push_ptr: push.as_mut_ptr() as u64,
        sig_count: 1,
        sig_ptr: sig.as_mut_ptr() as u64,
        ..Default::default()
    };
    let ioctl_nr = drm::drm_iowr_pub(DRM_NOUVEAU_EXEC, size_of_u32::<NouveauExec>());
    drm::drm_ioctl_named(fd, ioctl_nr, &mut req, "nouveau_exec_signal")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_init_struct_size() {
        assert_eq!(
            std::mem::size_of::<NouveauVmInit>(),
            16,
            "NouveauVmInit must match kernel drm_nouveau_vm_init (2× u64 = 16 bytes)"
        );
    }

    #[test]
    fn vm_bind_struct_size() {
        assert_eq!(
            std::mem::size_of::<NouveauVmBind>(),
            40,
            "NouveauVmBind must match kernel drm_nouveau_vm_bind (4× u32 + 3× u64 = 40 bytes)"
        );
    }

    #[test]
    fn vm_bind_entry_struct_size() {
        assert_eq!(
            std::mem::size_of::<NouveauVmBindEntry>(),
            40,
            "NouveauVmBindEntry must match kernel drm_nouveau_vm_bind_op (4× u32 + 3× u64 = 40 bytes)"
        );
    }

    #[test]
    fn exec_struct_size() {
        assert_eq!(
            std::mem::size_of::<NouveauExec>(),
            40,
            "NouveauExec must match kernel drm_nouveau_exec (4× u32 + 3× u64 = 40 bytes)"
        );
    }

    #[test]
    fn exec_push_struct_size() {
        assert_eq!(
            std::mem::size_of::<NouveauExecPush>(),
            16,
            "NouveauExecPush must match kernel drm_nouveau_exec_push (1× u64 + 2× u32 = 16 bytes)"
        );
    }
}
