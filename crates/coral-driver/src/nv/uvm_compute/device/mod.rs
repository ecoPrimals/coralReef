// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`NvUvmComputeDevice`] construction, RM channel setup, and GPFIFO submission.

use std::collections::HashMap;

use crate::error::DriverResult;
use crate::nv::uvm::{NvGpuDevice, NvUvmDevice, RmClient};

use super::types::{CtxBuffer, GpuGen, UvmBuffer};

mod gpfifo;
mod memory;
mod open_kmod;
mod open_userspace;

/// Compute device backed by the NVIDIA proprietary driver (RM + UVM).
///
/// Implements the full dispatch pipeline: RM object allocation, UVM memory
/// mapping, QMD construction (via reused `qmd.rs`), and GPFIFO submission.
///
/// ## Thread safety (`Send` / `Sync`)
///
/// The type holds `std::fs::File` handles and kernel-mapped CPU addresses returned
/// by the RM/UVM ioctls. Those OS resources are safe to move across threads (`Send`)
/// and to share by immutable reference (`Sync`) when the public API contract is
/// followed: mutating operations go through `&mut self` on [`crate::ComputeDevice`],
/// and the embedded [`std::collections::HashMap`] keys/values are `Send` + `Sync`.
/// GPU submission is serialized through the single GPFIFO channel owned by this
/// struct; no unsynchronized interior mutability exposes hardware state.
pub struct NvUvmComputeDevice {
    pub(super) client: RmClient,
    /// RM UVM ioctl surface (`/dev/nvidia-uvm`); held for VA map/unmap and GPU registration.
    pub(super) uvm: NvUvmDevice,
    /// DRM/render GPU node; held for mmap, RM ioctl context, and device identity.
    pub(super) gpu: NvGpuDevice,
    pub(super) gpu_gen: GpuGen,
    pub(super) h_device: u32,
    #[expect(dead_code, reason = "held for RM_CONTROL calls (e.g. perf queries)")]
    pub(super) h_subdevice: u32,
    #[expect(
        dead_code,
        reason = "held for VA space teardown and future sub-allocations"
    )]
    pub(super) h_vaspace: u32,
    #[expect(dead_code, reason = "held for channel group teardown")]
    pub(super) h_changrp: u32,
    #[expect(
        dead_code,
        reason = "held for channel teardown / GPFIFO ring ownership"
    )]
    pub(super) h_channel: u32,
    pub(super) h_compute: u32,
    /// Stable GPU UUID — used for `UVM_MAP_EXTERNAL_ALLOCATION` and teardown identity.
    pub(super) gpu_uuid: [u8; 16],
    pub(super) buffers: HashMap<u32, UvmBuffer>,
    /// GR context buffers promoted to RM via `GPU_PROMOTE_CTX`.
    /// Freed on drop to release the RM allocations.
    pub(super) ctx_buffers: Vec<CtxBuffer>,
    pub(super) next_handle: u32,
    pub(super) next_mem_handle: u32,
    /// Inflight temporary buffers that survive until `sync()`.
    pub(super) inflight: Vec<crate::BufferHandle>,
    /// Deferred-free buffers from previous dispatches. Freed on drop or
    /// when explicitly drained. Prevents VA recycling races on Blackwell.
    pub(super) deferred_free: Vec<crate::BufferHandle>,
    /// CPU-mapped pointer to the USERD page (for `GP_PUT` doorbell writes).
    pub(super) userd_cpu_addr: u64,
    /// CPU-mapped pointer to the GPFIFO ring (for writing GPFIFO entries).
    pub(super) gpfifo_cpu_addr: u64,
    /// Current `GP_PUT` index (next slot to write in the GPFIFO ring).
    pub(super) gp_put: u32,
    /// Handle of the `NV01_MEMORY_VIRTUAL` for DMA mapping.
    pub(super) h_virt_mem: u32,
    #[expect(dead_code, reason = "kept alive for USERD mmap lifetime")]
    pub(super) userd_mmap_fd: std::fs::File,
    #[expect(dead_code, reason = "kept alive for GPFIFO mmap lifetime")]
    pub(super) gpfifo_mmap_fd: std::fs::File,
    #[expect(dead_code, reason = "kept alive for USERMODE doorbell mmap lifetime")]
    pub(super) usermode_mmap_fd: std::fs::File,
    /// CPU-mapped pointer to the error notifier buffer (16 bytes per entry).
    pub(super) errnotif_cpu_addr: u64,
    #[expect(dead_code, reason = "kept alive for error notifier mmap lifetime")]
    pub(super) errnotif_mmap_fd: std::fs::File,
    /// CPU-mapped pointer to the USERMODE doorbell register page.
    pub(super) doorbell_addr: u64,
    /// Work submit token returned by RM (written to doorbell to notify GPU).
    pub(super) work_submit_token: u32,
    /// Which subchannel the compute engine is bound to.
    /// Blackwell proprietary: 1 (matches GR engine type from RM bind).
    /// Pre-Blackwell / nouveau: 0 (SET_OBJECT on subchannel 0).
    pub(super) compute_subchannel: u32,
    /// Whether this GPU uses semaphore-based completion (Blackwell+).
    /// Blackwell removed GP_GET from the USERD control struct, so we must
    /// use a semaphore release in the push buffer to signal completion.
    pub(super) uses_semaphore_fence: bool,
    /// CPU-mapped address of the 4-byte fence value (for semaphore completion).
    pub(super) fence_cpu_addr: u64,
    /// GPU virtual address of the fence buffer (for semaphore release target).
    pub(super) fence_gpu_va: u64,
    /// Current fence value (incremented on each submission).
    pub(super) fence_value: u32,
    #[expect(dead_code, reason = "kept alive for fence mmap lifetime")]
    pub(super) fence_mmap_fd: Option<std::fs::File>,
    /// CPU-mapped address of the persistent fence push buffer (6 dwords).
    pub(super) fence_pb_cpu_addr: u64,
    /// GPU virtual address of the fence push buffer.
    pub(super) fence_pb_gpu_va: u64,
    #[expect(dead_code, reason = "kept alive for fence pb mmap lifetime")]
    pub(super) fence_pb_mmap_fd: Option<std::fs::File>,
    /// Handle to `/dev/coral-rm` for kmod-based buffer allocation (Blackwell+).
    pub(super) coral_kmod: Option<crate::nv::coral_kmod::CoralKmod>,
    /// h_client from kmod's INIT_COMPUTE, needed for kmod buffer ops.
    pub(super) kmod_h_client: u32,
    /// Whether this device uses UVM external mapping (Blackwell+ externally-owned VA space)
    /// instead of RM DMA mapping. When true, `gpu_map_buffer` uses
    /// `UVM_CREATE_EXTERNAL_RANGE` + `UVM_MAP_EXTERNAL_ALLOCATION`.
    pub(super) uses_uvm_mapping: bool,
    /// Next available GPU VA for bump allocation (only used when `uses_uvm_mapping` is true).
    pub(super) uvm_va_next: u64,
    /// Vendor-agnostic hardware capabilities (built from GenerationProfile at open time).
    pub(super) caps: crate::HardwareCapabilities,
}

