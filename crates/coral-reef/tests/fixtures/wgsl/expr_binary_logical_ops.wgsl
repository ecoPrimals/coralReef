// SPDX-License-Identifier: AGPL-3.0-or-later
// expr_binary: LogicalAnd, LogicalOr (&&, ||)
// Exercises: OpPLop3, predicate logic - use in if conditions to avoid select coercion

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> input: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let x = input[idx];
    let y = input[(idx + 1u) % 64u];
    let z = input[(idx + 2u) % 64u];

    let a = x > 0.0;
    let b = y < 1.0;
    let c = z >= 0.5;

    var result: f32 = 0.0;
    if a && b {
        result = 1.0;
    }
    if a || c {
        result = result + 2.0;
    }
    if (a && b) || c {
        result = result + 4.0;
    }
    if a && (b || c) {
        result = result + 8.0;
    }

    out[idx] = result;
}
