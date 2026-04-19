// SPDX-License-Identifier: AGPL-3.0-or-later
//! Userspace interface to `coral-kmod.ko` — kernel-privileged RM proxy.
//!
//! When the kernel module is loaded, `/dev/coral-rm` provides ioctls that
//! create compute channels with `RS_PRIV_LEVEL_KERNEL`, enabling
//! `GPU_PROMOTE_CTX` and `KGR_GET_CONTEXT_BUFFERS_INFO` on Blackwell+.
//!
//! If `/dev/coral-rm` is not available, the caller falls back to the
//! direct userspace RM path (which works for pre-Blackwell GPUs).

use std::os::fd::AsRawFd;
use std::os::unix::io::BorrowedFd;

use crate::error::{DriverError, DriverResult};

const CORAL_RM_PATH: &str = "/dev/coral-rm";
const CORAL_IOCTL_MAGIC: u32 = b'C' as u32;

const CORAL_MAX_CTX_BUFFERS: usize = 16;
const CORAL_MAX_BIND_RESOURCES: usize = 16;

/// Result of a successful `CORAL_IOCTL_INIT_COMPUTE`.
///
/// Contains all RM handles needed by coral-driver to operate the channel.
#[derive(Debug)]
#[allow(missing_docs)]
pub struct KmodChannelInfo {
    pub h_client: u32,
    pub h_device: u32,
    pub h_subdevice: u32,
    pub h_vaspace: u32,
    pub h_channel: u32,
    pub h_changrp: u32,
    pub h_ctxshare: u32,
    pub h_compute: u32,
    pub h_virt_mem: u32,
    pub h_usermode: u32,
    pub hw_channel_id: u32,
    pub work_submit_token: u32,
    pub channel_class: u32,
    pub compute_class: u32,
    pub gpfifo_entries: u32,
    pub gpfifo_gpu_va: u64,
    pub h_userd_mem: u32,
    pub h_gpfifo_mem: u32,
    pub h_errnotif_mem: u32,
    pub h_fence_mem: u32,
    pub userd_is_vram: bool,
    pub ctl_fd: i32,
    pub userd_size: u64,
    pub gpfifo_size: u64,
    pub gpu_uuid: [u8; 16],
    pub ctx_bufs: Vec<KmodCtxBuf>,
}

/// A promoted GR context buffer returned by the kernel module.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct KmodCtxBuf {
    pub buffer_id: u16,
    pub h_memory: u32,
    pub size: u64,
    pub gpu_va: u64,
}

/// A bound context buffer resource returned by `CORAL_IOCTL_BIND_CHANNEL`.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct KmodBoundResource {
    pub resource_id: u32,
    pub alignment: u64,
    pub size: u64,
    pub gpu_va: u64,
}

/// Result of a successful `CORAL_IOCTL_BIND_CHANNEL`.
#[derive(Debug)]
#[allow(missing_docs)]
pub struct KmodBindResult {
    pub resource_count: u32,
    pub channel_engine_type: u32,
    pub hw_channel_id: u32,
    pub tsg_id: u32,
    pub resources: Vec<KmodBoundResource>,
}

/// Handle to the `/dev/coral-rm` chardev.
pub struct CoralKmod {
    fd: std::fs::File,
}

// ── ioctl parameter structs (matching coral_kmod_uapi.h) ────────────

#[repr(C)]
#[derive(Default)]
struct CoralCtxBufInfo {
    buffer_id: u16,
    initialized: u8,
    _pad: u8,
    h_memory: u32,
    size: u64,
    gpu_va: u64,
}

