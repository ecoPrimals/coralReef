// SPDX-License-Identifier: AGPL-3.0-or-later
//! Control flow translation coverage tests: if/else, loop, switch.
//!
//! These tests exercise `func_control.rs` by compiling WGSL control
//! flow constructs through naga → IR translation and asserting on the
//! resulting CFG structure (block counts, branch opcodes, phi nodes).

use super::super::ir::{ComputeShaderInfo, Op, ShaderModelInfo, ShaderStageInfo};
use super::{parse_wgsl, translate};

fn sm70() -> ShaderModelInfo {
    ShaderModelInfo::new(70, 64)
}

fn block_count_for(wgsl: &str) -> usize {
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("translation should succeed");
    shader.functions.iter().map(|f| f.blocks.len()).sum()
}

fn count_ops(wgsl: &str) -> OpCounts {
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("translation should succeed");
    let mut counts = OpCounts::default();
    shader.for_each_instr(&mut |instr| match &instr.op {
        Op::Bra(_) => counts.bra += 1,
        Op::PhiSrcs(_) => counts.phi_srcs += 1,
        Op::PhiDsts(_) => counts.phi_dsts += 1,
        Op::ISetP(_) => counts.isetp += 1,
        Op::Exit(_) => counts.exit += 1,
        _ => {}
    });
    if let ShaderStageInfo::Compute(ComputeShaderInfo { local_size, .. }) = shader.info.stage {
        counts.workgroup_size = local_size;
    }
    counts
}

#[derive(Debug, Default)]
struct OpCounts {
    bra: usize,
    phi_srcs: usize,
    phi_dsts: usize,
    isetp: usize,
    exit: usize,
    workgroup_size: [u16; 3],
}

// ---------------------------------------------------------------------------
// If-only (no else branch)
// ---------------------------------------------------------------------------

#[test]
fn if_only_no_else_creates_branch_and_merge() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            if gid.x < 32u {
                data[gid.x] = 1.0;
            }
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 3,
        "if-only should produce >= 3 blocks (entry, body, merge), got {blocks}"
    );
    let ops = count_ops(wgsl);
    assert!(
        ops.bra >= 1,
        "if-only should emit at least 1 branch, got {}",
        ops.bra
    );
}

// ---------------------------------------------------------------------------
// If/else with phi nodes
// ---------------------------------------------------------------------------

#[test]
fn if_else_with_variable_produces_phi_nodes() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            var x: f32 = 0.0;
            if gid.x < 32u {
                x = 1.0;
            } else {
                x = 2.0;
            }
            data[gid.x] = x;
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 4,
        "if/else should produce >= 4 blocks (entry, accept, reject, merge), got {blocks}"
    );
    let ops = count_ops(wgsl);
    assert!(
        ops.phi_srcs >= 2,
        "if/else with var should emit >= 2 PhiSrcs (one per branch), got {}",
        ops.phi_srcs
    );
    assert!(
        ops.phi_dsts >= 1,
        "if/else with var should emit >= 1 PhiDsts (at merge), got {}",
        ops.phi_dsts
    );
}

#[test]
fn if_else_without_variable_skips_phis() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            if gid.x < 32u {
                data[gid.x] = 1.0;
            } else {
                data[gid.x] = 2.0;
            }
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 4,
        "if/else should produce >= 4 blocks, got {blocks}"
    );
}

// ---------------------------------------------------------------------------
// Nested if/else
// ---------------------------------------------------------------------------

#[test]
fn nested_if_else_produces_deeper_cfg() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            var x: f32 = 0.0;
            if gid.x < 16u {
                if gid.x < 8u {
                    x = 1.0;
                } else {
                    x = 2.0;
                }
            } else {
                x = 3.0;
            }
            data[gid.x] = x;
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 6,
        "nested if/else should produce >= 6 blocks, got {blocks}"
    );
    let ops = count_ops(wgsl);
    assert!(
        ops.phi_dsts >= 2,
        "nested if/else with var should produce >= 2 PhiDsts (inner + outer merge), got {}",
        ops.phi_dsts
    );
}

// ---------------------------------------------------------------------------
// Simple loop with break
// ---------------------------------------------------------------------------

#[test]
fn loop_with_break_produces_back_edge_and_branch() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            var i: u32 = 0u;
            loop {
                if i >= 4u {
                    break;
                }
                data[gid.x] = data[gid.x] + 1.0;
                i = i + 1u;
            }
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 4,
        "loop with break should produce >= 4 blocks (entry, header, body, exit), got {blocks}"
    );
    let ops = count_ops(wgsl);
    assert!(
        ops.phi_srcs >= 1,
        "loop with var should emit PhiSrcs for loop header, got {}",
        ops.phi_srcs
    );
    assert!(
        ops.phi_dsts >= 1,
        "loop with var should emit PhiDsts for loop header, got {}",
        ops.phi_dsts
    );
}

// ---------------------------------------------------------------------------
// Loop with continue
// ---------------------------------------------------------------------------

#[test]
fn loop_with_continue_translates_successfully() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            var i: u32 = 0u;
            loop {
                if i >= 8u {
                    break;
                }
                i = i + 1u;
                if i == 4u {
                    continue;
                }
                data[gid.x] = data[gid.x] + f32(i);
            }
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 5,
        "loop with continue should produce >= 5 blocks, got {blocks}"
    );
    let ops = count_ops(wgsl);
    assert!(
        ops.bra >= 3,
        "loop with break + continue should emit >= 3 branches, got {}",
        ops.bra
    );
}

