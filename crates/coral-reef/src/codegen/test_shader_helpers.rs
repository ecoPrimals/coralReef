// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared test helpers for codegen pass tests.

use super::ir::{
    BasicBlock, ComputeShaderInfo, Function, Instr, LabelAllocator, PhiAllocator,
    SSAValueAllocator, Shader, ShaderInfo, ShaderIoInfo, ShaderModelInfo, ShaderStageInfo,
};
use coral_reef_stubs::cfg::CFGBuilder;

/// Build a minimal single-block shader for testing codegen passes.
///
/// Leaks the `ShaderModelInfo` to produce a `'static` lifetime — acceptable
/// in test code where the process exits after the test.
pub fn make_shader_with_function(
    instrs: Vec<Instr>,
    ssa_alloc: SSAValueAllocator,
) -> Shader<'static> {
    let sm = Box::leak(Box::new(ShaderModelInfo::new(70, 64)));
    let mut label_alloc = LabelAllocator::new();
    let mut cfg_builder = CFGBuilder::new();
    let block = BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs,
    };
    cfg_builder.add_block(block);
    let function = Function {
        ssa_alloc,
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    };
    Shader {
        sm,
        info: ShaderInfo {
            max_warps_per_sm: 0,
            gpr_count: 0,
            control_barrier_count: 0,
            instr_count: 0,
            static_cycle_count: 0,
            spills_to_mem: 0,
            fills_from_mem: 0,
            spills_to_reg: 0,
            fills_from_reg: 0,
            shared_local_mem_size: 0,
            max_crs_depth: 0,
            uses_global_mem: false,
            writes_global_mem: false,
            uses_fp64: false,
            stage: ShaderStageInfo::Compute(ComputeShaderInfo {
                local_size: [1, 1, 1],
                shared_mem_size: 0,
            }),
            io: ShaderIoInfo::None,
        },
        functions: vec![function],
        fma_policy: crate::FmaPolicy::default(),
    }
}
