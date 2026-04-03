// SPDX-License-Identifier: AGPL-3.0-only
#![cfg(feature = "naga")]
//! Multi-architecture and legacy SM encoder coverage tests.
//!
//! Tests compile WGSL shaders across multiple NVIDIA architectures
//! (SM70–SM89, SM50/SM32/SM20 via raw SM API) and AMD (RDNA2/3/4).

use std::fmt::Write;

use coral_reef::{AmdArch, CompileOptions, GpuTarget, NvArch};

fn opts_for(nv: NvArch) -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Nvidia(nv),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn compile_for(wgsl: &str, nv: NvArch) -> Result<Vec<u8>, coral_reef::CompileError> {
    coral_reef::compile_wgsl(wgsl, &opts_for(nv))
}

fn compile_fixture_all_nv(wgsl: &str) {
    for &nv in NvArch::ALL {
        let r = compile_for(wgsl, nv);
        assert!(r.is_ok(), "{nv}: {}", r.unwrap_err());
    }
}

fn compile_raw_sm(wgsl: &str, sm: u8) {
    let r = coral_reef::compile_wgsl_raw_sm(wgsl, sm);
    assert!(r.is_ok(), "SM{sm}: {}", r.unwrap_err());
}

fn compile_fixture_legacy_nv(wgsl: &str) {
    for sm in [50, 32, 30, 21, 20] {
        let r = coral_reef::compile_wgsl_raw_sm(wgsl, sm);
        assert!(r.is_ok(), "SM{sm}: {}", r.unwrap_err());
    }
}

// --- Multi-architecture coverage ---
// These tests compile fixtures across SM75/SM80/SM86/SM89 + AMD,
// exercising architecture-specific latency tables, scheduling, and legalization.

#[test]
fn multi_arch_basic_compute() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = f32(id.x);
    out[id.x] = x * x + 2.0 * x + 1.0;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn multi_arch_shared_memory_barrier() {
    let wgsl = r"
var<workgroup> shmem: array<f32, 256>;
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    shmem[lid.x] = f32(lid.x);
    workgroupBarrier();
    out[lid.x] = shmem[255u - lid.x];
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn multi_arch_integer_ops() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let a = id.x;
    let b = a + 42u;
    let c = a * b;
    let d = c / (a + 1u);
    let e = d % 7u;
    let f = a & 0xFFu;
    let g = a | 0x100u;
    let h = a ^ b;
    let i = a << 2u;
    let j = b >> 1u;
    out[id.x] = e + f + g + h + i + j;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn multi_arch_float_transcendentals() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = f32(id.x) * 0.01;
    let a = sin(x);
    let b = cos(x);
    let c = exp2(x);
    let d = log2(x + 1.0);
    let e = sqrt(x + 1.0);
    let f = pow(x + 1.0, 2.5);
    out[id.x] = a + b + c + d + e + f;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn multi_arch_type_conversions() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let u = id.x;
    let i = i32(u);
    let f = f32(i);
    let u2 = u32(f);
    let f2 = f32(u2);
    out[id.x] = f + f2;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn multi_arch_control_flow() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    var sum: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 10u { break; }
        if i % 2u == 0u {
            sum = sum + f32(i);
        } else {
            sum = sum - f32(i) * 0.5;
        }
        i = i + 1u;
    }
    out[id.x] = sum + f32(id.x);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn multi_arch_vectors() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let v = vec4<f32>(f32(id.x), f32(id.x) * 2.0, f32(id.x) * 3.0, 1.0);
    let w = v * 2.0 + vec4<f32>(1.0, 2.0, 3.0, 4.0);
    out[id.x] = v.x * w.x + v.y * w.y + v.z * w.z + v.w * w.w;
}
";
    compile_fixture_all_nv(wgsl);
}

