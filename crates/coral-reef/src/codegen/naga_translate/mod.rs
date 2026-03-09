// SPDX-License-Identifier: AGPL-3.0-only
//! Naga IR → codegen IR translator.
//!
//! Translates a `naga::Module` (parsed from SPIR-V or WGSL) into the
//! internal SSA-based IR (`Shader`), which then flows through the
//! optimization / legalization / RA / encoding pipeline.

#![allow(clippy::wildcard_imports)]
use super::ir::*;
use crate::error::CompileError;

pub(super) mod expr;
mod expr_binary;
pub(super) mod func;
mod func_builtins;
mod func_control;
mod func_math;
mod func_mem;
mod func_ops;

fn mem_access_global_b32() -> MemAccess {
    MemAccess {
        mem_type: MemType::B32,
        space: MemSpace::Global(MemAddrType::A64),
        order: MemOrder::Weak,
        eviction_priority: MemEvictionPriority::Normal,
    }
}

fn lit_scalar(lit: &naga::Literal) -> naga::Scalar {
    match lit {
        naga::Literal::F32(_) => naga::Scalar {
            kind: naga::ScalarKind::Float,
            width: 4,
        },
        naga::Literal::F64(_) => naga::Scalar {
            kind: naga::ScalarKind::Float,
            width: 8,
        },
        naga::Literal::U32(_) => naga::Scalar {
            kind: naga::ScalarKind::Uint,
            width: 4,
        },
        naga::Literal::I32(_) => naga::Scalar {
            kind: naga::ScalarKind::Sint,
            width: 4,
        },
        naga::Literal::U64(_) => naga::Scalar {
            kind: naga::ScalarKind::Uint,
            width: 8,
        },
        naga::Literal::I64(_) => naga::Scalar {
            kind: naga::ScalarKind::Sint,
            width: 8,
        },
        naga::Literal::Bool(_) => naga::Scalar {
            kind: naga::ScalarKind::Bool,
            width: 1,
        },
        _ => naga::Scalar {
            kind: naga::ScalarKind::Uint,
            width: 4,
        },
    }
}

mod sys_regs {
    pub const SR_TID_X: u8 = 0x21;
    pub const SR_TID_Y: u8 = 0x22;
    pub const SR_TID_Z: u8 = 0x23;
    pub const SR_CTAID_X: u8 = 0x25;
    pub const SR_CTAID_Y: u8 = 0x26;
    pub const SR_CTAID_Z: u8 = 0x27;
    pub const SR_NTID_X: u8 = 0x29;
    pub const SR_NTID_Y: u8 = 0x2a;
    pub const SR_NTID_Z: u8 = 0x2b;
    pub const SR_NCTAID_X: u8 = 0x2d;
    pub const SR_NCTAID_Y: u8 = 0x2e;
    pub const SR_NCTAID_Z: u8 = 0x2f;
    pub const SR_LANEID: u8 = 0x00;
}

/// Top-level translator state.
pub struct NagaTranslator<'sm, 'mod_lt> {
    sm: &'sm dyn ShaderModel,
    module: &'mod_lt naga::Module,
}

impl<'sm, 'mod_lt> NagaTranslator<'sm, 'mod_lt> {
    pub fn new(sm: &'sm dyn ShaderModel, module: &'mod_lt naga::Module) -> Self {
        Self { sm, module }
    }

    /// Translate a compute shader entry point into a `Shader`.
    pub fn translate_compute(
        &self,
        entry_point: &naga::EntryPoint,
    ) -> Result<Shader<'sm>, CompileError> {
        if entry_point.stage != naga::ShaderStage::Compute {
            return Err(CompileError::InvalidInput(
                format!("expected compute stage, got {:?}", entry_point.stage,).into(),
            ));
        }

        let function = self.translate_function(&entry_point.function, Some(entry_point))?;

        let local_size = [
            entry_point.workgroup_size[0] as u16,
            entry_point.workgroup_size[1] as u16,
            entry_point.workgroup_size[2] as u16,
        ];
        let shared_mem_size = self.compute_shared_mem_size();

        let info = ShaderInfo {
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
            uses_global_mem: true,
            writes_global_mem: true,
            uses_fp64: false,
            stage: ShaderStageInfo::Compute(ComputeShaderInfo {
                local_size,
                shared_mem_size,
            }),
            io: ShaderIoInfo::None,
        };

