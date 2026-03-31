// SPDX-License-Identifier: AGPL-3.0-only
//! `CoralIR` reference executor — walks optimized `Shader` ops in pure safe Rust.
//!
//! This is the "oracle" path: given the same `CoralIR` that GPU and JIT backends
//! consume, evaluate each Op directly on the CPU. Results from this executor are
//! the ground truth against which JIT output is validated.
//!
//! The Op dispatch mirrors `coral-reef-jit/translate.rs` exactly — when the JIT
//! adds a new Op, adding the corresponding evaluation here closes the coverage
//! gap. Clippy's exhaustive-match lint enforces parity.

mod eval_ops;

use std::collections::HashMap;

use coral_reef::codegen::ir::{
    self, Dst, LogicOp2, MemSpace, Op, Phi, Pred, PredRef, Shader, Src, SrcMod, SrcRef,
};

use crate::types::{BindingData, CpuError, ExecuteCpuResponse};

/// Simulated register file: maps SSA value indices to 32-bit words.
///
/// f64 values occupy two consecutive SSA slots (lo/hi) stored as a single
/// `RegValue::F64`. This mirrors the GPU register model where f64 spans two GPRs.
#[derive(Debug, Clone)]
enum RegValue {
    I32(u32),
    F32(f32),
    F64(f64),
    Bool(bool),
}

impl RegValue {
    fn as_u32(&self) -> u32 {
        match self {
            Self::I32(v) => *v,
            Self::F32(v) => v.to_bits(),
            Self::F64(_) => 0,
            Self::Bool(b) => u32::from(*b),
        }
    }

    #[expect(clippy::cast_possible_wrap, reason = "register-width reinterpret u32↔i32")]
    fn as_i32(&self) -> i32 {
        self.as_u32() as i32
    }

    #[expect(clippy::cast_possible_truncation, reason = "f64→f32 demotion expected")]
    const fn as_f32(&self) -> f32 {
        match self {
            Self::F32(v) => *v,
            Self::I32(v) => f32::from_bits(*v),
            Self::F64(v) => *v as f32,
            Self::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    fn as_f64(&self) -> f64 {
        match self {
            Self::F64(v) => *v,
            Self::F32(v) => f64::from(*v),
            Self::I32(v) => f64::from(*v),
            Self::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::I32(v) => *v != 0,
            Self::F32(v) => *v != 0.0,
            Self::F64(v) => *v != 0.0,
        }
    }
}

/// Invocation context holding system register values for one thread.
struct InvocationCtx {
    workgroup_id: [u32; 3],
    local_id: [u32; 3],
    num_workgroups: [u32; 3],
    workgroup_size: [u32; 3],
}

/// Execute a compiled `CoralIR` shader on the CPU, returning modified bindings.
///
/// This is the reference executor entry point. It compiles WGSL to `CoralIR` via
/// the same pipeline the GPU backends use, then interprets the resulting ops.
///
/// # Errors
///
/// Returns [`CpuError`] on unsupported ops or execution failures.
pub fn execute_coral_ir(
    request: &crate::types::ExecuteCpuRequest,
) -> Result<ExecuteCpuResponse, CpuError> {
    use coral_reef::CompileOptions;
    use coral_reef::gpu_arch::{GpuTarget, NvArch};

    let start = std::time::Instant::now();

    let options = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm86),
        ..Default::default()
    };
    let sm = coral_reef::shader_model_for(options.target)
        .map_err(|e| CpuError::Internal(e.to_string()))?;

    let shader = coral_reef::compile_wgsl_to_ir(&request.wgsl_source, &options, sm.as_ref())
        .map_err(|e| CpuError::Internal(e.to_string()))?;

    let mut buffers: Vec<Vec<u8>> = request.bindings.iter().map(|b| b.data.to_vec()).collect();

    let workgroup_size = crate::extract_workgroup_size(&shader);
    let [wg_count_x, wg_count_y, wg_count_z] = request.workgroups;