/// Naga expression + `func_ops` paths: compose/swizzle, splat, mat2×2, f64 vec2,
/// `arrayLength` on `array<vec4<f32>>`, `all`/`any`, `select`, int bitwise ops.
#[test]
fn multi_arch_naga_expr_and_func_ops() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read_write> di: array<f64>;
@group(0) @binding(2) var<storage, read_write> buf: array<vec4<f32>>;
@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = f32(gid.x);
    let b = a + 1.0;
    let c = a + 2.0;
    let v3 = vec3<f32>(a, b, c);
    let v4 = vec4<f32>(v3, 1.0);
    let s = vec2<u32>(gid.x);
    let u2 = s + vec2<u32>(1u, 2u);
    let m = mat2x2<f32>(vec2<f32>(1.0, 2.0), vec2<f32>(3.0, 4.0));
    let d = vec2<f64>(f64(gid.x), 1.0) + vec2<f64>(2.0, 3.0);
    let t = m * vec2<f32>(f32(d.x), f32(d.y));
    let n = arrayLength(&buf);
    let i = gid.x % n;
    let vx = buf[i] * vec4<f32>(2.0, 2.0, 2.0, 2.0);
    let cmp = vec2<f32>(a, b) < vec2<f32>(1e6, 1e6);
    let ok = all(cmp) || any(vec2<f32>(a, c) > vec2<f32>(1e3, 1e3));
    let lo = vec3<f32>(1.0, 2.0, 3.0);
    let hi = vec3<f32>(10.0, 20.0, 30.0);
    let pick = select(lo, hi, vec3<bool>(gid.x < 4u, gid.x < 8u, gid.x < 12u));
    let xi = i32(gid.x);
    let bits = ~xi + (-xi >> 1);
    out[gid.x] = t.x + t.y + vx.x + f32(u2.x)
        + pick.x + f32(bits)
        + select(0.0, 1.0, ok)
        + f32(arrayLength(&di));
    buf[gid.x] = v4.wzyx + vec4<f32>(f32(bits & 7), 0.0, 0.0, 0.0);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn multi_arch_atomics() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let old = atomicAdd(&counter, 1u);
    out[gid.x] = old;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn multi_arch_nested_loops() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var total: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 8u { break; }
        var j: u32 = 0u;
        loop {
            if j >= 8u { break; }
            total = total + f32(i * 8u + j);
            j = j + 1u;
        }
        i = i + 1u;
    }
    out[0] = total;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn multi_arch_multiple_bindings() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    c[id.x] = a[id.x] * b[id.x] + a[id.x] + b[id.x];
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn multi_arch_register_pressure() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..32 {
        let _ = writeln!(wgsl, "  var v{i}: f32 = f32({i});");
    }
    for i in 0..32 {
        let _ = writeln!(wgsl, "  v{i} = v{i} * 2.0 + 1.0;");
    }
    wgsl.push_str("  out[0] = v0 + v31;\n}\n");
    compile_fixture_all_nv(&wgsl);
}

#[test]
fn multi_arch_bit_manipulation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    let a = countOneBits(x);
    let b = countLeadingZeros(x);
    let d = reverseBits(x);
    let e = firstLeadingBit(x);
    out[id.x] = a + b + d + e;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn multi_arch_min_max_clamp() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = f32(id.x) * 0.1;
    let a = min(x, 5.0);
    let b = max(x, 0.5);
    let c = clamp(x, 1.0, 10.0);
    let d = abs(x - 5.0);
    let f = floor(x);
    let g = ceil(x);
    out[id.x] = a + b + c + d + f + g;
}
";
    compile_fixture_all_nv(wgsl);
}

// --- Opt-level sweep across architectures ---

#[test]
fn multi_arch_opt_levels() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    var x = f32(id.x);
    x = x * x + 2.0 * x + 1.0;
    if x > 100.0 { x = 100.0; }
    out[id.x] = x;
}
";
    for &nv in NvArch::ALL {
        for opt in 0..=3 {
            let mut o = opts_for(nv);
            o.opt_level = opt;
            let r = coral_reef::compile_wgsl(wgsl, &o);
            assert!(r.is_ok(), "{nv} opt={opt}: {}", r.unwrap_err());
        }
    }
}

// --- AMD-specific coverage ---

#[test]
fn amd_rdna2_rdna3_rdna4() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    out[0] = 42.0;
}
";
    for amd in [AmdArch::Rdna2, AmdArch::Rdna3, AmdArch::Rdna4] {
        let o = CompileOptions {
            target: GpuTarget::Amd(amd),
            opt_level: 2,
            ..CompileOptions::default()
        };
        let r = coral_reef::compile_wgsl(wgsl, &o);
        assert!(r.is_ok(), "{amd:?}: {}", r.unwrap_err());
    }
}

// --- Legacy SM encoder coverage (SM50, SM32, SM20) ---

