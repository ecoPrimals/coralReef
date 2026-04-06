// SPDX-License-Identifier: AGPL-3.0-or-later
// spill_values/spiller: high register pressure
// Exercises: spiller, live range splitting, many SSA values

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> input: array<f32>;

@compute @workgroup_size(1)
fn main() {
    let i0 = input[0];
    let i1 = input[1];
    let i2 = input[2];
    let i3 = input[3];
    let i4 = input[4];
    let i5 = input[5];
    let i6 = input[6];
    let i7 = input[7];

    var t0 = i0 + i1;
    var t1 = i1 + i2;
    var t2 = i2 + i3;
    var t3 = i3 + i4;
    var t4 = i4 + i5;
    var t5 = i5 + i6;
    var t6 = i6 + i7;
    var t7 = i7 + i0;

    t0 = t0 * t1 + t2;
    t1 = t1 * t2 + t3;
    t2 = t2 * t3 + t4;
    t3 = t3 * t4 + t5;
    t4 = t4 * t5 + t6;
    t5 = t5 * t6 + t7;
    t6 = t6 * t7 + t0;
    t7 = t7 * t0 + t1;

    t0 = sqrt(t0) + sqrt(t1);
    t1 = sqrt(t2) + sqrt(t3);
    t2 = sqrt(t4) + sqrt(t5);
    t3 = sqrt(t6) + sqrt(t7);

    t0 = t0 + t1 + t2 + t3;
    t1 = t0 * 0.5 + log(max(t0, 0.01));
    t2 = exp(t1 * 0.01) + inverseSqrt(max(t0, 0.01));

    out[0] = t0 + t1 + t2;
}