    for wg_z in 0..wg_count_z {
        for wg_y in 0..wg_count_y {
            for wg_x in 0..wg_count_x {
                for tz in 0..workgroup_size[2] {
                    for ty in 0..workgroup_size[1] {
                        for tx in 0..workgroup_size[0] {
                            let ctx = InvocationCtx {
                                workgroup_id: [wg_x, wg_y, wg_z],
                                local_id: [tx, ty, tz],
                                num_workgroups: [wg_count_x, wg_count_y, wg_count_z],
                                workgroup_size,
                            };
                            execute_invocation(&shader, &mut buffers, &ctx)?;
                        }
                    }
                }
            }
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "elapsed nanoseconds will not exceed u64 in practice"
    )]
    let elapsed_ns = start.elapsed().as_nanos() as u64;

    let output_bindings = buffers
        .into_iter()
        .zip(request.bindings.iter())
        .map(|(buf, orig)| BindingData {
            group: orig.group,
            binding: orig.binding,
            data: bytes::Bytes::from(buf),
            usage: orig.usage,
        })
        .collect();

    Ok(ExecuteCpuResponse {
        bindings: output_bindings,
        execution_time_ns: elapsed_ns,
        strategy_used: None,
        cache_hit: false,
        revalidated: false,
    })
}

fn execute_invocation(
    shader: &Shader<'_>,
    buffers: &mut [Vec<u8>],
    ctx: &InvocationCtx,
) -> Result<(), CpuError> {
    if shader.functions.is_empty() {
        return Ok(());
    }

    let func = &shader.functions[0];
    let mut regs: HashMap<u32, RegValue> = HashMap::new();
    let mut phi_state: HashMap<Phi, RegValue> = HashMap::new();

    let label_to_block: HashMap<String, usize> = func
        .blocks
        .iter()
        .enumerate()
        .map(|(i, bb)| (format!("{}", bb.label), i))
        .collect();

    let mut block_idx = 0;
    while block_idx < func.blocks.len() {
        let bb = &func.blocks[block_idx];
        let mut next_block = Some(block_idx + 1);

        for instr in &bb.instrs {
            if instr.pred.is_false() {
                continue;
            }
            if !instr.pred.is_true() && !eval_pred(&instr.pred, &regs) {
                continue;
            }
            match eval_op(
                &instr.op,
                &mut regs,
                &mut phi_state,
                buffers,
                ctx,
                &label_to_block,
            )? {
                OpEffect::Continue => {}
                OpEffect::Branch(target_idx) => {
                    next_block = Some(target_idx);
                    break;
                }
                OpEffect::Exit => {
                    return Ok(());
                }
            }
        }

        match next_block {
            Some(idx) if idx < func.blocks.len() => block_idx = idx,
            _ => return Ok(()),
        }
    }

    Ok(())
}

enum OpEffect {
    Continue,
    Branch(usize),
    Exit,
}

fn eval_pred(pred: &Pred, regs: &HashMap<u32, RegValue>) -> bool {
    let raw = match &pred.predicate {
        PredRef::SSA(ssa) => regs
            .get(&ssa.idx())
            .is_some_and(RegValue::as_bool),
        PredRef::None | PredRef::Reg(_) => true,
    };
    if pred.inverted { !raw } else { raw }
}

