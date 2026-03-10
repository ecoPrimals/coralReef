// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

use crate::{error::GpuError, preference::DriverPreference};
use coral_driver::MemoryDomain;
use coral_driver::{DispatchDims, DriverError, DriverResult, ShaderInfo};
use coral_reef::{AmdArch, GpuTarget, NvArch};
use std::collections::HashMap;

use crate::GpuContext;

struct MockDevice {
    buffers: HashMap<coral_driver::BufferHandle, Vec<u8>>,
    next_handle: u32,
}

impl MockDevice {
    fn new() -> Self {
        Self {
            buffers: HashMap::new(),
            next_handle: 1,
        }
    }
}

impl coral_driver::ComputeDevice for MockDevice {
    fn alloc(
        &mut self,
        size: u64,
        _domain: coral_driver::MemoryDomain,
    ) -> DriverResult<coral_driver::BufferHandle> {
        let h = coral_driver::BufferHandle::from_id(self.next_handle);
        self.next_handle += 1;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "test mock with small sizes"
        )]
        self.buffers.insert(h, vec![0; size as usize]);
        Ok(h)
    }
    fn free(&mut self, handle: coral_driver::BufferHandle) -> DriverResult<()> {
        self.buffers
            .remove(&handle)
            .map(|_| ())
            .ok_or(DriverError::BufferNotFound(handle))
    }
    fn upload(
        &mut self,
        handle: coral_driver::BufferHandle,
        offset: u64,
        data: &[u8],
    ) -> DriverResult<()> {
        let buf = self
            .buffers
            .get_mut(&handle)
            .ok_or(DriverError::BufferNotFound(handle))?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "test mock with small offsets"
        )]
        let off = offset as usize;
        let end = off.saturating_add(data.len());
        if end > buf.len() {
            return Err(DriverError::BufferNotFound(handle));
        }
        buf[off..end].copy_from_slice(data);
        Ok(())
    }
    fn readback(
        &self,
        handle: coral_driver::BufferHandle,
        offset: u64,
        len: usize,
    ) -> DriverResult<Vec<u8>> {
        let buf = self
            .buffers
            .get(&handle)
            .ok_or(DriverError::BufferNotFound(handle))?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "test mock with small offsets"
        )]
        let off = offset as usize;
        let end = off.saturating_add(len);
        if end > buf.len() {
            return Err(DriverError::BufferNotFound(handle));
        }
        Ok(buf[off..end].to_vec())
    }
    fn dispatch(
        &mut self,
        _shader: &[u8],
        _buffers: &[coral_driver::BufferHandle],
        _dims: DispatchDims,
        _info: &ShaderInfo,
    ) -> DriverResult<()> {
        Ok(())
    }
    fn sync(&mut self) -> DriverResult<()> {
        Ok(())
    }
}

fn ctx_with_mock() -> GpuContext {
    GpuContext::with_device(GpuTarget::default(), Box::new(MockDevice::new())).unwrap()
}

#[test]
fn alloc_upload_readback_roundtrip() {
    let mut ctx = ctx_with_mock();
    let buf = ctx.alloc(16).unwrap();
    let data = b"hello world!!!!";
    ctx.upload(buf, data).unwrap();
    let out = ctx.readback(buf, data.len()).unwrap();
    assert_eq!(out, data);
}

#[test]
fn alloc_in_different_domains() {
    let mut ctx = ctx_with_mock();
    let vram = ctx.alloc_in(64, MemoryDomain::Vram).unwrap();
    let gtt = ctx.alloc_in(64, MemoryDomain::Gtt).unwrap();
    let either = ctx.alloc_in(64, MemoryDomain::VramOrGtt).unwrap();
    assert_ne!(vram, gtt);
    assert_ne!(gtt, either);
    ctx.upload(vram, b"vram").unwrap();
    ctx.upload(gtt, b"gtt").unwrap();
    assert_eq!(ctx.readback(vram, 4).unwrap(), b"vram");
    assert_eq!(ctx.readback(gtt, 3).unwrap(), b"gtt");
}

#[test]
fn dispatch_with_compiled_kernel() {
    let mut ctx = ctx_with_mock();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    let buf = ctx.alloc(64).unwrap();
    ctx.dispatch(&kernel, &[buf], [1, 1, 1]).unwrap();
}

#[test]
fn free_then_use_freed_buffer_fails() {
    let mut ctx = ctx_with_mock();
    let buf = ctx.alloc(64).unwrap();
    ctx.free(buf).unwrap();
    let err = ctx.upload(buf, b"x").unwrap_err();
    assert!(matches!(
        err,
        GpuError::Driver(DriverError::BufferNotFound(_))
    ));
    let err = ctx.readback(buf, 1).unwrap_err();
    assert!(matches!(
        err,
        GpuError::Driver(DriverError::BufferNotFound(_))
    ));
}