#[repr(C)]
struct CoralInitComputeParams {
    gpu_index: u32,
    sm_version: u32,
    h_client: u32,
    h_device: u32,
    h_subdevice: u32,
    h_vaspace: u32,
    h_channel: u32,
    h_changrp: u32,
    h_ctxshare: u32,
    h_compute: u32,
    h_virt_mem: u32,
    h_usermode: u32,
    hw_channel_id: u32,
    work_submit_token: u32,
    channel_class: u32,
    compute_class: u32,
    gpfifo_entries: u32,
    _pad0: u32,
    gpfifo_gpu_va: u64,
    h_userd_mem: u32,
    h_gpfifo_mem: u32,
    h_errnotif_mem: u32,
    h_fence_mem: u32,
    userd_is_vram: u32,
    ctl_fd: i32,
    userd_size: u64,
    gpfifo_size: u64,
    gpu_uuid: [u8; 16],
    ctx_buf_count: u32,
    _pad2: u32,
    ctx_bufs: [CoralCtxBufInfo; CORAL_MAX_CTX_BUFFERS],
    status: u32,
    _pad3: u32,
}

#[repr(C)]
struct CoralDestroyComputeParams {
    h_client: u32,
    status: u32,
}

#[repr(C)]
struct CoralStatusParams {
    version_major: u32,
    version_minor: u32,
    max_gpus: u32,
    active_channels: u32,
}

#[repr(C)]
struct CoralMapMemoryParams {
    h_client: u32,
    resource_type: u32,
    cpu_addr: u64,
    size: u64,
    map_fd: i32,
    status: u32,
}

#[repr(C)]
#[derive(Default)]
struct CoralBindResourceInfo {
    resource_id: u32,
    alignment: u64,
    size: u64,
    gpu_va: u64,
}

#[repr(C)]
struct CoralBindChannelParams {
    gpu_uuid: [u8; 16],
    h_client: u32,
    h_vaspace: u32,
    h_channel: u32,
    sm_version: u32,
    resource_count: u32,
    channel_engine_type: u32,
    hw_channel_id: u32,
    tsg_id: u32,
    resources: [CoralBindResourceInfo; CORAL_MAX_BIND_RESOURCES],
    status: u32,
    _pad: u32,
}

#[repr(C)]
struct CoralAllocGpuBufferParams {
    h_client: u32,
    _pad0: u32,
    size: u64,
    h_memory: u32,
    status: u32,
    gpu_va: u64,
}

#[repr(C)]
struct CoralFreeGpuBufferParams {
    h_client: u32,
    h_memory: u32,
    gpu_va: u64,
    status: u32,
    _pad0: u32,
}

const fn coral_iowr(nr: u32, size: usize) -> u64 {
    let dir: u64 = (crate::drm::IOC_READ | crate::drm::IOC_WRITE) as u64;
    (dir << crate::drm::IOC_DIRSHIFT as u64)
        | ((CORAL_IOCTL_MAGIC as u64) << crate::drm::IOC_TYPESHIFT as u64)
        | ((nr as u64) << crate::drm::IOC_NRSHIFT as u64)
        | ((size as u64) << crate::drm::IOC_SIZESHIFT as u64)
}

const fn coral_ior(nr: u32, size: usize) -> u64 {
    let dir: u64 = crate::drm::IOC_READ as u64;
    (dir << crate::drm::IOC_DIRSHIFT as u64)
        | ((CORAL_IOCTL_MAGIC as u64) << crate::drm::IOC_TYPESHIFT as u64)
        | ((nr as u64) << crate::drm::IOC_NRSHIFT as u64)
        | ((size as u64) << crate::drm::IOC_SIZESHIFT as u64)
}

fn coral_ioctl<T>(fd: std::os::fd::RawFd, request: u64, arg: &mut T, name: &'static str) -> DriverResult<()> {
    crate::drm::drm_ioctl_named(fd, request, arg, name)
}

impl CoralKmod {
    /// Try to open `/dev/coral-rm`. Returns `None` if the module is not loaded.
    pub fn try_open() -> Option<Self> {
        let fd = std::fs::File::options()
            .read(true)
            .write(true)
            .open(CORAL_RM_PATH)
            .ok()?;
        Some(Self { fd })
    }

