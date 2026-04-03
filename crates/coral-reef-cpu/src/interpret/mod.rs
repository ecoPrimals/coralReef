// SPDX-License-Identifier: AGPL-3.0-only
//! Naga IR tree-walk interpreter for CPU execution of WGSL compute shaders.
//!
//! Walks `naga::Module` statements and expressions directly — no code generation.
//! Native `f64` arithmetic is the primary motivation (GPUs often lack real f64).

mod eval;

use std::collections::HashMap;

use crate::types::{
    BindingData, BindingUsage, CpuError, ExecuteCpuRequest, ExecuteCpuResponse, UniformData,
};
use eval::{
    default_value_for_type, eval_binary, eval_cast, eval_const_expr, eval_function_argument,
    eval_global_binding, eval_literal, eval_load, eval_math, eval_unary, store_to_pointer,
};

/// Execute a WGSL compute shader on the CPU.
///
/// Parses the WGSL source, locates the entry point, and interprets the naga IR
/// for each workgroup invocation. Returns the modified bindings.
///
/// # Errors
///
/// Returns [`CpuError`] on parse failures, missing entry points, or unsupported IR.
pub fn execute_cpu(request: &ExecuteCpuRequest) -> Result<ExecuteCpuResponse, CpuError> {
    let module = parse_wgsl(&request.wgsl_source)?;
    let info = validate_module(&module)?;
    let ep_index = find_entry_point(&module, request.entry_point.as_deref())?;

    let mut memory = BindingMemory::from_request(&request.bindings, &request.uniforms);

    let start = std::time::Instant::now();
    let ep = &module.entry_points[ep_index];
    let workgroup_size = ep.workgroup_size;

    for wg_z in 0..request.workgroups[2] {
        for wg_y in 0..request.workgroups[1] {
            for wg_x in 0..request.workgroups[0] {
                for lz in 0..workgroup_size[2] {
                    for ly in 0..workgroup_size[1] {
                        for lx in 0..workgroup_size[0] {
                            let global_id = [
                                wg_x * workgroup_size[0] + lx,
                                wg_y * workgroup_size[1] + ly,
                                wg_z * workgroup_size[2] + lz,
                            ];
                            let ctx = InvocationContext {
                                global_invocation_id: global_id,
                                local_invocation_id: [lx, ly, lz],
                                workgroup_id: [wg_x, wg_y, wg_z],
                                num_workgroups: request.workgroups,
                            };
                            interpret_function(&module, &info, &ep.function, &ctx, &mut memory)?;
                        }
                    }
                }
            }
        }
    }
    let elapsed = start.elapsed();

    let output_bindings = memory.into_output_bindings();

    Ok(ExecuteCpuResponse {
        bindings: output_bindings,
        execution_time_ns: u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX),
        strategy_used: None,
        cache_hit: false,
        revalidated: false,
    })
}

fn parse_wgsl(source: &str) -> Result<naga::Module, CpuError> {
    naga::front::wgsl::parse_str(source).map_err(|e| CpuError::Parse(format!("{e}")))
}

fn validate_module(module: &naga::Module) -> Result<naga::valid::ModuleInfo, CpuError> {
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(module)
        .map_err(|e| CpuError::Validation(format!("{e}")))
}

fn find_entry_point(module: &naga::Module, name: Option<&str>) -> Result<usize, CpuError> {
    name.map_or_else(
        || {
            module
                .entry_points
                .iter()
                .position(|ep| ep.stage == naga::ShaderStage::Compute)
                .ok_or_else(|| CpuError::EntryPointNotFound("(first @compute)".to_string()))
        },
        |entry_name| {
            module
                .entry_points
                .iter()
                .position(|ep| ep.name == entry_name && ep.stage == naga::ShaderStage::Compute)
                .ok_or_else(|| CpuError::EntryPointNotFound(entry_name.to_string()))
        },
    )
}

/// Per-invocation built-in values.
pub(crate) struct InvocationContext {
    pub(crate) global_invocation_id: [u32; 3],
    pub(crate) local_invocation_id: [u32; 3],
    pub(crate) workgroup_id: [u32; 3],
    pub(crate) num_workgroups: [u32; 3],
}