#[allow(clippy::too_many_lines)]
fn eval_op(
    op: &Op,
    regs: &mut HashMap<u32, RegValue>,
    phi_state: &mut HashMap<Phi, RegValue>,
    buffers: &mut [Vec<u8>],
    ctx: &InvocationCtx,
    label_to_block: &HashMap<String, usize>,
) -> Result<OpEffect, CpuError> {
    match op {
        Op::FAdd(op) => {
            let a = resolve_f32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_f32(&op.srcs[1], regs, buffers, ctx)?;
            def_dst(&op.dst, RegValue::F32(a + b), regs);
        }
        Op::FMul(op) => {
            let a = resolve_f32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_f32(&op.srcs[1], regs, buffers, ctx)?;
            def_dst(&op.dst, RegValue::F32(a * b), regs);
        }
        Op::FFma(op) => {
            let a = resolve_f32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_f32(&op.srcs[1], regs, buffers, ctx)?;
            let c = resolve_f32(&op.srcs[2], regs, buffers, ctx)?;
            def_dst(&op.dst, RegValue::F32(a.mul_add(b, c)), regs);
        }
        Op::FMnMx(op) => {
            let a = resolve_f32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_f32(&op.srcs[1], regs, buffers, ctx)?;
            let is_min = resolve_bool(&op.srcs[2], regs, buffers, ctx)?;
            let result = if is_min { a.min(b) } else { a.max(b) };
            def_dst(&op.dst, RegValue::F32(result), regs);
        }
        Op::FSetP(op) => {
            let a = resolve_f32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_f32(&op.srcs[1], regs, buffers, ctx)?;
            let result = eval_ops::float_cmp(a, b, op.cmp_op);
            def_dst(&op.dst, RegValue::Bool(result), regs);
        }
        Op::DAdd(op) => {
            let a = resolve_f64(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_f64(&op.srcs[1], regs, buffers, ctx)?;
            def_dst_f64(&op.dst, a + b, regs);
        }
        Op::DMul(op) => {
            let a = resolve_f64(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_f64(&op.srcs[1], regs, buffers, ctx)?;
            def_dst_f64(&op.dst, a * b, regs);
        }
        Op::DFma(op) => {
            let a = resolve_f64(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_f64(&op.srcs[1], regs, buffers, ctx)?;
            let c = resolve_f64(&op.srcs[2], regs, buffers, ctx)?;
            def_dst_f64(&op.dst, a.mul_add(b, c), regs);
        }
        Op::DMnMx(op) => {
            let a = resolve_f64(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_f64(&op.srcs[1], regs, buffers, ctx)?;
            let is_min = resolve_bool(&op.srcs[2], regs, buffers, ctx)?;
            let result = if is_min { a.min(b) } else { a.max(b) };
            def_dst_f64(&op.dst, result, regs);
        }
        Op::DSetP(op) => {
            let a = resolve_f64(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_f64(&op.srcs[1], regs, buffers, ctx)?;
            let result = eval_ops::float_cmp_f64(a, b, op.cmp_op);
            def_dst(&op.dst, RegValue::Bool(result), regs);
        }
        Op::F64Sqrt(op) => {
            let a = resolve_f64_unary(&op.src, regs, buffers, ctx)?;
            def_dst_f64(&op.dst, a.sqrt(), regs);
        }
        Op::F64Rcp(op) => {
            let a = resolve_f64_unary(&op.src, regs, buffers, ctx)?;
            def_dst_f64(&op.dst, 1.0 / a, regs);
        }
        Op::F64Exp2(op) => {
            let a = resolve_f64_unary(&op.src, regs, buffers, ctx)?;
            def_dst_f64(&op.dst, a.exp2(), regs);
        }
        Op::F64Log2(op) => {
            let a = resolve_f64_unary(&op.src, regs, buffers, ctx)?;
            def_dst_f64(&op.dst, a.log2(), regs);
        }
        Op::F64Sin(op) => {
            let a = resolve_f64_unary(&op.src, regs, buffers, ctx)?;
            def_dst_f64(&op.dst, a.sin(), regs);
        }
        Op::F64Cos(op) => {
            let a = resolve_f64_unary(&op.src, regs, buffers, ctx)?;
            def_dst_f64(&op.dst, a.cos(), regs);
        }

        // --- Integer arithmetic ---
        Op::IAdd3(op) => {
            let a = resolve_u32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_u32(&op.srcs[1], regs, buffers, ctx)?;
            let c = resolve_u32(&op.srcs[2], regs, buffers, ctx)?;
            def_dst(&op.dsts[0], RegValue::I32(a.wrapping_add(b).wrapping_add(c)), regs);
        }
        Op::IAdd2(op) => {
            let a = resolve_u32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_u32(&op.srcs[1], regs, buffers, ctx)?;
            def_dst(&op.dsts[0], RegValue::I32(a.wrapping_add(b)), regs);
        }
        Op::IMad(op) => {
            let a = resolve_u32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_u32(&op.srcs[1], regs, buffers, ctx)?;
            let c = resolve_u32(&op.srcs[2], regs, buffers, ctx)?;
            def_dst(&op.dst, RegValue::I32(a.wrapping_mul(b).wrapping_add(c)), regs);
        }
        Op::IMul(op) => {
            let a = resolve_u32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_u32(&op.srcs[1], regs, buffers, ctx)?;
            def_dst(&op.dst, RegValue::I32(a.wrapping_mul(b)), regs);
        }
        Op::IMnMx(op) => {
            let a = resolve_i32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_i32(&op.srcs[1], regs, buffers, ctx)?;
            let is_min = resolve_bool(&op.srcs[2], regs, buffers, ctx)?;
            let result = if is_min { a.min(b) } else { a.max(b) };
            #[expect(clippy::cast_sign_loss, reason = "reinterpret i32 as u32 bit pattern")]
            def_dst(&op.dst, RegValue::I32(result as u32), regs);
        }
        Op::IAbs(op) => {
            let a = resolve_i32_unary(&op.src, regs, buffers, ctx)?;
            def_dst(&op.dst, RegValue::I32(a.unsigned_abs()), regs);
        }
        Op::ISetP(op) => {
            let a = resolve_i32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_i32(&op.srcs[1], regs, buffers, ctx)?;
            let result = eval_ops::int_cmp(a, b, op.cmp_op, op.cmp_type.is_signed());
            def_dst(&op.dst, RegValue::Bool(result), regs);
        }
        Op::Lop2(op) => {
            let a = resolve_u32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_u32(&op.srcs[1], regs, buffers, ctx)?;
            let result = match op.op {
                LogicOp2::And => a & b,
                LogicOp2::Or => a | b,
                LogicOp2::Xor => a ^ b,
                LogicOp2::PassB => b,
            };
            def_dst(&op.dst, RegValue::I32(result), regs);
        }
        Op::Shl(op) => {
            let a = resolve_u32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_u32(&op.srcs[1], regs, buffers, ctx)?;
            def_dst(&op.dst, RegValue::I32(a.wrapping_shl(b)), regs);
        }
        #[expect(
            clippy::cast_sign_loss,
            clippy::cast_possible_wrap,
            reason = "signed shift reinterprets i32↔u32"
        )]
        Op::Shr(op) => {
            let a = resolve_u32(&op.srcs[0], regs, buffers, ctx)?;
            let b = resolve_u32(&op.srcs[1], regs, buffers, ctx)?;
            let result = if op.signed {
                (a as i32).wrapping_shr(b) as u32
            } else {
                a.wrapping_shr(b)
            };
            def_dst(&op.dst, RegValue::I32(result), regs);
        }
        Op::PopC(op) => {
            let a = resolve_u32_unary(&op.src, regs, buffers, ctx)?;
            def_dst(&op.dst, RegValue::I32(a.count_ones()), regs);
        }
        Op::BRev(op) => {
            let a = resolve_u32_unary(&op.src, regs, buffers, ctx)?;
            def_dst(&op.dst, RegValue::I32(a.reverse_bits()), regs);
        }

        // --- Type conversions ---
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "float-to-int conversion mirrors GPU truncation semantics"
        )]
        Op::F2I(op) => {
            let src = resolve_float(&op.src, op.src_type, regs, buffers, ctx)?;
            let result = if op.dst_type.is_signed() {
                (src as i32) as u32
            } else {
                src as u32
            };
            def_dst(&op.dst, RegValue::I32(result), regs);
        }
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_sign_loss,
            reason = "int-to-float conversion mirrors GPU precision semantics"
        )]
        Op::I2F(op) => {
            let src = resolve_i32_unary(&op.src, regs, buffers, ctx)?;
            let result = if op.src_type.is_signed() {
                src as f32
            } else {
                (src as u32) as f32
            };
            def_dst(&op.dst, RegValue::F32(result), regs);
        }
        #[expect(clippy::cast_possible_truncation, reason = "f64→f32 demotion mirrors GPU semantics")]
        Op::F2F(op) => {
            let src = resolve_float(&op.src, op.src_type, regs, buffers, ctx)?;
            match op.dst_type {
                ir::FloatType::F64 => def_dst_f64(&op.dst, src, regs),
                _ => def_dst(&op.dst, RegValue::F32(src as f32), regs),
            }
        }
        Op::I2I(op) => {
            let src = resolve_u32_unary(&op.src, regs, buffers, ctx)?;
            def_dst(&op.dst, RegValue::I32(src), regs);
        }
        #[expect(clippy::cast_possible_truncation, reason = "f64→f32 demotion mirrors GPU semantics")]
        Op::FRnd(op) => {
            let src = resolve_float(&op.src, op.src_type, regs, buffers, ctx)?;
            let rounded = eval_ops::apply_rnd_mode(src, op.rnd_mode);
            match op.dst_type {
                ir::FloatType::F64 => def_dst_f64(&op.dst, rounded, regs),
                _ => def_dst(&op.dst, RegValue::F32(rounded as f32), regs),
            }
        }

        // --- Memory ---
        Op::Ld(op) => {
            if !matches!(op.access.space, MemSpace::Global(_)) {
                return Err(CpuError::Unsupported("Ld from non-global memory".into()));
            }
            let addr = resolve_addr(&op.addr, regs, buffers, ctx)?;
            #[expect(clippy::cast_sign_loss, reason = "memory offsets reinterpreted as unsigned")]
            let final_addr = addr.wrapping_add(op.offset as usize);
            let val = read_u32_from_buffers(buffers, final_addr);
            def_dst(&op.dst, RegValue::I32(val), regs);
        }
        Op::St(op) => {
            if !matches!(op.access.space, MemSpace::Global(_)) {
                return Err(CpuError::Unsupported("St to non-global memory".into()));
            }
            let addr = resolve_addr(&op.srcs[0], regs, buffers, ctx)?;
            let data = resolve_u32(&op.srcs[1], regs, buffers, ctx)?;
            #[expect(clippy::cast_sign_loss, reason = "memory offsets reinterpreted as unsigned")]
            let final_addr = addr.wrapping_add(op.offset as usize);
            write_u32_to_buffers(buffers, final_addr, data);
        }

        // --- Control flow ---
        Op::Bra(op) => {
            let label_str = format!("{}", op.target);
            let target_idx = label_to_block
                .get(&label_str)
                .copied()
                .ok_or_else(|| CpuError::Internal(format!("unknown branch target {label_str}")))?;

            if op.cond.reference == SrcRef::True {
                return Ok(OpEffect::Branch(target_idx));
            }
            let cond = resolve_bool(&op.cond, regs, buffers, ctx)?;
            if cond {
                return Ok(OpEffect::Branch(target_idx));
            }
        }
        Op::Exit(_) => return Ok(OpEffect::Exit),

        // --- Data movement ---
        Op::Mov(op) => {
            let val = resolve_reg_value(&op.src, regs, buffers, ctx)?;
            def_dst(&op.dst, val, regs);
        }
        Op::Copy(op) => {
            let val = resolve_reg_value(&op.src, regs, buffers, ctx)?;
            def_dst(&op.dst, val, regs);
        }
        Op::Sel(op) => {
            let cond = resolve_bool(&op.srcs[0], regs, buffers, ctx)?;
            let a = resolve_reg_value(&op.srcs[1], regs, buffers, ctx)?;
            let b = resolve_reg_value(&op.srcs[2], regs, buffers, ctx)?;
            def_dst(&op.dst, if cond { a } else { b }, regs);
        }

        // --- System registers ---
        Op::S2R(op) => {
            let val = eval_sys_reg(op.idx, ctx)?;
            def_dst(&op.dst, RegValue::I32(val), regs);
        }
        Op::CS2R(op) => {
            let val = eval_sys_reg(op.idx, ctx)?;
            def_dst(&op.dst, RegValue::I32(val), regs);
        }

        // --- Transcendentals ---
        Op::Transcendental(op) => {
            let src = resolve_f32(&op.src, regs, buffers, ctx)?;
            let result = eval_ops::eval_transcendental(src, op.op)?;
            def_dst(&op.dst, RegValue::F32(result), regs);
        }

        // --- Phi nodes ---
        Op::PhiSrcs(op) => {
            for (phi, src) in op.srcs.iter() {
                let val = resolve_reg_value(src, regs, buffers, ctx)?;
                phi_state.insert(*phi, val);
            }
        }
        Op::PhiDsts(op) => {
            for (phi, dst) in op.dsts.iter() {
                let val = phi_state
                    .get(phi)
                    .cloned()
                    .unwrap_or(RegValue::I32(0));
                def_dst(dst, val, regs);
            }
        }

        Op::Undef(op) => {
            def_dst(&op.dst, RegValue::I32(0), regs);
        }

        // --- No-ops ---
        Op::Nop(_) | Op::Annotate(_) | Op::Bar(_) | Op::MemBar(_) => {}

        // --- Unsupported categories ---
        Op::Tex(_) | Op::Tld(_) | Op::Tld4(_) | Op::Tmml(_) | Op::Txd(_) | Op::Txq(_)
        | Op::SuLd(_) | Op::SuSt(_) | Op::SuAtom(_) => {
            return Err(CpuError::Unsupported("texture operations".into()));
        }
        Op::Vote(_) | Op::Match(_) | Op::Redux(_) | Op::Shfl(_) => {
            return Err(CpuError::Unsupported("warp operations".into()));
        }

        _ => {
            return Err(CpuError::Unsupported(format!("CoralIR op: {op}")));
        }
    }
    Ok(OpEffect::Continue)
}

