// SPDX-License-Identifier: AGPL-3.0-or-later
//! Binary operator translation coverage tests.
//!
//! Exercises `expr_binary.rs` by compiling WGSL with each category of
//! binary operator and verifying the expected IR opcodes are emitted.

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
// Float arithmetic
// ---------------------------------------------------------------------------

#[test]
fn f32_add_emits_fadd() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] + d[1]; }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::FAdd(_))));
}

#[test]
fn f32_sub_emits_fadd_with_neg() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] - d[1]; }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::FAdd(_))));
}

#[test]
fn f32_mul_emits_fmul() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] * d[1]; }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::FMul(_))));
}

#[test]
fn f32_div_emits_transcendental_rcp_and_fmul() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] / d[1]; }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::Transcendental(_))));
    assert!(has_op(wgsl, |op| matches!(op, Op::FMul(_))));
}

// ---------------------------------------------------------------------------
// Integer arithmetic
// ---------------------------------------------------------------------------

#[test]
fn i32_add_emits_iadd() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<i32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] + d[1]; }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::IAdd3(_))));
}

#[test]
fn u32_multiply_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] * d[1]; }
    ";
    translates_ok(wgsl);
}

#[test]
fn u32_divide_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] / d[1]; }
    ";
    translates_ok(wgsl);
}

#[test]
fn u32_modulo_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] % d[1]; }
    ";
    translates_ok(wgsl);
}

// ---------------------------------------------------------------------------
// Bitwise operations
// ---------------------------------------------------------------------------

#[test]
fn bitwise_and_emits_lop3() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] & d[1]; }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::Lop3(_))));
}

#[test]
fn bitwise_or_emits_lop3() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] | d[1]; }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::Lop3(_))));
}

#[test]
fn bitwise_xor_emits_lop3() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] ^ d[1]; }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::Lop3(_))));
}

// ---------------------------------------------------------------------------
// Shift operations
// ---------------------------------------------------------------------------

#[test]
fn shift_left_emits_shf() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] << d[1]; }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::Shf(_))));
}

#[test]
fn shift_right_emits_shf() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] >> d[1]; }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::Shf(_))));
}

// ---------------------------------------------------------------------------
// Comparison operations (float)
// ---------------------------------------------------------------------------

#[test]
fn f32_less_than_emits_fsetp() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() {
            if d[0] < d[1] { d[0] = 1.0; }
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::FSetP(_))));
}

#[test]
fn f32_greater_equal_emits_fsetp() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() {
            if d[0] >= d[1] { d[0] = 1.0; }
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::FSetP(_))));
}

#[test]
fn f32_equal_emits_fsetp() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() {
            if d[0] == d[1] { d[0] = 1.0; }
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::FSetP(_))));
}

// ---------------------------------------------------------------------------
// Comparison operations (integer)
// ---------------------------------------------------------------------------

#[test]
fn u32_less_than_emits_isetp() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<u32>;
        @compute @workgroup_size(1) fn main() {
            if d[0] < d[1] { d[0] = 1u; }
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::ISetP(_))));
}

#[test]
fn i32_not_equal_emits_isetp() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<i32>;
        @compute @workgroup_size(1) fn main() {
            if d[0] != d[1] { d[0] = 1; }
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::ISetP(_))));
}

// ---------------------------------------------------------------------------
// Logical operations
// ---------------------------------------------------------------------------

#[test]
fn logical_and_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() {
            if d[0] > 0.0 && d[1] > 0.0 { d[0] = 1.0; }
        }
    ";
    translates_ok(wgsl);
}

#[test]
fn logical_or_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() {
            if d[0] > 0.0 || d[1] > 0.0 { d[0] = 1.0; }
        }
    ";
    translates_ok(wgsl);
}

// ---------------------------------------------------------------------------
// Vector binary operations
// ---------------------------------------------------------------------------

#[test]
fn vec4_f32_add_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<vec4<f32>>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] + d[1]; }
    ";
    translates_ok(wgsl);
    assert!(has_op(wgsl, |op| matches!(op, Op::FAdd(_))));
}

#[test]
fn vec2_u32_bitwise_and_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<vec2<u32>>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] & d[1]; }
    ";
    translates_ok(wgsl);
}

// ---------------------------------------------------------------------------
// Mixed-type expressions
// ---------------------------------------------------------------------------

#[test]
fn f32_modulo_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() { d[0] = d[0] % d[1]; }
    ";
    translates_ok(wgsl);
}