        Ok(Shader {
            sm: self.sm,
            info,
            functions: vec![function],
        })
    }

    fn compute_shared_mem_size(&self) -> u16 {
        let mut total = 0u32;
        for (_, gv) in self.module.global_variables.iter() {
            if gv.space == naga::AddressSpace::WorkGroup {
                let ty = &self.module.types[gv.ty];
                total += ty.inner.size(self.module.to_ctx());
            }
        }
        total.min(u32::from(u16::MAX)) as u16
    }

    fn translate_function(
        &self,
        func: &naga::Function,
        entry_point: Option<&naga::EntryPoint>,
    ) -> Result<Function, CompileError> {
        let mut ft = func::FuncTranslator::new(self.sm, self.module, func);

        ft.start_block();
        ft.pre_allocate_local_vars();

        if let Some(ep) = entry_point {
            if ep.stage == naga::ShaderStage::Compute {
                ft.emit_compute_prologue(ep)?;
            }
        }

        ft.translate_block(&func.body)?;

        ft.push_instr(Instr::new(OpExit {}));
        ft.finish_block()?;

        Ok(ft.build_function())
    }
}

/// Parse SPIR-V bytes into a naga Module.
pub fn parse_spirv(data: &[u32]) -> Result<naga::Module, CompileError> {
    let bytes: Vec<u8> = data.iter().flat_map(|w| w.to_le_bytes()).collect();
    let opts = naga::front::spv::Options::default();
    naga::front::spv::parse_u8_slice(&bytes, &opts)
        .map_err(|e| CompileError::InvalidInput(format!("SPIR-V parse error: {e}").into()))
}

/// Parse WGSL source into a naga Module.
pub fn parse_wgsl(source: &str) -> Result<naga::Module, CompileError> {
    naga::front::wgsl::parse_str(source)
        .map_err(|e| CompileError::InvalidInput(format!("WGSL parse error: {e}").into()))
}

/// Parse GLSL compute shader source into a naga Module.
pub fn parse_glsl(source: &str) -> Result<naga::Module, CompileError> {
    let opts = naga::front::glsl::Options::from(naga::ShaderStage::Compute);
    let mut frontend = naga::front::glsl::Frontend::default();
    frontend
        .parse(&opts, source)
        .map_err(|e| CompileError::InvalidInput(format!("GLSL parse error: {e:?}").into()))
}

/// Translate a naga Module into a Shader for a compute entry point.
pub fn translate<'sm>(
    module: &naga::Module,
    sm: &'sm dyn ShaderModel,
    entry_point_name: &str,
) -> Result<Shader<'sm>, CompileError> {
    let ep = module
        .entry_points
        .iter()
        .find(|ep| ep.name == entry_point_name)
        .ok_or_else(|| {
            CompileError::InvalidInput(
                format!("entry point '{}' not found", entry_point_name,).into(),
            )
        })?;

    let translator = NagaTranslator::new(sm, module);
    translator.translate_compute(ep)
}

#[cfg(test)]
mod tests {
    use super::super::ir::{ComputeShaderInfo, Op, ShaderModelInfo, ShaderStageInfo};
    use super::{parse_glsl, parse_spirv, parse_wgsl, translate};
    use crate::error::CompileError;

    fn sm70() -> ShaderModelInfo {
        ShaderModelInfo::new(70, 64)
    }

