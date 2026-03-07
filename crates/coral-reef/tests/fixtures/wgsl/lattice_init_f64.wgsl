// lattice_init_f64.wgsl — Cold/hot start for SU(3) lattice gauge links
//
// Prepend: complex_f64.wgsl + su3.wgsl + lcg_f64.wgsl + su3_extended_f64.wgsl
//
// Two entry points:
//   cold_start: set all links to identity
//   hot_start:  generate random SU(3) near identity per link
//
// Buffer: links[V × 4 × 18] f64  (V sites × 4 directions × 9 complex = 18 f64)
// RNG state: rng_state[V × 4] u32 (one per link, seeded from host)

struct InitParams {
    volume:  u32,
    n_links: u32,  // volume × 4
    epsilon: f64,  // spread for hot start
}

@group(0) @binding(0) var<uniform>             params:    InitParams;
@group(0) @binding(1) var<storage, read_write> links:     array<f64>;
@group(0) @binding(2) var<storage, read_write> rng_state: array<u32>;

@compute @workgroup_size(64)
fn cold_start(@builtin(global_invocation_id) gid: vec3<u32>) {
    let link_id = gid.x;
    if (link_id >= params.n_links) { return; }

    let id_mat = su3_identity();
    let base = link_id * 18u;
    var idv = id_mat;
    for (var i = 0u; i < 9u; i = i + 1u) {
        links[base + i * 2u]      = idv[i].x;
        links[base + i * 2u + 1u] = idv[i].y;
    }
}

@compute @workgroup_size(64)
fn hot_start(@builtin(global_invocation_id) gid: vec3<u32>) {
    let link_id = gid.x;
    if (link_id >= params.n_links) { return; }

    var state = rng_state[link_id];
    let m = su3_random_near_identity(&state, params.epsilon);
    rng_state[link_id] = state;

    let base = link_id * 18u;
    var mv = m;
    for (var i = 0u; i < 9u; i = i + 1u) {
        links[base + i * 2u]      = mv[i].x;
        links[base + i * 2u + 1u] = mv[i].y;
    }
}
