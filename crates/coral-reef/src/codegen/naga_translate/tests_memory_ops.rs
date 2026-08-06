// SPDX-License-Identifier: AGPL-3.0-or-later
//! Memory operation translation coverage tests.
//!
//! Exercises `func_mem.rs` and memory-related paths in `expr.rs` by
//! compiling WGSL with loads, stores, shared memory, and atomics.

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

fn count_op(wgsl: &str, pred: impl Fn(&Op) -> bool) -> usize {
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("translation should succeed");
    let mut count = 0;
    shader.for_each_instr(&mut |instr| {
        if pred(&instr.op) {
            count += 1;
        }
    });
    count
}

// ---------------------------------------------------------------------------
// Global memory loads and stores
// ---------------------------------------------------------------------------

#[test]
fn global_load_emits_ld() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read> src: array<f32>;
        @group(0) @binding(1) var<storage, read_write> dst: array<f32>;
        @compute @workgroup_size(1) fn main() { dst[0] = src[0]; }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::Ld(_))));
}

#[test]
fn global_store_emits_st() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() { d[0] = 42.0; }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::St(_))));
}

#[test]
fn vec4_load_store_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<vec4<f32>>;
        @compute @workgroup_size(1) fn main() { d[0] = d[1]; }
    ";
    translates_ok(wgsl);
    assert!(has_op(wgsl, |op| matches!(op, Op::Ld(_))));
    assert!(has_op(wgsl, |op| matches!(op, Op::St(_))));
}

#[test]
fn multi_element_store_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<vec2<f32>>;
        @compute @workgroup_size(1) fn main() {
            d[0] = vec2<f32>(1.0, 2.0);
        }
    ";
    translates_ok(wgsl);
    assert!(has_op(wgsl, |op| matches!(op, Op::St(_))));
}

// ---------------------------------------------------------------------------
// Shared (workgroup) memory
// ---------------------------------------------------------------------------

#[test]
fn shared_memory_load_store_translates() {
    let wgsl = r"
        var<workgroup> shared_data: array<f32, 64>;
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
            shared_data[lid.x] = d[lid.x];
            workgroupBarrier();
            d[lid.x] = shared_data[63u - lid.x];
        }
    ";
    translates_ok(wgsl);
    assert!(has_op(wgsl, |op| matches!(op, Op::Bar(_))));
}

// ---------------------------------------------------------------------------
// Atomic operations
// ---------------------------------------------------------------------------

#[test]
fn atomic_add_emits_atom() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;
        @compute @workgroup_size(1) fn main() {
            atomicAdd(&counter, 1u);
        }
    ";
    assert!(has_op(wgsl, |op| matches!(op, Op::Atom(_))));
}

#[test]
fn atomic_max_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> val: atomic<i32>;
        @compute @workgroup_size(1) fn main() {
            atomicMax(&val, 42);
        }
    ";
    translates_ok(wgsl);
    assert!(has_op(wgsl, |op| matches!(op, Op::Atom(_))));
}

#[test]
fn atomic_exchange_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> val: atomic<u32>;
        @group(0) @binding(1) var<storage, read_write> out: array<u32>;
        @compute @workgroup_size(1) fn main() {
            out[0] = atomicExchange(&val, 99u);
        }
    ";
    translates_ok(wgsl);
}

#[test]
fn atomic_compare_exchange_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> val: atomic<u32>;
        @group(0) @binding(1) var<storage, read_write> out: array<u32>;
        @compute @workgroup_size(1) fn main() {
            let result = atomicCompareExchangeWeak(&val, 0u, 1u);
            out[0] = result.old_value;
        }
    ";
    translates_ok(wgsl);
}

// ---------------------------------------------------------------------------
// Indexed array access
// ---------------------------------------------------------------------------

#[test]
fn dynamic_array_index_load_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read> src: array<f32>;
        @group(0) @binding(1) var<storage, read_write> dst: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            dst[gid.x] = src[gid.x];
        }
    ";
    translates_ok(wgsl);
}

#[test]
fn struct_field_access_translates() {
    let wgsl = r"
        struct Particle {
            pos: vec3<f32>,
            vel: vec3<f32>,
        }
        @group(0) @binding(0) var<storage, read_write> particles: array<Particle>;
        @compute @workgroup_size(1) fn main() {
            particles[0].vel = particles[0].pos;
        }
    ";
    translates_ok(wgsl);
}

// ---------------------------------------------------------------------------
// Uniform (constant buffer) access
// ---------------------------------------------------------------------------

#[test]
fn uniform_buffer_load_translates() {
    let wgsl = r"
        struct Params { scale: f32, offset: f32 }
        @group(0) @binding(0) var<uniform> params: Params;
        @group(0) @binding(1) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() {
            d[0] = d[0] * params.scale + params.offset;
        }
    ";
    translates_ok(wgsl);
}

// ---------------------------------------------------------------------------
// Local variable load/store
// ---------------------------------------------------------------------------

#[test]
fn local_var_load_store_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() {
            var tmp: f32 = d[0];
            tmp = tmp * 2.0;
            d[0] = tmp;
        }
    ";
    translates_ok(wgsl);
}

#[test]
fn local_array_access_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @compute @workgroup_size(1) fn main() {
            var arr: array<f32, 4>;
            arr[0] = 1.0; arr[1] = 2.0; arr[2] = 3.0; arr[3] = 4.0;
            d[0] = arr[0] + arr[1] + arr[2] + arr[3];
        }
    ";
    translates_ok(wgsl);
}

// ---------------------------------------------------------------------------
// Array length intrinsic
// ---------------------------------------------------------------------------

#[test]
fn array_length_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> d: array<f32>;
        @group(0) @binding(1) var<storage, read_write> out: array<u32>;
        @compute @workgroup_size(1) fn main() {
            out[0] = arrayLength(&d);
        }
    ";
    translates_ok(wgsl);
}
