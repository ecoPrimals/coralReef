// SPDX-License-Identifier: AGPL-3.0-only
//! Expression evaluation, type casting, pointer operations, and binding I/O.
//!
//! Pure evaluation functions separated from the interpreter orchestration
//! in `mod.rs` for file-size compliance and logical cohesion.

use super::{eval_expr, InterpreterState, Value};
use crate::types::CpuError;

pub(super) fn eval_function_argument(
    state: &InterpreterState<'_>,
    index: usize,
) -> Result<Value, CpuError> {
    let arg = state
        .function
        .arguments
        .get(index)
        .ok_or_else(|| CpuError::Internal(format!("FunctionArgument({index}) out of range")))?;

    match arg.binding {
        Some(naga::Binding::BuiltIn(naga::BuiltIn::GlobalInvocationId)) => Ok(Value::Vector(vec![
            Value::U32(state.ctx.global_invocation_id[0]),
            Value::U32(state.ctx.global_invocation_id[1]),
            Value::U32(state.ctx.global_invocation_id[2]),
        ])),
        Some(naga::Binding::BuiltIn(naga::BuiltIn::LocalInvocationId)) => Ok(Value::Vector(vec![
            Value::U32(state.ctx.local_invocation_id[0]),
            Value::U32(state.ctx.local_invocation_id[1]),
            Value::U32(state.ctx.local_invocation_id[2]),
        ])),
        Some(naga::Binding::BuiltIn(naga::BuiltIn::WorkGroupId)) => Ok(Value::Vector(vec![
            Value::U32(state.ctx.workgroup_id[0]),
            Value::U32(state.ctx.workgroup_id[1]),
            Value::U32(state.ctx.workgroup_id[2]),
        ])),
        Some(naga::Binding::BuiltIn(naga::BuiltIn::NumWorkGroups)) => Ok(Value::Vector(vec![
            Value::U32(state.ctx.num_workgroups[0]),
            Value::U32(state.ctx.num_workgroups[1]),
            Value::U32(state.ctx.num_workgroups[2]),
        ])),
        Some(naga::Binding::BuiltIn(ref builtin)) => {
            Err(CpuError::Unsupported(format!("builtin {builtin:?}")))
        }
        Some(naga::Binding::Location { .. }) => {
            Err(CpuError::Unsupported("location binding in compute".into()))
        }
        None => Err(CpuError::Internal(format!(
            "FunctionArgument({index}) has no binding"
        ))),
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "Naga literal down-conversions (u64→u32, i64→i32, AbstractInt→i32) \
              are intentional: WGSL literals may use wide types internally"
)]
pub(super) fn eval_literal(lit: &naga::Literal) -> Value {
    match *lit {
        naga::Literal::F32(v) => Value::F32(v),
        naga::Literal::F64(v) | naga::Literal::AbstractFloat(v) => Value::F64(v),
        naga::Literal::U32(v) => Value::U32(v),
        naga::Literal::I32(v) => Value::I32(v),
        naga::Literal::Bool(v) => Value::Bool(v),
        naga::Literal::U64(v) => Value::U32(v as u32),
        naga::Literal::I64(v) | naga::Literal::AbstractInt(v) => Value::I32(v as i32),
        naga::Literal::F16(v) => Value::F32(f32::from(v)),
    }
}

pub(super) fn eval_const_expr(
    module: &naga::Module,
    handle: naga::Handle<naga::Expression>,
) -> Result<Value, CpuError> {
    let expr = &module.global_expressions[handle];
    match *expr {
        naga::Expression::Literal(ref lit) => Ok(eval_literal(lit)),
        naga::Expression::ZeroValue(ty) => Ok(default_value_for_type(module, ty)),
        naga::Expression::Compose { ref components, .. } => {
            let vals: Vec<Value> = components
                .iter()
                .map(|&c| eval_const_expr(module, c))
                .collect::<Result<_, _>>()?;
            Ok(Value::Vector(vals))
        }
        _ => Err(CpuError::Unsupported(format!("const expr: {expr:?}"))),
    }
}