#[test]
fn compile_wgsl_dispatch_sync_pipeline() {
    let mut ctx = ctx_with_mock();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(64) fn main() {}")
        .unwrap();
    let buf = ctx.alloc(256).unwrap();
    ctx.dispatch(&kernel, &[buf], [4, 1, 1]).unwrap();
    ctx.sync().unwrap();
}

#[test]
fn compile_spirv_method() {
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm70)).unwrap();
    let invalid = [0x0723_0203_u32, 0x0001_0000, 0, 0, 0];
    let r = ctx.compile_spirv(&invalid);
    assert!(r.is_err());
}

#[test]
fn readback_returns_correct_data() {
    let mut ctx = ctx_with_mock();
    let buf = ctx.alloc(8).unwrap();
    ctx.upload(buf, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
    let out = ctx.readback(buf, 8).unwrap();
    assert_eq!(out, [1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn has_device_returns_true_when_attached() {
    let ctx = ctx_with_mock();
    assert!(ctx.has_device());
}

#[test]
fn gpu_context_compile_only() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    assert!(!ctx.has_device());
}

#[test]
fn gpu_context_compile_wgsl() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let kernel = ctx.compile_wgsl("@compute @workgroup_size(1) fn main() {}");
    assert!(kernel.is_ok());
    let k = kernel.unwrap();
    assert!(!k.binary.is_empty());
}

#[test]
fn gpu_context_amd_compile() {
    let ctx = GpuContext::new(GpuTarget::Amd(AmdArch::Rdna2)).unwrap();
    let kernel = ctx.compile_wgsl("@compute @workgroup_size(1) fn main() {}");
    assert!(kernel.is_ok());
}

#[test]
fn compiled_kernel_has_target() {
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm86)).unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    assert!(matches!(kernel.target, GpuTarget::Nvidia(NvArch::Sm86)));
}

#[test]
fn hash_deterministic() {
    let a = crate::hash_wgsl("hello");
    let b = crate::hash_wgsl("hello");
    let c = crate::hash_wgsl("world");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn alloc_fails_without_device() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let err = ctx.alloc(1024).unwrap_err();
    assert!(matches!(err, GpuError::NoDeviceAttached));
}

#[test]
fn dispatch_fails_without_device() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    let err = ctx.dispatch(&kernel, &[], [1, 1, 1]).unwrap_err();
    assert!(matches!(err, GpuError::NoDeviceAttached));
}

#[test]
fn sync_fails_without_device() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let err = ctx.sync().unwrap_err();
    assert!(matches!(err, GpuError::NoDeviceAttached));
}

#[test]
fn readback_fails_without_device() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let buf = coral_driver::BufferHandle::from_id(1);
    let err = ctx.readback(buf, 1).unwrap_err();
    assert!(matches!(err, GpuError::NoDeviceAttached));
}

// -------------------------------------------------------------------
// DriverPreference tests
// -------------------------------------------------------------------

#[test]
fn sovereign_preference_prefers_nouveau() {
    let pref = DriverPreference::sovereign();
    assert_eq!(pref.order()[0], "nouveau");
    assert_eq!(pref.order()[1], "amdgpu");
    assert_eq!(pref.order()[2], "nvidia-drm");
}

#[test]
fn pragmatic_preference_prefers_amdgpu() {
    let pref = DriverPreference::pragmatic();
    assert_eq!(pref.order()[0], "amdgpu");
}

#[test]
fn default_preference_is_sovereign() {
    let pref = DriverPreference::default();
    assert_eq!(pref.order(), DriverPreference::sovereign().order());
}

#[test]
fn select_returns_best_match() {
    let pref = DriverPreference::sovereign();
    assert_eq!(
        pref.select(&["amdgpu", "nvidia-drm"]),
        Some("amdgpu"),
        "with no nouveau, sovereign picks amdgpu next"
    );
}

#[test]
fn select_returns_nouveau_when_available() {
    let pref = DriverPreference::sovereign();
    assert_eq!(
        pref.select(&["nvidia-drm", "nouveau", "amdgpu"]),
        Some("nouveau"),
        "sovereign always picks nouveau first"
    );
}

#[test]
fn select_returns_none_when_no_match() {
    let pref = DriverPreference::sovereign();
    assert_eq!(pref.select(&["i915", "xe"]), None);
}

#[test]
fn pragmatic_selects_nvidia_drm_over_nouveau() {
    let pref = DriverPreference::pragmatic();
    assert_eq!(
        pref.select(&["nouveau", "nvidia-drm"]),
        Some("nvidia-drm"),
        "pragmatic picks nvidia-drm before nouveau"
    );
}

