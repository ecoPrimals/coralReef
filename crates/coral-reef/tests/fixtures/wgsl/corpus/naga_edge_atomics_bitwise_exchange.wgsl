// SPDX-License-Identifier: AGPL-3.0-only
// Exercises: naga_translate `emit_atomic` — And, Or, Xor, Exchange (non-CAS).

@group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = atomicAnd(&counter, 0xFFu);
    let b = atomicOr(&counter, 0x100u);
    let c = atomicXor(&counter, 0x0Fu);
    let d = atomicExchange(&counter, 42u);
    out[gid.x] = a + b + c + d;
}
