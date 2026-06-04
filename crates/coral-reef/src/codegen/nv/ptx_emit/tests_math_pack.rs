// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn ptx_pack4x8unorm() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let v = vec4<f32>(0.0, 0.5, 0.75, 1.0);
    out[0] = pack4x8unorm(v);
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "pack4x8unorm: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(ptx.contains("or.b32"), "should pack bytes: {ptx:.600}");
}

#[test]
fn ptx_pack4x8snorm() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let v = vec4<f32>(-1.0, -0.5, 0.0, 1.0);
    out[0] = pack4x8snorm(v);
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "pack4x8snorm: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("cvt.rni.s32.f32"),
        "should use signed convert: {ptx:.600}"
    );
}

#[test]
fn ptx_unpack4x8unorm() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main() {
    let v = unpack4x8unorm(input[0]);
    out[0] = v.x;
    out[1] = v.y;
    out[2] = v.z;
    out[3] = v.w;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "unpack4x8unorm: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("cvt.rn.f32.u32"),
        "should convert bytes to float: {ptx:.600}"
    );
}

#[test]
fn ptx_unpack4x8snorm() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main() {
    let v = unpack4x8snorm(input[0]);
    out[0] = v.x;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "unpack4x8snorm: {result:?}");
}

#[test]
fn ptx_pack2x16float() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let v = vec2<f32>(1.0, 2.0);
    out[0] = pack2x16float(v);
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "pack2x16float: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("cvt.rn.f16.f32"),
        "should convert f32 to f16: {ptx:.600}"
    );
}

#[test]
fn ptx_unpack2x16float() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main() {
    let v = unpack2x16float(input[0]);
    out[0] = v.x;
    out[1] = v.y;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "unpack2x16float: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("cvt.f32.f16"),
        "should convert f16 to f32: {ptx:.600}"
    );
}

#[test]
fn ptx_pack2x16unorm() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let v = vec2<f32>(0.5, 1.0);
    out[0] = pack2x16unorm(v);
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "pack2x16unorm: {result:?}");
}

#[test]
fn ptx_pack2x16snorm() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let v = vec2<f32>(-0.5, 0.5);
    out[0] = pack2x16snorm(v);
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "pack2x16snorm: {result:?}");
}

#[test]
fn ptx_unpack2x16unorm() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main() {
    let v = unpack2x16unorm(input[0]);
    out[0] = v.x;
    out[1] = v.y;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "unpack2x16unorm: {result:?}");
}

#[test]
fn ptx_unpack2x16snorm() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main() {
    let v = unpack2x16snorm(input[0]);
    out[0] = v.x;
    out[1] = v.y;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "unpack2x16snorm: {result:?}");
}

#[test]
fn ptx_membar_before_ret() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = f32(gid.x);
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "should compile: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("membar.sys"),
        "SM120 PTX must have membar.sys before ret: {ptx:.600}"
    );
    let membar_pos = ptx.find("membar.sys").unwrap();
    let ret_pos = ptx.rfind("ret;").unwrap();
    assert!(
        membar_pos < ret_pos,
        "membar.sys must come before ret: membar@{membar_pos} ret@{ret_pos}"
    );
}

#[test]
fn ptx_matrix_inverse3x3() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main() {
    let m = mat3x3<f32>(
        vec3<f32>(1.0, 0.0, 0.0),
        vec3<f32>(0.0, 2.0, 0.0),
        vec3<f32>(0.0, 0.0, 4.0)
    );
    let inv = transpose(m);
    let det = determinant(m);
    out[0] = det;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "3x3 determinant should compile: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(ptx.contains("mul.f32"), "det3x3 uses mul: {ptx:.400}");
    assert!(ptx.contains("sub.f32"), "det3x3 uses sub: {ptx:.400}");
}

#[test]
fn ptx_matrix_inverse4x4() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main() {
    let m = mat4x4<f32>(
        vec4<f32>(2.0, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, 3.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 5.0, 0.0),
        vec4<f32>(0.0, 0.0, 0.0, 7.0)
    );
    let det = determinant(m);
    out[0] = det;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(result.is_ok(), "4x4 determinant should compile: {result:?}");
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(ptx.contains("mul.f32"), "det4x4 uses mul: {ptx:.400}");
    assert!(
        ptx.contains("sub.f32") || ptx.contains("add.f32"),
        "det4x4 uses add/sub: {ptx:.400}"
    );
}

#[test]
fn ptx_matrix_det4x4_nondiag() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main() {
    let m = mat4x4<f32>(
        vec4<f32>(1.0, 2.0, 3.0, 4.0),
        vec4<f32>(5.0, 6.0, 7.0, 8.0),
        vec4<f32>(9.0, 10.0, 11.0, 12.0),
        vec4<f32>(13.0, 14.0, 15.0, 16.0)
    );
    let det = determinant(m);
    out[0] = det;
}
";
    let result = emit_compute_ptx(wgsl, 120);
    assert!(
        result.is_ok(),
        "4x4 determinant (non-diagonal) should compile: {result:?}"
    );
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(ptx.contains("mul.f32"), "det4x4 uses mul: {ptx:.400}");
    assert!(
        ptx.contains("sub.f32"),
        "det4x4 uses sub (cofactor signs): {ptx:.400}"
    );
}