    /// Check if the kernel module is loaded and responsive.
    pub fn query_status(&self) -> DriverResult<(u32, u32, u32)> {
        let mut params = CoralStatusParams {
            version_major: 0,
            version_minor: 0,
            max_gpus: 0,
            active_channels: 0,
        };

        let cmd = coral_ior(3, std::mem::size_of::<CoralStatusParams>());
        coral_ioctl(self.fd.as_raw_fd(), cmd, &mut params, "CORAL_QUERY_STATUS")?;

        Ok((
            params.version_major,
            params.version_minor,
            params.active_channels,
        ))
    }

    /// Initialize a kernel-privileged compute channel.
    pub fn init_compute(
        &self,
        gpu_index: u32,
        sm_version: u32,
    ) -> DriverResult<KmodChannelInfo> {
        // SAFETY: repr(C) struct, zeroed is valid.
        let mut params: CoralInitComputeParams = unsafe { std::mem::zeroed() };
        params.gpu_index = gpu_index;
        params.sm_version = sm_version;

        let cmd = coral_iowr(1, std::mem::size_of::<CoralInitComputeParams>());
        coral_ioctl(
            self.fd.as_raw_fd(),
            cmd,
            &mut params,
            "CORAL_INIT_COMPUTE",
        )?;
        if params.status != 0 {
            return Err(DriverError::SubmitFailed(
                format!(
                    "CORAL_INIT_COMPUTE: RM status=0x{:08X}",
                    params.status
                )
                .into(),
            ));
        }

        let mut ctx_bufs = Vec::new();
        let count = (params.ctx_buf_count as usize).min(CORAL_MAX_CTX_BUFFERS);
        for cb in &params.ctx_bufs[..count] {
            ctx_bufs.push(KmodCtxBuf {
                buffer_id: cb.buffer_id,
                h_memory: cb.h_memory,
                size: cb.size,
                gpu_va: cb.gpu_va,
            });
        }

        Ok(KmodChannelInfo {
            h_client: params.h_client,
            h_device: params.h_device,
            h_subdevice: params.h_subdevice,
            h_vaspace: params.h_vaspace,
            h_channel: params.h_channel,
            h_changrp: params.h_changrp,
            h_ctxshare: params.h_ctxshare,
            h_compute: params.h_compute,
            h_virt_mem: params.h_virt_mem,
            h_usermode: params.h_usermode,
            hw_channel_id: params.hw_channel_id,
            work_submit_token: params.work_submit_token,
            channel_class: params.channel_class,
            compute_class: params.compute_class,
            gpfifo_entries: params.gpfifo_entries,
            gpfifo_gpu_va: params.gpfifo_gpu_va,
            h_userd_mem: params.h_userd_mem,
            h_gpfifo_mem: params.h_gpfifo_mem,
            h_errnotif_mem: params.h_errnotif_mem,
            h_fence_mem: params.h_fence_mem,
            userd_is_vram: params.userd_is_vram != 0,
            ctl_fd: params.ctl_fd,
            userd_size: params.userd_size,
            gpfifo_size: params.gpfifo_size,
            gpu_uuid: params.gpu_uuid,
            ctx_bufs,
        })
    }

    /// CPU-map a channel resource via the kernel-privileged RM client.
    pub fn map_channel_memory(
        &self,
        h_client: u32,
        resource_type: u32,
    ) -> DriverResult<(u64, u64)> {
        let mut params = CoralMapMemoryParams {
            h_client,
            resource_type,
            cpu_addr: 0,
            size: 0,
            map_fd: 0,
            status: 0,
        };

        let cmd = coral_iowr(4, std::mem::size_of::<CoralMapMemoryParams>());
        coral_ioctl(
            self.fd.as_raw_fd(),
            cmd,
            &mut params,
            "CORAL_MAP_CHANNEL_MEMORY",
        )?;
        if params.status != 0 {
            return Err(DriverError::SubmitFailed(
                format!(
                    "CORAL_MAP_CHANNEL_MEMORY (type={resource_type}): RM status=0x{:08X}",
                    params.status
                )
                .into(),
            ));
        }

        Ok((params.cpu_addr, params.size))
    }