fn eval_sys_reg(idx: u8, ctx: &InvocationCtx) -> Result<u32, CpuError> {
    use coral_reef_jit_builtins::{
        SR_CTAID_X, SR_CTAID_Y, SR_CTAID_Z, SR_CLOCK_LO, SR_LANEID, SR_NCTAID_X, SR_NCTAID_Y,
        SR_NCTAID_Z, SR_NTID_X, SR_NTID_Y, SR_NTID_Z, SR_TID_X, SR_TID_Y, SR_TID_Z,
    };
    match idx {
        SR_TID_X => Ok(ctx.local_id[0]),
        SR_TID_Y => Ok(ctx.local_id[1]),
        SR_TID_Z => Ok(ctx.local_id[2]),
        SR_CTAID_X => Ok(ctx.workgroup_id[0]),
        SR_CTAID_Y => Ok(ctx.workgroup_id[1]),
        SR_CTAID_Z => Ok(ctx.workgroup_id[2]),
        SR_NTID_X => Ok(ctx.workgroup_size[0]),
        SR_NTID_Y => Ok(ctx.workgroup_size[1]),
        SR_NTID_Z => Ok(ctx.workgroup_size[2]),
        SR_NCTAID_X => Ok(ctx.num_workgroups[0]),
        SR_NCTAID_Y => Ok(ctx.num_workgroups[1]),
        SR_NCTAID_Z => Ok(ctx.num_workgroups[2]),
        SR_LANEID | SR_CLOCK_LO => Ok(0),
        _ => Err(CpuError::Unsupported(format!(
            "system register 0x{idx:02x}"
        ))),
    }
}

