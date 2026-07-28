// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn ptx_math_normalize() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = vec3<f32>(f32(gid.x), 1.0, 2.0);
    let n = normalize(v);
    out[gid.x] = n.x;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "normalize: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("rsqrt.approx.f32"),
        "normalize should use rsqrt: {ptx:.600}"
    );
}

#[test]
fn ptx_math_length() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = vec3<f32>(f32(gid.x), 1.0, 2.0);
    out[gid.x] = length(v);
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "length: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("sqrt.rn.f32"),
        "length should use sqrt: {ptx:.600}"
    );
}

#[test]
fn ptx_math_cross() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = vec3<f32>(1.0, 0.0, 0.0);
    let b = vec3<f32>(0.0, 1.0, 0.0);
    let c = cross(a, b);
    out[gid.x] = c.z;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "cross: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("fma.rn.f32"),
        "cross should use fma for component products: {ptx:.600}"
    );
}

#[test]
fn ptx_math_distance() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = vec3<f32>(f32(gid.x), 0.0, 0.0);
    let b = vec3<f32>(0.0, 1.0, 0.0);
    out[gid.x] = distance(a, b);
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "distance: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("sqrt.rn.f32"),
        "distance should use sqrt: {ptx:.600}"
    );
}

#[test]
fn ptx_texture_load() {
    let wgsl = r"
@group(0) @binding(0) var my_tex: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let texel = textureLoad(my_tex, vec2<u32>(gid.x, 0u), 0);
    out[gid.x] = texel.x;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "texture load: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("tld.b.2d"),
        "should emit tld.b.2d for textureLoad: {ptx:.800}"
    );
}

#[test]
fn ptx_image_query_num_layers() {
    let wgsl = r"
@group(0) @binding(0) var my_img: texture_storage_2d_array<rgba8unorm, read_write>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let layers = textureNumLayers(my_img);
    out[gid.x] = layers;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "NumLayers: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("suq.array_size.b32"),
        "should emit suq.array_size.b32: {ptx:.800}"
    );
}

#[test]
fn ptx_control_flow_branching() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var result: f32 = 0.0;
    if gid.x < 32u {
        result = f32(gid.x) * 2.0;
    } else {
        result = f32(gid.x) * 0.5;
    }
    out[gid.x] = result;
}
";
    let result = emit_compute_ptx(wgsl, 80);
    assert!(result.is_ok(), "branching: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains('@') || ptx.contains("bra"),
        "branching should produce conditional or branch: {ptx:.600}"
    );
}

#[test]
fn ptx_loop_accumulation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var acc: f32 = 0.0;
    for (var i: u32 = 0u; i < 16u; i = i + 1u) {
        acc = acc + f32(i);
    }
    out[gid.x] = acc;
}
";
    let result = emit_compute_ptx(wgsl, 80);
    assert!(result.is_ok(), "loop: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("add.f32") || ptx.contains("add.rn.f32"),
        "loop should produce f32 adds: {ptx:.600}"
    );
}

#[test]
fn ptx_multi_arch_sm70() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = f32(gid.x) * 3.14;
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "SM70: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains(".target sm_70"),
        "should target sm_70: {ptx:.200}"
    );
}

#[test]
fn ptx_multi_arch_sm75() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = gid.x * gid.x;
}
";
    let result = emit_compute_ptx(wgsl, 75);
    assert!(result.is_ok(), "SM75: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains(".target sm_75"),
        "should target sm_75: {ptx:.200}"
    );
}

#[test]
fn ptx_shared_memory_usage() {
    let wgsl = r"
var<workgroup> shared_data: array<f32, 64>;

@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(global_invocation_id) gid: vec3<u32>) {
    shared_data[lid.x] = f32(lid.x);
    workgroupBarrier();
    out[gid.x] = shared_data[63u - lid.x];
}
";
    let result = emit_compute_ptx(wgsl, 80);
    assert!(result.is_ok(), "shared memory: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains(".shared"),
        "should declare shared memory: {ptx:.600}"
    );
    assert!(ptx.contains("bar.sync"), "should emit barrier: {ptx:.600}");
}