    /// Perform kernel-privileged channel resource binding via nvUvmInterface.
    ///
    /// The caller creates the full RM channel from userspace, then passes
    /// the handles here. The kernel module uses `nvUvmInterfaceRetainChannel`
    /// and `nvUvmInterfaceBindChannelResources` to bind GR context buffers
    /// with kernel privilege, bypassing the `GPU_PROMOTE_CTX` restriction.
    pub fn bind_channel(
        &self,
        gpu_uuid: &[u8; 16],
        h_client: u32,
        h_vaspace: u32,
        h_channel: u32,
        sm_version: u32,
    ) -> DriverResult<KmodBindResult> {
        // SAFETY: repr(C) struct, zeroed is valid.
        let mut params: CoralBindChannelParams = unsafe { std::mem::zeroed() };
        params.gpu_uuid = *gpu_uuid;
        params.h_client = h_client;
        params.h_vaspace = h_vaspace;
        params.h_channel = h_channel;
        params.sm_version = sm_version;

        let cmd = coral_iowr(5, std::mem::size_of::<CoralBindChannelParams>());
        coral_ioctl(
            self.fd.as_raw_fd(),
            cmd,
            &mut params,
            "CORAL_BIND_CHANNEL",
        )?;
        if params.status != 0 {
            return Err(DriverError::SubmitFailed(
                format!(
                    "CORAL_BIND_CHANNEL: RM status=0x{:08X}",
                    params.status
                )
                .into(),
            ));
        }

        let count = (params.resource_count as usize).min(CORAL_MAX_BIND_RESOURCES);
        let resources = params.resources[..count]
            .iter()
            .map(|r| KmodBoundResource {
                resource_id: r.resource_id,
                alignment: r.alignment,
                size: r.size,
                gpu_va: r.gpu_va,
            })
            .collect();

        Ok(KmodBindResult {
            resource_count: params.resource_count,
            channel_engine_type: params.channel_engine_type,
            hw_channel_id: params.hw_channel_id,
            tsg_id: params.tsg_id,
            resources,
        })
    }

    /// Allocate a VRAM buffer and map it into the GPU VA space from kernel
    /// context. Returns `(h_memory, gpu_va)`.
    pub fn alloc_gpu_buffer(
        &self,
        h_client: u32,
        size: u64,
    ) -> DriverResult<(u32, u64)> {
        let mut params: CoralAllocGpuBufferParams = unsafe { std::mem::zeroed() };
        params.h_client = h_client;
        params.size = size;

        let cmd = coral_iowr(6, std::mem::size_of::<CoralAllocGpuBufferParams>());
        coral_ioctl(
            self.fd.as_raw_fd(),
            cmd,
            &mut params,
            "CORAL_ALLOC_GPU_BUFFER",
        )?;
        if params.status != 0 {
            return Err(DriverError::SubmitFailed(
                format!(
                    "CORAL_ALLOC_GPU_BUFFER: RM status=0x{:08X}",
                    params.status
                )
                .into(),
            ));
        }

        Ok((params.h_memory, params.gpu_va))
    }

    /// Free a VRAM buffer previously allocated with `alloc_gpu_buffer`.
    pub fn free_gpu_buffer(
        &self,
        h_client: u32,
        h_memory: u32,
        gpu_va: u64,
    ) -> DriverResult<()> {
        let mut params = CoralFreeGpuBufferParams {
            h_client,
            h_memory,
            gpu_va,
            status: 0,
            _pad0: 0,
        };

        let cmd = coral_iowr(7, std::mem::size_of::<CoralFreeGpuBufferParams>());
        coral_ioctl(
            self.fd.as_raw_fd(),
            cmd,
            &mut params,
            "CORAL_FREE_GPU_BUFFER",
        )?;

        Ok(())
    }