/// System register constants (duplicated from coral-reef-jit builtins to avoid
/// circular dependency — the builtins module is the authoritative source).
mod coral_reef_jit_builtins {
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
    pub const SR_CLOCK_LO: u8 = 0x50;
}

// --- Source resolution helpers ---

fn resolve_src_ref(
    src_ref: &SrcRef,
    regs: &HashMap<u32, RegValue>,
    buffers: &[Vec<u8>],
) -> Result<RegValue, CpuError> {
    match src_ref {
        SrcRef::Zero => Ok(RegValue::I32(0)),
        SrcRef::True => Ok(RegValue::Bool(true)),
        SrcRef::False => Ok(RegValue::Bool(false)),
        SrcRef::Imm32(val) => Ok(RegValue::I32(*val)),
        SrcRef::SSA(ssa_ref) => {
            let idx = ssa_ref[0].idx();
            regs.get(&idx)
                .cloned()
                .ok_or_else(|| CpuError::Internal(format!("undefined SSA value {idx}")))
        }
        SrcRef::CBuf(cbuf_ref) => resolve_cbuf(cbuf_ref, buffers),
        SrcRef::Reg(_) => Err(CpuError::Unsupported(
            "register references in pre-RA IR".into(),
        )),
    }
}

/// Each buffer gets a unique 1 MB address region so that `Ld`/`St` addresses
/// can be decoded back into (buffer index, byte offset) pairs.
const BUFFER_STRIDE: u32 = 0x10_0000;