#[test]
fn ptx_math_tan() {
    let wgsl = "
@group(0) @binding(0) var<storage, read_write> buf: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    buf[gid.x] = tan(buf[gid.x]);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "tan: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("sin.approx"),
        "should use sin.approx: {ptx:.600}"
    );
    assert!(
        ptx.contains("cos.approx"),
        "should use cos.approx: {ptx:.600}"
    );
    assert!(ptx.contains("div.rn"), "should divide sin/cos: {ptx:.600}");
}

#[test]
fn ptx_math_atan() {
    let wgsl = "
@group(0) @binding(0) var<storage, read_write> buf: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    buf[gid.x] = atan(buf[gid.x]);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "atan: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(ptx.contains("fma.rn"), "should use fma approx: {ptx:.600}");
    assert!(ptx.contains("div.rn"), "should have division: {ptx:.600}");
}

#[test]
fn ptx_math_atan2() {
    let wgsl = "
@group(0) @binding(0) var<storage, read_write> buf: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    buf[gid.x] = atan2(buf[gid.x], buf[gid.x + 1u]);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "atan2: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(ptx.contains("div.rn"), "atan2 needs division: {ptx:.600}");
}

#[test]
fn ptx_math_asin() {
    let wgsl = "
@group(0) @binding(0) var<storage, read_write> buf: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    buf[gid.x] = asin(buf[gid.x]);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "asin: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(ptx.contains("rsqrt.approx"), "asin uses rsqrt: {ptx:.600}");
}

#[test]
fn ptx_math_acos() {
    let wgsl = "
@group(0) @binding(0) var<storage, read_write> buf: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    buf[gid.x] = acos(buf[gid.x]);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "acos: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(ptx.contains("rsqrt.approx"), "acos uses rsqrt: {ptx:.600}");
    assert!(
        ptx.contains("sub.f32"),
        "acos subtracts from pi/2: {ptx:.600}"
    );
}

#[test]
fn ptx_math_reflect() {
    let wgsl = "
@group(0) @binding(0) var<storage, read_write> buf: array<vec3<f32>>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let incident = buf[gid.x];
    let normal = buf[gid.x + 1u];
    buf[gid.x] = reflect(incident, normal);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "reflect: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("fma.rn.f32"),
        "reflect uses dot product: {ptx:.600}"
    );
    assert!(ptx.contains("sub.f32"), "reflect subtracts: {ptx:.600}");
}

#[test]
fn ptx_math_extract_bits() {
    let wgsl = "
@group(0) @binding(0) var<storage, read_write> buf: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    buf[gid.x] = extractBits(buf[gid.x], 4u, 8u);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "extractBits: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(ptx.contains("bfe.u32"), "should emit bfe: {ptx:.600}");
}

#[test]
fn ptx_math_insert_bits() {
    let wgsl = "
@group(0) @binding(0) var<storage, read_write> buf: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    buf[gid.x] = insertBits(buf[gid.x], buf[gid.x + 1u], 4u, 8u);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "insertBits: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(ptx.contains("bfi.b32"), "should emit bfi: {ptx:.600}");
}

#[test]
fn ptx_texture_query_num_levels() {
    let wgsl = "
@group(0) @binding(0) var t: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = textureNumLevels(t);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "textureNumLevels: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("txq.num_mip_levels"),
        "should emit txq.num_mip_levels: {ptx:.600}"
    );
}

#[test]
fn ptx_texture_query_size() {
    let wgsl = "
@group(0) @binding(0) var t: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<vec2<u32>>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = textureDimensions(t);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "textureDimensions: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("txq.width"),
        "should emit txq.width: {ptx:.600}"
    );
    assert!(
        ptx.contains("txq.height"),
        "should emit txq.height: {ptx:.600}"
    );
}

