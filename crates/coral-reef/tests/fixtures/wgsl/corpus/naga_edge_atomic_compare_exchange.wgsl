// SPDX-License-Identifier: AGPL-3.0-only
// Exercises: naga_translate `emit_atomic` — `AtomicFunction::Exchange` with `compare: Some(_)`
//            (`AtomOp::CmpExch` / compare-exchange weak). Mirrors `pipeline_atomics_compare_exchange_weak_u32`.

@group(0) @binding(0) var<storage, read_write> a: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    atomicStore(&a, 8u);
    let r = atomicCompareExchangeWeak(&a, 8u, 99u);
    out[gid.x] = r.old_value + select(0u, 1u, r.exchanged);
}