#[expect(clippy::cast_possible_truncation, reason = "buffer index fits in u32")]
fn resolve_cbuf(cbuf: &ir::CBufRef, buffers: &[Vec<u8>]) -> Result<RegValue, CpuError> {
    match &cbuf.buf {
        ir::CBuf::Binding(idx) => {
            if *idx == 0 {
                let binding_idx = (cbuf.offset / 8) as usize;
                if binding_idx < buffers.len() {
                    return Ok(RegValue::I32(binding_idx as u32 * BUFFER_STRIDE));
                }
                return Ok(RegValue::I32(0));
            }
            let user_idx = (*idx as usize).saturating_sub(1);
            if user_idx >= buffers.len() {
                return Ok(RegValue::I32(0));
            }
            let offset = cbuf.offset as usize;
            let buf = &buffers[user_idx];
            if offset + 4 <= buf.len() {
                let val = u32::from_le_bytes([
                    buf[offset],
                    buf[offset + 1],
                    buf[offset + 2],
                    buf[offset + 3],
                ]);
                Ok(RegValue::I32(val))
            } else {
                Ok(RegValue::I32(0))
            }
        }
        _ => Err(CpuError::Unsupported("bindless CBuf".into())),
    }
}

fn apply_src_mod_f32(val: f32, modifier: SrcMod) -> f32 {
    match modifier {
        SrcMod::None => val,
        SrcMod::FNeg | SrcMod::INeg => -val,
        SrcMod::FAbs => val.abs(),
        SrcMod::FNegAbs => -(val.abs()),
        SrcMod::BNot => f32::from_bits(!val.to_bits()),
    }
}

