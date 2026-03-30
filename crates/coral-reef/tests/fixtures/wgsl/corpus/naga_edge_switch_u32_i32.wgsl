// SPDX-License-Identifier: AGPL-3.0-only
// Exercises: naga_translate `translate_switch` — valued `SwitchValue::U32` cases with default,
//            and `SwitchValue::I32` cases (signed selector / negative case labels).

@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let sel_u = gid.x % 4u;
    var acc_u: u32 = 0u;
    switch (sel_u) {
        case 0u: {
            acc_u = 10u;
        }
        case 1u: {
            acc_u = 20u;
        }
        case 2u: {
            acc_u = 30u;
        }
        default: {
            acc_u = 99u;
        }
    }

    let sel_i = i32(sel_u) - 1;
    var acc_i: i32 = 0;
    switch (sel_i) {
        case -1: {
            acc_i = 1;
        }
        case 0: {
            acc_i = 2;
        }
        case 1: {
            acc_i = 3;
        }
        default: {
            acc_i = 9;
        }
    }

    out[gid.x] = acc_u + u32(acc_i);
}
