// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared test helpers for codegen pass tests.

use super::ir::{
    BasicBlock, Function, Instr, LabelAllocator, PhiAllocator, SSAValueAllocator, Shader,
    ShaderInfo, ShaderModelInfo,
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
        info: ShaderInfo::compute([1, 1, 1], 0),
        functions: vec![function],
        fma_policy: crate::FmaPolicy::default(),
    }
}