impl NvUvmComputeDevice {
    /// Open a UVM compute device for the specified GPU index and SM version.
    ///
    /// On Blackwell+ (SM >= 100), first attempts to use `coral-kmod.ko` for
    /// kernel-privileged channel setup (required for `GPU_PROMOTE_CTX`).
    /// Falls back to the direct userspace RM path if the module is not loaded.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::DriverError`] if any step in the initialization chain fails.
    pub fn open(gpu_index: u32, sm: u32) -> DriverResult<Self> {
        use crate::nv::generation::{self, BootStrategy};
        let profile = generation::profile_for_sm(sm);
        if matches!(profile.boot_strategy, BootStrategy::KmodPromote)
            && let Some(kmod) = crate::nv::coral_kmod::CoralKmod::try_open()
        {
            match Self::open_via_kmod(kmod, gpu_index, sm) {
                Ok(dev) => return Ok(dev),
                Err(e) => {
                    eprintln!("[coral-driver] kmod init failed (sm={sm}): {e}");
                    tracing::warn!("coral-kmod init failed ({e}), falling back to userspace RM");
                }
            }
        }

        Self::open_userspace(gpu_index, sm)
    }

    #[expect(
        clippy::missing_const_for_fn,
        reason = "mutates self for handle allocation; not const-compatible"
    )]
    pub(super) fn alloc_handle(&mut self) -> u32 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }

    #[expect(
        clippy::missing_const_for_fn,
        reason = "mutates self for handle allocation; not const-compatible"
    )]
    pub(super) fn alloc_mem_handle(&mut self) -> u32 {
        let h = self.next_mem_handle;
        self.next_mem_handle += 1;
        h
    }

    /// The SM version this device targets.
    #[must_use]
    pub const fn sm_version(&self) -> u32 {
        self.gpu_gen.sm
    }

    /// Whether this device is operational.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.h_compute != 0
    }
}