#[test]
fn ptx_math_face_forward() {
    let wgsl = "
@group(0) @binding(0) var<storage, read_write> buf: array<vec3<f32>>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = buf[gid.x];
    let i = buf[gid.x + 1u];
    let nref = buf[gid.x + 2u];
    buf[gid.x] = faceForward(n, i, nref);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "faceForward: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("setp.lt.f32"),
        "faceForward uses comparison: {ptx:.600}"
    );
    assert!(
        ptx.contains("selp.f32"),
        "faceForward uses selp: {ptx:.600}"
    );
}

#[test]
fn ptx_texture_query_num_layers() {
    let wgsl = "
@group(0) @binding(0) var t: texture_2d_array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = textureNumLayers(t);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "textureNumLayers: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("txq.array_size"),
        "should emit txq.array_size: {ptx:.600}"
    );
}

#[test]
fn ptx_math_sinh() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = sinh(f32(gid.x) * 0.01);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "sinh compile: {result:?}");
    let binding = result.unwrap();
    let ptx = String::from_utf8_lossy(&binding.binary);
    assert!(ptx.contains("ex2.approx"), "sinh uses ex2: {ptx:.600}");
}

#[test]
fn ptx_math_cosh() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = cosh(f32(gid.x) * 0.01);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "cosh compile: {result:?}");
    let binding = result.unwrap();
    let ptx = String::from_utf8_lossy(&binding.binary);
    assert!(ptx.contains("ex2.approx"), "cosh uses ex2: {ptx:.600}");
}

#[test]
fn ptx_math_asinh() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = asinh(f32(gid.x) * 0.1);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "asinh compile: {result:?}");
    let binding = result.unwrap();
    let ptx = String::from_utf8_lossy(&binding.binary);
    assert!(ptx.contains("lg2.approx"), "asinh uses lg2: {ptx:.600}");
}

#[test]
fn ptx_math_acosh() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x) * 0.1 + 1.5;
    out[gid.x] = acosh(x);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "acosh compile: {result:?}");
    let binding = result.unwrap();
    let ptx = String::from_utf8_lossy(&binding.binary);
    assert!(ptx.contains("lg2.approx"), "acosh uses lg2: {ptx:.600}");
}

#[test]
fn ptx_math_atanh() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x) * 0.01;
    out[gid.x] = atanh(x);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "atanh compile: {result:?}");
    let binding = result.unwrap();
    let ptx = String::from_utf8_lossy(&binding.binary);
    assert!(ptx.contains("lg2.approx"), "atanh uses lg2: {ptx:.600}");
}

#[test]
fn ptx_math_ldexp() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = ldexp(1.5, i32(gid.x));
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "ldexp compile: {result:?}");
    let binding = result.unwrap();
    let ptx = String::from_utf8_lossy(&binding.binary);
    assert!(ptx.contains("ex2.approx"), "ldexp uses ex2: {ptx:.600}");
}

#[test]
fn ptx_math_first_trailing_bit() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = firstTrailingBit(gid.x + 1u);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "firstTrailingBit: {result:?}");
    let binding = result.unwrap();
    let ptx = String::from_utf8_lossy(&binding.binary);
    assert!(
        ptx.contains("brev.b32"),
        "firstTrailingBit uses brev: {ptx:.600}"
    );
}

#[test]
fn ptx_math_first_leading_bit() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = firstLeadingBit(gid.x + 1u);
}
";
    let result = emit_compute_ptx(wgsl, 70);
    assert!(result.is_ok(), "firstLeadingBit: {result:?}");
    let binding = result.unwrap();
    let ptx = String::from_utf8_lossy(&binding.binary);
    assert!(
        ptx.contains("clz.b32"),
        "firstLeadingBit uses clz: {ptx:.600}"
    );
}
