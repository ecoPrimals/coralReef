// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

// --- HMMA / tensor-core GEMM codegen tests ---

#[test]
fn test_compile_gemm_f16f32_sm80() {
    let shape = GemmShape { m: 16, n: 8, k: 16 };
    let target = GpuTarget::Nvidia(NvArch::Sm80);
    let result = compile_gemm(shape, GemmPrecision::F16F32, target);
    let compiled = result.expect("compile_gemm should succeed for SM80 f16f32");
    assert_eq!(compiled.format, BinaryFormat::Ptx);
    let ptx = String::from_utf8(compiled.binary).expect("PTX should be valid UTF-8");
    assert!(ptx.contains("mma.sync.aligned.m16n8k16.row.col.f32.f16.f16.f32"));
    assert!(ptx.contains(".target sm_80"));
}

#[test]
fn test_compile_gemm_rejects_pre_sm80() {
    let shape = GemmShape { m: 16, n: 8, k: 16 };
    let target = GpuTarget::Nvidia(NvArch::Sm70);
    let err = compile_gemm(shape, GemmPrecision::F16F32, target)
        .expect_err("SM70 should be rejected for GEMM");
    assert!(matches!(err, CompileError::UnsupportedArch(_)));
}

#[test]
fn test_compile_gemm_rejects_amd_target() {
    let shape = GemmShape { m: 16, n: 8, k: 16 };
    let target = GpuTarget::Amd(AmdArch::Rdna2);
    let err = compile_gemm(shape, GemmPrecision::F16F32, target)
        .expect_err("AMD should be rejected for HMMA GEMM");
    assert!(matches!(err, CompileError::UnsupportedArch(_)));
}

#[test]
fn test_compile_gemm_rejects_misaligned_k() {
    let shape = GemmShape { m: 16, n: 8, k: 13 };
    let target = GpuTarget::Nvidia(NvArch::Sm80);
    let err = compile_gemm(shape, GemmPrecision::F16F32, target)
        .expect_err("misaligned K should be rejected");
    assert!(matches!(err, CompileError::InvalidInput(_)));
}

#[test]
fn test_compile_gemm_rejects_zero_dimensions() {
    let target = GpuTarget::Nvidia(NvArch::Sm80);
    for shape in [
        GemmShape { m: 0, n: 8, k: 16 },
        GemmShape { m: 16, n: 0, k: 16 },
        GemmShape { m: 16, n: 8, k: 0 },
    ] {
        let err = compile_gemm(shape, GemmPrecision::F16F32, target)
            .expect_err("zero dimension should be rejected");
        assert!(matches!(err, CompileError::InvalidInput(_)));
    }
}

#[test]
fn test_compile_gemm_sm120_blackwell() {
    let shape = GemmShape {
        m: 32,
        n: 16,
        k: 32,
    };
    let target = GpuTarget::Nvidia(NvArch::Sm120);
    let compiled = compile_gemm(shape, GemmPrecision::F16F32, target)
        .expect("compile_gemm should succeed for SM120 (Blackwell)");
    assert_eq!(compiled.format, BinaryFormat::Ptx);
    let ptx = String::from_utf8(compiled.binary).expect("PTX should be valid UTF-8");
    assert!(ptx.contains(".target sm_120"));
}

#[test]
fn test_compile_gemm_tf32_requires_k_aligned_to_8() {
    let target = GpuTarget::Nvidia(NvArch::Sm80);
    let ok_shape = GemmShape { m: 16, n: 8, k: 8 };
    compile_gemm(ok_shape, GemmPrecision::Tf32, target).expect("TF32 with K=8 should succeed");

    let bad_shape = GemmShape { m: 16, n: 8, k: 12 };
    let err = compile_gemm(bad_shape, GemmPrecision::Tf32, target)
        .expect_err("TF32 with K=12 should fail (not aligned to 8)");
    assert!(matches!(err, CompileError::InvalidInput(_)));
}

#[test]
fn test_compile_gemm_rejects_misaligned_m() {
    let target = GpuTarget::Nvidia(NvArch::Sm80);
    let shape = GemmShape { m: 17, n: 8, k: 16 };
    let err = compile_gemm(shape, GemmPrecision::F16F32, target)
        .expect_err("M=17 (not multiple of 16) should be rejected");
    assert!(matches!(err, CompileError::InvalidInput(_)));
}

#[test]
fn test_compile_gemm_rejects_misaligned_n() {
    let target = GpuTarget::Nvidia(NvArch::Sm80);
    let shape = GemmShape {
        m: 16,
        n: 12,
        k: 16,
    };
    let err = compile_gemm(shape, GemmPrecision::F16F32, target)
        .expect_err("N=12 (not multiple of 8) should be rejected");
    assert!(matches!(err, CompileError::InvalidInput(_)));
}