/// Backing store for all bindings: keyed by `(group, binding)`.
pub(crate) struct BindingMemory {
    pub(crate) buffers: HashMap<(u32, u32), Vec<u8>>,
    usage: HashMap<(u32, u32), BindingUsage>,
}

impl BindingMemory {
    fn from_request(bindings: &[BindingData], uniforms: &[UniformData]) -> Self {
        let mut buffers = HashMap::new();
        let mut usage = HashMap::new();
        for b in bindings {
            buffers.insert((b.group, b.binding), b.data.to_vec());
            usage.insert((b.group, b.binding), b.usage);
        }
        for u in uniforms {
            buffers.insert((u.group, u.binding), u.data.to_vec());
            usage.insert((u.group, u.binding), BindingUsage::ReadOnly);
        }
        Self { buffers, usage }
    }

    pub(crate) fn read_f32(
        &self,
        group: u32,
        binding: u32,
        byte_offset: usize,
    ) -> Result<f32, CpuError> {
        let buf = self
            .buffers
            .get(&(group, binding))
            .ok_or(CpuError::MissingBinding { group, binding })?;
        if byte_offset + 4 > buf.len() {
            return Err(CpuError::Internal(format!(
                "out of bounds read at offset {byte_offset} in binding ({group}, {binding}) of len {}",
                buf.len()
            )));
        }
        Ok(f32::from_le_bytes([
            buf[byte_offset],
            buf[byte_offset + 1],
            buf[byte_offset + 2],
            buf[byte_offset + 3],
        ]))
    }

    pub(crate) fn write_f32(
        &mut self,
        group: u32,
        binding: u32,
        byte_offset: usize,
        value: f32,
    ) -> Result<(), CpuError> {
        let buf = self
            .buffers
            .get_mut(&(group, binding))
            .ok_or(CpuError::MissingBinding { group, binding })?;
        if byte_offset + 4 > buf.len() {
            return Err(CpuError::Internal(format!(
                "out of bounds write at offset {byte_offset} in binding ({group}, {binding}) of len {}",
                buf.len()
            )));
        }
        let bytes = value.to_le_bytes();
        buf[byte_offset..byte_offset + 4].copy_from_slice(&bytes);
        Ok(())
    }

    pub(crate) fn write_u32(
        &mut self,
        group: u32,
        binding: u32,
        byte_offset: usize,
        value: u32,
    ) -> Result<(), CpuError> {
        let buf = self
            .buffers
            .get_mut(&(group, binding))
            .ok_or(CpuError::MissingBinding { group, binding })?;
        if byte_offset + 4 > buf.len() {
            return Err(CpuError::Internal(format!(
                "out of bounds write at offset {byte_offset} in binding ({group}, {binding}) of len {}",
                buf.len()
            )));
        }
        let bytes = value.to_le_bytes();
        buf[byte_offset..byte_offset + 4].copy_from_slice(&bytes);
        Ok(())
    }

    fn into_output_bindings(self) -> Vec<BindingData> {
        let mut result: Vec<BindingData> = self
            .buffers
            .into_iter()
            .filter(|(key, _)| {
                self.usage
                    .get(key)
                    .is_some_and(|u| matches!(u, BindingUsage::ReadWrite | BindingUsage::WriteOnly))
            })
            .map(|((group, binding), data)| BindingData {
                group,
                binding,
                data: bytes::Bytes::from(data),
                usage: self
                    .usage
                    .get(&(group, binding))
                    .copied()
                    .unwrap_or_default(),
            })
            .collect();
        result.sort_by_key(|b| (b.group, b.binding));
        result
    }
}

/// Interpreter value — supports the scalar types needed for compute shaders.
#[derive(Debug, Clone)]
pub(crate) enum Value {
    /// 32-bit float.
    F32(f32),
    /// 64-bit float (the whole point of CPU execution).
    F64(f64),
    /// Unsigned 32-bit integer.
    U32(u32),
    /// Signed 32-bit integer.
    I32(i32),
    /// Boolean.
    Bool(bool),
    /// Vector (up to 4 components).
    Vector(Vec<Self>),
}

