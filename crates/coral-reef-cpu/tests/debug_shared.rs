// SPDX-License-Identifier: AGPL-3.0-only
//! Temporary diagnostic: dump CoralIR for shared memory reduction shader.

#[test]
fn dump_sum_reduce_ir() {
    use coral_reef::CompileOptions;
    use coral_reef::gpu_arch::{GpuTarget, NvArch};

    let wgsl = r"
var<workgroup> smem: array<f32, 16>;

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(16)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
) {
    smem[lid.x] = input[gid.x];
    workgroupBarrier();

    for (var stride = 8u; stride > 0u; stride = stride >> 1u) {
        if lid.x < stride {
            smem[lid.x] = smem[lid.x] + smem[lid.x + stride];
        }
        workgroupBarrier();
    }

    if lid.x == 0u {
        output[0u] = smem[0u];
    }
}
";

    let options = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm86),
        ..Default::default()
    };
    let sm = coral_reef::shader_model_for(options.target).unwrap();
    let shader = coral_reef::compile_wgsl_to_ir(wgsl, &options, sm.as_ref()).unwrap();

    eprintln!(
        "=== shared_mem_bytes: {} ===",
        shader.info.shared_mem_bytes()
    );
    for (fi, func) in shader.functions.iter().enumerate() {
        eprintln!("--- function {fi} ---");
        for (bi, bb) in func.blocks.iter().enumerate() {
            eprintln!("  block {bi} (label: {}):", bb.label);
            for (ii, instr) in bb.instrs.iter().enumerate() {
                eprintln!("    [{ii}] {instr}");
            }
        }
    }
}
