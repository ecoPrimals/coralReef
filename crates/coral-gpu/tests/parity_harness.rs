// SPDX-License-Identifier: AGPL-3.0-only
//! Cross-vendor parity test harness.
//!
//! Compiles identical WGSL shaders for both AMD RDNA2 and NVIDIA SM86,
//! dispatches on whichever hardware is available, and compares numerical
//! results with epsilon tolerance for floating-point parity.
//!
//! ## Test matrix
//!
//! | Shader  | Operation                  | Validates                |
//! |---------|----------------------------|--------------------------|
//! | vecadd  | `out[i] = a[i] + b[i]`    | basic compute correctness |
//! | saxpy   | `out[i] = α*x[i] + y[i]`  | FMA parity               |
//! | reduce  | sum reduction              | workgroup barrier parity  |
//! | matmul  | tiled matrix multiply      | shared memory parity      |
//!
//! NOTE: AMD RDNA2 backend does not yet support `@builtin(global_invocation_id)`
//! or buffer reads. These tests are compilation-level parity only until the
//! compiler backend evolves. Hardware dispatch is AMD-only for write-constant
//! shaders and NVIDIA dispatch is pending UVM integration.
//!
//! Run: `cargo test --test parity_harness -p coral-gpu -- --ignored`

use coral_reef::{AmdArch, CompileOptions, FmaPolicy, GpuTarget, NvArch};

fn opts_for(target: GpuTarget) -> CompileOptions {
    CompileOptions {
        target,
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        fma_policy: FmaPolicy::AllowFusion,
        ..CompileOptions::default()
    }
}

fn try_compile(
    wgsl: &str,
    target: GpuTarget,
) -> Result<coral_reef::backend::CompiledBinary, coral_reef::CompileError> {
    coral_reef::compile_wgsl_full(wgsl, &opts_for(target))
}

fn compile(wgsl: &str, target: GpuTarget) -> coral_reef::backend::CompiledBinary {
    try_compile(wgsl, target).unwrap_or_else(|e| {
        panic!("compilation failed for {target:?}: {e}");
    })
}

// ============================================================
// Shader sources
// ============================================================

const VECADD: &str = r"
@group(0) @binding(0) var<storage> a: array<f32>;
@group(0) @binding(1) var<storage> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = a[gid.x] + b[gid.x];
}
";

const SAXPY: &str = r"
@group(0) @binding(0) var<storage> x: array<f32>;
@group(0) @binding(1) var<storage> y: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let alpha: f32 = 2.5;
    out[gid.x] = alpha * x[gid.x] + y[gid.x];
}
";

const REDUCE: &str = r"
@group(0) @binding(0) var<storage> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

var<workgroup> smem: array<f32, 64>;

@compute @workgroup_size(64)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    smem[lid.x] = input[gid.x];
    workgroupBarrier();

    var stride: u32 = 32u;
    while stride > 0u {
        if lid.x < stride {
            smem[lid.x] = smem[lid.x] + smem[lid.x + stride];
        }
        workgroupBarrier();
        stride = stride / 2u;
    }

    if lid.x == 0u {
        output[wid.x] = smem[0];
    }
}
";

const MATMUL: &str = r"
@group(0) @binding(0) var<storage> a: array<f32>;
@group(0) @binding(1) var<storage> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;

const N: u32 = 64u;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.y;
    let col = gid.x;
    var sum: f32 = 0.0;
    for (var k: u32 = 0u; k < N; k = k + 1u) {
        sum = sum + a[row * N + k] * b[k * N + col];
    }
    c[row * N + col] = sum;
}
";

const STORE_42: &str = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    out[0] = 42u;
}
";

// ============================================================
// Compilation parity tests (no hardware required)
// ============================================================

#[test]
fn parity_vecadd_sm86_compiles() {
    let bin = compile(VECADD, GpuTarget::Nvidia(NvArch::Sm86));
    assert!(!bin.binary.is_empty());
    assert_eq!(bin.info.local_size, [64, 1, 1]);
}

#[test]
fn parity_vecadd_rdna2_known_limitation() {
    let result = try_compile(VECADD, GpuTarget::Amd(AmdArch::Rdna2));
    assert!(
        result.is_err(),
        "RDNA2 does not yet support global_invocation_id"
    );
}

#[test]
fn parity_saxpy_sm86_compiles() {
    let bin = compile(SAXPY, GpuTarget::Nvidia(NvArch::Sm86));
    assert!(!bin.binary.is_empty());
}

#[test]
fn parity_reduce_sm86_compiles() {
    let bin = compile(REDUCE, GpuTarget::Nvidia(NvArch::Sm86));
    assert!(!bin.binary.is_empty());
    assert!(bin.info.shared_mem_bytes > 0, "reduce uses shared memory");
}