#[test]
fn test_compile_gemm_tiled_thread_mapping() {
    let shape = GemmShape {
        m: 64,
        n: 32,
        k: 16,
    };
    let target = GpuTarget::Nvidia(NvArch::Sm86);
    let compiled =
        compile_gemm(shape, GemmPrecision::F16F32, target).expect("64x32x16 f16f32 should succeed");
    assert_eq!(
        compiled.info.local_size,
        [32, 1, 1],
        "1 warp per CTA = 32 threads"
    );
    let ptx = String::from_utf8(compiled.binary).expect("valid UTF-8");
    assert!(
        ptx.contains(".reqntid 32"),
        "kernel must set required thread count"
    );
    assert!(ptx.contains("%ctaid.x"), "must use block ID for N tiling");
    assert!(ptx.contains("%ctaid.y"), "must use block ID for M tiling");
    assert!(
        ptx.contains("Grid: (4, 4, 1)"),
        "32/8=4 along N, 64/16=4 along M"
    );
}

#[test]
fn test_compile_gemm_large_shape_128x128x64() {
    let shape = GemmShape {
        m: 128,
        n: 128,
        k: 64,
    };
    let target = GpuTarget::Nvidia(NvArch::Sm80);
    let compiled =
        compile_gemm(shape, GemmPrecision::F16F32, target).expect("128x128x64 should succeed");
    let ptx = String::from_utf8(compiled.binary).expect("valid UTF-8");
    let mma_count = ptx
        .lines()
        .filter(|l| l.trim_start().starts_with("mma.sync.aligned"))
        .count();
    assert_eq!(mma_count, 4, "K=64/16 = 4 MMA instructions");
    assert!(
        ptx.contains("Grid: (16, 8, 1)"),
        "128/8=16 N blocks, 128/16=8 M blocks"
    );
}

// --- f64 type resolution tests ---

#[test]
fn test_f64_math_on_call_result() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> output: array<f64>;

fn compute_distance(x: f64, y: f64) -> f64 {
    return x * x + y * y;
}

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = output[0u];
    let y = output[1u];
    let dist_sq = compute_distance(x, y);
    let dist = sqrt(dist_sq);
    let inv = f64(1.0) / dist;
    output[gid.x] = inv * exp(-dist / f64(2.0));
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        fp64_software: true,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "f64 math on user-function CallResult should compile: {result:?}"
    );
}

#[test]
fn test_f64_math_on_nested_struct_member() {
    let wgsl = r"
struct Inner {
    value: f64,
    scale: f64,
}

struct Outer {
    inner: Inner,
    phase: f64,
}

@group(0) @binding(0) var<storage, read_write> output: array<f64>;
@group(0) @binding(1) var<uniform> params: Outer;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = sqrt(params.inner.value);
    let e = exp(params.inner.scale);
    output[gid.x] = v + e + params.phase;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "f64 math on nested struct members should compile: {result:?}"
    );
}

// --- Subgroup operation tests ---

#[test]
fn test_subgroup_add_reduce_sm70() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = output[gid.x];
    let sum = subgroupAdd(val);
    output[gid.x] = sum;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "subgroupAdd should compile for SM70: {result:?}"
    );
}

#[test]
fn test_subgroup_broadcast_sm70() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = output[gid.x];
    let broadcast = subgroupBroadcast(val, 0u);
    output[gid.x] = broadcast;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "subgroupBroadcast should compile for SM70: {result:?}"
    );
}

#[test]
fn test_subgroup_ballot_sm70() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>,
        @builtin(subgroup_invocation_id) lane: u32) {
    let is_lower_half = lane < 32u;
    let ballot = subgroupBallot(is_lower_half);
    output[gid.x] = ballot.x;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "subgroupBallot should compile for SM70: {result:?}"
    );
}

#[test]
fn test_subgroup_add_reduce_sm120_ptx() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = output[gid.x];
    let sum = subgroupAdd(val);
    output[gid.x] = sum;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm120),
        ..CompileOptions::default()
    };
    let result = compile_wgsl_full(wgsl, &opts);
    assert!(
        result.is_ok(),
        "subgroupAdd should compile for SM120: {result:?}"
    );
    let compiled = result.unwrap();
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains("redux.sync"),
        "SM120 subgroupAdd should emit redux.sync: {ptx}"
    );
}

#[test]
fn test_subgroup_size_builtin_sm70() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>,
        @builtin(subgroup_size) sg_size: u32,
        @builtin(num_subgroups) num_sg: u32) {
    output[gid.x] = sg_size + num_sg;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "SubgroupSize + NumSubgroups builtins should compile for SM70: {result:?}"
    );
}

#[test]
fn test_subgroup_size_in_reduction_sm70() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(128)
fn main(@builtin(global_invocation_id) gid: vec3<u32>,
        @builtin(subgroup_size) warp_size: u32,
        @builtin(subgroup_invocation_id) lane: u32) {
    var val = data[gid.x];
    let sum = subgroupAdd(val);
    if lane == 0u {
        data[gid.x / warp_size] = sum;
    }
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "SubgroupSize in reduction kernel should compile: {result:?}"
    );
}

