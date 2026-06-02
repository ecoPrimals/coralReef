// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn ptx_image_store_2d_rgba8() {
    let wgsl = r"
@group(0) @binding(0)
var output_tex: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(1)
fn main() {
    textureStore(output_tex, vec2<u32>(0u, 0u), vec4<f32>(1.0, 0.0, 0.0, 1.0));
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "ImageStore should compile: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains(".surfref"),
        "should declare surface: {ptx:.200}"
    );
    assert!(
        ptx.contains("sust.b.2d"),
        "should emit sust.b.2d: {ptx:.400}"
    );
}

#[test]
fn ptx_image_load_2d_rgba32() {
    let wgsl = r"
@group(0) @binding(0)
var input_tex: texture_storage_2d<rgba32float, read>;
@group(0) @binding(1)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let pixel = textureLoad(input_tex, vec2<i32>(i32(gid.x), 0i));
    out[gid.x] = pixel.x;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "ImageLoad should compile: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("suld.b.2d"),
        "should emit suld.b.2d: {ptx:.400}"
    );
}

#[test]
fn ptx_image_store_rg32float() {
    let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_2d<rg32float, write>;

@compute @workgroup_size(1)
fn main() {
    textureStore(tex, vec2<u32>(0u, 0u), vec4<f32>(1.0, 2.0, 0.0, 0.0));
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "rg32float store: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("sust.b.2d.v2.b32"),
        "should emit v2.b32 for rg32float: {ptx:.400}"
    );
}

#[test]
fn ptx_image_store_r32uint() {
    let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_2d<r32uint, write>;

@compute @workgroup_size(1)
fn main() {
    textureStore(tex, vec2<u32>(0u, 0u), vec4<u32>(42u, 0u, 0u, 0u));
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "r32uint store: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("sust.b.2d.b32"),
        "should emit b32 for r32uint: {ptx:.400}"
    );
}

#[test]
fn ptx_image_store_rgba16float() {
    let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(1)
fn main() {
    textureStore(tex, vec2<u32>(0u, 0u), vec4<f32>(1.0, 0.5, 0.25, 0.125));
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "rgba16float store: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("sust.b.2d.v4.b16"),
        "should emit v4.b16 for rgba16float: {ptx:.400}"
    );
}

#[test]
fn ptx_image_load_r32float() {
    let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_2d<r32float, read>;
@group(0) @binding(1)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = textureLoad(tex, vec2<i32>(i32(gid.x), 0i));
    out[gid.x] = v.x;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "r32float load: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("suld.b.2d.b32"),
        "should emit b32 for r32float: {ptx:.400}"
    );
}

#[test]
fn ptx_image_store_bgra8unorm() {
    let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_2d<bgra8unorm, write>;

@compute @workgroup_size(1)
fn main() {
    textureStore(tex, vec2<u32>(0u, 0u), vec4<f32>(0.0, 1.0, 0.0, 1.0));
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "bgra8unorm store: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("sust.b.2d.v4.b8"),
        "should emit v4.b8 for bgra8unorm: {ptx:.400}"
    );
}

#[test]
fn ptx_image_query_size_2d() {
    let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_2d<rgba8unorm, read>;

@group(0) @binding(1)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let dims = textureDimensions(tex);
    out[0] = dims.x;
    out[1] = dims.y;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "ImageQuery size 2d: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("suq.width.b32"),
        "should emit suq.width: {ptx:.400}"
    );
    assert!(
        ptx.contains("suq.height.b32"),
        "should emit suq.height for 2d: {ptx:.400}"
    );
}

#[test]
fn ptx_image_query_size_1d() {
    let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_1d<r32uint, read>;

@group(0) @binding(1)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let w = textureDimensions(tex);
    out[0] = w;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "ImageQuery size 1d: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("suq.width.b32"),
        "should emit suq.width for 1d: {ptx:.400}"
    );
}

#[test]
fn ptx_image_sample_2d_level_zero() {
    let wgsl = r"
@group(0) @binding(0)
var my_tex: texture_2d<f32>;
@group(0) @binding(1)
var my_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) / 64.0, 0.5);
    let color = textureSampleLevel(my_tex, my_sampler, uv, 0.0);
    out[gid.x] = color.r;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "ImageSample 2d level: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains(".texref"),
        "should declare .texref: {ptx:.200}"
    );
    assert!(
        ptx.contains("tex.level.2d.v4.f32.f32"),
        "should emit tex.level.2d: {ptx:.600}"
    );
}

#[test]
fn ptx_image_sample_1d_explicit_lod() {
    let wgsl = r"
@group(0) @binding(0)
var my_tex: texture_1d<f32>;
@group(0) @binding(1)
var my_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let u = f32(gid.x) / 32.0;
    let val = textureSampleLevel(my_tex, my_sampler, u, 2.0);
    out[gid.x] = val.r;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "ImageSample 1d lod: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("tex.level.1d.v4.f32.f32"),
        "should emit tex.level.1d: {ptx:.600}"
    );
}

