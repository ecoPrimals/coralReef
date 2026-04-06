// SPDX-License-Identifier: AGPL-3.0-or-later
//! Texture, memory, and shader I/O coverage tests.
//!
//! Targets tex.rs (sm20/32/50/70), mem.rs load/store patterns,
//! `shader_io.rs` uniform buffers, and spiller legacy paths.

use std::fmt::Write;

use coral_reef::{CompileError, CompileOptions, GpuArch};

fn opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn compile_fixture_sm70(wgsl: &str) {
    let r = coral_reef::compile_wgsl(wgsl, &opts());
    assert!(r.is_ok(), "SM70: {}", r.unwrap_err());
}

fn compile_fixture_legacy_nv(wgsl: &str) {
    for sm in [50, 32, 30, 21, 20] {
        let r = coral_reef::compile_wgsl_raw_sm(wgsl, sm);
        assert!(r.is_ok(), "SM{sm}: {}", r.unwrap_err());
    }
}

fn try_compile_sm70(wgsl: &str) {
    match coral_reef::compile_wgsl(wgsl, &opts()) {
        Ok(binary) => assert!(!binary.is_empty()),
        Err(CompileError::NotImplemented(_)) => {}
        Err(e) => panic!("SM70: {e}"),
    }
}

fn try_compile_legacy_nv(wgsl: &str) {
    for sm in [50, 32, 30, 21, 20] {
        match coral_reef::compile_wgsl_raw_sm(wgsl, sm) {
            Ok(binary) => assert!(!binary.is_empty()),
            Err(CompileError::NotImplemented(_)) => {}
            Err(e) => panic!("SM{sm}: {e}"),
        }
    }
}

// =============================================================================
// Texture instruction coverage — targets sm20/32/50/70 tex.rs (0% covered)
// =============================================================================

const TEX_SAMPLE_WGSL: &str = r"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<storage, read_write> out: array<vec4<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) * 0.01, 0.5);
    out[gid.x] = textureSampleLevel(tex, samp, uv, 0.0);
}
";

const TEX_LOAD_WGSL: &str = r"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<vec4<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = textureLoad(tex, vec2<i32>(i32(gid.x), 0), 0);
}
";

const TEX_GATHER_WGSL: &str = r"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<storage, read_write> out: array<vec4<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) * 0.01, 0.5);
    out[gid.x] = textureGather(0, tex, samp, uv);
}
";

const TEX_DIMENSIONS_WGSL: &str = r"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    let dims = textureDimensions(tex, 0);
    out[0] = dims.x;
    out[1] = dims.y;
}
";

const TEX_ARRAY_LOAD_WGSL: &str = r"
@group(0) @binding(0) var tex_arr: texture_2d_array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<vec4<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let layer = i32(gid.x % 4u);
    out[gid.x] = textureLoad(tex_arr, vec2<i32>(i32(gid.x), 0), layer, 0);
}
";

const TEX_3D_LOAD_WGSL: &str = r"
@group(0) @binding(0) var tex3d: texture_3d<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<vec4<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = textureLoad(tex3d, vec3<i32>(i32(gid.x), 0, 0), 0);
}
";

const TEX_STORAGE_WRITE_WGSL: &str = r"
@group(0) @binding(0) var tex: texture_storage_2d<rgba8unorm, write>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let color = vec4<f32>(f32(gid.x) * 0.01, 0.5, 0.3, 1.0);
    textureStore(tex, vec2<i32>(i32(gid.x), 0), color);
}
";

#[test]
fn tex_sample_sm70() {
    try_compile_sm70(TEX_SAMPLE_WGSL);
}

#[test]
fn tex_load_sm70() {
    try_compile_sm70(TEX_LOAD_WGSL);
}

#[test]
fn tex_gather_sm70() {
    try_compile_sm70(TEX_GATHER_WGSL);
}

#[test]
fn tex_dimensions_sm70() {
    try_compile_sm70(TEX_DIMENSIONS_WGSL);
}