    // ---------------------------------------------------------------------------
    // Parsing tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_parse_wgsl_valid_minimal_compute() {
        let wgsl = r"
            @compute @workgroup_size(64)
            fn main() {}
        ";
        let result = parse_wgsl(wgsl);
        assert!(
            result.is_ok(),
            "valid minimal compute shader should parse: {result:?}"
        );
        let module = result.unwrap();
        assert_eq!(module.entry_points.len(), 1);
        assert_eq!(module.entry_points[0].name, "main");
        assert_eq!(module.entry_points[0].workgroup_size, [64, 1, 1]);
    }

    #[test]
    fn test_parse_wgsl_invalid_returns_error() {
        let invalid_wgsl = "fn main() { let x = ; }";
        let result = parse_wgsl(invalid_wgsl);
        let err = result.expect_err("invalid WGSL should return error");
        assert!(matches!(err, CompileError::InvalidInput(_)));
    }

    #[test]
    fn test_parse_spirv_empty_returns_error() {
        let empty: &[u32] = &[];
        let result = parse_spirv(empty);
        let err = result.expect_err("empty SPIR-V should return error");
        assert!(matches!(err, CompileError::InvalidInput(_)));
    }

    #[test]
    fn test_parse_spirv_invalid_magic_returns_error() {
        let wrong_magic = [0xDEAD_BEEFu32, 0x0001_0000, 0, 0, 0];
        let result = parse_spirv(&wrong_magic);
        let err = result.expect_err("invalid SPIR-V magic should return error");
        assert!(matches!(err, CompileError::InvalidInput(_)));
    }

    #[test]
    fn test_parse_glsl_valid_minimal_compute() {
        let glsl = r"#version 450
            layout(local_size_x = 64) in;
            void main() {}
        ";
        let result = parse_glsl(glsl);
        assert!(
            result.is_ok(),
            "valid GLSL compute shader should parse: {result:?}"
        );
        let module = result.unwrap();
        assert_eq!(module.entry_points.len(), 1);
        assert_eq!(module.entry_points[0].name, "main");
        assert_eq!(module.entry_points[0].workgroup_size, [64, 1, 1]);
    }

    #[test]
    fn test_parse_glsl_invalid_returns_error() {
        let invalid_glsl = "#version 450\nvoid main() { int x = ; }";
        let result = parse_glsl(invalid_glsl);
        let err = result.expect_err("invalid GLSL should return error");
        assert!(matches!(err, CompileError::InvalidInput(_)));
    }

    #[test]
    fn test_translate_glsl_compute_with_buffer() {
        let glsl = r"#version 450
            layout(local_size_x = 64) in;
            layout(std430, binding = 0) buffer Data { float data[]; };
            void main() {
                uint gid = gl_GlobalInvocationID.x;
                data[gid] = data[gid] + 1.0;
            }
        ";
        let module = parse_glsl(glsl).unwrap();
        let sm = sm70();
        let result = translate(&module, &sm, "main");
        assert!(
            result.is_ok(),
            "GLSL compute with buffer should translate: {:?}",
            result.err()
        );
        let shader = result.unwrap();
        if let ShaderStageInfo::Compute(ComputeShaderInfo { local_size, .. }) = shader.info.stage {
            assert_eq!(local_size, [64, 1, 1]);
        } else {
            panic!("expected Compute stage info");
        }
    }

    // ---------------------------------------------------------------------------
    // Translation tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_translate_valid_compute_produces_shader_with_workgroup_size() {
        let wgsl = r"
            @compute @workgroup_size(8, 4, 2)
            fn main() {}
        ";
        let module = parse_wgsl(wgsl).unwrap();
        let sm = sm70();
        let result = translate(&module, &sm, "main");
        assert!(result.is_ok(), "valid compute module should translate");
        let shader = result.unwrap();
        if let ShaderStageInfo::Compute(ComputeShaderInfo { local_size, .. }) = shader.info.stage {
            assert_eq!(local_size, [8, 4, 2]);
        } else {
            panic!("expected Compute stage info");
        }
    }

    #[test]
    fn test_translate_nonexistent_entry_point_returns_error() {
        let wgsl = r"
            @compute @workgroup_size(1)
            fn main() {}
        ";
        let module = parse_wgsl(wgsl).unwrap();
        let sm = sm70();
        let result = translate(&module, &sm, "nonexistent");
        match result {
            Ok(_) => panic!("expected error for nonexistent entry point"),
            Err(e) => assert!(matches!(e, CompileError::InvalidInput(_))),
        }
    }

    #[test]
    fn test_translate_non_compute_entry_point_returns_error() {
        let wgsl = r"
            @vertex
            fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4<f32> {
                return vec4<f32>(0.0, 0.0, 0.0, 1.0);
            }
        ";
        let module = parse_wgsl(wgsl).unwrap();
        let sm = sm70();
        let result = translate(&module, &sm, "vs_main");
        match result {
            Ok(_) => panic!("vertex entry point should fail"),
            Err(e) => assert!(matches!(e, CompileError::InvalidInput(_))),
        }
    }

    // ---------------------------------------------------------------------------
    // End-to-end WGSL → IR tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_e2e_compute_with_barrier_emits_op_bar() {
        let wgsl = r"
            @compute @workgroup_size(64)
            fn main() {
                workgroupBarrier();
            }
        ";
        let module = parse_wgsl(wgsl).unwrap();
        let sm = sm70();
        let shader = translate(&module, &sm, "main").unwrap();
        let mut has_bar = false;
        shader.for_each_instr(&mut |instr| {
            if matches!(instr.op, Op::Bar(_)) {
                has_bar = true;
            }
        });
        assert!(has_bar, "shader with workgroupBarrier should emit OpBar");
    }

    #[test]
    fn test_e2e_compute_with_global_invocation_id_emits_s2r() {
        let wgsl = r"
            @group(0) @binding(0) var<storage, read_write> data: array<f32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                data[gid.x] = data[gid.x] + 1.0;
            }
        ";
        let module = parse_wgsl(wgsl).unwrap();
        let sm = sm70();
        let shader = translate(&module, &sm, "main").unwrap();
        let mut has_s2r = false;
        shader.for_each_instr(&mut |instr| {
            if matches!(instr.op, Op::S2R(_)) {
                has_s2r = true;
            }
        });
        assert!(
            has_s2r,
            "shader with global_invocation_id should emit S2R (system register reads)"
        );
    }

    #[test]
    fn test_e2e_compute_with_binary_arithmetic_emits_ops() {
        let wgsl = r"
            @group(0) @binding(0) var<storage, read_write> data: array<f32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                data[gid.x] = data[gid.x] + 1.0;
            }
        ";
        let module = parse_wgsl(wgsl).unwrap();
        let sm = sm70();
        let shader = translate(&module, &sm, "main").unwrap();
        let mut has_fadd = false;
        shader.for_each_instr(&mut |instr| {
            if matches!(instr.op, Op::FAdd(_)) {
                has_fadd = true;
            }
        });
        assert!(
            has_fadd,
            "shader with f32 + should emit OpFAdd or equivalent arithmetic"
        );
    }

    #[test]
    fn test_e2e_compute_with_if_else_has_multiple_blocks() {
        let wgsl = r"
            @group(0) @binding(0) var<storage, read_write> data: array<f32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                if gid.x < 32u {
                    data[gid.x] = 1.0;
                } else {
                    data[gid.x] = 2.0;
                }
            }
        ";
        let module = parse_wgsl(wgsl).unwrap();
        let sm = sm70();
        let shader = translate(&module, &sm, "main").unwrap();
        let mut total_blocks = 0;
        for func in &shader.functions {
            total_blocks += func.blocks.len();
        }
        assert!(
            total_blocks > 1,
            "shader with if/else should have multiple CFG blocks, got {total_blocks}"
        );
    }

    #[test]
    fn test_e2e_compute_with_loop_has_back_edge() {
        let wgsl = r"
            @group(0) @binding(0) var<storage, read_write> data: array<f32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                var i: u32 = 0u;
                loop {
                    if i >= 4u {
                        break;
                    }
                    data[gid.x] = data[gid.x] + 1.0;
                    i = i + 1u;
                }
            }
        ";
        let module = parse_wgsl(wgsl).unwrap();
        let sm = sm70();
        let shader = translate(&module, &sm, "main").unwrap();
        let mut has_back_edge = false;
        for func in &shader.functions {
            for b in 0..func.blocks.len() {
                for &succ in func.blocks.successors(b) {
                    if func.blocks.predecessors(succ).contains(&b) {
                        has_back_edge = true;
                        break;
                    }
                }
            }
        }
        assert!(has_back_edge, "shader with loop should have CFG back edge");
    }

    // ---------------------------------------------------------------------------
    // Expression translation coverage tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_translate_cast_operations() {
        let wgsl = r"
            @group(0) @binding(0) var<storage, read_write> data: array<f32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                let f = f32(gid.x);
                let i = i32(f);
                let u = u32(i);
                data[gid.x] = f32(u);
            }
        ";
        let module = parse_wgsl(wgsl).expect("valid WGSL");
        let sm = sm70();
        let result = translate(&module, &sm, "main");
        assert!(result.is_ok(), "cast operations should translate");
        let shader = result.unwrap();
        let mut has_f2i = false;
        let mut has_i2f = false;
        shader.for_each_instr(&mut |instr| {
            if matches!(instr.op, Op::F2I(_)) {
                has_f2i = true;
            }
            if matches!(instr.op, Op::I2F(_)) {
                has_i2f = true;
            }
        });
        assert!(has_f2i, "cast f32->i32 should emit F2I");
        assert!(has_i2f, "cast u32->f32 should emit I2F");
    }

    #[test]
    fn test_translate_select_operations() {
        let wgsl = r"
            @group(0) @binding(0) var<storage, read_write> data: array<f32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                let a = data[gid.x];
                let b = data[gid.x + 1u];
                data[gid.x] = select(a, b, a < b);
            }
        ";
        let module = parse_wgsl(wgsl).expect("valid WGSL");
        let sm = sm70();
        let result = translate(&module, &sm, "main");
        assert!(result.is_ok(), "select operations should translate");
        let shader = result.unwrap();
        let mut has_sel = false;
        let mut has_cmp = false;
        shader.for_each_instr(&mut |instr| {
            if matches!(instr.op, Op::Sel(_)) {
                has_sel = true;
            }
            if matches!(instr.op, Op::FSetP(_) | Op::ISetP(_)) {
                has_cmp = true;
            }
        });
        assert!(has_sel, "select() should emit OpSel");
        assert!(has_cmp, "a < b comparison should emit FSetP or ISetP");
    }

    #[test]
    fn test_translate_unary_operations() {
        let wgsl = r"
            @group(0) @binding(0) var<storage, read_write> data: array<f32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                let x = data[gid.x];
                data[gid.x] = abs(-x);
            }
        ";
        let module = parse_wgsl(wgsl).expect("valid WGSL");
        let sm = sm70();
        let result = translate(&module, &sm, "main");
        assert!(result.is_ok(), "unary ops should translate");
        let shader = result.unwrap();
        let mut has_fadd = false;
        shader.for_each_instr(&mut |instr| {
            if matches!(instr.op, Op::FAdd(_)) {
                has_fadd = true;
            }
        });
        assert!(has_fadd, "abs(-x) uses FAdd with fneg/fabs modifiers");
    }

    #[test]
    fn test_translate_vector_component_access() {
        let wgsl = r"
            @group(0) @binding(0) var<storage, read_write> data: array<f32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                let sum = gid.x + gid.y + gid.z;
                data[sum] = 1.0;
            }
        ";
        let module = parse_wgsl(wgsl).expect("valid WGSL");
        let sm = sm70();
        let result = translate(&module, &sm, "main");
        assert!(result.is_ok(), "vector component access should translate");
        let shader = result.unwrap();
        let mut s2r_count = 0u32;
        shader.for_each_instr(&mut |instr| {
            if matches!(instr.op, Op::S2R(_)) {
                s2r_count += 1;
            }
        });
        assert!(
            s2r_count >= 3,
            "gid.x + gid.y + gid.z should emit multiple S2R, got {s2r_count}"
        );
    }

    #[test]
    fn test_translate_multiple_bindings() {
        let wgsl = r"
            @group(0) @binding(0) var<storage, read> input: array<f32>;
            @group(0) @binding(1) var<storage, read_write> output: array<f32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                output[gid.x] = input[gid.x] * 2.0;
            }
        ";
        let module = parse_wgsl(wgsl).expect("valid WGSL");
        let sm = sm70();
        let result = translate(&module, &sm, "main");
        assert!(result.is_ok(), "multiple bindings should translate");
        let shader = result.unwrap();
        let mut has_fmul = false;
        let mut has_ld = false;
        shader.for_each_instr(&mut |instr| {
            if matches!(instr.op, Op::FMul(_)) {
                has_fmul = true;
            }
            if matches!(instr.op, Op::Ld(_)) {
                has_ld = true;
            }
        });
        assert!(has_fmul, "input * 2.0 should emit FMul");
        assert!(has_ld, "array loads should emit Ld");
    }

    #[test]
    fn test_translate_nested_control_flow() {
        let wgsl = r"
            @group(0) @binding(0) var<storage, read_write> data: array<u32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                var sum = 0u;
                for (var i = 0u; i < 10u; i = i + 1u) {
                    if (i < 5u) {
                        sum = sum + data[i];
                    }
                }
                data[gid.x] = sum;
            }
        ";
        let module = parse_wgsl(wgsl).expect("valid WGSL");
        let sm = sm70();
        let result = translate(&module, &sm, "main");
        assert!(result.is_ok(), "nested control flow should translate");
        let shader = result.unwrap();
        let mut total_blocks = 0;
        for func in &shader.functions {
            total_blocks += func.blocks.len();
        }
        assert!(
            total_blocks > 1,
            "nested if+loop should have multiple CFG blocks, got {total_blocks}"
        );
    }

    #[test]
    fn test_translate_math_function_variety() {
        let wgsl = r"
            @group(0) @binding(0) var<storage, read_write> data: array<f32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                let x = data[gid.x];
                data[gid.x] = clamp(floor(x), 0.0, 1.0);
            }
        ";
        let module = parse_wgsl(wgsl).expect("valid WGSL");
        let sm = sm70();
        let result = translate(&module, &sm, "main");
        assert!(result.is_ok(), "math functions should translate");
        let shader = result.unwrap();
        let mut has_fmnmx = false;
        let mut has_frnd = false;
        shader.for_each_instr(&mut |instr| {
            if matches!(instr.op, Op::FMnMx(_)) {
                has_fmnmx = true;
            }
            if matches!(instr.op, Op::FRnd(_)) {
                has_frnd = true;
            }
        });
        assert!(has_fmnmx, "clamp should emit FMnMx");
        assert!(has_frnd, "floor should emit FRnd");
    }
}
