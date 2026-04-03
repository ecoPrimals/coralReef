// SPDX-License-Identifier: AGPL-3.0-only
//! `CoralIR` `Op` → Cranelift CLIF translator.
//!
//! Walks an optimized `Shader` (from `compile_wgsl_to_ir`) and emits equivalent
//! Cranelift instructions for each `CoralIR` operation. Only the compute-shader
//! subset of ops is supported; texture, warp, and display ops return errors.

use std::collections::HashMap;

use coral_reef::codegen::ir::{
    self, Dst, FRndMode, LogicOp2, MemSpace, Op, Phi, Pred, PredRef, Src, SrcMod, SrcRef,
};
use cranelift_codegen::ir::{
    AbiParam, Block, ExtFuncData, ExternalName, InstBuilder, MemFlags, Value, types,
};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};

use crate::builtins::{self, SysRegMapping};
use crate::cmp_codes::{float_cmp_to_cc, int_cmp_to_cc};
use crate::error::JitError;

/// Type alias for the JIT'd kernel function pointer.
pub type KernelFn = unsafe extern "C" fn(
    *mut *mut u8, // bindings_ptr
    u32,
    u32,
    u32, // global_id x,y,z
    u32,
    u32,
    u32, // workgroup_id x,y,z
    u32,
    u32,
    u32, // local_id x,y,z
    u32,
    u32,
    u32, // num_workgroups x,y,z
    u32,
    u32,
    u32, // workgroup_size x,y,z
);

/// Compiled JIT module containing the kernel function pointer.
///
/// Owns either a `JITModule` (legacy cranelift-jit path) or a `JitMemory`
/// (sovereign rustix path) to keep the backing code alive.
pub struct CompiledKernel {
    _backing: CompiledBacking,
    fn_ptr: *const u8,
}

impl CompiledKernel {
    /// Create a new `CompiledKernel` from a backing and function pointer.
    pub(crate) const fn new(backing: CompiledBacking, fn_ptr: *const u8) -> Self {
        Self {
            _backing: backing,
            fn_ptr,
        }
    }
}

// SAFETY: The backing memory (JITModule or JitMemory) owns the code region
// exclusively. The fn_ptr is derived from that owned region and remains valid
// for the struct's lifetime. No mutable aliasing is possible.
#[expect(
    unsafe_code,
    reason = "CompiledKernel owns its code region exclusively"
)]
unsafe impl Send for CompiledKernel {}
#[expect(unsafe_code, reason = "fn_ptr is read-only and backed by owned memory")]
unsafe impl Sync for CompiledKernel {}

/// Owns the compiled code memory so it lives as long as the kernel pointer.
pub(crate) enum CompiledBacking {
    #[expect(dead_code, reason = "holds JitMemory ownership for fn_ptr lifetime")]
    Sovereign(crate::runtime::JitMemory),
}

impl CompiledKernel {
    /// Get the kernel function pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the `CompiledKernel` outlives any calls to the
    /// returned function pointer, and that the arguments match the expected signature.
    #[expect(
        unsafe_code,
        reason = "JIT function pointer transmute is inherent to JIT"
    )]
    #[must_use]
    pub unsafe fn as_fn(&self) -> KernelFn {
        // SAFETY: fn_ptr was produced by Cranelift JIT and has the correct signature.
        // The caller is responsible for argument correctness and lifetime.
        unsafe { std::mem::transmute(self.fn_ptr) }
    }
}

/// How libm function imports are resolved — directly via
/// `Function::import_function` for sovereign compilation.
enum LibmResolver<'c> {
    /// Sovereign path: import functions directly into the `Function`, recording
    /// names for later relocation resolution.
    Sovereign {
        call_conv: CallConv,
        names: &'c mut Vec<(&'static str, cranelift_codegen::ir::FuncRef)>,
    },
}

pub(crate) struct FunctionTranslator<'a, 'b, 'c> {
    builder: &'a mut FunctionBuilder<'b>,
    resolver: LibmResolver<'c>,
    ssa_map: HashMap<u32, Value>,
    entry_block: Option<Block>,
    block_map: HashMap<usize, Block>,
    label_to_block_idx: HashMap<String, usize>,
    ptr_type: types::Type,
    libm_fns: HashMap<&'static str, cranelift_codegen::ir::FuncRef>,
    current_block_terminated: bool,
    /// Cranelift Variables for phi nodes (loop-carried values).
    phi_vars: HashMap<Phi, Variable>,
}