#[test]
fn tex_array_load_sm70() {
    try_compile_sm70(TEX_ARRAY_LOAD_WGSL);
}

#[test]
fn tex_3d_load_sm70() {
    try_compile_sm70(TEX_3D_LOAD_WGSL);
}

#[test]
fn tex_storage_write_sm70() {
    try_compile_sm70(TEX_STORAGE_WRITE_WGSL);
}

#[test]
fn tex_sample_legacy_all() {
    try_compile_legacy_nv(TEX_SAMPLE_WGSL);
}

#[test]
fn tex_load_legacy_all() {
    try_compile_legacy_nv(TEX_LOAD_WGSL);
}

#[test]
fn tex_gather_legacy_all() {
    try_compile_legacy_nv(TEX_GATHER_WGSL);
}

#[test]
fn tex_dimensions_legacy_all() {
    try_compile_legacy_nv(TEX_DIMENSIONS_WGSL);
}

#[test]
fn tex_array_load_legacy_all() {
    try_compile_legacy_nv(TEX_ARRAY_LOAD_WGSL);
}

#[test]
fn tex_3d_load_legacy_all() {
    try_compile_legacy_nv(TEX_3D_LOAD_WGSL);
}

#[test]
fn tex_storage_write_legacy_all() {
    try_compile_legacy_nv(TEX_STORAGE_WRITE_WGSL);
}

#[test]
fn tex_depth_comparison_sm70() {
    let wgsl = r"
@group(0) @binding(0) var tex: texture_depth_2d;
@group(0) @binding(1) var samp: sampler_comparison;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) * 0.01, 0.5);
    out[gid.x] = textureSampleCompareLevel(tex, samp, uv, 0.5);
}
";
    try_compile_sm70(wgsl);
}

#[test]
fn tex_depth_comparison_legacy_all() {
    let wgsl = r"
@group(0) @binding(0) var tex: texture_depth_2d;
@group(0) @binding(1) var samp: sampler_comparison;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) * 0.01, 0.5);
    out[gid.x] = textureSampleCompareLevel(tex, samp, uv, 0.5);
}
";
    try_compile_legacy_nv(wgsl);
}

// =============================================================================
// Memory coverage — diverse load/store patterns for sm20/32/50 mem.rs
// =============================================================================

const MEM_VECTOR_LOADS_WGSL: &str = r"
@group(0) @binding(0) var<storage, read> inp: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> out: array<vec4<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = inp[gid.x];
    out[gid.x] = v * 2.0 + vec4<f32>(1.0, 2.0, 3.0, 4.0);
}
";

const MEM_MIXED_TYPES_WGSL: &str = r"
@group(0) @binding(0) var<storage, read> data_f: array<f32>;
@group(0) @binding(1) var<storage, read> data_u: array<u32>;
@group(0) @binding(2) var<storage, read> data_i: array<i32>;
@group(0) @binding(3) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let f = data_f[gid.x];
    let u = data_u[gid.x];
    let i = data_i[gid.x];
    out[gid.x] = f + f32(u) + f32(i);
}
";

const MEM_SHARED_REDUCTION_WGSL: &str = r"
var<workgroup> sdata: array<f32, 128>;
@group(0) @binding(0) var<storage, read> inp: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(128)
fn main(@builtin(local_invocation_id) lid: vec3<u32>,
        @builtin(workgroup_id) wid: vec3<u32>) {
    let idx = wid.x * 128u + lid.x;
    sdata[lid.x] = inp[idx];
    workgroupBarrier();
    if lid.x == 0u {
        var sum: f32 = 0.0;
        var i: u32 = 0u;
        loop {
            if i >= 128u { break; }
            sum = sum + sdata[i];
            i = i + 1u;
        }
        out[wid.x] = sum;
    }
}
";

#[test]
fn mem_vector_loads_sm70() {
    compile_fixture_sm70(MEM_VECTOR_LOADS_WGSL);
}