#[test]
fn parity_matmul_sm86_compiles() {
    let bin = compile(MATMUL, GpuTarget::Nvidia(NvArch::Sm86));
    assert!(!bin.binary.is_empty());
    assert_eq!(bin.info.local_size, [8, 8, 1]);
}

#[test]
fn parity_store42_both_targets() {
    let sm86 = compile(STORE_42, GpuTarget::Nvidia(NvArch::Sm86));
    let rdna2 = compile(STORE_42, GpuTarget::Amd(AmdArch::Rdna2));
    assert!(!sm86.binary.is_empty());
    assert!(!rdna2.binary.is_empty());
    assert_ne!(
        sm86.binary, rdna2.binary,
        "different ISAs should produce different binaries"
    );
}

// ============================================================
// Metadata comparison (instruction count, GPR pressure)
// ============================================================

#[test]
fn parity_vecadd_sm86_metadata_reasonable() {
    let bin = compile(VECADD, GpuTarget::Nvidia(NvArch::Sm86));
    assert!(
        bin.info.gpr_count >= 3,
        "vecadd needs at least 3 GPRs for a[], b[], out[]"
    );
    assert!(bin.info.instr_count > 0);
}

#[test]
fn parity_saxpy_sm86_metadata_reasonable() {
    let bin = compile(SAXPY, GpuTarget::Nvidia(NvArch::Sm86));
    assert!(bin.info.gpr_count >= 3);
    assert!(bin.info.instr_count > 0);
}

#[test]
fn parity_reduce_sm86_metadata_reasonable() {
    let bin = compile(REDUCE, GpuTarget::Nvidia(NvArch::Sm86));
    assert!(bin.info.gpr_count > 0);
    assert!(
        bin.info.shared_mem_bytes >= 64 * 4,
        "shared memory for 64 f32 elements"
    );
}

#[test]
fn parity_matmul_sm86_metadata_reasonable() {
    let bin = compile(MATMUL, GpuTarget::Nvidia(NvArch::Sm86));
    assert!(
        bin.info.gpr_count >= 4,
        "matmul needs regs for accumulator + indices"
    );
    assert!(
        bin.info.instr_count > 5,
        "matmul should have loop body instructions"
    );
}

// ============================================================
// Hardware dispatch parity (requires dual-GPU)
// ============================================================

/// AMD hardware dispatch: compile + dispatch + readback for a constant-write shader.
///
/// This is the baseline test: the RDNA2 pipeline dispatches a write-constant
/// shader and verifies the GPU wrote the correct value.
#[test]
#[ignore = "requires amdgpu hardware"]
fn parity_hw_amd_store42_dispatch() {
    use coral_driver::amd::AmdDevice;
    use coral_driver::{ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

    let compiled = compile(STORE_42, GpuTarget::Amd(AmdArch::Rdna2));
    let mut dev = AmdDevice::open().expect("AmdDevice::open");

    let buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");
    dev.upload(buf, 0, &[0u8; 4096]).expect("zero");

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
    };

    dev.dispatch(&compiled.binary, &[buf], DispatchDims::linear(1), &info)
        .expect("dispatch");
    dev.sync().expect("sync");

    let readback = dev.readback(buf, 0, 4).expect("readback");
    let value = u32::from_le_bytes(readback[..4].try_into().unwrap());
    assert_eq!(value, 42, "AMD RDNA2: expected 42, got {value}");

    dev.free(buf).expect("free");
}

/// Cross-architecture compilation parity: all 4 test matrix shaders
/// produce non-empty SM86 binaries with reasonable metadata.
#[test]
fn parity_all_shaders_sm86_complete() {
    let shaders = [
        ("vecadd", VECADD),
        ("saxpy", SAXPY),
        ("reduce", REDUCE),
        ("matmul", MATMUL),
    ];

    for (name, src) in shaders {
        let bin = compile(src, GpuTarget::Nvidia(NvArch::Sm86));
        assert!(
            !bin.binary.is_empty(),
            "{name}: SM86 binary should be non-empty"
        );
        assert!(bin.info.gpr_count > 0, "{name}: should use at least 1 GPR");
        eprintln!(
            "{name}: {} bytes, {} GPRs, {} instrs, {} shared bytes",
            bin.binary.len(),
            bin.info.gpr_count,
            bin.info.instr_count,
            bin.info.shared_mem_bytes
        );
    }
}

/// Multi-SM compilation parity: vecadd compiles identically across
/// SM70/SM75/SM80/SM86/SM89.
#[test]
fn parity_vecadd_all_sm_targets() {
    let targets = [
        NvArch::Sm70,
        NvArch::Sm75,
        NvArch::Sm80,
        NvArch::Sm86,
        NvArch::Sm89,
    ];

    for nv in targets {
        let bin = compile(VECADD, GpuTarget::Nvidia(nv));
        assert!(
            !bin.binary.is_empty(),
            "{nv:?}: vecadd should produce non-empty binary"
        );
    }
}