impl Value {
    pub(crate) fn as_f64(&self) -> Result<f64, CpuError> {
        match self {
            Self::F64(v) => Ok(*v),
            Self::F32(v) => Ok(f64::from(*v)),
            Self::U32(v) => Ok(f64::from(*v)),
            Self::I32(v) => Ok(f64::from(*v)),
            _ => Err(CpuError::Internal("expected f64-compatible value".into())),
        }
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "WGSL bitcast semantics: i32→u32 reinterprets bits"
    )]
    pub(crate) fn as_u32(&self) -> Result<u32, CpuError> {
        match self {
            Self::U32(v) => Ok(*v),
            Self::I32(v) => Ok(*v as u32),
            Self::Bool(b) => Ok(u32::from(*b)),
            _ => Err(CpuError::Internal("expected u32-compatible value".into())),
        }
    }

    #[expect(
        clippy::cast_possible_wrap,
        reason = "WGSL bitcast semantics: u32→i32 reinterprets bits"
    )]
    pub(crate) fn as_i32(&self) -> Result<i32, CpuError> {
        match self {
            Self::I32(v) => Ok(*v),
            Self::U32(v) => Ok(*v as i32),
            _ => Err(CpuError::Internal("expected i32-compatible value".into())),
        }
    }

    pub(crate) fn as_bool(&self) -> Result<bool, CpuError> {
        match self {
            Self::Bool(v) => Ok(*v),
            Self::U32(v) => Ok(*v != 0),
            Self::I32(v) => Ok(*v != 0),
            _ => Err(CpuError::Internal("expected bool-compatible value".into())),
        }
    }
}

/// Per-invocation interpreter state for a single function call.
pub(crate) struct InterpreterState<'a> {
    pub(crate) module: &'a naga::Module,
    pub(crate) info: &'a naga::valid::ModuleInfo,
    pub(crate) function: &'a naga::Function,
    pub(crate) ctx: &'a InvocationContext,
    pub(crate) memory: &'a mut BindingMemory,
    pub(crate) locals: HashMap<naga::Handle<naga::LocalVariable>, Value>,
    pub(crate) expressions: HashMap<naga::Handle<naga::Expression>, Value>,
}

fn interpret_function(
    module: &naga::Module,
    info: &naga::valid::ModuleInfo,
    function: &naga::Function,
    ctx: &InvocationContext,
    memory: &mut BindingMemory,
) -> Result<Option<Value>, CpuError> {
    let mut state = InterpreterState {
        module,
        info,
        function,
        ctx,
        memory,
        locals: HashMap::new(),
        expressions: HashMap::new(),
    };

    for (handle, local) in function.local_variables.iter() {
        let init = local
            .init
            .and_then(|h| eval_expr(&state, h).ok())
            .unwrap_or_else(|| default_value_for_type(module, local.ty));
        state.locals.insert(handle, init);
    }

    execute_block(&mut state, &function.body)
}

fn execute_block(
    state: &mut InterpreterState<'_>,
    block: &naga::Block,
) -> Result<Option<Value>, CpuError> {
    for stmt in block {
        if let Some(ret) = execute_statement(state, stmt)? {
            return Ok(Some(ret));
        }
    }
    Ok(None)
}