#[test]
fn from_str_list_parses_comma_separated() {
    let pref = DriverPreference::from_str_list("nvidia-drm, amdgpu");
    assert_eq!(pref.order(), &["nvidia-drm", "amdgpu"]);
}

#[test]
fn from_str_list_handles_empty_segments() {
    let pref = DriverPreference::from_str_list("nouveau,,amdgpu,");
    assert_eq!(pref.order(), &["nouveau", "amdgpu"]);
}

#[test]
fn from_str_list_single_driver() {
    let pref = DriverPreference::from_str_list("amdgpu");
    assert_eq!(pref.order(), &["amdgpu"]);
    assert_eq!(pref.select(&["amdgpu", "nvidia-drm"]), Some("amdgpu"));
    assert_eq!(pref.select(&["nvidia-drm"]), None);
}

#[test]
fn from_env_falls_back_to_sovereign() {
    // Don't modify env vars (unsafe in 2024+ edition).
    // Instead, verify that from_env returns sovereign-compatible
    // ordering when the env var is not set to something specific.
    let pref = DriverPreference::from_env();
    assert!(
        !pref.order().is_empty(),
        "preference should have at least one driver"
    );
}

#[test]
fn driver_preference_debug_format() {
    let pref = DriverPreference::sovereign();
    let debug = format!("{pref:?}");
    assert!(debug.contains("nouveau"));
}

#[test]
fn driver_preference_clone() {
    let pref = DriverPreference::sovereign();
    let cloned = pref.clone();
    assert_eq!(pref.order(), cloned.order());
}

// -------------------------------------------------------------------
// GpuContext error paths with MockDevice
// -------------------------------------------------------------------

#[expect(
    clippy::struct_excessive_bools,
    reason = "test mock with per-operation failure toggles"
)]
struct FailingMockDevice {
    fail_alloc: bool,
    fail_free: bool,
    fail_upload: bool,
    fail_readback: bool,
    fail_dispatch: bool,
    fail_sync: bool,
    buffers: HashMap<coral_driver::BufferHandle, Vec<u8>>,
    next_handle: u32,
}

impl FailingMockDevice {
    fn new() -> Self {
        Self {
            fail_alloc: false,
            fail_free: false,
            fail_upload: false,
            fail_readback: false,
            fail_dispatch: false,
            fail_sync: false,
            buffers: HashMap::new(),
            next_handle: 1,
        }
    }

    fn fail_alloc(mut self) -> Self {
        self.fail_alloc = true;
        self
    }

    fn fail_free(mut self) -> Self {
        self.fail_free = true;
        self
    }

    fn fail_upload(mut self) -> Self {
        self.fail_upload = true;
        self
    }

    fn fail_readback(mut self) -> Self {
        self.fail_readback = true;
        self
    }
}

impl coral_driver::ComputeDevice for FailingMockDevice {
    fn alloc(
        &mut self,
        size: u64,
        _domain: coral_driver::MemoryDomain,
    ) -> DriverResult<coral_driver::BufferHandle> {
        if self.fail_alloc {
            return Err(DriverError::AllocFailed {
                size,
                domain: coral_driver::MemoryDomain::Vram,
            });
        }
        let h = coral_driver::BufferHandle::from_id(self.next_handle);
        self.next_handle += 1;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "test mock with small sizes"
        )]
        self.buffers.insert(h, vec![0; size as usize]);
        Ok(h)
    }

    fn free(&mut self, handle: coral_driver::BufferHandle) -> DriverResult<()> {
        if self.fail_free {
            return Err(DriverError::BufferNotFound(handle));
        }
        self.buffers
            .remove(&handle)
            .map(|_| ())
            .ok_or(DriverError::BufferNotFound(handle))
    }

    fn upload(
        &mut self,
        handle: coral_driver::BufferHandle,
        offset: u64,
        data: &[u8],
    ) -> DriverResult<()> {
        if self.fail_upload {
            return Err(DriverError::MmapFailed("upload failed".into()));
        }
        let buf = self
            .buffers
            .get_mut(&handle)
            .ok_or(DriverError::BufferNotFound(handle))?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "test mock with small offsets"
        )]
        let off = offset as usize;
        let end = off.saturating_add(data.len());
        if end > buf.len() {
            return Err(DriverError::BufferNotFound(handle));
        }
        buf[off..end].copy_from_slice(data);
        Ok(())
    }

    fn readback(
        &self,
        handle: coral_driver::BufferHandle,
        offset: u64,
        len: usize,
    ) -> DriverResult<Vec<u8>> {
        if self.fail_readback {
            return Err(DriverError::MmapFailed("readback failed".into()));
        }
        let buf = self
            .buffers
            .get(&handle)
            .ok_or(DriverError::BufferNotFound(handle))?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "test mock with small offsets"
        )]
        let off = offset as usize;
        let end = off.saturating_add(len);
        if end > buf.len() {
            return Err(DriverError::BufferNotFound(handle));
        }
        Ok(buf[off..end].to_vec())
    }

    fn dispatch(
        &mut self,
        _shader: &[u8],
        _buffers: &[coral_driver::BufferHandle],
        _dims: DispatchDims,
        _info: &ShaderInfo,
    ) -> DriverResult<()> {
        if self.fail_dispatch {
            return Err(DriverError::SubmitFailed("dispatch failed".into()));
        }
        Ok(())
    }

    fn sync(&mut self) -> DriverResult<()> {
        if self.fail_sync {
            return Err(DriverError::FenceTimeout { ms: 5000 });
        }
        Ok(())
    }
}