#[expect(
    clippy::float_cmp,
    reason = "IEEE 754 exact comparison matches WGSL specification semantics"
)]
pub(super) fn eval_binary(
    op: naga::BinaryOperator,
    left: &Value,
    right: &Value,
) -> Result<Value, CpuError> {
    use naga::BinaryOperator as B;
    match (left, right) {
        (Value::F32(a), Value::F32(b)) => Ok(Value::F32(match op {
            B::Add => a + b,
            B::Subtract => a - b,
            B::Multiply => a * b,
            B::Divide => a / b,
            B::Modulo => a % b,
            B::Equal => return Ok(Value::Bool(a == b)),
            B::NotEqual => return Ok(Value::Bool(a != b)),
            B::Less => return Ok(Value::Bool(a < b)),
            B::LessEqual => return Ok(Value::Bool(a <= b)),
            B::Greater => return Ok(Value::Bool(a > b)),
            B::GreaterEqual => return Ok(Value::Bool(a >= b)),
            _ => return Err(CpuError::Unsupported(format!("f32 binary op {op:?}"))),
        })),
        (Value::F64(a), Value::F64(b)) => Ok(Value::F64(match op {
            B::Add => a + b,
            B::Subtract => a - b,
            B::Multiply => a * b,
            B::Divide => a / b,
            B::Modulo => a % b,
            B::Equal => return Ok(Value::Bool(a == b)),
            B::NotEqual => return Ok(Value::Bool(a != b)),
            B::Less => return Ok(Value::Bool(a < b)),
            B::LessEqual => return Ok(Value::Bool(a <= b)),
            B::Greater => return Ok(Value::Bool(a > b)),
            B::GreaterEqual => return Ok(Value::Bool(a >= b)),
            _ => return Err(CpuError::Unsupported(format!("f64 binary op {op:?}"))),
        })),
        (Value::U32(a), Value::U32(b)) => Ok(Value::U32(match op {
            B::Add => a.wrapping_add(*b),
            B::Subtract => a.wrapping_sub(*b),
            B::Multiply => a.wrapping_mul(*b),
            B::Divide if *b != 0 => a / b,
            B::Modulo if *b != 0 => a % b,
            B::Divide | B::Modulo => 0,
            B::And => a & b,
            B::InclusiveOr => a | b,
            B::ExclusiveOr => a ^ b,
            B::ShiftLeft => a.wrapping_shl(*b),
            B::ShiftRight => a.wrapping_shr(*b),
            B::Equal => return Ok(Value::Bool(a == b)),
            B::NotEqual => return Ok(Value::Bool(a != b)),
            B::Less => return Ok(Value::Bool(a < b)),
            B::LessEqual => return Ok(Value::Bool(a <= b)),
            B::Greater => return Ok(Value::Bool(a > b)),
            B::GreaterEqual => return Ok(Value::Bool(a >= b)),
            _ => return Err(CpuError::Unsupported(format!("u32 binary op {op:?}"))),
        })),
        #[expect(
            clippy::cast_sign_loss,
            reason = "WGSL shift amounts use u32 semantics"
        )]
        (Value::I32(a), Value::I32(b)) => Ok(Value::I32(match op {
            B::Add => a.wrapping_add(*b),
            B::Subtract => a.wrapping_sub(*b),
            B::Multiply => a.wrapping_mul(*b),
            B::Divide if *b != 0 => a / b,
            B::Modulo if *b != 0 => a % b,
            B::Divide | B::Modulo => 0,
            B::And => a & b,
            B::InclusiveOr => a | b,
            B::ExclusiveOr => a ^ b,
            B::ShiftLeft => a.wrapping_shl(*b as u32),
            B::ShiftRight => a.wrapping_shr(*b as u32),
            B::Equal => return Ok(Value::Bool(a == b)),
            B::NotEqual => return Ok(Value::Bool(a != b)),
            B::Less => return Ok(Value::Bool(a < b)),
            B::LessEqual => return Ok(Value::Bool(a <= b)),
            B::Greater => return Ok(Value::Bool(a > b)),
            B::GreaterEqual => return Ok(Value::Bool(a >= b)),
            _ => return Err(CpuError::Unsupported(format!("i32 binary op {op:?}"))),
        })),
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(match op {
            B::LogicalAnd => *a && *b,
            B::LogicalOr => *a || *b,
            B::Equal => a == b,
            B::NotEqual => a != b,
            _ => return Err(CpuError::Unsupported(format!("bool binary op {op:?}"))),
        })),
        (Value::Vector(a), Value::Vector(b)) if a.len() == b.len() => {
            let components: Vec<Value> = a
                .iter()
                .zip(b.iter())
                .map(|(l, r)| eval_binary(op, l, r))
                .collect::<Result<_, _>>()?;
            Ok(Value::Vector(components))
        }
        _ => Err(CpuError::Unsupported(format!(
            "binary op {op:?} on mismatched types"
        ))),
    }
}