#[test]
fn test_subgroup_ballot_copy_prop_f64_sm70() {
    // Regression: subgroupBallot returns uvec4 (4 components). Copy propagation
    // must not assert comps()==1 when encountering multi-component SSA refs.
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> output: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>,
        @builtin(subgroup_invocation_id) lane: u32) {
    let is_active = lane < 32u;
    let ballot = subgroupBallot(is_active);
    let count = countOneBits(ballot.x) + countOneBits(ballot.y);
    output[gid.x] = f64(count);
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        fp64_software: true,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "subgroupBallot + f64 copy prop should not panic: {result:?}"
    );
}

#[test]
fn test_sum_reduce_subgroup_f64_sm70() {
    // Pattern from ecosystem sum_reduce_subgroup_f64.wgsl benchmark:
    // Uses SubgroupSize/NumSubgroups builtins with f64 data storage.
    // The reduction itself uses f32 partial sums; f64 is for accumulation.
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> data: array<f32>;
@group(0) @binding(1) var<storage, read_write> partial: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>,
        @builtin(subgroup_size) warp_size: u32,
        @builtin(subgroup_invocation_id) lane: u32,
        @builtin(num_subgroups) n_warps: u32) {
    let val = data[gid.x];
    let warp_sum = subgroupAdd(val);
    if lane == 0u {
        let warp_idx = gid.x / warp_size;
        partial[warp_idx] = f64(warp_sum);
    }
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        fp64_software: true,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "f64 subgroup reduction with SubgroupSize should compile: {result:?}"
    );
}

// --- Integer subgroup scan/reduce tests (WHATS_NEXT #59) ---

#[test]
fn test_subgroup_add_u32_reduce_sm70() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> data: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = data[gid.x];
    let sum = subgroupAdd(val);
    data[gid.x] = sum;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "u32 subgroupAdd should compile for SM70: {result:?}"
    );
}

#[test]
fn test_subgroup_add_i32_reduce_sm70() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> data: array<i32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = data[gid.x];
    let sum = subgroupAdd(val);
    data[gid.x] = sum;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "i32 subgroupAdd should compile for SM70: {result:?}"
    );
}

#[test]
fn test_subgroup_add_u32_reduce_sm86() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> data: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = data[gid.x];
    let sum = subgroupAdd(val);
    data[gid.x] = sum;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm86),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "u32 subgroupAdd should compile for SM86 (redux path → SASS): {result:?}"
    );
}

#[test]
fn test_subgroup_add_i32_reduce_sm86_sass() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> data: array<i32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = data[gid.x];
    let sum = subgroupAdd(val);
    data[gid.x] = sum;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm86),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "i32 subgroupAdd should compile for SM86 (redux path → SASS): {result:?}"
    );
}

#[test]
fn test_subgroup_min_max_u32_reduce_sm80_sass() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> data: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = data[gid.x];
    let mn = subgroupMin(val);
    let mx = subgroupMax(val);
    data[gid.x] = mn + mx;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm80),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "u32 subgroupMin/Max should compile for SM80 (redux → SASS): {result:?}"
    );
}

#[test]
fn test_subgroup_add_f32_reduce_sm80_sass() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = data[gid.x];
    let sum = subgroupAdd(val);
    data[gid.x] = sum;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm80),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "f32 subgroupAdd should compile for SM80 (redux → SASS): {result:?}"
    );
}

#[test]
fn test_subgroup_inclusive_scan_u32_sm70() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> data: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = data[gid.x];
    let prefix = subgroupInclusiveAdd(val);
    data[gid.x] = prefix;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "u32 subgroupInclusiveAdd should compile for SM70: {result:?}"
    );
}

#[test]
fn test_subgroup_exclusive_scan_u32_sm70() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> data: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = data[gid.x];
    let prefix = subgroupExclusiveAdd(val);
    data[gid.x] = prefix;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "u32 subgroupExclusiveAdd should compile for SM70: {result:?}"
    );
}

#[test]
fn test_subgroup_min_max_u32_sm70() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> mins: array<u32>;
@group(0) @binding(1) var<storage, read_write> maxs: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = mins[gid.x];
    mins[gid.x] = subgroupMin(val);
    maxs[gid.x] = subgroupMax(val);
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "u32 subgroupMin/Max should compile for SM70 (shfl path): {result:?}"
    );
}

#[test]
fn test_subgroup_and_or_xor_u32_sm70() {
    let wgsl = r"
enable subgroups;

@group(0) @binding(0) var<storage, read_write> data: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let val = data[gid.x];
    let a = subgroupAnd(val);
    let o = subgroupOr(val);
    let x = subgroupXor(val);
    data[gid.x] = a ^ o ^ x;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "u32 subgroupAnd/Or/Xor should compile for SM70 (shfl path): {result:?}"
    );
}