fn execute_statement(
    state: &mut InterpreterState<'_>,
    stmt: &naga::Statement,
) -> Result<Option<Value>, CpuError> {
    use naga::Statement;
    match *stmt {
        Statement::Emit(ref range) => {
            for handle in range.clone() {
                let val = eval_expr(state, handle)?;
                state.expressions.insert(handle, val);
            }
        }
        Statement::Store { pointer, value } => {
            let val = eval_expr(state, value)?;
            store_to_pointer(state, pointer, &val)?;
        }
        Statement::Return { value } => {
            let ret = value.map(|v| eval_expr(state, v)).transpose()?;
            return Ok(ret.or(Some(Value::Bool(true))));
        }
        Statement::Block(ref block) => {
            if let Some(v) = execute_block(state, block)? {
                return Ok(Some(v));
            }
        }
        Statement::If {
            condition,
            ref accept,
            ref reject,
        } => {
            let cond = eval_expr(state, condition)?.as_bool()?;
            let branch = if cond { accept } else { reject };
            if let Some(v) = execute_block(state, branch)? {
                return Ok(Some(v));
            }
        }
        Statement::Loop {
            ref body,
            ref continuing,
            break_if,
        } => {
            const MAX_ITERATIONS: usize = 1_000_000;
            for _ in 0..MAX_ITERATIONS {
                if let Some(v) = execute_block(state, body)? {
                    return Ok(Some(v));
                }
                if let Some(v) = execute_block(state, continuing)? {
                    return Ok(Some(v));
                }
                if let Some(cond_handle) = break_if {
                    let cond = eval_expr(state, cond_handle)?.as_bool()?;
                    if cond {
                        break;
                    }
                }
            }
        }
        Statement::Call {
            function,
            ref arguments,
            result,
        } => {
            let called = &state.module.functions[function];
            let _arg_values: Vec<Value> = arguments
                .iter()
                .map(|&arg_handle| eval_expr(state, arg_handle))
                .collect::<Result<_, _>>()?;
            let ret =
                interpret_function(state.module, state.info, called, state.ctx, state.memory)?;
            if let Some(res_handle) = result {
                state
                    .expressions
                    .insert(res_handle, ret.unwrap_or(Value::Bool(false)));
            }
        }
        Statement::Switch { .. } => {
            tracing::warn!("switch statements not yet implemented in CPU interpreter");
        }
        _ => {}
    }
    Ok(None)
}