pub(super) fn eval_unary(op: naga::UnaryOperator, val: &Value) -> Result<Value, CpuError> {
    use naga::UnaryOperator as U;
    match (op, val) {
        (U::Negate, Value::F32(v)) => Ok(Value::F32(-v)),
        (U::Negate, Value::F64(v)) => Ok(Value::F64(-v)),
        (U::Negate, Value::I32(v)) => Ok(Value::I32(-v)),
        (U::BitwiseNot, Value::U32(v)) => Ok(Value::U32(!v)),
        (U::BitwiseNot, Value::I32(v)) => Ok(Value::I32(!v)),
        (U::LogicalNot, Value::Bool(v)) => Ok(Value::Bool(!v)),
        (_, Value::Vector(v)) => {
            let components: Vec<Value> = v
                .iter()
                .map(|c| eval_unary(op, c))
                .collect::<Result<_, _>>()?;
            Ok(Value::Vector(components))
        }
        _ => Err(CpuError::Unsupported(format!("unary op {op:?}"))),
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "f64→f32 narrowing is intentional for f32 scalar results"
)]
pub(super) fn eval_math(
    fun: naga::MathFunction,
    primary: &Value,
    secondary: Option<&Value>,
    tertiary: Option<&Value>,
) -> Result<Value, CpuError> {
    if let Value::Vector(components) = primary {
        let sec_vec =
            secondary.and_then(|v| if let Value::Vector(bv) = v { Some(bv.as_slice()) } else { None });
        let ter_vec =
            tertiary.and_then(|v| if let Value::Vector(cv) = v { Some(cv.as_slice()) } else { None });

        let results: Vec<Value> = components
            .iter()
            .enumerate()
            .map(|(i, comp)| {
                let si = sec_vec.and_then(|sv| sv.get(i)).or(secondary);
                let ti = ter_vec.and_then(|tv| tv.get(i)).or(tertiary);
                eval_math(fun, comp, si, ti)
            })
            .collect::<Result<_, _>>()?;
        return Ok(Value::Vector(results));
    }

    match primary {
        Value::F32(val) => {
            let wide = f64::from(*val);
            let result = eval_math_f64(fun, wide, secondary, tertiary)?;
            Ok(Value::F32(result as f32))
        }
        Value::F64(val) => {
            let result = eval_math_f64(fun, *val, secondary, tertiary)?;
            Ok(Value::F64(result))
        }
        Value::U32(val) => eval_math_u32(fun, *val, secondary),
        Value::I32(val) => eval_math_i32(fun, *val, secondary),
        _ => Err(CpuError::Unsupported(format!(
            "math {fun:?} on {primary:?}"
        ))),
    }
}

