// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

//! [`GpuContext`] accessors, `compile_wgsl_cached` / `compile_spirv` / `compile_glsl` error paths.

use crate::GpuContext;
use crate::error::GpuError;
use coral_driver::{BufferHandle, MemoryDomain};
use coral_reef::{CompileOptions, GpuTarget, NvArch, compile_glsl};

use super::common::{ctx_with_mock, wgsl_to_spirv_words};

#[test]
fn gpu_context_accessors_without_device() {
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm86)).unwrap();
    assert_eq!(ctx.target(), GpuTarget::Nvidia(NvArch::Sm86));
    assert_eq!(
        ctx.compile_options().target,
        GpuTarget::Nvidia(NvArch::Sm86)
    );
    assert!(!ctx.has_device());
}

#[test]
fn gpu_context_accessors_with_mock_device() {
    let ctx = ctx_with_mock();
    assert!(ctx.has_device());
    assert_eq!(ctx.target(), GpuTarget::default());
}

#[test]
fn compile_wgsl_cached_invalid_does_not_cache_and_repeats_error() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let bad = "this is not wgsl {";
    assert!(ctx.compile_wgsl_cached(bad).is_err());
    assert!(
        ctx.compile_wgsl_cached(bad).is_err(),
        "failed compiles must not populate the cache"
    );
}

#[test]
fn compile_wgsl_cached_empty_source() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let r = ctx.compile_wgsl_cached("");
    assert!(r.is_err(), "empty WGSL should not compile: {r:?}");
}

#[test]
fn compile_spirv_empty_buffer_errors() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let r = ctx.compile_spirv(&[]);
    assert!(matches!(r, Err(GpuError::Compile(_))), "got {r:?}");
}

#[test]
fn compile_spirv_invalid_words_errors() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let junk = [0xdead_beef_u32, 0xcafe_babe];
    let r = ctx.compile_spirv(&junk);
    assert!(matches!(r, Err(GpuError::Compile(_))), "got {r:?}");
}

#[test]
fn compile_spirv_valid_roundtrip_from_wgsl() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let spirv = wgsl_to_spirv_words("@compute @workgroup_size(1) fn main() {}");
    let k = ctx.compile_spirv(&spirv);
    assert!(k.is_ok());
    let kernel = k.expect("valid SPIR-V from WGSL should compile");
    assert!(!kernel.binary.is_empty());
}

#[test]
fn compile_glsl_errors_use_context_options() {
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm75)).unwrap();
    assert_eq!(
        ctx.compile_options().target,
        GpuTarget::Nvidia(NvArch::Sm75)
    );
    let r = compile_glsl("not glsl at all {{{", ctx.compile_options());
    assert!(r.is_err(), "malformed GLSL should fail: {r:?}");
}

#[test]
fn compile_wgsl_invalid_source_errors() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let r = ctx.compile_wgsl("not wgsl {{{");
    assert!(matches!(r, Err(GpuError::Compile(_))), "got {r:?}");
}

#[test]
fn from_parts_overwrites_options_target_to_match_explicit_target() {
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let ctx = GpuContext::from_parts(
        GpuTarget::Nvidia(NvArch::Sm89),
        Box::new(super::common::MockDevice::new()),
        opts,
    )
    .unwrap();
    assert_eq!(ctx.target(), GpuTarget::Nvidia(NvArch::Sm89));
    assert_eq!(
        ctx.compile_options().target,
        GpuTarget::Nvidia(NvArch::Sm89)
    );
}

#[test]
fn gpu_context_device_ops_without_attached_device_return_no_device_attached() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let handle = BufferHandle::from_id(1);
    assert!(matches!(ctx.alloc(64), Err(GpuError::NoDeviceAttached)));
    assert!(matches!(
        ctx.alloc_in(64, MemoryDomain::Vram),
        Err(GpuError::NoDeviceAttached)
    ));
    assert!(matches!(ctx.free(handle), Err(GpuError::NoDeviceAttached)));
    assert!(matches!(
        ctx.upload(handle, &[1, 2, 3]),
        Err(GpuError::NoDeviceAttached)
    ));
    assert!(matches!(
        ctx.readback(handle, 4),
        Err(GpuError::NoDeviceAttached)
    ));
    assert!(matches!(ctx.sync(), Err(GpuError::NoDeviceAttached)));

    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    assert!(matches!(
        ctx.dispatch(&kernel, &[], [1, 1, 1]),
        Err(GpuError::NoDeviceAttached)
    ));
    let entry = kernel.to_cache_entry();
    assert!(matches!(
        ctx.dispatch_precompiled(&entry, &[], [1, 1, 1]),
        Err(GpuError::NoDeviceAttached)
    ));
}
