// SPDX-License-Identifier: AGPL-3.0-only

//! Interpolation, min/max, and builtin resolution coverage tests.

use super::super::ir::{Op, ShaderModelInfo};
use super::{parse_wgsl, translate};

fn sm70() -> ShaderModelInfo {
    ShaderModelInfo::new(70, 64)
}

// ---------------------------------------------------------------------------
// Interpolation & misc math coverage
// ---------------------------------------------------------------------------

#[test]
fn test_translate_fma() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = data[gid.x];
            let b = data[gid.x + 1u];
            let c = data[gid.x + 2u];
            data[gid.x] = fma(a, b, c);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("fma should translate");
    let mut has_ffma = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FFma(_)) {
            has_ffma = true;
        }
    });
    assert!(has_ffma, "fma should emit OpFFma");
}

#[test]
fn test_translate_sign() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = sign(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("sign should translate");
    let mut has_fsetp = false;
    let mut has_sel = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FSetP(_)) {
            has_fsetp = true;
        }
        if matches!(instr.op, Op::Sel(_)) {
            has_sel = true;
        }
    });
    assert!(has_fsetp, "sign needs FSetP for >0 and <0 comparisons");
    assert!(has_sel, "sign needs Sel to choose -1/0/+1");
}

#[test]
fn test_translate_mix() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = data[0u];
            let b = data[1u];
            let t = data[2u];
            data[gid.x] = mix(a, b, t);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("mix should translate");
    let mut has_ffma = false;
    let mut has_fadd = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FFma(_)) {
            has_ffma = true;
        }
        if matches!(instr.op, Op::FAdd(_)) {
            has_fadd = true;
        }
    });
    assert!(has_fadd, "mix = a + t*(b-a) needs FAdd for (b-a)");
    assert!(has_ffma, "mix = (b-a)*t + a needs FFma");
}

#[test]
fn test_translate_step() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let edge = data[0u];
            let x = data[gid.x];
            data[gid.x] = step(edge, x);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("step should translate");
    let mut has_fsetp = false;
    let mut has_sel = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FSetP(_)) {
            has_fsetp = true;
        }
        if matches!(instr.op, Op::Sel(_)) {
            has_sel = true;
        }
    });
    assert!(has_fsetp, "step needs FSetP for x >= edge");
    assert!(has_sel, "step needs Sel to choose 0.0 or 1.0");
}

#[test]
fn test_translate_smoothstep() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let lo = data[0u];
            let hi = data[1u];
            let x = data[gid.x];
            data[gid.x] = smoothstep(lo, hi, x);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("smoothstep should translate");
    let mut has_transcendental = false;
    let mut fmul_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            has_transcendental = true;
        }
        if matches!(instr.op, Op::FMul(_)) {
            fmul_count += 1;
        }
    });
    assert!(has_transcendental, "smoothstep uses Rcp for 1/(hi-lo)");
    assert!(
        fmul_count >= 3,
        "smoothstep needs multiple FMul for t*t*(3-2*t), got {fmul_count}"
    );
}

// ---------------------------------------------------------------------------
// Min / Max coverage
// ---------------------------------------------------------------------------

#[test]
fn test_translate_min_max() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = data[gid.x];
            let b = data[gid.x + 1u];
            data[gid.x] = min(a, b) + max(a, b);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("min/max should translate");
    let mut fmnmx_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FMnMx(_)) {
            fmnmx_count += 1;
        }
    });
    assert!(
        fmnmx_count >= 2,
        "min + max should emit at least 2 FMnMx, got {fmnmx_count}"
    );
}

// ---------------------------------------------------------------------------
// Builtin resolution coverage
// ---------------------------------------------------------------------------

#[test]
fn test_translate_local_invocation_id() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
            data[lid.x] = lid.y + lid.z;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("local_invocation_id should translate");
    let mut s2r_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::S2R(_)) {
            s2r_count += 1;
        }
    });
    assert!(
        s2r_count >= 3,
        "local_invocation_id needs 3 S2R (TID_X/Y/Z), got {s2r_count}"
    );
}

#[test]
fn test_translate_workgroup_id() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(workgroup_id) wg: vec3<u32>) {
            data[wg.x] = wg.y + wg.z;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("workgroup_id should translate");
    let mut s2r_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::S2R(_)) {
            s2r_count += 1;
        }
    });
    assert!(
        s2r_count >= 3,
        "workgroup_id needs 3 S2R (CTAID_X/Y/Z), got {s2r_count}"
    );
}

#[test]
fn test_translate_num_workgroups() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(num_workgroups) nwg: vec3<u32>) {
            data[0u] = nwg.x * nwg.y * nwg.z;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("num_workgroups should translate");
    let mut s2r_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::S2R(_)) {
            s2r_count += 1;
        }
    });
    assert!(
        s2r_count >= 3,
        "num_workgroups needs 3 S2R (NCTAID_X/Y/Z), got {s2r_count}"
    );
}

#[test]
fn test_translate_local_invocation_index() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(8, 4, 2)
        fn main(@builtin(local_invocation_index) idx: u32) {
            data[idx] = idx;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("local_invocation_index should translate");
    let mut s2r_count = 0u32;
    let mut has_imad = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::S2R(_)) {
            s2r_count += 1;
        }
        if matches!(instr.op, Op::IMad(_)) {
            has_imad = true;
        }
    });
    assert!(
        s2r_count >= 3,
        "local_invocation_index needs TID_X/Y/Z reads, got {s2r_count}"
    );
    assert!(has_imad, "local_invocation_index linearization uses IMad");
}