#[test]
fn ptx_image_sample_3d_level() {
    let wgsl = r"
@group(0) @binding(0)
var vol_tex: texture_3d<f32>;
@group(0) @binding(1)
var vol_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uvw = vec3<f32>(f32(gid.x) / 8.0, 0.5, 0.5);
    let val = textureSampleLevel(vol_tex, vol_sampler, uvw, 0.0);
    out[gid.x] = val.r + val.g;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "ImageSample 3d: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("tex.level.3d.v4.f32.f32"),
        "should emit tex.level.3d: {ptx:.600}"
    );
}

#[test]
fn ptx_image_sample_2d_gradient() {
    let wgsl = r"
@group(0) @binding(0)
var grad_tex: texture_2d<f32>;
@group(0) @binding(1)
var grad_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) / 16.0, 0.5);
    let ddx = vec2<f32>(1.0 / 16.0, 0.0);
    let ddy = vec2<f32>(0.0, 1.0 / 16.0);
    let val = textureSampleGrad(grad_tex, grad_sampler, uv, ddx, ddy);
    out[gid.x] = val.r;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "ImageSample 2d gradient: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("tex.grad.2d.v4.f32.f32"),
        "should emit tex.grad.2d: {ptx:.600}"
    );
}

#[test]
fn ptx_image_sample_uint_texture() {
    let wgsl = r"
@group(0) @binding(0)
var uint_tex: texture_2d<u32>;
@group(0) @binding(1)
var uint_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) / 32.0, 0.5);
    let val = textureSampleLevel(uint_tex, uint_sampler, uv, 0.0);
    out[gid.x] = val.r;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "ImageSample u32: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains(".texref"),
        "should declare .texref for u32 texture: {ptx:.200}"
    );
    assert!(
        ptx.contains("tex.level.2d.v4.u32.u32"),
        "should emit tex with u32 channel type: {ptx:.600}"
    );
}

#[test]
fn ptx_texture_gather_2d() {
    let wgsl = r"
@group(0) @binding(0)
var gather_tex: texture_2d<f32>;
@group(0) @binding(1)
var gather_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) / 16.0, 0.5);
    let gathered = textureGather(0, gather_tex, gather_sampler, uv);
    out[gid.x] = gathered.x + gathered.y + gathered.z + gathered.w;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "textureGather: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("tld4.r.2d.v4.f32.f32"),
        "should emit tld4.r.2d for component 0: {ptx:.600}"
    );
}

#[test]
fn ptx_function_call_inline_simple() {
    let wgsl = r"
fn double(x: u32) -> u32 {
    return x * 2u;
}

@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = double(gid.x);
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "Function call inlining: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("mul.lo.u32"),
        "should emit multiply from inlined double(): {ptx:.600}"
    );
}

#[test]
fn ptx_function_call_inline_multi_arg() {
    let wgsl = r"
fn add_scaled(a: f32, b: f32, scale: f32) -> f32 {
    return (a + b) * scale;
}

@group(0) @binding(0)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x);
    out[gid.x] = add_scaled(x, 1.0, 2.0);
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "Multi-arg inline call: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("add.f32"),
        "should emit add from inlined function: {ptx:.600}"
    );
    assert!(
        ptx.contains("mul.f32"),
        "should emit mul from inlined function: {ptx:.600}"
    );
}

#[test]
fn ptx_function_call_inline_void() {
    let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

fn write_at(idx: u32, val: u32) {
    out[idx] = val;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    write_at(gid.x, 99u);
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "Void function inline: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("st.global"),
        "should emit store from inlined void function: {ptx:.600}"
    );
}

#[test]
fn ptx_function_call_inline_nested() {
    let wgsl = r"
fn square(x: u32) -> u32 {
    return x * x;
}

fn sum_of_squares(a: u32, b: u32) -> u32 {
    return square(a) + square(b);
}

@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = sum_of_squares(gid.x, gid.x + 1u);
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "Nested call inlining: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("mul.lo.u32"),
        "should emit multiplies from nested inlined calls: {ptx:.600}"
    );
    assert!(
        ptx.contains("add.u32"),
        "should emit add from outer inlined function: {ptx:.600}"
    );
}

#[test]
fn ptx_workgroup_uniform_load() {
    let wgsl = r"
var<workgroup> shared_val: u32;

@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    if lid.x == 0u {
        shared_val = 42u;
    }
    let uniform_val = workgroupUniformLoad(&shared_val);
    out[lid.x] = uniform_val;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(
        result.is_ok(),
        "WorkGroupUniformLoad should compile: {result:?}"
    );
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("bar.sync"),
        "should emit barrier for workgroupUniformLoad: {ptx:.600}"
    );
    assert!(
        ptx.contains("ld.shared"),
        "should emit shared memory load: {ptx:.600}"
    );
}

#[test]
fn ptx_image_atomic_add_2d() {
    let wgsl = r"
@group(0) @binding(0)
var atomic_tex: texture_storage_2d<r32uint, read_write>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    textureStore(atomic_tex, vec2<u32>(gid.x, 0u), vec4<u32>(1u, 0u, 0u, 0u));
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(
        result.is_ok(),
        "Storage texture write should compile: {result:?}"
    );
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("sust.b.2d"),
        "should emit surface store: {ptx:.600}"
    );
}

