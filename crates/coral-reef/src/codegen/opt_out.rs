// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

#![allow(clippy::wildcard_imports)]

use super::{
    api::{DEBUG, GetDebugFlags},
    ir::*,
};

fn try_combine_outs(emit: &mut Instr, cut: &Instr) -> bool {
    let Op::Out(emit) = &mut emit.op else {
        return false;
    };

    let Op::Out(cut) = &cut.op else {
        return false;
    };

    if emit.out_type != OutType::Emit || cut.out_type != OutType::Cut {
        return false;
    }

    let Some(handle) = emit.dst.as_ssa() else {
        return false;
    };

    if cut.handle.as_ssa() != Some(handle) {
        return false;
    }

    if emit.stream != cut.stream {
        return false;
    }

    emit.dst = cut.dst.clone();
    emit.out_type = OutType::EmitThenCut;

    true
}

impl Shader<'_> {
    pub fn opt_out(&mut self) {
        if !matches!(self.info.stage, ShaderStageInfo::Geometry(_)) {
            return;
        }

        for f in &mut self.functions {
            for b in &mut f.blocks {
                let mut instrs = Vec::new();
                for instr in b.instrs.drain(..) {
                    if let Some(prev) = instrs.last_mut() {
                        if try_combine_outs(prev, &instr) {
                            if DEBUG.annotate() {
                                instrs.push(Instr::new(OpAnnotate {
                                    annotation: "combined by opt_out".into(),
                                }));
                            }
                            continue;
                        }
                    }
                    instrs.push(instr);
                }
                b.instrs = instrs;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        BasicBlock, ComputeShaderInfo, Dst, Function, GeometryShaderInfo, Instr, LabelAllocator,
        OpExit, OpOut, PhiAllocator, RegFile, SSAValueAllocator, Shader, ShaderInfo, ShaderIoInfo,
        ShaderModelInfo, ShaderStageInfo, Src,
    };
    use crate::codegen::nv::shader_header::OutputTopology;
    use coral_reef_stubs::cfg::CFGBuilder;

    fn make_geometry_shader_with_emit_cut(
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
                stage: ShaderStageInfo::Geometry(GeometryShaderInfo {
                    passthrough_enable: false,
                    stream_out_mask: 0,
                    threads_per_input_primitive: 1,
                    output_topology: OutputTopology::TriangleStrip,
                    max_output_vertex_count: 256,
                }),
                io: ShaderIoInfo::None,
            },
            functions: vec![function],
        }
    }

    #[test]
    fn test_opt_out_combines_emit_and_cut() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let handle = ssa_alloc.alloc(RegFile::GPR);
        let stream = Src::ZERO;

        let mut shader = make_geometry_shader_with_emit_cut(
            vec![
                Instr::new(OpOut {
                    dst: handle.into(),
                    handle: handle.into(),
                    stream: stream.clone(),
                    out_type: OutType::Emit,
                }),
                Instr::new(OpOut {
                    dst: Dst::None,
                    handle: handle.into(),
                    stream,
                    out_type: OutType::Cut,
                }),
                Instr::new(OpExit {}),
            ],
            ssa_alloc,
        );

        assert_eq!(shader.functions[0].blocks[0].instrs.len(), 3);

        shader.opt_out();

        let block = &shader.functions[0].blocks[0];
        assert_eq!(
            block.instrs.len(),
            2,
            "emit+cut should be combined into one"
        );
        let Op::Out(op) = &block.instrs[0].op else {
            panic!("expected Out");
        };
        assert_eq!(op.out_type, OutType::EmitThenCut);
    }

    #[test]
    fn test_opt_out_noop_for_compute() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let mut shader = make_geometry_shader_with_emit_cut(vec![Instr::new(OpExit {})], ssa_alloc);
        shader.info.stage = ShaderStageInfo::Compute(ComputeShaderInfo {
            local_size: [1, 1, 1],
            shared_mem_size: 0,
        });
        let instr_count_before = shader.functions[0].blocks[0].instrs.len();
        shader.opt_out();
        assert_eq!(
            shader.functions[0].blocks[0].instrs.len(),
            instr_count_before,
            "opt_out should be noop for compute"
        );
    }
}