fn apply_src_mod_f64(val: f64, modifier: SrcMod) -> f64 {
    match modifier {
        SrcMod::None => val,
        SrcMod::FNeg | SrcMod::INeg => -val,
        SrcMod::FAbs => val.abs(),
        SrcMod::FNegAbs => -(val.abs()),
        SrcMod::BNot => f64::from_bits(!val.to_bits()),
    }
}

const fn apply_src_mod_i32(val: i32, modifier: SrcMod) -> i32 {
    match modifier {
        SrcMod::INeg | SrcMod::FNeg => val.wrapping_neg(),
        SrcMod::BNot => !val,
        SrcMod::None | SrcMod::FAbs | SrcMod::FNegAbs => val,
    }
}

fn resolve_f32(
    src: &Src,
    regs: &HashMap<u32, RegValue>,
    buffers: &[Vec<u8>],
    ctx: &InvocationCtx,
) -> Result<f32, CpuError> {
    let rv = resolve_reg_value(src, regs, buffers, ctx)?;
    Ok(apply_src_mod_f32(rv.as_f32(), src.modifier))
}

fn resolve_f64(
    src: &Src,
    regs: &HashMap<u32, RegValue>,
    buffers: &[Vec<u8>],
    ctx: &InvocationCtx,
) -> Result<f64, CpuError> {
    let rv = resolve_reg_value(src, regs, buffers, ctx)?;
    Ok(apply_src_mod_f64(rv.as_f64(), src.modifier))
}

fn resolve_f64_unary(
    src: &Src,
    regs: &HashMap<u32, RegValue>,
    buffers: &[Vec<u8>],
    ctx: &InvocationCtx,
) -> Result<f64, CpuError> {
    resolve_f64(src, regs, buffers, ctx)
}

fn resolve_float(
    src: &Src,
    ft: ir::FloatType,
    regs: &HashMap<u32, RegValue>,
    buffers: &[Vec<u8>],
    ctx: &InvocationCtx,
) -> Result<f64, CpuError> {
    match ft {
        ir::FloatType::F64 => resolve_f64(src, regs, buffers, ctx),
        _ => Ok(f64::from(resolve_f32(src, regs, buffers, ctx)?)),
    }
}