#[test]
fn mem_mixed_types_sm70() {
    compile_fixture_sm70(MEM_MIXED_TYPES_WGSL);
}

#[test]
fn mem_shared_reduction_sm70() {
    try_compile_sm70(MEM_SHARED_REDUCTION_WGSL);
}

#[test]
fn mem_vector_loads_legacy_all() {
    compile_fixture_legacy_nv(MEM_VECTOR_LOADS_WGSL);
}

#[test]
fn mem_mixed_types_legacy_all() {
    compile_fixture_legacy_nv(MEM_MIXED_TYPES_WGSL);
}

#[test]
fn mem_shared_reduction_legacy_all() {
    try_compile_legacy_nv(MEM_SHARED_REDUCTION_WGSL);
}

// =============================================================================
// Shader I/O coverage — shader_io.rs (0% covered)
// =============================================================================

#[test]
fn shader_io_uniform_buffer() {
    let wgsl = r"
struct Params {
    scale: f32,
    offset: f32,
    count: u32,
    _pad: u32,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < params.count {
        out[gid.x] = f32(gid.x) * params.scale + params.offset;
    }
}
";
    compile_fixture_sm70(wgsl);
    compile_fixture_legacy_nv(wgsl);
}

#[test]
fn shader_io_multiple_groups() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(1) @binding(0) var<storage, read> b: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = a[gid.x] + b[gid.x];
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn packed_vec_sm70_legacy() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read> inp: array<vec2<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = inp[gid.x];
    let b = inp[gid.x + 1u];
    out[gid.x] = vec2<f32>(a.x * b.y + a.y * b.x, a.x * b.x - a.y * b.y);
}
";
    compile_fixture_sm70(wgsl);
    compile_fixture_legacy_nv(wgsl);
}

// =============================================================================
// Spiller: extreme spill pressure across legacy architectures
// =============================================================================

#[test]
fn spill_extreme_legacy_all() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @group(0) @binding(1) var<storage, read> inp: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..64 {
        let _ = writeln!(wgsl, "  let v{i} = inp[{i}] + f32({i});");
    }
    wgsl.push_str("  var sum: f32 = 0.0;\n");
    for i in 0..64 {
        let _ = writeln!(wgsl, "  sum = sum + v{i};");
    }
    wgsl.push_str("  out[0] = sum;\n}\n");
    compile_fixture_legacy_nv(&wgsl);
}

// =============================================================================
// Control flow: legacy SM break/continue patterns
// =============================================================================

#[test]
fn control_flow_legacy_break_continue() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var sum: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 20u { break; }
        i = i + 1u;
        if inp[i] < 0.0 { continue; }
        if inp[i] > 100.0 { break; }
        sum = sum + inp[i];
    }
    out[0] = sum;
}
";
    compile_fixture_legacy_nv(wgsl);
}

#[test]
fn control_flow_legacy_deep_nesting() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = inp[0];
    let b = inp[1];
    let c = inp[2];
    var r: f32 = 0.0;
    if a > 0.0 {
        if b > 0.0 {
            if c > 0.0 { r = 1.0; } else { r = 2.0; }
        } else {
            if c > 0.0 { r = 3.0; } else { r = 4.0; }
        }
    } else {
        if b > 0.0 { r = 5.0; } else { r = 6.0; }
    }
    out[0] = r;
}
";
    compile_fixture_legacy_nv(wgsl);
}

// =============================================================================
// Fold/optimization coverage
// =============================================================================

#[test]
fn fold_constant_operations() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = 2.0 + 3.0;
    let b = a * 4.0;
    let c = b - 1.0;
    let d = 1.0 / c;
    let e = min(a, b);
    let f = max(c, d);
    out[0] = e + f;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn fold_integer_constants() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    let a = 10u + 20u;
    let b = a * 3u;
    let c = b - 5u;
    let d = a & 0xFFu;
    let e = b | 0x100u;
    let f = c ^ d;
    out[0] = e + f;
}
";
    compile_fixture_sm70(wgsl);
}