    /// Destroy a kernel-privileged compute channel.
    pub fn destroy_compute(&self, h_client: u32) -> DriverResult<()> {
        let mut params = CoralDestroyComputeParams {
            h_client,
            status: 0,
        };

        let cmd = coral_iowr(2, std::mem::size_of::<CoralDestroyComputeParams>());
        coral_ioctl(
            self.fd.as_raw_fd(),
            cmd,
            &mut params,
            "CORAL_DESTROY_COMPUTE",
        )?;

        Ok(())
    }
}

/// Map the USERD page (contains `GP_PUT` / `GP_GET` offsets).
pub const CORAL_MAP_USERD: u32 = 0;
/// Map the GPFIFO ring buffer.
pub const CORAL_MAP_GPFIFO: u32 = 1;
/// Map the VOLTA_USERMODE doorbell register page.
pub const CORAL_MAP_DOORBELL: u32 = 2;
/// Map the error notifier buffer.
pub const CORAL_MAP_ERRNOTIF: u32 = 3;

/// Map an RM memory object into user-space using a kernel-privileged fd.
///
/// Performs `NV_ESC_RM_MAP_MEMORY` on `ctl_fd` (the fd returned by
/// `init_compute`), then `mmap(MAP_FIXED)` to populate the physical pages.
/// Returns the usable CPU virtual address of the mapping.
///
/// `mmap_fd` is the fd on which the mmap context is created (usually
/// the same as `ctl_fd` for system memory, or the GPU device fd for
/// BAR-mapped objects like VOLTA_USERMODE).
pub fn kmod_map_rm_memory(
    ctl_fd: i32,
    mmap_fd: i32,
    h_client: u32,
    h_device: u32,
    h_memory: u32,
    offset: u64,
    length: u64,
) -> DriverResult<u64> {
    use crate::nv::uvm::structs::NvRmMapMemoryParams;
    use crate::nv::uvm::{nv_ioctl_rw, NV_ESC_RM_MAP_MEMORY};

    let mut params = NvRmMapMemoryParams {
        h_client,
        h_device,
        h_memory,
        pad: 0,
        offset,
        length,
        p_linear_address: 0,
        status: 0,
        flags: 0,
        fd: mmap_fd,
        pad2: 0,
    };

    let ioctl_nr = nv_ioctl_rw(NV_ESC_RM_MAP_MEMORY, std::mem::size_of::<NvRmMapMemoryParams>());
    crate::drm::drm_ioctl_named(ctl_fd, ioctl_nr, &mut params, "RM_MAP_MEMORY(kmod_fd)")?;

    if params.status != 0 {
        return Err(DriverError::SubmitFailed(
            format!(
                "RM_MAP_MEMORY(kmod_fd): status=0x{:08X} h_memory=0x{h_memory:08X}",
                params.status
            )
            .into(),
        ));
    }

    let rm_addr = params.p_linear_address;
    let len = usize::try_from(length)
        .map_err(|_| DriverError::SubmitFailed("length overflow".into()))?;

    // SAFETY: rm_addr and length were validated by RM_MAP_MEMORY.
    // MAP_FIXED replaces the RM-reserved VMA with a page-backed mapping.
    let mapped = unsafe {
        rustix::mm::mmap(
            rm_addr as *mut std::ffi::c_void,
            len,
            rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
            rustix::mm::MapFlags::SHARED | rustix::mm::MapFlags::FIXED,
            BorrowedFd::borrow_raw(mmap_fd),
            0,
        )
    }
    .map_err(|e| {
        DriverError::SubmitFailed(
            format!(
                "mmap after RM_MAP_MEMORY failed: {e} addr=0x{rm_addr:016X} h_memory=0x{h_memory:08X}"
            )
            .into(),
        )
    })?;

    Ok(mapped as u64)
}