fn eval_math_f64(
    fun: naga::MathFunction,
    val: f64,
    arg_b: Option<&Value>,
    arg_c: Option<&Value>,
) -> Result<f64, CpuError> {
    use naga::MathFunction as M;
    Ok(match fun {
        M::Abs => val.abs(),
        M::Floor => val.floor(),
        M::Ceil => val.ceil(),
        M::Round => val.round(),
        M::Fract => val.fract(),
        M::Sqrt => val.sqrt(),
        M::InverseSqrt => 1.0 / val.sqrt(),
        M::Sin => val.sin(),
        M::Cos => val.cos(),
        M::Tan => val.tan(),
        M::Asin => val.asin(),
        M::Acos => val.acos(),
        M::Atan => val.atan(),
        M::Sinh => val.sinh(),
        M::Cosh => val.cosh(),
        M::Tanh => val.tanh(),
        M::Exp => val.exp(),
        M::Exp2 => val.exp2(),
        M::Log => val.ln(),
        M::Log2 => val.log2(),
        M::Sign => {
            if val > 0.0 {
                1.0
            } else if val < 0.0 {
                -1.0
            } else {
                0.0
            }
        }
        M::Saturate => val.clamp(0.0, 1.0),
        M::Min => {
            let rhs = arg_b
                .ok_or_else(|| CpuError::Internal("min needs 2 args".into()))?
                .as_f64()?;
            val.min(rhs)
        }
        M::Max => {
            let rhs = arg_b
                .ok_or_else(|| CpuError::Internal("max needs 2 args".into()))?
                .as_f64()?;
            val.max(rhs)
        }
        M::Clamp => {
            let lo = arg_b
                .ok_or_else(|| CpuError::Internal("clamp needs 3 args".into()))?
                .as_f64()?;
            let hi = arg_c
                .ok_or_else(|| CpuError::Internal("clamp needs 3 args".into()))?
                .as_f64()?;
            val.clamp(lo, hi)
        }
        M::Mix => {
            let y = arg_b
                .ok_or_else(|| CpuError::Internal("mix needs 3 args".into()))?
                .as_f64()?;
            let t = arg_c
                .ok_or_else(|| CpuError::Internal("mix needs 3 args".into()))?
                .as_f64()?;
            val.mul_add(1.0 - t, y * t)
        }
        M::Step => {
            let x = arg_b
                .ok_or_else(|| CpuError::Internal("step needs 2 args".into()))?
                .as_f64()?;
            if x < val { 0.0 } else { 1.0 }
        }
        M::SmoothStep => {
            let high = arg_b
                .ok_or_else(|| CpuError::Internal("smoothstep needs 3 args".into()))?
                .as_f64()?;
            let x = arg_c
                .ok_or_else(|| CpuError::Internal("smoothstep needs 3 args".into()))?
                .as_f64()?;
            let t = ((x - val) / (high - val)).clamp(0.0, 1.0);
            t * t * 2.0f64.mul_add(-t, 3.0)
        }
        M::Pow => {
            let exp = arg_b
                .ok_or_else(|| CpuError::Internal("pow needs 2 args".into()))?
                .as_f64()?;
            val.powf(exp)
        }
        M::Atan2 => {
            let x = arg_b
                .ok_or_else(|| CpuError::Internal("atan2 needs 2 args".into()))?
                .as_f64()?;
            val.atan2(x)
        }
        _ => {
            return Err(CpuError::Unsupported(format!(
                "math function {fun:?} on f64"
            )))
        }
    })
}

fn eval_math_u32(
    fun: naga::MathFunction,
    val: u32,
    arg_b: Option<&Value>,
) -> Result<Value, CpuError> {
    use naga::MathFunction as M;
    Ok(Value::U32(match fun {
        M::Min => {
            let rhs = arg_b
                .ok_or_else(|| CpuError::Internal("min needs 2 args".into()))?
                .as_u32()?;
            val.min(rhs)
        }
        M::Max => {
            let rhs = arg_b
                .ok_or_else(|| CpuError::Internal("max needs 2 args".into()))?
                .as_u32()?;
            val.max(rhs)
        }
        M::CountOneBits => val.count_ones(),
        M::ReverseBits => val.reverse_bits(),
        M::FirstTrailingBit => {
            if val == 0 {
                u32::MAX
            } else {
                val.trailing_zeros()
            }
        }
        M::FirstLeadingBit => {
            if val == 0 {
                u32::MAX
            } else {
                31 - val.leading_zeros()
            }
        }
        _ => return Err(CpuError::Unsupported(format!("math {fun:?} on u32"))),
    }))
}