pub(crate) fn eval_expr(
    state: &InterpreterState<'_>,
    handle: naga::Handle<naga::Expression>,
) -> Result<Value, CpuError> {
    if let Some(cached) = state.expressions.get(&handle) {
        return Ok(cached.clone());
    }

    let expr = &state.function.expressions[handle];
    match *expr {
        naga::Expression::Literal(ref lit) => Ok(eval_literal(lit)),
        naga::Expression::Constant(c) => {
            let constant = &state.module.constants[c];
            eval_const_expr(state.module, constant.init)
        }
        naga::Expression::ZeroValue(ty) => Ok(default_value_for_type(state.module, ty)),
        naga::Expression::Compose { ty, ref components } => {
            let vals: Vec<Value> = components
                .iter()
                .map(|&c| eval_expr(state, c))
                .collect::<Result<_, _>>()?;
            let inner = &state.module.types[ty].inner;
            if let naga::TypeInner::Vector { .. } = *inner {
                Ok(Value::Vector(vals))
            } else if vals.len() == 1 {
                Ok(vals.into_iter().next().expect("checked len"))
            } else {
                Ok(Value::Vector(vals))
            }
        }
        naga::Expression::AccessIndex { base, index } => {
            let base_val = eval_expr(state, base)?;
            match base_val {
                Value::Vector(ref v) => v.get(index as usize).cloned().ok_or_else(|| {
                    CpuError::Internal(format!("vector index {index} out of range"))
                }),
                _ => Err(CpuError::Internal("AccessIndex on non-vector".into())),
            }
        }
        naga::Expression::Access { base, index } => {
            let idx = eval_expr(state, index)?.as_u32()? as usize;
            let base_expr = &state.function.expressions[base];
            if let naga::Expression::GlobalVariable(var) = *base_expr {
                let gv = &state.module.global_variables[var];
                if let Some(ref rb) = gv.binding {
                    let byte_offset = idx * 4;
                    return Ok(Value::F32(state.memory.read_f32(
                        rb.group,
                        rb.binding,
                        byte_offset,
                    )?));
                }
            }
            let base_val = eval_expr(state, base)?;
            match base_val {
                Value::Vector(ref v) => v
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| CpuError::Internal(format!("dynamic index {idx} out of range"))),
                _ => Err(CpuError::Internal("Access on non-indexable value".into())),
            }
        }
        naga::Expression::Binary { op, left, right } => {
            let lhs = eval_expr(state, left)?;
            let rhs = eval_expr(state, right)?;
            eval_binary(op, &lhs, &rhs)
        }
        naga::Expression::Unary { op, expr: inner } => {
            let val = eval_expr(state, inner)?;
            eval_unary(op, &val)
        }
        naga::Expression::Math {
            fun,
            arg,
            arg1,
            arg2,
            ..
        } => {
            let primary = eval_expr(state, arg)?;
            let secondary = arg1.map(|h| eval_expr(state, h)).transpose()?;
            let tertiary = arg2.map(|h| eval_expr(state, h)).transpose()?;
            eval_math(fun, &primary, secondary.as_ref(), tertiary.as_ref())
        }
        naga::Expression::As {
            expr: inner, kind, ..
        } => {
            let val = eval_expr(state, inner)?;
            eval_cast(&val, kind)
        }
        naga::Expression::Splat { size, value } => {
            let val = eval_expr(state, value)?;
            let count = size as usize;
            Ok(Value::Vector(vec![val; count]))
        }
        naga::Expression::Select {
            condition,
            accept,
            reject,
        } => {
            let cond = eval_expr(state, condition)?.as_bool()?;
            if cond {
                eval_expr(state, accept)
            } else {
                eval_expr(state, reject)
            }
        }
        naga::Expression::Load { pointer } => eval_load(state, pointer),
        naga::Expression::FunctionArgument(idx) => eval_function_argument(state, idx as usize),
        naga::Expression::LocalVariable(var) => state
            .locals
            .get(&var)
            .cloned()
            .ok_or_else(|| CpuError::Internal("uninitialized local variable".into())),
        naga::Expression::GlobalVariable(var) => {
            let gv = &state.module.global_variables[var];
            gv.binding.as_ref().map_or_else(
                || match gv.space {
                    naga::AddressSpace::Private => Ok(default_value_for_type(state.module, gv.ty)),
                    _ => Err(CpuError::Internal(format!(
                        "global variable without binding in space {:?}",
                        gv.space
                    ))),
                },
                |rb| eval_global_binding(state, *rb, gv),
            )
        }
        _ => Err(CpuError::Unsupported(format!("{expr:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BindingData;

    #[test]
    fn execute_trivial_shader() {
        let wgsl = r"
@group(0) @binding(0) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    output[id.x] = 42.0;
}
";
        let request = ExecuteCpuRequest {
            wgsl_source: wgsl.into(),
            entry_point: None,
            workgroups: [4, 1, 1],
            bindings: vec![BindingData {
                group: 0,
                binding: 0,
                data: bytes::Bytes::from(vec![0u8; 16]),
                usage: BindingUsage::ReadWrite,
            }],
            uniforms: vec![],
            strategy: crate::types::ExecutionStrategy::Interpret,
        };
        let result = execute_cpu(&request);
        assert!(result.is_ok(), "execute_cpu failed: {result:?}");
    }

    #[test]
    fn parse_error_reported() {
        let request = ExecuteCpuRequest {
            wgsl_source: "not valid wgsl {{{{".into(),
            entry_point: None,
            workgroups: [1, 1, 1],
            bindings: vec![],
            uniforms: vec![],
            strategy: crate::types::ExecutionStrategy::Interpret,
        };
        let result = execute_cpu(&request);
        assert!(matches!(result, Err(CpuError::Parse(_))));
    }

    #[test]
    fn missing_entry_point() {
        let wgsl = "@compute @workgroup_size(1) fn other() {}";
        let request = ExecuteCpuRequest {
            wgsl_source: wgsl.into(),
            entry_point: Some("nonexistent".into()),
            workgroups: [1, 1, 1],
            bindings: vec![],
            uniforms: vec![],
            strategy: crate::types::ExecutionStrategy::Interpret,
        };
        let result = execute_cpu(&request);
        assert!(matches!(result, Err(CpuError::EntryPointNotFound(_))));
    }
}