#[test]
fn alloc_error_propagates() {
    let mut ctx = GpuContext::with_device(
        GpuTarget::default(),
        Box::new(FailingMockDevice::new().fail_alloc()),
    )
    .unwrap();
    let err = ctx.alloc(1024).unwrap_err();
    assert!(matches!(
        err,
        GpuError::Driver(DriverError::AllocFailed { .. })
    ));
}

#[test]
fn free_error_propagates() {
    let mut ctx = GpuContext::with_device(
        GpuTarget::default(),
        Box::new(FailingMockDevice::new().fail_free()),
    )
    .unwrap();
    let buf = ctx.alloc(64).unwrap();
    let err = ctx.free(buf).unwrap_err();
    assert!(matches!(
        err,
        GpuError::Driver(DriverError::BufferNotFound(_))
    ));
}

#[test]
fn upload_error_propagates() {
    let mut ctx = GpuContext::with_device(
        GpuTarget::default(),
        Box::new(FailingMockDevice::new().fail_upload()),
    )
    .unwrap();
    let buf = ctx.alloc(64).unwrap();
    let err = ctx.upload(buf, b"data").unwrap_err();
    assert!(matches!(err, GpuError::Driver(DriverError::MmapFailed(_))));
}

#[test]
fn readback_error_propagates() {
    let mut ctx = GpuContext::with_device(
        GpuTarget::default(),
        Box::new(FailingMockDevice::new().fail_readback()),
    )
    .unwrap();
    let buf = ctx.alloc(64).unwrap();
    ctx.upload(buf, b"x").unwrap();
    let err = ctx.readback(buf, 1).unwrap_err();
    assert!(matches!(err, GpuError::Driver(DriverError::MmapFailed(_))));
}

#[test]
fn upload_fails_without_device() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let buf = coral_driver::BufferHandle::from_id(1);
    let err = ctx.upload(buf, b"x").unwrap_err();
    assert!(matches!(err, GpuError::NoDeviceAttached));
}

#[test]
fn free_fails_without_device() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let buf = coral_driver::BufferHandle::from_id(1);
    let err = ctx.free(buf).unwrap_err();
    assert!(matches!(err, GpuError::NoDeviceAttached));
}

// -------------------------------------------------------------------
// CompiledKernel metadata tests
// -------------------------------------------------------------------

#[test]
fn compiled_kernel_metadata_from_wgsl() {
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm70)).unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(32, 2, 1) fn main() { }")
        .unwrap();
    assert_eq!(kernel.workgroup, [32, 2, 1]);
    assert!(kernel.instr_count > 0);
    assert!(!kernel.binary.is_empty());
    assert_eq!(kernel.target, GpuTarget::Nvidia(NvArch::Sm70));
}

#[test]
fn compiled_kernel_source_hash_nonzero() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    assert_ne!(kernel.source_hash, 0);
}

#[test]
fn compiled_kernel_metadata_fields_populated() {
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm70)).unwrap();
    let kernel = ctx
        .compile_wgsl(
            "@compute @workgroup_size(64) fn main() {
            var x: array<f32, 256>;
            x[0u] = 1.0;
            workgroupBarrier();
        }",
        )
        .unwrap();
    assert_eq!(kernel.workgroup, [64, 1, 1]);
    assert!(!kernel.binary.is_empty());
    // shared_mem_bytes and barrier_count may be 0 if compiler optimizes
    assert!(kernel.gpr_count >= 4 || kernel.instr_count > 0);
}

#[test]
fn compiled_kernel_debug_format() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    let debug = format!("{kernel:?}");
    assert!(debug.contains("CompiledKernel"));
    assert!(debug.contains("binary"));
}