fn eval_math_i32(
    fun: naga::MathFunction,
    val: i32,
    arg_b: Option<&Value>,
) -> Result<Value, CpuError> {
    use naga::MathFunction as M;
    Ok(Value::I32(match fun {
        M::Abs => val.abs(),
        M::Min => {
            let rhs = arg_b
                .ok_or_else(|| CpuError::Internal("min needs 2 args".into()))?
                .as_i32()?;
            val.min(rhs)
        }
        M::Max => {
            let rhs = arg_b
                .ok_or_else(|| CpuError::Internal("max needs 2 args".into()))?
                .as_i32()?;
            val.max(rhs)
        }
        M::Sign => val.signum(),
        _ => return Err(CpuError::Unsupported(format!("math {fun:?} on i32"))),
    }))
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "eval_cast implements WGSL type coercion: all casts are by spec"
)]
pub(super) fn eval_cast(val: &Value, kind: naga::ScalarKind) -> Result<Value, CpuError> {
    match kind {
        naga::ScalarKind::Float => match val {
            Value::F32(v) => Ok(Value::F32(*v)),
            Value::F64(v) => Ok(Value::F64(*v)),
            Value::U32(v) => Ok(Value::F32(*v as f32)),
            Value::I32(v) => Ok(Value::F32(*v as f32)),
            Value::Bool(v) => Ok(Value::F32(if *v { 1.0 } else { 0.0 })),
            Value::Vector(v) => {
                let casted: Vec<Value> =
                    v.iter().map(|c| eval_cast(c, kind)).collect::<Result<_, _>>()?;
                Ok(Value::Vector(casted))
            }
        },
        naga::ScalarKind::Uint => match val {
            Value::U32(v) => Ok(Value::U32(*v)),
            Value::I32(v) => Ok(Value::U32(*v as u32)),
            Value::F32(v) => Ok(Value::U32(*v as u32)),
            Value::F64(v) => Ok(Value::U32(*v as u32)),
            Value::Bool(v) => Ok(Value::U32(u32::from(*v))),
            Value::Vector(v) => {
                let casted: Vec<Value> =
                    v.iter().map(|c| eval_cast(c, kind)).collect::<Result<_, _>>()?;
                Ok(Value::Vector(casted))
            }
        },
        naga::ScalarKind::Sint => match val {
            Value::I32(v) => Ok(Value::I32(*v)),
            Value::U32(v) => Ok(Value::I32(*v as i32)),
            Value::F32(v) => Ok(Value::I32(*v as i32)),
            Value::F64(v) => Ok(Value::I32(*v as i32)),
            Value::Bool(v) => Ok(Value::I32(i32::from(*v))),
            Value::Vector(v) => {
                let casted: Vec<Value> =
                    v.iter().map(|c| eval_cast(c, kind)).collect::<Result<_, _>>()?;
                Ok(Value::Vector(casted))
            }
        },
        naga::ScalarKind::Bool => match val {
            Value::Bool(v) => Ok(Value::Bool(*v)),
            Value::U32(v) => Ok(Value::Bool(*v != 0)),
            Value::I32(v) => Ok(Value::Bool(*v != 0)),
            Value::F32(v) => Ok(Value::Bool(*v != 0.0)),
            Value::F64(v) => Ok(Value::Bool(*v != 0.0)),
            Value::Vector(v) => {
                let casted: Vec<Value> =
                    v.iter().map(|c| eval_cast(c, kind)).collect::<Result<_, _>>()?;
                Ok(Value::Vector(casted))
            }
        },
        _ => Err(CpuError::Unsupported(format!("cast to {kind:?}"))),
    }
}

pub(super) fn eval_load(
    state: &InterpreterState<'_>,
    pointer: naga::Handle<naga::Expression>,
) -> Result<Value, CpuError> {
    let expr = &state.function.expressions[pointer];
    match *expr {
        naga::Expression::LocalVariable(var) => state
            .locals
            .get(&var)
            .cloned()
            .ok_or_else(|| CpuError::Internal("load from uninitialized local".into())),
        naga::Expression::GlobalVariable(var) => {
            let gv = &state.module.global_variables[var];
            gv.binding.as_ref().map_or_else(
                || Ok(default_value_for_type(state.module, gv.ty)),
                |rb| eval_global_binding(state, *rb, gv),
            )
        }
        naga::Expression::AccessIndex { base, index } => {
            let base_val = eval_load(state, base)?;
            match base_val {
                Value::Vector(ref v) => v
                    .get(index as usize)
                    .cloned()
                    .ok_or_else(|| CpuError::Internal(format!("load index {index} out of range"))),
                _ => Err(CpuError::Internal("load AccessIndex on non-vector".into())),
            }
        }
        _ => eval_expr(state, pointer),
    }
}