#[test]
fn ptx_ray_query_initialize_proceed() {
    let wgsl = r"
enable wgpu_ray_query;

@group(0) @binding(0)
var accel: acceleration_structure;
@group(0) @binding(1)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var rq: ray_query;
    let desc = RayDesc(0u, 0xFFu, 0.001, 1000.0, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0));
    rayQueryInitialize(&rq, accel, desc);
    let hit = rayQueryProceed(&rq);
    if hit {
        out[gid.x] = 1u;
    } else {
        out[gid.x] = 0u;
    }
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(
        result.is_ok(),
        "RayQuery Initialize+Proceed should compile for SM120: {result:?}"
    );
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("ray_query_initialize"),
        "should emit RT initialize comment: {ptx:.800}"
    );
    assert!(
        ptx.contains("rt.trace.proceed"),
        "should emit RT proceed comment: {ptx:.800}"
    );
    assert!(
        ptx.contains("setp.eq.u32"),
        "should emit predicate for proceed result: {ptx:.800}"
    );
}

#[test]
fn ptx_ray_query_get_intersection() {
    let wgsl = r"
enable wgpu_ray_query;

@group(0) @binding(0)
var accel: acceleration_structure;
@group(0) @binding(1)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var rq: ray_query;
    let desc = RayDesc(0u, 0xFFu, 0.001, 1000.0, vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, -1.0, 0.0));
    rayQueryInitialize(&rq, accel, desc);
    let hit = rayQueryProceed(&rq);
    let intersection = rayQueryGetCommittedIntersection(&rq);
    out[gid.x] = intersection.t;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(
        result.is_ok(),
        "RayQuery GetIntersection should compile for SM120: {result:?}"
    );
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("_rt_query_get_intersection_t"),
        "should emit RT core intersection query calls: {ptx:.800}"
    );
}

#[test]
fn ptx_ray_query_terminate() {
    let wgsl = r"
enable wgpu_ray_query;

@group(0) @binding(0)
var accel: acceleration_structure;
@group(0) @binding(1)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var rq: ray_query;
    let desc = RayDesc(0u, 0xFFu, 0.01, 100.0, vec3<f32>(0.0), vec3<f32>(1.0, 0.0, 0.0));
    rayQueryInitialize(&rq, accel, desc);
    let hit = rayQueryProceed(&rq);
    rayQueryTerminate(&rq);
    out[gid.x] = 42u;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(
        result.is_ok(),
        "RayQuery Terminate should compile for SM120: {result:?}"
    );
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("rt.trace.terminate"),
        "should emit terminate comment: {ptx:.800}"
    );
}

#[test]
fn ptx_ray_query_rejects_sm70() {
    let wgsl = r"
enable wgpu_ray_query;

@group(0) @binding(0)
var accel: acceleration_structure;
@group(0) @binding(1)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var rq: ray_query;
    let desc = RayDesc(0u, 0xFFu, 0.001, 1000.0, vec3<f32>(0.0), vec3<f32>(0.0, 0.0, 1.0));
    rayQueryInitialize(&rq, accel, desc);
    let hit = rayQueryProceed(&rq);
    out[gid.x] = 0u;
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(
        result.is_err(),
        "RayQuery should reject SM70 (requires SM75+)"
    );
    let err = result.unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("SM75"),
        "error should mention SM75 requirement: {msg}"
    );
}

#[test]
fn ptx_depth_compare_sample_2d() {
    let wgsl = r"
@group(0) @binding(0)
var depth_tex: texture_depth_2d;
@group(0) @binding(1)
var shadow_sampler: sampler_comparison;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) / 64.0, 0.5);
    let shadow = textureSampleCompareLevel(depth_tex, shadow_sampler, uv, 0.75);
    out[gid.x] = shadow;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "depth compare 2d: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("tex.level.compare.2d.f32.f32"),
        "should emit tex.level.compare.2d: {ptx:.800}"
    );
    assert!(
        ptx.contains(".texref"),
        "should declare .texref for depth texture: {ptx:.200}"
    );
}

#[test]
fn ptx_image_sample_2d_array() {
    let wgsl = r"
@group(0) @binding(0)
var arr_tex: texture_2d_array<f32>;
@group(0) @binding(1)
var arr_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) / 64.0, 0.5);
    let layer = i32(gid.x % 4u);
    let color = textureSampleLevel(arr_tex, arr_sampler, uv, layer, 0.0);
    out[gid.x] = color.r;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "array 2d sample: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("tex.level.a2d"),
        "should emit tex.level.a2d for texture_2d_array: {ptx:.800}"
    );
}

#[test]
fn ptx_image_sample_cube() {
    let wgsl = r"
@group(0) @binding(0)
var cube_tex: texture_cube<f32>;
@group(0) @binding(1)
var cube_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dir = vec3<f32>(f32(gid.x) / 64.0, 1.0, 0.5);
    let color = textureSampleLevel(cube_tex, cube_sampler, dir, 0.0);
    out[gid.x] = color.r;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "cube sample: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("tex.level.cube"),
        "should emit tex.level.cube for texture_cube: {ptx:.800}"
    );
}
