// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! Shared mock [`coral_driver::ComputeDevice`] implementations and helpers for unit tests.

use crate::GpuContext;
use coral_driver::{DispatchDims, DriverError, DriverResult, ShaderInfo};
use coral_reef::GpuTarget;
use std::collections::HashMap;

/// Expected FNV-1a 64-bit hash when the input is empty (only offset basis mixed in).
pub(super) const EMPTY_WGSL_FNV1A_HASH: u64 = 0xcbf2_9ce4_8422_2325;

pub(super) struct MockDevice {
    pub(super) buffers: HashMap<coral_driver::BufferHandle, Vec<u8>>,
    pub(super) next_handle: u32,
}

impl MockDevice {
    pub(super) fn new() -> Self {
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

pub(super) fn ctx_with_mock() -> GpuContext {
    GpuContext::with_device(GpuTarget::default(), Box::new(MockDevice::new())).unwrap()
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "test mock with per-operation failure toggles"
)]
pub(super) struct FailingMockDevice {
    pub(super) fail_alloc: bool,
    pub(super) fail_free: bool,
    pub(super) fail_upload: bool,
    pub(super) fail_readback: bool,
    pub(super) fail_dispatch: bool,
    pub(super) fail_sync: bool,
    pub(super) buffers: HashMap<coral_driver::BufferHandle, Vec<u8>>,
    pub(super) next_handle: u32,
}

impl FailingMockDevice {
    pub(super) fn new() -> Self {
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

    pub(super) fn fail_alloc(mut self) -> Self {
        self.fail_alloc = true;
        self
    }

    pub(super) fn fail_free(mut self) -> Self {
        self.fail_free = true;
        self
    }

    pub(super) fn fail_upload(mut self) -> Self {
        self.fail_upload = true;
        self
    }

    pub(super) fn fail_readback(mut self) -> Self {
        self.fail_readback = true;
        self
    }

    pub(super) fn fail_dispatch(mut self) -> Self {
        self.fail_dispatch = true;
        self
    }

    pub(super) fn fail_sync(mut self) -> Self {
        self.fail_sync = true;
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

/// WGSL → SPIR-V words for exercising [`crate::GpuContext::compile_spirv`] (same pattern as coral-reef integration tests).
pub(super) fn wgsl_to_spirv_words(source: &str) -> Vec<u32> {
    let module = naga::front::wgsl::parse_str(source).unwrap();
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
    naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None).unwrap()
}
