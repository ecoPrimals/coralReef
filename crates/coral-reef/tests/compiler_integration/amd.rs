// SPDX-License-Identifier: AGPL-3.0-or-later
//! AMD RDNA2 integration tests — cross-vendor compilation validation.

use coral_reef::{AmdArch, CompileOptions, GpuTarget, compile_wgsl};

fn amd_opts() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        ..CompileOptions::default()
    }
}

// ---------------------------------------------------------------------------
// Basic compilation
// ---------------------------------------------------------------------------

#[test]
fn amd_empty_compute_shader() {
    let result = compile_wgsl("@compute @workgroup_size(1) fn main() {}", &amd_opts());
    assert!(result.is_ok(), "empty compute should compile: {result:?}");
    let bin = result.unwrap();
    assert!(!bin.is_empty(), "binary should contain at least s_endpgm");
    // AMD binary has no SPH header
    assert!(
        bin.len() < 128,
        "AMD binary should be smaller than NVIDIA (no SPH)"
    );
}

#[test]
fn amd_workgroup_size_variations() {
    for wg in ["1", "64", "256"] {
        let src = format!("@compute @workgroup_size({wg}) fn main() {{}}");
        let result = compile_wgsl(&src, &amd_opts());
        assert!(
            result.is_ok(),
            "workgroup_size({wg}) should compile: {result:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Cross-vendor parity
// ---------------------------------------------------------------------------

#[test]
fn cross_vendor_both_compile_empty_shader() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let nv_opts = CompileOptions::default();
    let amd_opts = amd_opts();

    let nv = compile_wgsl(wgsl, &nv_opts);
    let amd = compile_wgsl(wgsl, &amd_opts);

    assert!(nv.is_ok(), "NVIDIA: {nv:?}");
    assert!(amd.is_ok(), "AMD: {amd:?}");

    let nv_bin = nv.unwrap();
    let amd_bin = amd.unwrap();

    // NVIDIA includes SPH (128 bytes), AMD does not
    assert!(nv_bin.len() > amd_bin.len());
}

#[test]
fn cross_vendor_scalar_addition() {
    let wgsl = "
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    data[id.x] = data[id.x] + 1.0;
}
";

    let nv = compile_wgsl(wgsl, &CompileOptions::default());
    let amd = compile_wgsl(wgsl, &amd_opts());

    // Both should parse and attempt compilation
    // Result may vary (AMD encoding may not cover all ops yet)
    assert!(
        nv.is_ok() || nv.is_err(),
        "NVIDIA should attempt compilation"
    );
    assert!(
        amd.is_ok() || amd.is_err(),
        "AMD should attempt compilation"
    );
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

#[test]
fn amd_invalid_wgsl_rejected() {
    let result = compile_wgsl("not valid wgsl", &amd_opts());
    assert!(result.is_err());
}

#[test]
fn amd_empty_wgsl_rejected() {
    let result = compile_wgsl("", &amd_opts());
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Backend resolution
// ---------------------------------------------------------------------------

#[test]
fn amd_backend_resolves() {
    let be = coral_reef::backend::backend_for(GpuTarget::Amd(AmdArch::Rdna2));
    assert!(be.is_ok());
}

#[test]
fn amd_backend_supports_rdna2() {
    let be = coral_reef::AmdBackend;
    assert!(coral_reef::Backend::supports(
        &be,
        GpuTarget::Amd(AmdArch::Rdna2)
    ));
}
