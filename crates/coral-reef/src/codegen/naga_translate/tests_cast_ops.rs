// SPDX-License-Identifier: AGPL-3.0-or-later
//! Type conversion and cast translation coverage tests.
//!
//! Exercises `func_ops.rs` `translate_cast` by compiling WGSL with various
//! type conversions and verifying the expected IR opcodes are emitted.

use super::super::ir::{Op, ShaderModelInfo};
use super::{parse_wgsl, translate};

fn sm70() -> ShaderModelInfo {
    ShaderModelInfo::new(70, 64)
}

fn has_op(wgsl: &str, pred: impl Fn(&Op) -> bool) -> bool {
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("translation should succeed");
    let mut found = false;
    shader.for_each_instr(&mut |instr| {
        if pred(&instr.op) {
            found = true;
        }
    });
    found
}

fn translates_ok(wgsl: &str) {
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    translate(&module, &sm, "main").expect("translation should succeed");
}

// ---------------------------------------------------------------------------
// Integer → Float conversions
// ---------------------------------------------------------------------------

#[test]
fn i32_to_f32_emits_i2f() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @group(0) @binding(1) var<storage, read> si: array<i32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            d[gid.x] = f32(si[gid.x]);
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::I2F(_))));
}

#[test]
fn u32_to_f32_emits_i2f() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            d[gid.x] = f32(gid.x);
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::I2F(_))));
}

// ---------------------------------------------------------------------------
// Float → Integer conversions
// ---------------------------------------------------------------------------

#[test]
fn f32_to_i32_emits_f2i() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<i32>;
        @group(0) @binding(1) var<storage, read> sf: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            d[gid.x] = i32(sf[gid.x]);
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::F2I(_))));
}

#[test]
fn f32_to_u32_emits_f2i() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @group(0) @binding(1) var<storage, read> sf: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            d[gid.x] = u32(sf[gid.x]);
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::F2I(_))));
}

// ---------------------------------------------------------------------------
// Bitcast (reinterpret) conversions
// ---------------------------------------------------------------------------

#[test]
fn bitcast_f32_to_u32_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @group(0) @binding(1) var<storage, read> sf: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            d[gid.x] = bitcast<u32>(sf[gid.x]);
        }
    ";
    translates_ok(wgsl);
}

#[test]
fn bitcast_u32_to_f32_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @group(0) @binding(1) var<storage, read> su: array<u32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            d[gid.x] = bitcast<f32>(su[gid.x]);
        }
    ";
    translates_ok(wgsl);
}

#[test]
fn bitcast_i32_to_f32_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @group(0) @binding(1) var<storage, read> si: array<i32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            d[gid.x] = bitcast<f32>(si[gid.x]);
        }
    ";
    translates_ok(wgsl);
}

#[test]
fn bitcast_vec2_u32_to_vec2_f32_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<vec2<f32>>;
        @group(0) @binding(1) var<storage, read> su: array<vec2<u32>>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            d[gid.x] = bitcast<vec2<f32>>(su[gid.x]);
        }
    ";
    translates_ok(wgsl);
}

// ---------------------------------------------------------------------------
// Same-type "casts" (identity / bitwidth-preserving)
// ---------------------------------------------------------------------------

#[test]
fn i32_to_u32_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @group(0) @binding(1) var<storage, read> si: array<i32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            d[gid.x] = u32(si[gid.x]);
        }
    ";
    translates_ok(wgsl);
}

#[test]
fn u32_to_i32_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<i32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            d[gid.x] = i32(gid.x);
        }
    ";
    translates_ok(wgsl);
}

// ---------------------------------------------------------------------------
// Bool conversions
// ---------------------------------------------------------------------------

#[test]
fn bool_to_u32_via_select_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @group(0) @binding(1) var<storage, read> sf: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let cond = sf[gid.x] > 0.0;
            d[gid.x] = select(0u, 1u, cond);
        }
    ";
    translates_ok(wgsl);
}

// ---------------------------------------------------------------------------
// Vector type conversions
// ---------------------------------------------------------------------------

#[test]
fn vec3_u32_to_vec3_f32_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let fv = vec3<f32>(gid);
            d[0] = fv.x + fv.y + fv.z;
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::I2F(_))));
}

#[test]
fn vec2_f32_to_vec2_i32_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<i32>;
        @group(0) @binding(1) var<storage, read> sf: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let v = vec2<f32>(sf[0], sf[1]);
            let iv = vec2<i32>(v);
            d[gid.x] = iv.x + iv.y;
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::F2I(_))));
}

// ---------------------------------------------------------------------------
// Mixed arithmetic with implicit casts
// ---------------------------------------------------------------------------

#[test]
fn mixed_int_float_arithmetic_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let idx = f32(gid.x);
            let scaled = idx * 2.5;
            d[gid.x] = scaled + f32(gid.y);
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::I2F(_))));
}

// ---------------------------------------------------------------------------
// Relational: all, any
// ---------------------------------------------------------------------------

#[test]
fn all_vec_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @group(0) @binding(1) var<storage, read> sf: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = sf[0] > 0.0;
            let b = sf[1] > 0.0;
            let v = vec2<bool>(a, b);
            d[gid.x] = select(0.0, 1.0, all(v));
        }
    ";
    translates_ok(wgsl);
}

#[test]
fn any_vec_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @group(0) @binding(1) var<storage, read> sf: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = sf[0] > 0.0;
            let b = sf[1] > 0.0;
            let v = vec2<bool>(a, b);
            d[gid.x] = select(0.0, 1.0, any(v));
        }
    ";
    translates_ok(wgsl);
}
