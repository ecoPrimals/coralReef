// SPDX-License-Identifier: AGPL-3.0-or-later
// Data types: structs, arrays of structs
// Exercises: struct field access, array indexing, copy
// Uses local_invocation_id for AMD compatibility (no SR_NTID)

struct Particle {
    pos: vec3<f32>,
    vel: vec3<f32>,
    mass: f32,
}

struct Config {
    count: u32,
    dt: f32,
}

@group(0) @binding(0) var<storage, read_write> particles: array<Particle>;
@group(0) @binding(1) var<uniform> config: Config;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let idx = lid.x;
    if idx >= config.count { return; }
    var p = particles[idx];
    p.pos = p.pos + p.vel * config.dt;
    particles[idx] = p;
}
