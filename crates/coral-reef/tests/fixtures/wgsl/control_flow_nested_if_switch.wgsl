// SPDX-License-Identifier: AGPL-3.0-only
// Control flow: nested if/else (switch hits RA bug)
// Exercises: repair_ssa, phi nodes, CFG merge blocks

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> mode: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let m = mode[0];
    var result: f32 = 0.0;
    if m < 32u {
        if m > 0u {
            result = 1.0;
        } else {
            result = 2.0;
        }
    } else {
        if m == 0u {
            result = 3.0;
        } else if m == 1u {
            result = 4.0;
        } else if m == 2u {
            result = 4.0;
        } else {
            result = 5.0;
        }
    }
    out[0] = result;
}