impl<'a, 'b, 'c> FunctionTranslator<'a, 'b, 'c> {
    pub(crate) fn new_sovereign(
        builder: &'a mut FunctionBuilder<'b>,
        ptr_type: types::Type,
        call_conv: CallConv,
        libm_names: &'c mut Vec<(&'static str, cranelift_codegen::ir::FuncRef)>,
    ) -> Self {
        Self {
            builder,
            resolver: LibmResolver::Sovereign {
                call_conv,
                names: libm_names,
            },
            ssa_map: HashMap::new(),
            entry_block: None,
            block_map: HashMap::new(),
            label_to_block_idx: HashMap::new(),
            ptr_type,
            libm_fns: HashMap::new(),
            current_block_terminated: false,
            phi_vars: HashMap::new(),
        }
    }

    pub(crate) fn translate_function(&mut self, func: &ir::Function) -> Result<(), JitError> {
        if func.blocks.is_empty() {
            return Ok(());
        }

        for (i, bb) in func.blocks.iter().enumerate() {
            let block = self.builder.create_block();
            self.block_map.insert(i, block);
            let label_str = format!("{}", bb.label);
            self.label_to_block_idx.insert(label_str, i);
        }

        let first_block = self.block_map[&0];
        self.entry_block = Some(first_block);
        self.builder
            .append_block_params_for_function_params(first_block);
        self.builder.switch_to_block(first_block);
        self.builder.seal_block(first_block);

        for (block_idx, bb) in func.blocks.iter().enumerate() {
            let cl_block = self.block_map[&block_idx];

            if block_idx > 0 {
                self.builder.switch_to_block(cl_block);
            }

            for instr in &bb.instrs {
                self.translate_instr(instr)?;
            }

            if !self.current_block_terminated {
                if block_idx + 1 < func.blocks.len() {
                    let next = self.block_map[&(block_idx + 1)];
                    self.builder.ins().jump(next, &[]);
                } else {
                    self.builder.ins().return_(&[]);
                }
            }
            self.current_block_terminated = false;
        }

        for i in 1..func.blocks.len() {
            let block = self.block_map[&i];
            self.builder.seal_block(block);
        }

        Ok(())
    }

    fn translate_instr(&mut self, instr: &ir::Instr) -> Result<(), JitError> {
        if instr.pred.is_false() {
            return Ok(());
        }

        if !instr.pred.is_true() {
            return self.translate_predicated(instr);
        }

        self.translate_op(&instr.op)
    }

    fn translate_predicated(&mut self, instr: &ir::Instr) -> Result<(), JitError> {
        let pred_val = self.resolve_pred(&instr.pred)?;

        let then_block = self.builder.create_block();
        let merge_block = self.builder.create_block();

        self.builder
            .ins()
            .brif(pred_val, then_block, &[], merge_block, &[]);

        self.builder.switch_to_block(then_block);
        self.builder.seal_block(then_block);
        let was_terminated = self.current_block_terminated;
        self.current_block_terminated = false;
        self.translate_op(&instr.op)?;
        if !self.current_block_terminated {
            self.builder.ins().jump(merge_block, &[]);
        }
        self.current_block_terminated = was_terminated;

        self.builder.switch_to_block(merge_block);
        self.builder.seal_block(merge_block);
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn translate_op(&mut self, op: &Op) -> Result<(), JitError> {
        match op {
            Op::FAdd(op) => {
                let a = self.resolve_src_f32(&op.srcs[0])?;
                let b = self.resolve_src_f32(&op.srcs[1])?;
                let result = self.builder.ins().fadd(a, b);
                self.def_dst(&op.dst, result);
            }
            Op::FMul(op) => {
                let a = self.resolve_src_f32(&op.srcs[0])?;
                let b = self.resolve_src_f32(&op.srcs[1])?;
                let result = self.builder.ins().fmul(a, b);
                self.def_dst(&op.dst, result);
            }
            Op::FFma(op) => {
                let a = self.resolve_src_f32(&op.srcs[0])?;
                let b = self.resolve_src_f32(&op.srcs[1])?;
                let c = self.resolve_src_f32(&op.srcs[2])?;
                let result = self.builder.ins().fma(a, b, c);
                self.def_dst(&op.dst, result);
            }
            Op::FMnMx(op) => {
                let a = self.resolve_src_f32(&op.srcs[0])?;
                let b = self.resolve_src_f32(&op.srcs[1])?;
                let is_min = self.resolve_src_pred(&op.srcs[2])?;
                let min_val = self.builder.ins().fmin(a, b);
                let max_val = self.builder.ins().fmax(a, b);
                let result = self.builder.ins().select(is_min, min_val, max_val);
                self.def_dst(&op.dst, result);
            }
            Op::FSetP(op) => {
                let a = self.resolve_src_f32(&op.srcs[0])?;
                let b = self.resolve_src_f32(&op.srcs[1])?;
                let cc = float_cmp_to_cc(op.cmp_op);
                let result = self.builder.ins().fcmp(cc, a, b);
                self.def_dst(&op.dst, result);
            }
            Op::DAdd(op) => {
                let a = self.resolve_src_f64(&op.srcs[0])?;
                let b = self.resolve_src_f64(&op.srcs[1])?;
                let result = self.builder.ins().fadd(a, b);
                self.def_dst_f64(&op.dst, result);
            }
            Op::DMul(op) => {
                let a = self.resolve_src_f64(&op.srcs[0])?;
                let b = self.resolve_src_f64(&op.srcs[1])?;
                let result = self.builder.ins().fmul(a, b);
                self.def_dst_f64(&op.dst, result);
            }
            Op::DFma(op) => {
                let a = self.resolve_src_f64(&op.srcs[0])?;
                let b = self.resolve_src_f64(&op.srcs[1])?;
                let c = self.resolve_src_f64(&op.srcs[2])?;
                let result = self.builder.ins().fma(a, b, c);
                self.def_dst_f64(&op.dst, result);
            }
            Op::DMnMx(op) => {
                let a = self.resolve_src_f64(&op.srcs[0])?;
                let b = self.resolve_src_f64(&op.srcs[1])?;
                let is_min = self.resolve_src_pred(&op.srcs[2])?;
                let min_val = self.builder.ins().fmin(a, b);
                let max_val = self.builder.ins().fmax(a, b);
                let result = self.builder.ins().select(is_min, min_val, max_val);
                self.def_dst_f64(&op.dst, result);
            }
            Op::DSetP(op) => {
                let a = self.resolve_src_f64(&op.srcs[0])?;
                let b = self.resolve_src_f64(&op.srcs[1])?;
                let cc = float_cmp_to_cc(op.cmp_op);
                let result = self.builder.ins().fcmp(cc, a, b);
                self.def_dst(&op.dst, result);
            }
            Op::F64Sqrt(op) => {
                let a = self.resolve_src_f64(&op.src)?;
                let result = self.builder.ins().sqrt(a);
                self.def_dst_f64(&op.dst, result);
            }
            Op::F64Rcp(op) => {
                let a = self.resolve_src_f64(&op.src)?;
                let one = self.builder.ins().f64const(1.0);
                let result = self.builder.ins().fdiv(one, a);
                self.def_dst_f64(&op.dst, result);
            }
            Op::F64Exp2(op) => {
                let a = self.resolve_src_f64(&op.src)?;
                let result = self.call_f64_libm("exp2", a);
                self.def_dst_f64(&op.dst, result);
            }
            Op::F64Log2(op) => {
                let a = self.resolve_src_f64(&op.src)?;
                let result = self.call_f64_libm("log2", a);
                self.def_dst_f64(&op.dst, result);
            }
            Op::F64Sin(op) => {
                let a = self.resolve_src_f64(&op.src)?;
                let result = self.call_f64_libm("sin", a);
                self.def_dst_f64(&op.dst, result);
            }
            Op::F64Cos(op) => {
                let a = self.resolve_src_f64(&op.src)?;
                let result = self.call_f64_libm("cos", a);
                self.def_dst_f64(&op.dst, result);
            }
            Op::IAdd3(op) => {
                let a = self.resolve_src_any(&op.srcs[0])?;
                let b = self.resolve_src_any(&op.srcs[1])?;
                let c = self.resolve_src_any(&op.srcs[2])?;
                let ab = self.typed_iadd(a, b);
                let result = self.typed_iadd(ab, c);
                self.def_dst(&op.dsts[0], result);
            }
            Op::IAdd2(op) => {
                let a = self.resolve_src_any(&op.srcs[0])?;
                let b = self.resolve_src_any(&op.srcs[1])?;
                let result = self.typed_iadd(a, b);
                self.def_dst(&op.dsts[0], result);
            }
            Op::IMad(op) => {
                let a = self.resolve_src_any(&op.srcs[0])?;
                let b = self.resolve_src_any(&op.srcs[1])?;
                let c = self.resolve_src_any(&op.srcs[2])?;
                let ab = self.typed_imul(a, b);
                let result = self.typed_iadd(ab, c);
                self.def_dst(&op.dst, result);
            }
            Op::IMul(op) => {
                let a = self.resolve_src_any(&op.srcs[0])?;
                let b = self.resolve_src_any(&op.srcs[1])?;
                let result = self.typed_imul(a, b);
                self.def_dst(&op.dst, result);
            }
            Op::IMnMx(op) => {
                let a = self.resolve_src_i32(&op.srcs[0])?;
                let b = self.resolve_src_i32(&op.srcs[1])?;
                let is_min = self.resolve_src_pred(&op.srcs[2])?;
                let smin_val = self.builder.ins().smin(a, b);
                let smax_val = self.builder.ins().smax(a, b);
                let result = self.builder.ins().select(is_min, smin_val, smax_val);
                self.def_dst(&op.dst, result);
            }
            Op::IAbs(op) => {
                let a = self.resolve_src_i32(&op.src)?;
                let result = self.builder.ins().iabs(a);
                self.def_dst(&op.dst, result);
            }
            Op::ISetP(op) => {
                let a = self.resolve_src_i32(&op.srcs[0])?;
                let b = self.resolve_src_i32(&op.srcs[1])?;
                let cc = int_cmp_to_cc(op.cmp_op, !op.cmp_type.is_signed());
                let result = self.builder.ins().icmp(cc, a, b);
                self.def_dst(&op.dst, result);
            }
            Op::Lop2(op) => {
                let a = self.resolve_src_any(&op.srcs[0])?;
                let b = self.resolve_src_any(&op.srcs[1])?;
                let result = match op.op {
                    LogicOp2::And => self.builder.ins().band(a, b),
                    LogicOp2::Or => self.builder.ins().bor(a, b),
                    LogicOp2::Xor => self.builder.ins().bxor(a, b),
                    LogicOp2::PassB => b,
                };
                self.def_dst(&op.dst, result);
            }
            Op::Shl(op) => {
                let a = self.resolve_src_any(&op.srcs[0])?;
                let b = self.resolve_src_any(&op.srcs[1])?;
                let result = self.builder.ins().ishl(a, b);
                self.def_dst(&op.dst, result);
            }
            Op::Shr(op) => {
                let a = self.resolve_src_any(&op.srcs[0])?;
                let b = self.resolve_src_any(&op.srcs[1])?;
                let result = if op.signed {
                    self.builder.ins().sshr(a, b)
                } else {
                    self.builder.ins().ushr(a, b)
                };
                self.def_dst(&op.dst, result);
            }
            Op::PopC(op) => {
                let a = self.resolve_src_i32(&op.src)?;
                let result = self.builder.ins().popcnt(a);
                self.def_dst(&op.dst, result);
            }
            Op::BRev(op) => {
                let a = self.resolve_src_i32(&op.src)?;
                let result = self.builder.ins().bitrev(a);
                self.def_dst(&op.dst, result);
            }
            Op::F2I(op) => {
                let src = self.resolve_src_float(&op.src, op.src_type)?;
                let result = if op.dst_type.is_signed() {
                    self.builder.ins().fcvt_to_sint(types::I32, src)
                } else {
                    self.builder.ins().fcvt_to_uint(types::I32, src)
                };
                self.def_dst(&op.dst, result);
            }
            Op::I2F(op) => {
                let src = self.resolve_src_i32(&op.src)?;
                let result = if op.src_type.is_signed() {
                    self.builder.ins().fcvt_from_sint(types::F32, src)
                } else {
                    self.builder.ins().fcvt_from_uint(types::F32, src)
                };
                self.def_dst(&op.dst, result);
            }
            Op::F2F(op) => {
                let src = self.resolve_src_float(&op.src, op.src_type)?;
                let result = match (op.src_type, op.dst_type) {
                    (ir::FloatType::F32, ir::FloatType::F64) => {
                        self.builder.ins().fpromote(types::F64, src)
                    }
                    (ir::FloatType::F64, ir::FloatType::F32) => {
                        self.builder.ins().fdemote(types::F32, src)
                    }
                    _ => src,
                };
                if matches!(op.dst_type, ir::FloatType::F64) {
                    self.def_dst_f64(&op.dst, result);
                } else {
                    self.def_dst(&op.dst, result);
                }
            }
            Op::I2I(op) => {
                let src = self.resolve_src_i32(&op.src)?;
                self.def_dst(&op.dst, src);
            }
            Op::FRnd(op) => {
                let src = self.resolve_src_float(&op.src, op.src_type)?;
                let converted = match (op.src_type, op.dst_type) {
                    (ir::FloatType::F32, ir::FloatType::F64) => {
                        self.builder.ins().fpromote(types::F64, src)
                    }
                    (ir::FloatType::F64, ir::FloatType::F32) => {
                        self.builder.ins().fdemote(types::F32, src)
                    }
                    _ => src,
                };
                let result = self.apply_rnd_mode(converted, op.rnd_mode);
                if matches!(op.dst_type, ir::FloatType::F64) {
                    self.def_dst_f64(&op.dst, result);
                } else {
                    self.def_dst(&op.dst, result);
                }
            }
            Op::Ld(op) => {
                if matches!(op.access.space, MemSpace::Shared) {
                    return Err(JitError::UnsupportedOp(
                        "Ld from shared memory (use interpreter for workgroup shaders)".into(),
                    ));
                }
                if !matches!(op.access.space, MemSpace::Global(_)) {
                    return Err(JitError::UnsupportedOp(
                        "Ld from non-global memory space".into(),
                    ));
                }
                let addr = self.resolve_src_addr(&op.addr)?;
                let final_addr = self.offset_ptr(addr, i64::from(op.offset));
                let result = self
                    .builder
                    .ins()
                    .load(types::I32, MemFlags::new(), final_addr, 0);
                self.def_dst(&op.dst, result);
            }
            Op::St(op) => {
                if matches!(op.access.space, MemSpace::Shared) {
                    return Err(JitError::UnsupportedOp(
                        "St to shared memory (use interpreter for workgroup shaders)".into(),
                    ));
                }
                if !matches!(op.access.space, MemSpace::Global(_)) {
                    return Err(JitError::UnsupportedOp(
                        "St to non-global memory space".into(),
                    ));
                }
                let addr = self.resolve_src_addr(&op.srcs[0])?;
                let data = self.resolve_src_any(&op.srcs[1])?;
                let final_addr = self.offset_ptr(addr, i64::from(op.offset));
                self.builder
                    .ins()
                    .store(MemFlags::new(), data, final_addr, 0);
            }
            Op::Bra(op) => {
                let label_str = format!("{}", op.target);
                let block_idx = self
                    .label_to_block_idx
                    .get(&label_str)
                    .copied()
                    .ok_or_else(|| {
                        JitError::Translation(format!("unknown branch target {label_str}"))
                    })?;
                let target = self.block_map[&block_idx];

                if op.cond.reference == SrcRef::True {
                    self.builder.ins().jump(target, &[]);
                    self.current_block_terminated = true;
                } else {
                    let cond = self.resolve_src_pred(&op.cond)?;
                    let fallthrough = self.builder.create_block();
                    self.builder.ins().brif(cond, target, &[], fallthrough, &[]);
                    self.builder.switch_to_block(fallthrough);
                    self.builder.seal_block(fallthrough);
                }
            }
            Op::Exit(_) => {
                self.builder.ins().return_(&[]);
                self.current_block_terminated = true;
            }
            Op::Mov(op) => {
                let val = self.resolve_src_any(&op.src)?;
                self.def_dst(&op.dst, val);
            }
            Op::Copy(op) => {
                let val = self.resolve_src_any(&op.src)?;
                self.def_dst(&op.dst, val);
            }
            Op::Sel(op) => {
                let cond = self.resolve_src_pred(&op.srcs[0])?;
                let a = self.resolve_src_any(&op.srcs[1])?;
                let b = self.resolve_src_any(&op.srcs[2])?;
                let result = self.builder.ins().select(cond, a, b);
                self.def_dst(&op.dst, result);
            }

            Op::S2R(op) => {
                let val = self.translate_sys_reg(op.idx)?;
                self.def_dst(&op.dst, val);
            }
            Op::CS2R(op) => {
                let val = self.translate_sys_reg(op.idx)?;
                self.def_dst(&op.dst, val);
            }
            Op::Transcendental(op) => self.translate_transcendental(op)?,

            Op::Undef(op) => {
                let val = self.builder.ins().iconst(types::I32, 0);
                self.def_dst(&op.dst, val);
            }

            Op::PhiSrcs(op) => {
                for (phi, src) in op.srcs.iter() {
                    let val = self.resolve_src_any(src)?;
                    let src_type = self.builder.func.dfg.value_type(val);
                    let var = self.get_or_create_phi_var(*phi, src_type);
                    let declared_type = {
                        let probe = self.builder.use_var(var);
                        self.builder.func.dfg.value_type(probe)
                    };
                    let coerced = if declared_type == src_type {
                        val
                    } else {
                        self.builder
                            .ins()
                            .bitcast(declared_type, MemFlags::new(), val)
                    };
                    self.builder.def_var(var, coerced);
                }
            }
            Op::PhiDsts(op) => {
                for (phi, dst) in op.dsts.iter() {
                    let var = self.get_or_create_phi_var(*phi, types::I32);
                    let val = self.builder.use_var(var);
                    self.def_dst(dst, val);
                }
            }

            Op::Nop(_) | Op::Annotate(_) | Op::Bar(_) | Op::MemBar(_) => {}
            Op::Tex(_)
            | Op::Tld(_)
            | Op::Tld4(_)
            | Op::Tmml(_)
            | Op::Txd(_)
            | Op::Txq(_)
            | Op::SuLd(_)
            | Op::SuSt(_)
            | Op::SuAtom(_) => {
                return Err(JitError::UnsupportedOp("texture operations".into()));
            }
            Op::Vote(_) | Op::Match(_) | Op::Redux(_) | Op::Shfl(_) => {
                return Err(JitError::UnsupportedOp("warp operations".into()));
            }

            _ => {
                return Err(JitError::UnsupportedOp(format!("{op}")));
            }
        }
        Ok(())
    }

    fn translate_transcendental(&mut self, op: &ir::OpTranscendental) -> Result<(), JitError> {
        use ir::TranscendentalOp;
        let src = self.resolve_src_f32(&op.src)?;
        let result = match op.op {
            TranscendentalOp::Rcp => {
                let one = self.builder.ins().f32const(1.0);
                self.builder.ins().fdiv(one, src)
            }
            TranscendentalOp::Rsq => {
                let sq = self.builder.ins().sqrt(src);
                let one = self.builder.ins().f32const(1.0);
                self.builder.ins().fdiv(one, sq)
            }
            TranscendentalOp::Sqrt => self.builder.ins().sqrt(src),
            TranscendentalOp::Log2 => self.call_f32_libm("log2f", src),
            TranscendentalOp::Exp2 => self.call_f32_libm("exp2f", src),
            TranscendentalOp::Sin => self.call_f32_libm("sinf", src),
            TranscendentalOp::Cos => self.call_f32_libm("cosf", src),
            TranscendentalOp::Tanh => self.call_f32_libm("tanhf", src),
            _ => {
                return Err(JitError::UnsupportedOp(format!(
                    "transcendental {:?}",
                    op.op
                )));
            }
        };
        self.def_dst(&op.dst, result);
        Ok(())
    }

    fn translate_sys_reg(&mut self, idx: u8) -> Result<Value, JitError> {
        match builtins::sys_reg_to_param(idx) {
            Some(SysRegMapping::Param(param_idx)) => {
                let params = self.entry_block_params()?;
                if param_idx < params.len() {
                    Ok(params[param_idx])
                } else {
                    Err(JitError::Translation(format!(
                        "param index {param_idx} out of range"
                    )))
                }
            }
            Some(SysRegMapping::Constant(val)) => {
                Ok(self.builder.ins().iconst(types::I32, i64::from(val)))
            }
            None => Err(JitError::UnsupportedOp(format!(
                "system register 0x{idx:02x}"
            ))),
        }
    }

    fn resolve_src_i32(&mut self, src: &Src) -> Result<Value, JitError> {
        let raw = self.resolve_src_ref(&src.reference)?;
        self.apply_src_mod_int(raw, src.modifier)
    }

    fn resolve_src_f32(&mut self, src: &Src) -> Result<Value, JitError> {
        let raw = self.resolve_src_ref(&src.reference)?;
        let typed = self.ensure_f32(raw);
        self.apply_src_mod_float(typed, src.modifier)
    }

    fn resolve_src_f64(&mut self, src: &Src) -> Result<Value, JitError> {
        let raw = self.resolve_src_ref(&src.reference)?;
        let typed = self.ensure_f64(raw);
        self.apply_src_mod_float(typed, src.modifier)
    }

    fn resolve_src_float(&mut self, src: &Src, ft: ir::FloatType) -> Result<Value, JitError> {
        match ft {
            ir::FloatType::F32 | ir::FloatType::F16 => self.resolve_src_f32(src),
            ir::FloatType::F64 => self.resolve_src_f64(src),
        }
    }

    fn resolve_src_pred(&mut self, src: &Src) -> Result<Value, JitError> {
        let raw = self.resolve_src_ref(&src.reference)?;
        match src.modifier {
            SrcMod::BNot => Ok(self.builder.ins().bxor_imm(raw, 1)),
            _ => Ok(raw),
        }
    }

    fn resolve_src_addr(&mut self, src: &Src) -> Result<Value, JitError> {
        let raw = self.resolve_src_ref(&src.reference)?;
        let val_type = self.builder.func.dfg.value_type(raw);
        if val_type == self.ptr_type {
            Ok(raw)
        } else {
            Ok(self.builder.ins().uextend(self.ptr_type, raw))
        }
    }

    fn resolve_src_ref(&mut self, src_ref: &SrcRef) -> Result<Value, JitError> {
        match src_ref {
            SrcRef::Zero => Ok(self.builder.ins().iconst(types::I32, 0)),
            SrcRef::True => Ok(self.builder.ins().iconst(types::I8, 1)),
            SrcRef::False => Ok(self.builder.ins().iconst(types::I8, 0)),
            SrcRef::Imm32(val) => Ok(self.builder.ins().iconst(types::I32, i64::from(*val))),
            SrcRef::SSA(ssa_ref) => {
                let idx = ssa_ref[0].idx();
                self.ssa_map
                    .get(&idx)
                    .copied()
                    .ok_or_else(|| JitError::Translation(format!("undefined SSA value {idx}")))
            }
            SrcRef::CBuf(cbuf_ref) => self.resolve_cbuf(cbuf_ref),
            SrcRef::Reg(_) => Err(JitError::Translation(
                "register references not expected in pre-RA IR".into(),
            )),
        }
    }

    fn resolve_cbuf(&mut self, cbuf: &ir::CBufRef) -> Result<Value, JitError> {
        match &cbuf.buf {
            ir::CBuf::Binding(idx) => {
                if *idx == 0 {
                    return self.resolve_driver_cbuf(cbuf.offset);
                }
                let user_idx = i64::from(*idx - 1);
                let bindings_ptr = self.bindings_ptr()?;
                let byte_offset = user_idx * i64::from(self.ptr_type.bytes());
                let buf_ptr_ptr = self.offset_ptr(bindings_ptr, byte_offset);
                let mem_flags = MemFlags::new();
                let buf_ptr = self
                    .builder
                    .ins()
                    .load(self.ptr_type, mem_flags, buf_ptr_ptr, 0);
                if cbuf.offset == 0 {
                    Ok(buf_ptr)
                } else {
                    let field_off = self
                        .builder
                        .ins()
                        .iconst(self.ptr_type, i64::from(cbuf.offset));
                    let addr = self.builder.ins().iadd(buf_ptr, field_off);
                    Ok(self.builder.ins().load(types::I32, mem_flags, addr, 0))
                }
            }
            _ => Err(JitError::UnsupportedOp("bindless CBuf".into())),
        }
    }

    /// Resolve a read from `CBuf` 0 (driver info constant buffer).
    ///
    /// In the NVIDIA driver model, `CBuf` 0 contains 64-bit buffer base addresses
    /// for each user binding at 8-byte-aligned offsets. For JIT execution, we
    /// load the corresponding buffer pointer from the `bindings_ptr` array.
    fn resolve_driver_cbuf(&mut self, offset: u16) -> Result<Value, JitError> {
        let bindings_ptr = self.bindings_ptr()?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "pointer width (4 or 8) fits in u16"
        )]
        let ptr_bytes = self.ptr_type.bytes() as u16;
        let binding_idx = i64::from(offset / 8);
        let byte_off = binding_idx * i64::from(ptr_bytes);
        let buf_ptr_ptr = self.offset_ptr(bindings_ptr, byte_off);
        let mem_flags = MemFlags::new();
        if offset % 8 < ptr_bytes {
            Ok(self
                .builder
                .ins()
                .load(self.ptr_type, mem_flags, buf_ptr_ptr, 0))
        } else {
            Ok(self.builder.ins().iconst(types::I32, 0))
        }
    }

    /// Add a byte offset to a pointer, returning the pointer unchanged if offset is 0.
    fn offset_ptr(&mut self, base: Value, byte_offset: i64) -> Value {
        if byte_offset != 0 {
            let off = self.builder.ins().iconst(self.ptr_type, byte_offset);
            self.builder.ins().iadd(base, off)
        } else {
            base
        }
    }

    fn resolve_pred(&mut self, pred: &Pred) -> Result<Value, JitError> {
        match &pred.predicate {
            PredRef::None => {
                let val = i64::from(!pred.inverted);
                Ok(self.builder.ins().iconst(types::I8, val))
            }
            PredRef::SSA(ssa) => {
                let val = self.ssa_map.get(&ssa.idx()).copied().ok_or_else(|| {
                    JitError::Translation(format!("undefined pred SSA {}", ssa.idx()))
                })?;
                if pred.inverted {
                    Ok(self.builder.ins().bxor_imm(val, 1))
                } else {
                    Ok(val)
                }
            }
            PredRef::Reg(_) => Err(JitError::Translation(
                "register pred not expected in pre-RA IR".into(),
            )),
        }
    }

    fn def_dst(&mut self, dst: &Dst, val: Value) {
        if let Dst::SSA(ssa_ref) = dst {
            self.ssa_map.insert(ssa_ref[0].idx(), val);
        }
    }

    fn def_dst_f64(&mut self, dst: &Dst, val: Value) {
        if let Dst::SSA(ssa_ref) = dst {
            self.ssa_map.insert(ssa_ref[0].idx(), val);
            if ssa_ref.comps() > 1 {
                self.ssa_map.insert(ssa_ref[1].idx(), val);
            }
        }
    }

    /// Widen the narrower operand so both match the wider integer type.
    fn unify_int_widths(&mut self, a: Value, b: Value) -> (Value, Value) {
        let a_ty = self.builder.func.dfg.value_type(a);
        let b_ty = self.builder.func.dfg.value_type(b);
        if a_ty == b_ty {
            (a, b)
        } else if a_ty.bytes() > b_ty.bytes() {
            (a, self.builder.ins().uextend(a_ty, b))
        } else {
            (self.builder.ins().uextend(b_ty, a), b)
        }
    }

    fn typed_iadd(&mut self, a: Value, b: Value) -> Value {
        let (a, b) = self.unify_int_widths(a, b);
        self.builder.ins().iadd(a, b)
    }

    fn typed_imul(&mut self, a: Value, b: Value) -> Value {
        let (a, b) = self.unify_int_widths(a, b);
        self.builder.ins().imul(a, b)
    }

    /// Resolve a source value, returning its native width (i32 or i64 for pointers).
    fn resolve_src_any(&mut self, src: &Src) -> Result<Value, JitError> {
        let raw = self.resolve_src_ref(&src.reference)?;
        self.apply_src_mod_int(raw, src.modifier)
    }

    fn ensure_f32(&mut self, val: Value) -> Value {
        let ty = self.builder.func.dfg.value_type(val);
        if ty == types::F32 {
            val
        } else if ty == types::I32 {
            self.builder.ins().bitcast(types::F32, MemFlags::new(), val)
        } else {
            val
        }
    }

    fn ensure_f64(&mut self, val: Value) -> Value {
        let ty = self.builder.func.dfg.value_type(val);
        if ty == types::F64 {
            val
        } else if ty == types::I64 {
            self.builder.ins().bitcast(types::F64, MemFlags::new(), val)
        } else if ty == types::I32 {
            let extended = self.builder.ins().uextend(types::I64, val);
            self.builder
                .ins()
                .bitcast(types::F64, MemFlags::new(), extended)
        } else {
            val
        }
    }

    fn apply_src_mod_float(&mut self, val: Value, modifier: SrcMod) -> Result<Value, JitError> {
        match modifier {
            SrcMod::None => Ok(val),
            SrcMod::FNeg | SrcMod::INeg => Ok(self.builder.ins().fneg(val)),
            SrcMod::FAbs => Ok(self.builder.ins().fabs(val)),
            SrcMod::FNegAbs => {
                let abs = self.builder.ins().fabs(val);
                Ok(self.builder.ins().fneg(abs))
            }
            SrcMod::BNot => Err(JitError::Translation("BNot modifier on float value".into())),
        }
    }

    fn apply_src_mod_int(&mut self, val: Value, modifier: SrcMod) -> Result<Value, JitError> {
        match modifier {
            SrcMod::None => Ok(val),
            SrcMod::INeg => Ok(self.builder.ins().ineg(val)),
            SrcMod::BNot => Ok(self.builder.ins().bnot(val)),
            SrcMod::FNeg | SrcMod::FAbs | SrcMod::FNegAbs => {
                let fval = self.ensure_f32(val);
                let float_result = self.apply_src_mod_float(fval, modifier)?;
                Ok(self
                    .builder
                    .ins()
                    .bitcast(types::I32, MemFlags::new(), float_result))
            }
        }
    }

    /// Get the entry block parameters (function arguments in Cranelift).
    fn entry_block_params(&self) -> Result<&[Value], JitError> {
        let entry = self
            .entry_block
            .ok_or_else(|| JitError::Translation("entry block not set".into()))?;
        Ok(self.builder.block_params(entry))
    }

    /// Get the `bindings_ptr` value (first function parameter).
    fn bindings_ptr(&self) -> Result<Value, JitError> {
        Ok(self.entry_block_params()?[0])
    }

    // --- Phi variable management ---

    fn get_or_create_phi_var(&mut self, phi: Phi, ty: types::Type) -> Variable {
        if let Some(&var) = self.phi_vars.get(&phi) {
            return var;
        }
        let var = self.builder.declare_var(ty);
        self.phi_vars.insert(phi, var);
        var
    }

    fn apply_rnd_mode(&mut self, val: Value, mode: FRndMode) -> Value {
        match mode {
            FRndMode::NearestEven => self.builder.ins().nearest(val),
            FRndMode::Zero => self.builder.ins().trunc(val),
            FRndMode::NegInf => self.builder.ins().floor(val),
            FRndMode::PosInf => self.builder.ins().ceil(val),
        }
    }

    /// Call a single-argument libm function, returning its result.
    fn call_libm(&mut self, name: &'static str, ty: types::Type, arg: Value) -> Value {
        let func_ref = self.get_or_create_libm_fn(name, ty, ty);
        let call = self.builder.ins().call(func_ref, &[arg]);
        self.builder.inst_results(call)[0]
    }

    fn call_f32_libm(&mut self, name: &'static str, arg: Value) -> Value {
        self.call_libm(name, types::F32, arg)
    }

    fn call_f64_libm(&mut self, name: &'static str, arg: Value) -> Value {
        self.call_libm(name, types::F64, arg)
    }

    fn get_or_create_libm_fn(
        &mut self,
        name: &'static str,
        param_ty: types::Type,
        ret_ty: types::Type,
    ) -> cranelift_codegen::ir::FuncRef {
        if let Some(func_ref) = self.libm_fns.get(name) {
            return *func_ref;
        }
        let LibmResolver::Sovereign { call_conv, names } = &mut self.resolver;
        let mut sig = cranelift_codegen::ir::Signature::new(*call_conv);
        sig.params.push(AbiParam::new(param_ty));
        sig.returns.push(AbiParam::new(ret_ty));
        let sig_ref = self.builder.import_signature(sig);
        let user_ref = self.builder.func.declare_imported_user_function(
            cranelift_codegen::ir::UserExternalName::new(
                1,
                #[expect(clippy::cast_possible_truncation, reason = "libm fn count fits u32")]
                {
                    self.libm_fns.len() as u32
                },
            ),
        );
        let func_ref = self.builder.import_function(ExtFuncData {
            name: ExternalName::user(user_ref),
            signature: sig_ref,
            colocated: false,
            patchable: false,
        });
        names.push((name, func_ref));
        self.libm_fns.insert(name, func_ref);
        func_ref
    }
}
