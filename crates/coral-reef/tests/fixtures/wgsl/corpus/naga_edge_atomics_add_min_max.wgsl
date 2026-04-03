// SPDX-License-Identifier: AGPL-3.0-only
// Exercises: naga_translate `emit_atomic` — Add, Min, Max (`AtomOp` variants).

@group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = atomicAdd(&counter, 1u);
    let b = atomicMin(&counter, 0u);
    let c = atomicMax(&counter, 100u);
    out[gid.x] = a + b + c;
}