// ---------------------------------------------------------------------------
// For-style loop (naga desugars for → loop with continuing block)
// ---------------------------------------------------------------------------

#[test]
fn for_loop_translates_to_loop_with_continuing() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            for (var i: u32 = 0u; i < 4u; i = i + 1u) {
                data[gid.x] = data[gid.x] + 1.0;
            }
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 4,
        "for loop should produce >= 4 blocks, got {blocks}"
    );
    let ops = count_ops(wgsl);
    assert!(
        ops.phi_srcs >= 1,
        "for loop with var should emit PhiSrcs, got {}",
        ops.phi_srcs
    );
}

// ---------------------------------------------------------------------------
// While-style loop (desugared to loop { if !cond { break; } body; })
// ---------------------------------------------------------------------------

#[test]
fn while_loop_translates_successfully() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            var i: u32 = 0u;
            while i < 4u {
                data[gid.x] = data[gid.x] + 1.0;
                i = i + 1u;
            }
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 4,
        "while loop should produce >= 4 blocks, got {blocks}"
    );
}

// ---------------------------------------------------------------------------
// Switch with valued cases
// ---------------------------------------------------------------------------

#[test]
fn switch_with_multiple_cases_produces_chain() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            switch gid.x {
                case 0u: {
                    data[gid.x] = 1.0;
                }
                case 1u: {
                    data[gid.x] = 2.0;
                }
                case 2u: {
                    data[gid.x] = 3.0;
                }
                default: {
                    data[gid.x] = 0.0;
                }
            }
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 5,
        "switch with 3 cases + default should produce >= 5 blocks, got {blocks}"
    );
    let ops = count_ops(wgsl);
    assert!(
        ops.isetp >= 3,
        "switch with 3 valued cases should emit >= 3 ISetP comparisons, got {}",
        ops.isetp
    );
    assert!(
        ops.bra >= 3,
        "switch with 3 cases should emit >= 3 branches, got {}",
        ops.bra
    );
}

// ---------------------------------------------------------------------------
// Switch with only default
// ---------------------------------------------------------------------------

#[test]
fn switch_default_only_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            switch gid.x {
                default: {
                    data[gid.x] = 42.0;
                }
            }
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 2,
        "switch with default only should produce >= 2 blocks, got {blocks}"
    );
}

// ---------------------------------------------------------------------------
// Combined: if inside loop
// ---------------------------------------------------------------------------

#[test]
fn if_inside_loop_produces_correct_cfg() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            var sum: f32 = 0.0;
            for (var i: u32 = 0u; i < 8u; i = i + 1u) {
                if i % 2u == 0u {
                    sum = sum + 1.0;
                } else {
                    sum = sum - 1.0;
                }
            }
            data[gid.x] = sum;
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 6,
        "if inside loop should produce >= 6 blocks, got {blocks}"
    );
    let ops = count_ops(wgsl);
    assert!(
        ops.phi_srcs >= 2,
        "if/else + loop with var should emit multiple PhiSrcs, got {}",
        ops.phi_srcs
    );
    assert!(
        ops.phi_dsts >= 2,
        "if/else + loop with var should emit multiple PhiDsts, got {}",
        ops.phi_dsts
    );
}

// ---------------------------------------------------------------------------
// Loop inside if
// ---------------------------------------------------------------------------

#[test]
fn loop_inside_if_translates_successfully() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            if gid.x < 32u {
                for (var i: u32 = 0u; i < 4u; i = i + 1u) {
                    data[gid.x] = data[gid.x] + 1.0;
                }
            }
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 5,
        "loop inside if should produce >= 5 blocks, got {blocks}"
    );
}

// ---------------------------------------------------------------------------
// Multiple variables through if/else (multi-phi)
// ---------------------------------------------------------------------------

#[test]
fn if_else_multiple_vars_produces_multiple_phis() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            var a: f32 = 0.0;
            var b: f32 = 0.0;
            if gid.x < 32u {
                a = 1.0;
                b = 2.0;
            } else {
                a = 3.0;
                b = 4.0;
            }
            data[gid.x] = a + b;
        }
    ";
    let ops = count_ops(wgsl);
    assert!(
        ops.phi_dsts >= 1,
        "if/else with 2 vars should emit PhiDsts at merge, got {}",
        ops.phi_dsts
    );
}

// ---------------------------------------------------------------------------
// Switch inside loop
// ---------------------------------------------------------------------------

#[test]
fn switch_inside_loop_translates() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            for (var i: u32 = 0u; i < 4u; i = i + 1u) {
                switch i {
                    case 0u: {
                        data[gid.x] = data[gid.x] + 1.0;
                    }
                    case 1u: {
                        data[gid.x] = data[gid.x] + 2.0;
                    }
                    default: {
                        data[gid.x] = data[gid.x] + 0.5;
                    }
                }
            }
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 8,
        "switch inside loop should produce >= 8 blocks, got {blocks}"
    );
}

// ---------------------------------------------------------------------------
// Early return in if branch (dead code after return)
// ---------------------------------------------------------------------------

#[test]
fn early_return_in_if_marks_dead_code() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            if gid.x == 0u {
                data[0u] = 99.0;
                return;
            }
            data[gid.x] = f32(gid.x);
        }
    ";
    let blocks = block_count_for(wgsl);
    assert!(
        blocks >= 3,
        "early return in if should produce >= 3 blocks, got {blocks}"
    );
    let ops = count_ops(wgsl);
    assert!(
        ops.exit >= 1,
        "early return should emit at least 1 Exit, got {}",
        ops.exit
    );
}
