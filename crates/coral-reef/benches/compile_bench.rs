// SPDX-License-Identifier: AGPL-3.0-only
#![allow(missing_docs)]
//! Criterion benchmarks for coral-reef compiler throughput.
//!
//! Measures WGSL→SM70, WGSL→RDNA2, and SPIR-V→SM70 compile times.

use coral_reef::{AmdArch, CompileOptions, GpuTarget, NvArch, compile, compile_wgsl};
use criterion::{Criterion, black_box, criterion_group, criterion_main};

const WGSL_COMPUTE: &str = r"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    data[idx] = data[idx] * 2.0 + 1.0;
}
";

fn wgsl_to_spirv(wgsl: &str) -> Vec<u32> {
    let module = naga::front::wgsl::parse_str(wgsl).expect("WGSL should parse");
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .expect("module should validate");
    naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
        .expect("SPIR-V emission should succeed")
}

fn compile_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile");
    group.sample_size(50);

    // WGSL → SM70
    group.bench_function("wgsl_to_sm70", |b| {
        let opts = CompileOptions {
            target: GpuTarget::Nvidia(NvArch::Sm70),
            ..Default::default()
        };
        b.iter(|| {
            let result = compile_wgsl(black_box(WGSL_COMPUTE), &opts);
            let _ = black_box(result);
        });
    });

    // WGSL → RDNA2/GFX1030
    group.bench_function("wgsl_to_rdna2", |b| {
        let opts = CompileOptions {
            target: GpuTarget::Amd(AmdArch::Rdna2),
            ..Default::default()
        };
        b.iter(|| {
            let result = compile_wgsl(black_box(WGSL_COMPUTE), &opts);
            let _ = black_box(result);
        });
    });

    // SPIR-V → SM70 (pre-convert WGSL once, benchmark compile only)
    let spirv = wgsl_to_spirv(WGSL_COMPUTE);
    group.bench_function("spirv_to_sm70", |b| {
        let opts = CompileOptions {
            target: GpuTarget::Nvidia(NvArch::Sm70),
            ..Default::default()
        };
        b.iter(|| {
            let result = compile(black_box(&spirv), &opts);
            let _ = black_box(result);
        });
    });

    group.finish();
}

criterion_group!(benches, compile_benchmarks);
criterion_main!(benches);
