// SPDX-License-Identifier: AGPL-3.0-only
//! Layer 3 — AMD compute dispatch: compile a trivial shader + dispatch + sync.
//!
//! Run: `cargo test --test hw_amd_dispatch -- --ignored`

use coral_driver::amd::AmdDevice;
use coral_driver::{ComputeDevice, DispatchDims, ShaderInfo};
use coral_reef::gpu_arch::{AmdArch, GpuTarget};
use coral_reef::CompileOptions;

const TRIVIAL_SHADER: &str = r#"
@compute @workgroup_size(1)
fn main() {}
"#;

fn open_amd() -> AmdDevice {
    AmdDevice::open().expect("AmdDevice::open() failed — is amdgpu loaded?")
}

fn compile_for_rdna2(wgsl: &str) -> coral_reef::backend::CompiledBinary {
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        fma_policy: coral_reef::FmaPolicy::AllowFusion,
    };
    coral_reef::compile_wgsl_full(wgsl, &opts).expect("compile_wgsl_full")
}

#[test]
#[ignore = "requires amdgpu hardware"]
fn dispatch_trivial_shader() {
    let mut dev = open_amd();
    let compiled = compile_for_rdna2(TRIVIAL_SHADER);

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
    };

    dev.dispatch(&compiled.binary, &[], DispatchDims::linear(1), &info)
        .expect("dispatch");
    dev.sync().expect("sync");
}

#[test]
#[ignore = "requires amdgpu hardware"]
fn dispatch_multiple_workgroups() {
    let mut dev = open_amd();
    let compiled = compile_for_rdna2(TRIVIAL_SHADER);

    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
    };

    dev.dispatch(&compiled.binary, &[], DispatchDims::linear(64), &info)
        .expect("dispatch 64 workgroups");
    dev.sync().expect("sync");
}
