// SPDX-License-Identifier: AGPL-3.0-or-later
//
// diversity_f64.wgsl — Shannon and Simpson diversity indices on GPU
//
// Uses f32 log() cast to f64 for Shannon H' — driver-portable approach.
// Full f64 log requires barraCuda compile_shader_f64 pipeline.
//
// Dispatch: (n_communities, 1, 1) — one workgroup per community

struct Params {
    n_communities: u32,
    stride: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<storage, read> abundances: array<f64>;
@group(0) @binding(1) var<storage, read_write> output: array<f64>;
@group(0) @binding(2) var<uniform> params: Params;

var<workgroup> shared_shannon: array<f64, 256>;
var<workgroup> shared_simpson: array<f64, 256>;

fn log_f64(x: f64) -> f64 {
    return f64(log(f32(x)));
}

@compute @workgroup_size(256)
fn main(
    @builtin(workgroup_id) wg_id: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let community = wg_id.x;
    if community >= params.n_communities {
        return;
    }

    let local = lid.x;
    let base = community * params.stride;

    var sum_shannon: f64 = 0.0;
    var sum_simpson_sq: f64 = 0.0;

    var i = local;
    loop {
        if i >= params.stride {
            break;
        }
        let p = abundances[base + i];
        if p > 0.0 {
            sum_shannon = sum_shannon - p * log_f64(p);
        }
        sum_simpson_sq = sum_simpson_sq + p * p;
        i = i + 256u;
    }

    shared_shannon[local] = sum_shannon;
    shared_simpson[local] = sum_simpson_sq;
    workgroupBarrier();

    var s = 128u;
    loop {
        if s == 0u {
            break;
        }
        if local < s {
            shared_shannon[local] = shared_shannon[local] + shared_shannon[local + s];
            shared_simpson[local] = shared_simpson[local] + shared_simpson[local + s];
        }
        workgroupBarrier();
        s = s >> 1u;
    }

    if local == 0u {
        output[community * 2u] = shared_shannon[0];
        output[community * 2u + 1u] = 1.0 - shared_simpson[0];
    }
}