#[test]
fn legacy_sm50_basic_compute() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    out[0] = 42.0;
}
";
    compile_raw_sm(wgsl, 50);
}

#[test]
fn legacy_sm32_basic_compute() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    out[0] = 42.0;
}
";
    compile_raw_sm(wgsl, 32);
}

#[test]
fn legacy_sm20_basic_compute() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    out[0] = 42.0;
}
";
    compile_raw_sm(wgsl, 20);
}

#[test]
fn legacy_all_sm_arithmetic() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var x: f32 = 1.0;
    x = x * 2.0 + 3.0;
    x = x - 1.0;
    out[0] = x;
}
";
    compile_fixture_legacy_nv(wgsl);
}

#[test]
fn legacy_all_sm_integer_ops() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var a: u32 = 10u;
    var b: u32 = 3u;
    let c = a + b;
    let d = a * b;
    let e = a - b;
    let h = a & b;
    let i = a | b;
    let j = a ^ b;
    let k = a << 2u;
    let l = b >> 1u;
    out[0] = c + d + e + h + i + j + k + l;
}
";
    compile_fixture_legacy_nv(wgsl);
}

#[test]
fn legacy_all_sm_control_flow() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var sum: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 10u { break; }
        if i < 5u {
            sum = sum + 1.0;
        } else {
            sum = sum - 1.0;
        }
        i = i + 1u;
    }
    out[0] = sum;
}
";
    compile_fixture_legacy_nv(wgsl);
}

#[test]
fn legacy_all_sm_type_conversions() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let u: u32 = 42u;
    let i: i32 = i32(u);
    let f: f32 = f32(i);
    let u2: u32 = u32(f);
    out[0] = f + f32(u2);
}
";
    compile_fixture_legacy_nv(wgsl);
}

#[test]
fn legacy_all_sm_shared_memory() {
    let wgsl = r"
var<workgroup> wg_data: array<f32, 64>;
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    wg_data[lid.x] = f32(lid.x);
    workgroupBarrier();
    out[lid.x] = wg_data[63u - lid.x];
}
";
    compile_fixture_legacy_nv(wgsl);
}

#[test]
fn legacy_all_sm_transcendentals() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f32 = 1.5;
    let a = sin(x);
    let b = cos(x);
    let c = exp2(x);
    let d = log2(x);
    let e = sqrt(x);
    out[0] = a + b + c + d + e;
}
";
    compile_fixture_legacy_nv(wgsl);
}

#[test]
fn legacy_all_sm_multiple_bindings() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;
@compute @workgroup_size(1)
fn main() {
    c[0] = a[0] * b[0] + a[0] + b[0];
}
";
    compile_fixture_legacy_nv(wgsl);
}

#[test]
fn legacy_all_sm_nested_loops() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var total: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 4u { break; }
        var j: u32 = 0u;
        loop {
            if j >= 4u { break; }
            total = total + f32(i * 4u + j);
            j = j + 1u;
        }
        i = i + 1u;
    }
    out[0] = total;
}
";
    compile_fixture_legacy_nv(wgsl);
}

#[test]
fn legacy_sm50_register_pressure() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..32 {
        let _ = writeln!(wgsl, "  var v{i}: f32 = f32({i});");
    }
    for i in 0..32 {
        let _ = writeln!(wgsl, "  v{i} = v{i} * 2.0 + 1.0;");
    }
    wgsl.push_str("  out[0] = v0 + v31;\n}\n");
    compile_raw_sm(&wgsl, 50);
}

#[test]
fn legacy_sm50_vectors() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let v = vec4<f32>(1.0, 2.0, 3.0, 4.0);
    let w = v * 2.0 + vec4<f32>(0.5, 1.5, 2.5, 3.5);
    out[0] = v.x * w.x + v.y * w.y + v.z * w.z + v.w * w.w;
}
";
    compile_raw_sm(wgsl, 50);
}

#[test]
fn legacy_sm30_kepler_a() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var x: f32 = 5.0;
    x = x * x + 2.0 * x + 1.0;
    x = sin(x) + cos(x);
    out[0] = x;
}
";
    compile_raw_sm(wgsl, 30);
}

#[test]
fn legacy_sm21_fermi() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var x: f32 = 5.0;
    x = x * x + 2.0 * x + 1.0;
    x = sin(x) + cos(x);
    out[0] = x;
}
";
    compile_raw_sm(wgsl, 21);
}