fn resolve_i32(
    src: &Src,
    regs: &HashMap<u32, RegValue>,
    buffers: &[Vec<u8>],
    ctx: &InvocationCtx,
) -> Result<i32, CpuError> {
    let rv = resolve_reg_value(src, regs, buffers, ctx)?;
    Ok(apply_src_mod_i32(rv.as_i32(), src.modifier))
}

fn resolve_i32_unary(
    src: &Src,
    regs: &HashMap<u32, RegValue>,
    buffers: &[Vec<u8>],
    ctx: &InvocationCtx,
) -> Result<i32, CpuError> {
    resolve_i32(src, regs, buffers, ctx)
}

#[expect(clippy::cast_sign_loss, reason = "register-width reinterpret i32↔u32")]
fn resolve_u32(
    src: &Src,
    regs: &HashMap<u32, RegValue>,
    buffers: &[Vec<u8>],
    ctx: &InvocationCtx,
) -> Result<u32, CpuError> {
    Ok(resolve_i32(src, regs, buffers, ctx)? as u32)
}

fn resolve_u32_unary(
    src: &Src,
    regs: &HashMap<u32, RegValue>,
    buffers: &[Vec<u8>],
    ctx: &InvocationCtx,
) -> Result<u32, CpuError> {
    resolve_u32(src, regs, buffers, ctx)
}

fn resolve_bool(
    src: &Src,
    regs: &HashMap<u32, RegValue>,
    buffers: &[Vec<u8>],
    ctx: &InvocationCtx,
) -> Result<bool, CpuError> {
    let rv = resolve_reg_value(src, regs, buffers, ctx)?;
    let val = rv.as_bool();
    Ok(match src.modifier {
        SrcMod::BNot => !val,
        _ => val,
    })
}

fn resolve_reg_value(
    src: &Src,
    regs: &HashMap<u32, RegValue>,
    buffers: &[Vec<u8>],
    _ctx: &InvocationCtx,
) -> Result<RegValue, CpuError> {
    resolve_src_ref(&src.reference, regs, buffers)
}

fn resolve_addr(
    src: &Src,
    regs: &HashMap<u32, RegValue>,
    buffers: &[Vec<u8>],
    ctx: &InvocationCtx,
) -> Result<usize, CpuError> {
    let rv = resolve_reg_value(src, regs, buffers, ctx)?;
    Ok(rv.as_u32() as usize)
}

// --- Destination helpers ---

fn def_dst(dst: &Dst, val: RegValue, regs: &mut HashMap<u32, RegValue>) {
    if let Dst::SSA(ssa_ref) = dst {
        regs.insert(ssa_ref[0].idx(), val);
    }
}

fn def_dst_f64(dst: &Dst, val: f64, regs: &mut HashMap<u32, RegValue>) {
    if let Dst::SSA(ssa_ref) = dst {
        if ssa_ref.comps() > 1 {
            regs.insert(ssa_ref[1].idx(), RegValue::F64(val));
        }
        regs.insert(ssa_ref[0].idx(), RegValue::F64(val));
    }
}

// --- Buffer memory helpers ---

/// Decode a synthetic address into (buffer index, byte offset).
const fn decode_addr(addr: usize) -> (usize, usize) {
    let stride = BUFFER_STRIDE as usize;
    (addr / stride, addr % stride)
}

fn read_u32_from_buffers(buffers: &[Vec<u8>], addr: usize) -> u32 {
    let (buf_idx, byte_off) = decode_addr(addr);
    if let Some(buf) = buffers.get(buf_idx) {
        if byte_off + 4 <= buf.len() {
            return u32::from_le_bytes([
                buf[byte_off],
                buf[byte_off + 1],
                buf[byte_off + 2],
                buf[byte_off + 3],
            ]);
        }
    }
    0
}

fn write_u32_to_buffers(buffers: &mut [Vec<u8>], addr: usize, val: u32) {
    let (buf_idx, byte_off) = decode_addr(addr);
    if let Some(buf) = buffers.get_mut(buf_idx) {
        if byte_off + 4 <= buf.len() {
            buf[byte_off..byte_off + 4].copy_from_slice(&val.to_le_bytes());
        }
    }
}