pub(super) fn store_to_pointer(
    state: &mut InterpreterState<'_>,
    pointer: naga::Handle<naga::Expression>,
    value: &Value,
) -> Result<(), CpuError> {
    let expr = &state.function.expressions[pointer];
    match *expr {
        naga::Expression::LocalVariable(var) => {
            state.locals.insert(var, value.clone());
            Ok(())
        }
        naga::Expression::GlobalVariable(var) => {
            let gv = &state.module.global_variables[var];
            gv.binding
                .as_ref()
                .map_or(Ok(()), |rb| store_to_global_binding(state, *rb, gv, value))
        }
        naga::Expression::AccessIndex { base, index } => {
            let base_expr = &state.function.expressions[base];
            match *base_expr {
                naga::Expression::GlobalVariable(var) => {
                    let gv = &state.module.global_variables[var];
                    if let Some(ref rb) = gv.binding {
                        let byte_offset = index as usize * 4;
                        match value {
                            Value::F32(v) => {
                                state.memory.write_f32(rb.group, rb.binding, byte_offset, *v)
                            }
                            Value::U32(v) => {
                                state.memory.write_u32(rb.group, rb.binding, byte_offset, *v)
                            }
                            _ => Err(CpuError::Unsupported(
                                "store non-scalar to binding element".into(),
                            )),
                        }
                    } else {
                        Ok(())
                    }
                }
                naga::Expression::LocalVariable(var) => {
                    if let Some(Value::Vector(v)) = state.locals.get_mut(&var) {
                        if (index as usize) < v.len() {
                            v[index as usize] = value.clone();
                        }
                    }
                    Ok(())
                }
                _ => Err(CpuError::Unsupported("store to complex pointer".into())),
            }
        }
        naga::Expression::Access { base, index } => {
            let idx = eval_expr(state, index)?.as_u32()? as usize;
            let base_expr = &state.function.expressions[base];
            match *base_expr {
                naga::Expression::GlobalVariable(var) => {
                    let gv = &state.module.global_variables[var];
                    if let Some(ref rb) = gv.binding {
                        let byte_offset = idx * 4;
                        match value {
                            Value::F32(v) => {
                                state.memory.write_f32(rb.group, rb.binding, byte_offset, *v)
                            }
                            Value::U32(v) => {
                                state.memory.write_u32(rb.group, rb.binding, byte_offset, *v)
                            }
                            _ => Err(CpuError::Unsupported(
                                "store non-scalar to array element".into(),
                            )),
                        }
                    } else {
                        Ok(())
                    }
                }
                _ => Err(CpuError::Unsupported(
                    "store via dynamic Access on non-global".into(),
                )),
            }
        }
        _ => Err(CpuError::Unsupported(format!(
            "store to expression: {expr:?}"
        ))),
    }
}

pub(super) fn eval_global_binding(
    state: &InterpreterState<'_>,
    rb: naga::ResourceBinding,
    _gv: &naga::GlobalVariable,
) -> Result<Value, CpuError> {
    let buf = state
        .memory
        .buffers
        .get(&(rb.group, rb.binding))
        .ok_or(CpuError::MissingBinding {
            group: rb.group,
            binding: rb.binding,
        })?;
    let element_count = buf.len() / 4;
    let mut vals = Vec::with_capacity(element_count);
    for i in 0..element_count {
        let offset = i * 4;
        vals.push(Value::F32(f32::from_le_bytes([
            buf[offset],
            buf[offset + 1],
            buf[offset + 2],
            buf[offset + 3],
        ])));
    }
    if vals.len() == 1 {
        Ok(vals.into_iter().next().expect("checked"))
    } else {
        Ok(Value::Vector(vals))
    }
}

fn store_to_global_binding(
    state: &mut InterpreterState<'_>,
    rb: naga::ResourceBinding,
    _gv: &naga::GlobalVariable,
    value: &Value,
) -> Result<(), CpuError> {
    match value {
        Value::F32(v) => state.memory.write_f32(rb.group, rb.binding, 0, *v),
        Value::U32(v) => state.memory.write_u32(rb.group, rb.binding, 0, *v),
        Value::Vector(vals) => {
            for (i, v) in vals.iter().enumerate() {
                let offset = i * 4;
                match v {
                    Value::F32(f) => state.memory.write_f32(rb.group, rb.binding, offset, *f)?,
                    Value::U32(u) => state.memory.write_u32(rb.group, rb.binding, offset, *u)?,
                    _ => return Err(CpuError::Unsupported("store non-scalar element".into())),
                }
            }
            Ok(())
        }
        _ => Err(CpuError::Unsupported(format!(
            "store value type: {value:?}"
        ))),
    }
}

pub(super) fn default_value_for_type(
    module: &naga::Module,
    ty: naga::Handle<naga::Type>,
) -> Value {
    match module.types[ty].inner {
        naga::TypeInner::Scalar(s) => match s.kind {
            naga::ScalarKind::Float if s.width == 8 => Value::F64(0.0),
            naga::ScalarKind::Float => Value::F32(0.0),
            naga::ScalarKind::Sint => Value::I32(0),
            naga::ScalarKind::Bool => Value::Bool(false),
            _ => Value::U32(0),
        },
        naga::TypeInner::Vector { size, scalar } => {
            let count = size as usize;
            let elem = match scalar.kind {
                naga::ScalarKind::Float if scalar.width == 8 => Value::F64(0.0),
                naga::ScalarKind::Float => Value::F32(0.0),
                naga::ScalarKind::Sint => Value::I32(0),
                naga::ScalarKind::Bool => Value::Bool(false),
                _ => Value::U32(0),
            };
            Value::Vector(vec![elem; count])
        }
        _ => Value::U32(0),
    }
}
