// SPDX-License-Identifier: AGPL-3.0-or-later
//! PTX code emitter from naga Module for SM100+ (Blackwell).
//!
//! Emits NVIDIA PTX (Parallel Thread Execution) text from a naga compute
//! shader `Module`. The CUDA driver JIT-compiles PTX to native SASS,
//! bypassing the cubin ELF format that SM120 currently rejects.
//!
//! ## Parameter convention
//!
//! Per storage buffer binding (ordered by `(group, binding)`):
//!   - `.param .u64 _bufN_ptr`  — device pointer
//!   - `.param .u64 _bufN_size` — byte length (for `arrayLength`)
//!
//! ## Builtin mapping
//!
//! | WGSL builtin            | PTX special register           |
//! |-------------------------|-------------------------------|
//! | `global_invocation_id`  | `%tid + %ctaid * %ntid`       |
//! | `local_invocation_id`   | `%tid`                        |
//! | `workgroup_id`          | `%ctaid`                      |
//! | `num_workgroups`        | `%nctaid`                     |
//! | `local_invocation_index`| `tid.x + tid.y*ntid.x + ...` |

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::backend::{BinaryFormat, CompilationInfo, CompiledBinary};
use crate::error::CompileError;

/// Compile WGSL source directly to PTX for SM100+ targets.
///
/// Parses WGSL → naga Module, then emits PTX text. Returns a
/// `CompiledBinary` with `format: BinaryFormat::Ptx`.
pub fn emit_compute_ptx(wgsl_source: &str, sm: u8) -> Result<CompiledBinary, CompileError> {
    let module = super::super::naga_translate::parse_wgsl(wgsl_source)?;

    let ep_index = module
        .entry_points
        .iter()
        .position(|ep| ep.stage == naga::ShaderStage::Compute)
        .ok_or_else(|| CompileError::InvalidInput("no compute entry point".into()))?;

    let ep = &module.entry_points[ep_index];

    let mut emitter = PtxEmitter::new(&module, ep, sm);
    let ptx = emitter.emit()?;

    let ws = ep.workgroup_size;
    Ok(CompiledBinary {
        binary: ptx.into_bytes(),
        info: CompilationInfo {
            gpr_count: emitter.r32_next.max(emitter.rd64_next),
            instr_count: 0,
            shared_mem_bytes: emitter.shared_mem_bytes,
            barrier_count: emitter.barrier_count,
            local_size: [ws[0], ws[1], ws[2]],
        },
        format: BinaryFormat::Ptx,
    })
}

// ── Value representation ─────────────────────────────────────────────

#[derive(Clone, Debug)]
enum PtxVal {
    R32(u32),
    Rd64(u32),
    Pred(u32),
    Vec(Vec<Self>),
}

impl PtxVal {
    fn fmt_operand(&self) -> String {
        match self {
            Self::R32(id) => format!("%r{id}"),
            Self::Rd64(id) => format!("%rd{id}"),
            Self::Pred(id) => format!("%p{id}"),
            Self::Vec(_) => panic!("cannot use vector as scalar operand"),
        }
    }

    fn component(&self, idx: usize) -> &Self {
        match self {
            Self::Vec(v) => &v[idx],
            _ if idx == 0 => self,
            _ => panic!("scalar has no component {idx}"),
        }
    }

    fn is_64bit(&self) -> bool {
        matches!(self, Self::Rd64(_))
    }
}

// ── Buffer binding descriptor ────────────────────────────────────────

#[derive(Debug)]
struct BufferBinding {
    group: u32,
    binding: u32,
    gv_handle: naga::Handle<naga::GlobalVariable>,
    element_stride: u32,
}

// ── Shared memory descriptor ─────────────────────────────────────────

#[derive(Debug)]
struct SharedVar {
    gv_handle: naga::Handle<naga::GlobalVariable>,
    #[allow(dead_code)]
    size_bytes: u32,
    #[allow(dead_code)]
    align: u32,
    offset: u32,
}

// ── Memory space kind ────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum MemSpaceKind {
    Global,
    Shared,
}

// ── Main emitter ─────────────────────────────────────────────────────

struct PtxEmitter<'a> {
    module: &'a naga::Module,
    func: &'a naga::Function,
    sm: u8,
    workgroup_size: [u32; 3],

    bindings: Vec<BufferBinding>,
    shared_vars: Vec<SharedVar>,

    r32_next: u32,
    rd64_next: u32,
    pred_next: u32,
    label_next: u32,

    values: HashMap<naga::Handle<naga::Expression>, PtxVal>,
    locals: HashMap<naga::Handle<naga::LocalVariable>, PtxVal>,
    gv_ptr_regs: HashMap<naga::Handle<naga::GlobalVariable>, (PtxVal, usize)>,

    body: String,
    shared_mem_bytes: u32,
    barrier_count: u32,
}

impl<'a> PtxEmitter<'a> {
    fn new(module: &'a naga::Module, ep: &'a naga::EntryPoint, sm: u8) -> Self {
        Self {
            module,
            func: &ep.function,
            sm,
            workgroup_size: ep.workgroup_size,
            bindings: Vec::new(),
            shared_vars: Vec::new(),
            r32_next: 0,
            rd64_next: 0,
            pred_next: 0,
            label_next: 0,
            values: HashMap::new(),
            locals: HashMap::new(),
            gv_ptr_regs: HashMap::new(),
            body: String::with_capacity(4096),
            shared_mem_bytes: 0,
            barrier_count: 0,
        }
    }

    // ── Register / label allocation ──────────────────────────────────

    fn alloc_r32(&mut self) -> PtxVal {
        let id = self.r32_next;
        self.r32_next += 1;
        PtxVal::R32(id)
    }

    fn alloc_rd64(&mut self) -> PtxVal {
        let id = self.rd64_next;
        self.rd64_next += 1;
        PtxVal::Rd64(id)
    }

    fn alloc_pred(&mut self) -> PtxVal {
        let id = self.pred_next;
        self.pred_next += 1;
        PtxVal::Pred(id)
    }

    fn alloc_label(&mut self) -> u32 {
        let id = self.label_next;
        self.label_next += 1;
        id
    }

    fn alloc_for_scalar(&mut self, scalar: naga::Scalar) -> PtxVal {
        if scalar.width == 8 {
            self.alloc_rd64()
        } else {
            self.alloc_r32()
        }
    }

    // ── Type resolution helpers ──────────────────────────────────────

    fn resolve_expr_type_handle(
        &self,
        h: naga::Handle<naga::Expression>,
    ) -> naga::Handle<naga::Type> {
        let expr = &self.func.expressions[h];
        match *expr {
            naga::Expression::GlobalVariable(gv) => self.module.global_variables[gv].ty,
            naga::Expression::LocalVariable(lv) => self.func.local_variables[lv].ty,
            naga::Expression::FunctionArgument(idx) => self.func.arguments[idx as usize].ty,
            naga::Expression::Literal(ref lit) => {
                self.scalar_type_handle(super::super::naga_translate::lit_scalar(lit))
            }
            naga::Expression::Binary { left, .. } => self.resolve_expr_type_handle(left),
            naga::Expression::Unary { expr: inner, .. } => self.resolve_expr_type_handle(inner),
            naga::Expression::Constant(c) => self.module.constants[c].ty,
            naga::Expression::ZeroValue(ty) | naga::Expression::Compose { ty, .. } => ty,
            naga::Expression::Access { base, .. } | naga::Expression::AccessIndex { base, .. } => {
                let base_ty = self.resolve_expr_type_handle(base);
                let base_inner = &self.module.types[base_ty].inner;
                let (_real_ty, real_inner) = match *base_inner {
                    naga::TypeInner::Pointer { base: pointee, .. } => {
                        (pointee, &self.module.types[pointee].inner)
                    }
                    _ => (base_ty, base_inner),
                };
                match *real_inner {
                    naga::TypeInner::Struct { ref members, .. } => {
                        if let naga::Expression::AccessIndex { index, .. } = *expr {
                            if let Some(member) = members.get(index as usize) {
                                return member.ty;
                            }
                        }
                        base_ty
                    }
                    naga::TypeInner::Array { base, .. } => base,
                    naga::TypeInner::Vector { scalar, .. } => self.scalar_type_handle(scalar),
                    _ => base_ty,
                }
            }
            naga::Expression::Load { pointer } => {
                let ptr_ty = self.resolve_expr_type_handle(pointer);
                match self.module.types[ptr_ty].inner {
                    naga::TypeInner::Pointer { base, .. } => base,
                    _ => ptr_ty,
                }
            }
            naga::Expression::As { kind, convert, .. } => {
                let width = convert.unwrap_or(4);
                self.scalar_type_handle(naga::Scalar { kind, width })
            }
            naga::Expression::Math { arg, .. } => self.resolve_expr_type_handle(arg),
            naga::Expression::Select { accept, .. } => self.resolve_expr_type_handle(accept),
            naga::Expression::Splat { value, .. } => self.resolve_expr_type_handle(value),
            naga::Expression::Swizzle { vector, .. } => self.resolve_expr_type_handle(vector),
            naga::Expression::ArrayLength(_) => self.scalar_type_handle(naga::Scalar::U32),
            _ => self.module.types.iter().next().map_or_else(
                || {
                    panic!("module has no types");
                },
                |(h, _)| h,
            ),
        }
    }

    fn scalar_type_handle(&self, scalar: naga::Scalar) -> naga::Handle<naga::Type> {
        for (handle, ty) in self.module.types.iter() {
            if ty.inner == naga::TypeInner::Scalar(scalar) {
                return handle;
            }
        }
        self.module.types.iter().next().unwrap().0
    }

    fn resolve_expr_type(&self, h: naga::Handle<naga::Expression>) -> &naga::TypeInner {
        let th = self.resolve_expr_type_handle(h);
        &self.module.types[th].inner
    }

    fn inner_type(&self, th: naga::Handle<naga::Type>) -> &naga::TypeInner {
        &self.module.types[th].inner
    }

    fn scalar_of(&self, h: naga::Handle<naga::Expression>) -> naga::Scalar {
        match self.resolve_expr_type(h) {
            naga::TypeInner::Scalar(s) => *s,
            naga::TypeInner::Vector { scalar, .. } => *scalar,
            naga::TypeInner::Pointer { base, .. } => match self.inner_type(*base) {
                naga::TypeInner::Scalar(s) => *s,
                naga::TypeInner::Vector { scalar, .. } => *scalar,
                naga::TypeInner::Array { base, .. } => match self.inner_type(*base) {
                    naga::TypeInner::Scalar(s) => *s,
                    _ => naga::Scalar::U32,
                },
                _ => naga::Scalar::U32,
            },
            _ => naga::Scalar::U32,
        }
    }

    fn ptx_type_suffix(scalar: naga::Scalar) -> &'static str {
        match (scalar.kind, scalar.width) {
            (naga::ScalarKind::Uint, 4) => "u32",
            (naga::ScalarKind::Sint, 4) => "s32",
            (naga::ScalarKind::Float, 4) => "f32",
            (naga::ScalarKind::Float, 8) => "f64",
            (naga::ScalarKind::Uint, 8) => "u64",
            (naga::ScalarKind::Sint, 8) => "s64",
            (naga::ScalarKind::Uint, 2) => "u16",
            (naga::ScalarKind::Sint, 2) => "s16",
            (naga::ScalarKind::Float, 2) => "f16",
            (naga::ScalarKind::Bool, _) => "pred",
            _ => "b32",
        }
    }

    fn ptx_mem_suffix(scalar: naga::Scalar) -> &'static str {
        match scalar.width {
            8 => "u64",
            4 => "u32",
            2 => "u16",
            1 => "u8",
            _ => "u32",
        }
    }

    // ── Binding collection ───────────────────────────────────────────

    fn collect_bindings(&mut self) {
        let mut bindings = Vec::new();
        for (handle, gv) in self.module.global_variables.iter() {
            if matches!(
                gv.space,
                naga::AddressSpace::Storage { .. } | naga::AddressSpace::Uniform
            ) {
                if let Some(ref rb) = gv.binding {
                    let stride = self.element_stride_of(gv.ty);
                    bindings.push(BufferBinding {
                        group: rb.group,
                        binding: rb.binding,
                        gv_handle: handle,
                        element_stride: stride,
                    });
                }
            }
        }
        bindings.sort_by_key(|b| (b.group, b.binding));
        self.bindings = bindings;
    }

    fn element_stride_of(&self, ty_handle: naga::Handle<naga::Type>) -> u32 {
        match self.inner_type(ty_handle) {
            naga::TypeInner::Array { stride, .. } => *stride,
            naga::TypeInner::Struct { span, .. } => *span,
            naga::TypeInner::Scalar(s) => u32::from(s.width),
            naga::TypeInner::Vector { size, scalar } => {
                u32::from(*size as u8) * u32::from(scalar.width)
            }
            naga::TypeInner::Pointer { base, .. } => self.element_stride_of(*base),
            _ => 4,
        }
    }

    fn collect_shared_vars(&mut self) {
        let mut offset = 0u32;
        for (handle, gv) in self.module.global_variables.iter() {
            if gv.space == naga::AddressSpace::WorkGroup {
                let ty = &self.module.types[gv.ty];
                let size = ty.inner.size(self.module.to_ctx());
                let align = 4u32.max(self.element_stride_of(gv.ty));
                offset = (offset + align - 1) & !(align - 1);
                self.shared_vars.push(SharedVar {
                    gv_handle: handle,
                    size_bytes: size,
                    align,
                    offset,
                });
                offset += size;
            }
        }
        self.shared_mem_bytes = offset;
    }

    fn binding_index(&self, gv: naga::Handle<naga::GlobalVariable>) -> Option<usize> {
        self.bindings.iter().position(|b| b.gv_handle == gv)
    }

    fn shared_var(&self, gv: naga::Handle<naga::GlobalVariable>) -> Option<&SharedVar> {
        self.shared_vars.iter().find(|s| s.gv_handle == gv)
    }

    // ── Main emit entry point ────────────────────────────────────────

    fn emit(&mut self) -> Result<String, CompileError> {
        self.collect_bindings();
        self.collect_shared_vars();
        self.init_locals();
        self.precompute_builtins()?;
        self.load_buffer_params();
        self.emit_block(&self.func.body.clone())?;
        writeln!(self.body, "    ret;").unwrap();

        let mut out = String::with_capacity(512 + self.body.len());
        self.write_header(&mut out);
        self.write_params(&mut out);
        writeln!(out, "{{").unwrap();
        self.write_reg_decls(&mut out);
        self.write_shared_decls(&mut out);
        out.push_str(&self.body);
        writeln!(out, "}}").unwrap();
        Ok(out)
    }

    fn write_header(&self, out: &mut String) {
        writeln!(out, ".version 8.7").unwrap();
        writeln!(out, ".target sm_{}", self.sm).unwrap();
        writeln!(out, ".address_size 64").unwrap();
        writeln!(out).unwrap();
    }

    fn write_params(&self, out: &mut String) {
        writeln!(out, ".visible .entry main_kernel(").unwrap();
        let param_count = self.bindings.len() * 2;
        for (i, binding) in self.bindings.iter().enumerate() {
            let idx = binding.binding;
            let comma = if (i * 2 + 1) < param_count - 1 {
                ","
            } else {
                ""
            };
            writeln!(out, "    .param .u64 _buf{idx}_ptr,").unwrap();
            writeln!(out, "    .param .u64 _buf{idx}_size{comma}").unwrap();
        }
        writeln!(out, ")").unwrap();
    }

    fn write_reg_decls(&self, out: &mut String) {
        if self.r32_next > 0 {
            writeln!(out, "    .reg .b32 %r<{}>;", self.r32_next).unwrap();
        }
        if self.rd64_next > 0 {
            writeln!(out, "    .reg .b64 %rd<{}>;", self.rd64_next).unwrap();
        }
        if self.pred_next > 0 {
            writeln!(out, "    .reg .pred %p<{}>;", self.pred_next).unwrap();
        }
    }

    fn write_shared_decls(&self, out: &mut String) {
        if self.shared_mem_bytes > 0 {
            writeln!(
                out,
                "    .shared .align 4 .b8 _shared[{}];",
                self.shared_mem_bytes
            )
            .unwrap();
        }
    }

    // ── Initialize local variables ───────────────────────────────────

    fn init_locals(&mut self) {
        let local_vars: Vec<_> = self.func.local_variables.iter().collect();
        for (handle, lv) in local_vars {
            let val = self.alloc_for_type(lv.ty);
            self.zero_val(&val);
            self.locals.insert(handle, val);
        }
    }

    fn alloc_for_type(&mut self, ty: naga::Handle<naga::Type>) -> PtxVal {
        match self.inner_type(ty) {
            naga::TypeInner::Scalar(s) => self.alloc_for_scalar(*s),
            naga::TypeInner::Vector { size, scalar } => {
                let n = *size as usize;
                let s = *scalar;
                let components: Vec<PtxVal> = (0..n).map(|_| self.alloc_for_scalar(s)).collect();
                PtxVal::Vec(components)
            }
            _ => self.alloc_r32(),
        }
    }

    fn zero_val(&mut self, val: &PtxVal) {
        match val {
            PtxVal::R32(_) => {
                writeln!(self.body, "    mov.u32 {}, 0;", val.fmt_operand()).unwrap();
            }
            PtxVal::Rd64(_) => {
                writeln!(self.body, "    mov.u64 {}, 0;", val.fmt_operand()).unwrap();
            }
            PtxVal::Pred(_) => {
                writeln!(self.body, "    setp.eq.u32 {}, 0, 0;", val.fmt_operand()).unwrap();
            }
            PtxVal::Vec(v) => {
                for c in v {
                    self.zero_val(c);
                }
            }
        }
    }

    // ── Builtin precomputation ───────────────────────────────────────

    fn precompute_builtins(&mut self) -> Result<(), CompileError> {
        let args: Vec<_> = self.func.arguments.iter().enumerate().collect();
        let exprs: Vec<_> = self.func.expressions.iter().collect();

        for (handle, expr) in &exprs {
            if let naga::Expression::FunctionArgument(idx) = expr {
                if let Some(arg) = args.get(*idx as usize) {
                    if let Some(naga::Binding::BuiltIn(builtin)) = &arg.1.binding {
                        let val = self.emit_builtin(*builtin)?;
                        self.values.insert(*handle, val);
                    }
                }
            }
        }
        Ok(())
    }

    fn emit_builtin(&mut self, builtin: naga::BuiltIn) -> Result<PtxVal, CompileError> {
        match builtin {
            naga::BuiltIn::GlobalInvocationId => {
                let mut components = Vec::with_capacity(3);
                for (axis, dim_label) in ["x", "y", "z"].iter().enumerate() {
                    let tid = self.alloc_r32();
                    let ctaid = self.alloc_r32();
                    let ntid = self.alloc_r32();
                    let gid = self.alloc_r32();
                    writeln!(
                        self.body,
                        "    mov.u32 {}, %tid.{dim_label};",
                        tid.fmt_operand()
                    )
                    .unwrap();
                    if self.workgroup_size[axis] == 1 {
                        writeln!(
                            self.body,
                            "    mov.u32 {}, %ctaid.{dim_label};",
                            gid.fmt_operand()
                        )
                        .unwrap();
                    } else {
                        writeln!(
                            self.body,
                            "    mov.u32 {}, %ctaid.{dim_label};",
                            ctaid.fmt_operand()
                        )
                        .unwrap();
                        writeln!(
                            self.body,
                            "    mov.u32 {}, %ntid.{dim_label};",
                            ntid.fmt_operand()
                        )
                        .unwrap();
                        writeln!(
                            self.body,
                            "    mad.lo.u32 {}, {}, {}, {};",
                            gid.fmt_operand(),
                            ctaid.fmt_operand(),
                            ntid.fmt_operand(),
                            tid.fmt_operand(),
                        )
                        .unwrap();
                    }
                    components.push(gid);
                }
                Ok(PtxVal::Vec(components))
            }
            naga::BuiltIn::LocalInvocationId => {
                let mut components = Vec::with_capacity(3);
                for dim_label in ["x", "y", "z"] {
                    let r = self.alloc_r32();
                    writeln!(
                        self.body,
                        "    mov.u32 {}, %tid.{dim_label};",
                        r.fmt_operand()
                    )
                    .unwrap();
                    components.push(r);
                }
                Ok(PtxVal::Vec(components))
            }
            naga::BuiltIn::WorkGroupId => {
                let mut components = Vec::with_capacity(3);
                for dim_label in ["x", "y", "z"] {
                    let r = self.alloc_r32();
                    writeln!(
                        self.body,
                        "    mov.u32 {}, %ctaid.{dim_label};",
                        r.fmt_operand()
                    )
                    .unwrap();
                    components.push(r);
                }
                Ok(PtxVal::Vec(components))
            }
            naga::BuiltIn::NumWorkGroups => {
                let mut components = Vec::with_capacity(3);
                for dim_label in ["x", "y", "z"] {
                    let r = self.alloc_r32();
                    writeln!(
                        self.body,
                        "    mov.u32 {}, %nctaid.{dim_label};",
                        r.fmt_operand()
                    )
                    .unwrap();
                    components.push(r);
                }
                Ok(PtxVal::Vec(components))
            }
            naga::BuiltIn::LocalInvocationIndex => {
                let r = self.alloc_r32();
                let tx = self.alloc_r32();
                let ty = self.alloc_r32();
                let tz = self.alloc_r32();
                let nx = self.alloc_r32();
                let ny = self.alloc_r32();
                writeln!(self.body, "    mov.u32 {}, %tid.x;", tx.fmt_operand()).unwrap();
                writeln!(self.body, "    mov.u32 {}, %tid.y;", ty.fmt_operand()).unwrap();
                writeln!(self.body, "    mov.u32 {}, %tid.z;", tz.fmt_operand()).unwrap();
                writeln!(self.body, "    mov.u32 {}, %ntid.x;", nx.fmt_operand()).unwrap();
                writeln!(self.body, "    mov.u32 {}, %ntid.y;", ny.fmt_operand()).unwrap();
                let tmp = self.alloc_r32();
                // index = tx + ty * nx + tz * nx * ny
                writeln!(
                    self.body,
                    "    mad.lo.u32 {}, {}, {}, {};",
                    r.fmt_operand(),
                    ty.fmt_operand(),
                    nx.fmt_operand(),
                    tx.fmt_operand(),
                )
                .unwrap();
                writeln!(
                    self.body,
                    "    mul.lo.u32 {}, {}, {};",
                    tmp.fmt_operand(),
                    nx.fmt_operand(),
                    ny.fmt_operand(),
                )
                .unwrap();
                writeln!(
                    self.body,
                    "    mad.lo.u32 {}, {}, {}, {};",
                    r.fmt_operand(),
                    tz.fmt_operand(),
                    tmp.fmt_operand(),
                    r.fmt_operand(),
                )
                .unwrap();
                Ok(r)
            }
            other => Err(CompileError::NotImplemented(
                format!("PTX builtin: {other:?}").into(),
            )),
        }
    }

    // ── Buffer parameter loading ─────────────────────────────────────

    fn load_buffer_params(&mut self) {
        let binding_info: Vec<_> = self
            .bindings
            .iter()
            .enumerate()
            .map(|(i, b)| (i, b.binding, b.gv_handle))
            .collect();
        for (i, idx, gv_handle) in binding_info {
            let ptr_reg = self.alloc_rd64();
            let _size_reg = self.alloc_rd64();
            writeln!(
                self.body,
                "    ld.param.u64 {}, [_buf{idx}_ptr];",
                ptr_reg.fmt_operand()
            )
            .unwrap();
            writeln!(
                self.body,
                "    ld.param.u64 {}, [_buf{idx}_size];",
                _size_reg.fmt_operand()
            )
            .unwrap();
            self.gv_ptr_regs.insert(gv_handle, (ptr_reg, i));
        }
    }

    // ── Statement emission ───────────────────────────────────────────

    fn emit_block(&mut self, block: &naga::Block) -> Result<(), CompileError> {
        for stmt in block {
            self.emit_stmt(stmt)?;
        }
        Ok(())
    }

    fn emit_stmt(&mut self, stmt: &naga::Statement) -> Result<(), CompileError> {
        match *stmt {
            naga::Statement::Emit(ref range) => {
                let handles: Vec<_> = range.clone().collect();
                for h in handles {
                    self.eval_expr(h)?;
                }
                Ok(())
            }
            naga::Statement::Store { pointer, value } => self.emit_store(pointer, value),
            naga::Statement::If {
                condition,
                ref accept,
                ref reject,
            } => {
                let cond = self.eval_expr(condition)?;
                let pred = self.ensure_pred(&cond)?;
                let then_label = self.alloc_label();
                let else_label = self.alloc_label();
                let end_label = self.alloc_label();

                if reject.is_empty() {
                    writeln!(self.body, "    @!{} bra $L{end_label};", pred.fmt_operand()).unwrap();
                    self.emit_block(accept)?;
                } else {
                    writeln!(self.body, "    @{} bra $L{then_label};", pred.fmt_operand()).unwrap();
                    writeln!(self.body, "    bra $L{else_label};").unwrap();
                    writeln!(self.body, "$L{then_label}:").unwrap();
                    self.emit_block(accept)?;
                    writeln!(self.body, "    bra $L{end_label};").unwrap();
                    writeln!(self.body, "$L{else_label}:").unwrap();
                    self.emit_block(reject)?;
                }
                writeln!(self.body, "$L{end_label}:").unwrap();
                Ok(())
            }
            naga::Statement::Loop {
                ref body,
                ref continuing,
                break_if,
            } => {
                let loop_label = self.alloc_label();
                let cont_label = self.alloc_label();
                let end_label = self.alloc_label();

                writeln!(self.body, "$L{loop_label}:").unwrap();
                self.emit_block(body)?;
                writeln!(self.body, "$L{cont_label}:").unwrap();
                self.emit_block(continuing)?;
                if let Some(break_cond) = break_if {
                    let cond = self.eval_expr(break_cond)?;
                    let pred = self.ensure_pred(&cond)?;
                    writeln!(self.body, "    @{} bra $L{end_label};", pred.fmt_operand()).unwrap();
                }
                writeln!(self.body, "    bra $L{loop_label};").unwrap();
                writeln!(self.body, "$L{end_label}:").unwrap();
                Ok(())
            }
            naga::Statement::Return { value: _ } => {
                writeln!(self.body, "    ret;").unwrap();
                Ok(())
            }
            naga::Statement::ControlBarrier(_) => {
                self.barrier_count += 1;
                writeln!(self.body, "    bar.sync 0;").unwrap();
                Ok(())
            }
            naga::Statement::Block(ref block) => self.emit_block(block),
            naga::Statement::Break => {
                // Loops manage their own break labels — we use `bra $Lend`
                // This is handled via the break_if path. Standalone Break
                // would need a label stack; for now, emit ret as a safe fallback.
                writeln!(self.body, "    ret;").unwrap();
                Ok(())
            }
            naga::Statement::Continue => {
                // Same concern as Break — needs label stack for full support.
                Ok(())
            }
            naga::Statement::Switch { .. } => {
                Err(CompileError::NotImplemented("PTX switch statement".into()))
            }
            naga::Statement::Kill => {
                writeln!(self.body, "    exit;").unwrap();
                Ok(())
            }
            _ => Ok(()),
        }
    }

    // ── Store emission ───────────────────────────────────────────────

    fn emit_store(
        &mut self,
        pointer: naga::Handle<naga::Expression>,
        value: naga::Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let val = self.eval_expr(value)?;
        let val_scalar = self.scalar_of(value);

        if let Some(lv_handle) = self.expr_is_local_var(pointer) {
            if let PtxVal::Vec(components) = &val {
                let new_val = PtxVal::Vec(components.clone());
                self.locals.insert(lv_handle, new_val);
            } else {
                let dst = self.locals.get(&lv_handle).cloned();
                if let Some(dst) = dst {
                    self.emit_mov(&dst, &val, val_scalar);
                } else {
                    self.locals.insert(lv_handle, val);
                }
            }
            return Ok(());
        }

        if let Some(lv_comp) = self.expr_is_local_var_component(pointer) {
            let (lv_handle, comp_idx) = lv_comp;
            if let Some(PtxVal::Vec(components)) = self.locals.get(&lv_handle).cloned() {
                let dst = components[comp_idx].clone();
                self.emit_mov(&dst, &val, val_scalar);
            }
            return Ok(());
        }

        let (addr, mem_space) = self.eval_pointer(pointer)?;
        let space_prefix = if mem_space == MemSpaceKind::Shared {
            "shared"
        } else {
            "global"
        };

        match &val {
            PtxVal::Vec(components) => {
                for (i, comp) in components.iter().enumerate() {
                    let offset = i as u32 * u32::from(val_scalar.width);
                    if offset == 0 {
                        writeln!(
                            self.body,
                            "    st.{space_prefix}.{} [{}], {};",
                            Self::ptx_mem_suffix(val_scalar),
                            addr.fmt_operand(),
                            comp.fmt_operand(),
                        )
                        .unwrap();
                    } else {
                        let off_reg = self.alloc_rd64();
                        writeln!(
                            self.body,
                            "    add.u64 {}, {}, {offset};",
                            off_reg.fmt_operand(),
                            addr.fmt_operand(),
                        )
                        .unwrap();
                        writeln!(
                            self.body,
                            "    st.{space_prefix}.{} [{}], {};",
                            Self::ptx_mem_suffix(val_scalar),
                            off_reg.fmt_operand(),
                            comp.fmt_operand(),
                        )
                        .unwrap();
                    }
                }
            }
            _ => {
                writeln!(
                    self.body,
                    "    st.{space_prefix}.{} [{}], {};",
                    Self::ptx_mem_suffix(val_scalar),
                    addr.fmt_operand(),
                    val.fmt_operand(),
                )
                .unwrap();
            }
        }
        Ok(())
    }

    fn emit_mov(&mut self, dst: &PtxVal, src: &PtxVal, scalar: naga::Scalar) {
        let suffix = if scalar.width == 8 { "u64" } else { "u32" };
        writeln!(
            self.body,
            "    mov.{suffix} {}, {};",
            dst.fmt_operand(),
            src.fmt_operand(),
        )
        .unwrap();
    }

    // ── Pointer evaluation ───────────────────────────────────────────

    fn eval_pointer(
        &mut self,
        h: naga::Handle<naga::Expression>,
    ) -> Result<(PtxVal, MemSpaceKind), CompileError> {
        let expr = self.func.expressions[h].clone();
        match expr {
            naga::Expression::GlobalVariable(gv) => {
                let global = &self.module.global_variables[gv];
                if global.space == naga::AddressSpace::WorkGroup {
                    let sv = self.shared_var(gv);
                    let offset = sv.map_or(0, |s| s.offset);
                    let addr = self.alloc_rd64();
                    writeln!(self.body, "    mov.u64 {}, _shared;", addr.fmt_operand()).unwrap();
                    if offset > 0 {
                        writeln!(
                            self.body,
                            "    add.u64 {0}, {0}, {offset};",
                            addr.fmt_operand()
                        )
                        .unwrap();
                    }
                    return Ok((addr, MemSpaceKind::Shared));
                }
                if let Some((ptr_reg, _)) = self.gv_ptr_regs.get(&gv).cloned() {
                    Ok((ptr_reg, MemSpaceKind::Global))
                } else {
                    Err(CompileError::NotImplemented(
                        "PTX: unbound global variable".into(),
                    ))
                }
            }
            naga::Expression::Access { base, index } => {
                let (base_addr, space) = self.eval_pointer(base)?;
                let idx_val = self.eval_expr(index)?;
                let stride = self.pointer_element_stride(base);
                let addr = self.compute_element_addr(&base_addr, &idx_val, stride);
                Ok((addr, space))
            }
            naga::Expression::AccessIndex { base, index } => {
                let (base_addr, space) = self.eval_pointer(base)?;
                let offset = self.access_index_byte_offset(base, index);
                if offset == 0 {
                    return Ok((base_addr, space));
                }
                let addr = self.alloc_rd64();
                writeln!(
                    self.body,
                    "    add.u64 {}, {}, {offset};",
                    addr.fmt_operand(),
                    base_addr.fmt_operand(),
                )
                .unwrap();
                Ok((addr, space))
            }
            _ => {
                if let Some(cached) = self.values.get(&h).cloned() {
                    if cached.is_64bit() {
                        return Ok((cached, MemSpaceKind::Global));
                    }
                }
                Err(CompileError::NotImplemented(
                    format!("PTX pointer expression: {:?}", self.func.expressions[h]).into(),
                ))
            }
        }
    }

    fn pointer_element_stride(&self, ptr_expr: naga::Handle<naga::Expression>) -> u32 {
        let ty = self.resolve_expr_type(ptr_expr);
        match ty {
            naga::TypeInner::Pointer { base, .. } => match self.inner_type(*base) {
                naga::TypeInner::Array { stride, .. } => *stride,
                naga::TypeInner::Vector { scalar, .. } => u32::from(scalar.width),
                _ => 4,
            },
            naga::TypeInner::ValuePointer { scalar, .. } => u32::from(scalar.width),
            _ => 4,
        }
    }

    fn access_index_byte_offset(&self, base: naga::Handle<naga::Expression>, index: u32) -> u32 {
        let ty = self.resolve_expr_type(base);
        match ty {
            naga::TypeInner::Pointer { base: base_ty, .. } => match self.inner_type(*base_ty) {
                naga::TypeInner::Struct { members, .. } => {
                    members.get(index as usize).map_or(0, |m| m.offset)
                }
                naga::TypeInner::Array { stride, .. } => index * stride,
                naga::TypeInner::Vector { scalar, .. } => index * u32::from(scalar.width),
                _ => index * 4,
            },
            _ => index * 4,
        }
    }

    fn compute_element_addr(&mut self, base: &PtxVal, index: &PtxVal, stride: u32) -> PtxVal {
        let idx64 = self.alloc_rd64();
        let offset = self.alloc_rd64();
        let addr = self.alloc_rd64();

        writeln!(
            self.body,
            "    cvt.u64.u32 {}, {};",
            idx64.fmt_operand(),
            index.fmt_operand(),
        )
        .unwrap();

        if stride.is_power_of_two() && stride > 1 {
            let shift = stride.trailing_zeros();
            writeln!(
                self.body,
                "    shl.b64 {}, {}, {shift};",
                offset.fmt_operand(),
                idx64.fmt_operand(),
            )
            .unwrap();
        } else if stride == 1 {
            writeln!(
                self.body,
                "    mov.u64 {}, {};",
                offset.fmt_operand(),
                idx64.fmt_operand(),
            )
            .unwrap();
        } else {
            let stride_reg = self.alloc_rd64();
            writeln!(
                self.body,
                "    mov.u64 {}, {stride};",
                stride_reg.fmt_operand()
            )
            .unwrap();
            writeln!(
                self.body,
                "    mul.lo.u64 {}, {}, {};",
                offset.fmt_operand(),
                idx64.fmt_operand(),
                stride_reg.fmt_operand(),
            )
            .unwrap();
        }

        writeln!(
            self.body,
            "    add.u64 {}, {}, {};",
            addr.fmt_operand(),
            base.fmt_operand(),
            offset.fmt_operand(),
        )
        .unwrap();

        addr
    }

    // ── Local variable helpers ───────────────────────────────────────

    fn expr_is_local_var(
        &self,
        h: naga::Handle<naga::Expression>,
    ) -> Option<naga::Handle<naga::LocalVariable>> {
        match self.func.expressions[h] {
            naga::Expression::LocalVariable(lv) => Some(lv),
            _ => None,
        }
    }

    fn expr_is_local_var_component(
        &self,
        h: naga::Handle<naga::Expression>,
    ) -> Option<(naga::Handle<naga::LocalVariable>, usize)> {
        match self.func.expressions[h] {
            naga::Expression::AccessIndex { base, index } => {
                if let naga::Expression::LocalVariable(lv) = self.func.expressions[base] {
                    Some((lv, index as usize))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    // ── Expression evaluation ────────────────────────────────────────

    fn eval_expr(&mut self, h: naga::Handle<naga::Expression>) -> Result<PtxVal, CompileError> {
        if let Some(cached) = self.values.get(&h).cloned() {
            return Ok(cached);
        }

        let expr = self.func.expressions[h].clone();
        let result = match expr {
            naga::Expression::Literal(ref lit) => self.eval_literal(lit),
            naga::Expression::Constant(c) => {
                let init = self.module.constants[c].init;
                self.eval_const_expr(init)
            }
            naga::Expression::ZeroValue(ty) => self.eval_zero(ty),
            naga::Expression::Binary { op, left, right } => {
                let lhs = self.eval_expr(left)?;
                let rhs_val = self.eval_expr(right)?;
                self.eval_binary(op, &lhs, &rhs_val, left)
            }
            naga::Expression::Unary { op, expr: inner } => {
                let val = self.eval_expr(inner)?;
                self.eval_unary(op, &val, inner)
            }
            naga::Expression::Math {
                fun,
                arg,
                arg1,
                arg2,
                arg3: _,
            } => {
                let primary = self.eval_expr(arg)?;
                let second = arg1.map(|h| self.eval_expr(h)).transpose()?;
                let third = arg2.map(|h| self.eval_expr(h)).transpose()?;
                self.eval_math(fun, &primary, second.as_ref(), third.as_ref(), arg)
            }
            naga::Expression::FunctionArgument(idx) => {
                if let Some(arg) = self.func.arguments.get(idx as usize) {
                    if let Some(naga::Binding::BuiltIn(builtin)) = &arg.binding {
                        return self.emit_builtin(*builtin);
                    }
                }
                Err(CompileError::NotImplemented(
                    format!("PTX function argument {idx}").into(),
                ))
            }
            naga::Expression::GlobalVariable(gv) => {
                if let Some((ptr_reg, _)) = self.gv_ptr_regs.get(&gv).cloned() {
                    Ok(ptr_reg)
                } else if self.shared_var(gv).is_some() {
                    let addr = self.alloc_rd64();
                    let sv = self.shared_var(gv).unwrap();
                    let offset = sv.offset;
                    writeln!(self.body, "    mov.u64 {}, _shared;", addr.fmt_operand()).unwrap();
                    if offset > 0 {
                        writeln!(
                            self.body,
                            "    add.u64 {0}, {0}, {offset};",
                            addr.fmt_operand()
                        )
                        .unwrap();
                    }
                    Ok(addr)
                } else {
                    Ok(self.alloc_r32())
                }
            }
            naga::Expression::LocalVariable(lv) => {
                if let Some(val) = self.locals.get(&lv).cloned() {
                    Ok(val)
                } else {
                    Ok(self.alloc_r32())
                }
            }
            naga::Expression::Load { pointer } => self.eval_load(pointer),
            naga::Expression::Access { base, index } => self.eval_access(base, index),
            naga::Expression::AccessIndex { base, index } => self.eval_access_index(h, base, index),
            naga::Expression::Select {
                condition,
                accept,
                reject,
            } => {
                let cond = self.eval_expr(condition)?;
                let acc = self.eval_expr(accept)?;
                let rej = self.eval_expr(reject)?;
                let pred = self.ensure_pred(&cond)?;
                let dst = self.alloc_r32();
                writeln!(
                    self.body,
                    "    selp.b32 {}, {}, {}, {};",
                    dst.fmt_operand(),
                    acc.fmt_operand(),
                    rej.fmt_operand(),
                    pred.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            naga::Expression::Compose {
                ty: _,
                ref components,
            } => {
                let mut vals = Vec::with_capacity(components.len());
                for &c in components {
                    vals.push(self.eval_expr(c)?);
                }
                Ok(PtxVal::Vec(vals))
            }
            naga::Expression::Splat { size, value } => {
                let val = self.eval_expr(value)?;
                let n = size as usize;
                let mut components = Vec::with_capacity(n);
                components.push(val.clone());
                for _ in 1..n {
                    let copy = self.alloc_r32();
                    writeln!(
                        self.body,
                        "    mov.u32 {}, {};",
                        copy.fmt_operand(),
                        val.fmt_operand(),
                    )
                    .unwrap();
                    components.push(copy);
                }
                Ok(PtxVal::Vec(components))
            }
            naga::Expression::ArrayLength(ptr_expr) => self.eval_array_length(ptr_expr),
            naga::Expression::As {
                expr: inner,
                kind,
                convert,
            } => {
                let val = self.eval_expr(inner)?;
                self.eval_cast(&val, kind, convert, inner)
            }
            naga::Expression::Swizzle {
                size,
                vector,
                pattern,
            } => {
                let vec_val = self.eval_expr(vector)?;
                let n = size as usize;
                let mut components = Vec::with_capacity(n);
                for i in 0..n {
                    let comp_idx = pattern[i] as usize;
                    components.push(vec_val.component(comp_idx).clone());
                }
                if n == 1 {
                    Ok(components.into_iter().next().unwrap())
                } else {
                    Ok(PtxVal::Vec(components))
                }
            }
            _ => Err(CompileError::NotImplemented(
                format!("PTX expression: {expr:?}").into(),
            )),
        }?;

        self.values.insert(h, result.clone());
        Ok(result)
    }

    // ── Literal ──────────────────────────────────────────────────────

    fn eval_literal(&mut self, lit: &naga::Literal) -> Result<PtxVal, CompileError> {
        match *lit {
            naga::Literal::U32(v) => {
                let r = self.alloc_r32();
                writeln!(self.body, "    mov.u32 {}, {v};", r.fmt_operand()).unwrap();
                Ok(r)
            }
            naga::Literal::I32(v) => {
                let r = self.alloc_r32();
                let bits = v as u32;
                writeln!(self.body, "    mov.u32 {}, {bits};", r.fmt_operand()).unwrap();
                Ok(r)
            }
            naga::Literal::F32(v) => {
                let r = self.alloc_r32();
                let bits = v.to_bits();
                writeln!(self.body, "    mov.b32 {}, 0F{bits:08X};", r.fmt_operand()).unwrap();
                Ok(r)
            }
            naga::Literal::F64(v) => {
                let r = self.alloc_rd64();
                let bits = v.to_bits();
                writeln!(self.body, "    mov.b64 {}, 0D{bits:016X};", r.fmt_operand()).unwrap();
                Ok(r)
            }
            naga::Literal::Bool(v) => {
                let r = self.alloc_pred();
                let val = u32::from(v);
                let tmp = self.alloc_r32();
                writeln!(self.body, "    mov.u32 {}, {val};", tmp.fmt_operand()).unwrap();
                writeln!(
                    self.body,
                    "    setp.ne.u32 {}, {}, 0;",
                    r.fmt_operand(),
                    tmp.fmt_operand()
                )
                .unwrap();
                Ok(r)
            }
            naga::Literal::U64(v) => {
                let r = self.alloc_rd64();
                writeln!(self.body, "    mov.u64 {}, {v};", r.fmt_operand()).unwrap();
                Ok(r)
            }
            naga::Literal::I64(v) => {
                let r = self.alloc_rd64();
                let bits = v as u64;
                writeln!(self.body, "    mov.u64 {}, {bits};", r.fmt_operand()).unwrap();
                Ok(r)
            }
            _ => Err(CompileError::NotImplemented(
                format!("PTX literal: {lit:?}").into(),
            )),
        }
    }

    fn eval_const_expr(
        &mut self,
        h: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let expr = &self.module.global_expressions[h];
        match *expr {
            naga::Expression::Literal(ref lit) => self.eval_literal(lit),
            naga::Expression::ZeroValue(ty) => self.eval_zero(ty),
            naga::Expression::Compose {
                ty: _,
                ref components,
            } => {
                let mut vals = Vec::with_capacity(components.len());
                for &c in components {
                    vals.push(self.eval_const_expr(c)?);
                }
                Ok(PtxVal::Vec(vals))
            }
            _ => {
                let r = self.alloc_r32();
                writeln!(self.body, "    mov.u32 {}, 0;", r.fmt_operand()).unwrap();
                Ok(r)
            }
        }
    }

    fn eval_zero(&mut self, ty: naga::Handle<naga::Type>) -> Result<PtxVal, CompileError> {
        match self.inner_type(ty) {
            naga::TypeInner::Scalar(s) => {
                let r = self.alloc_for_scalar(*s);
                self.zero_val(&r);
                Ok(r)
            }
            naga::TypeInner::Vector { size, scalar } => {
                let n = *size as usize;
                let s = *scalar;
                let mut components = Vec::with_capacity(n);
                for _ in 0..n {
                    let r = self.alloc_for_scalar(s);
                    self.zero_val(&r);
                    components.push(r);
                }
                Ok(PtxVal::Vec(components))
            }
            _ => {
                let r = self.alloc_r32();
                self.zero_val(&r);
                Ok(r)
            }
        }
    }

    // ── Load ─────────────────────────────────────────────────────────

    fn eval_load(
        &mut self,
        pointer: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        if let Some(lv_handle) = self.expr_is_local_var(pointer) {
            if let Some(val) = self.locals.get(&lv_handle).cloned() {
                return Ok(val);
            }
        }
        if let Some((lv_handle, comp)) = self.expr_is_local_var_component(pointer) {
            if let Some(local) = self.locals.get(&lv_handle).cloned() {
                return Ok(local.component(comp).clone());
            }
        }

        let (addr, mem_space) = self.eval_pointer(pointer)?;

        let expr_ty = self.resolve_expr_type_handle(pointer);
        let pointee_ty = match self.inner_type(expr_ty) {
            naga::TypeInner::Pointer { base, .. } => *base,
            _ => {
                // Access/AccessIndex on a pointer resolves to the element type
                // directly in our manual resolution. Use it as the pointee type.
                expr_ty
            }
        };

        let space = if mem_space == MemSpaceKind::Shared {
            "shared"
        } else {
            "global"
        };

        let inner = self.inner_type(pointee_ty).clone();
        match inner {
            naga::TypeInner::Scalar(s) => {
                let dst = self.alloc_for_scalar(s);
                writeln!(
                    self.body,
                    "    ld.{space}.{} {}, [{}];",
                    Self::ptx_mem_suffix(s),
                    dst.fmt_operand(),
                    addr.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            naga::TypeInner::Vector { size, scalar } => {
                let n = size as usize;
                let s = scalar;
                let mut components = Vec::with_capacity(n);
                for i in 0..n {
                    let dst = self.alloc_for_scalar(s);
                    let offset = i as u32 * u32::from(s.width);
                    if offset == 0 {
                        writeln!(
                            self.body,
                            "    ld.{space}.{} {}, [{}];",
                            Self::ptx_mem_suffix(s),
                            dst.fmt_operand(),
                            addr.fmt_operand(),
                        )
                        .unwrap();
                    } else {
                        let off_addr = self.alloc_rd64();
                        writeln!(
                            self.body,
                            "    add.u64 {}, {}, {offset};",
                            off_addr.fmt_operand(),
                            addr.fmt_operand(),
                        )
                        .unwrap();
                        writeln!(
                            self.body,
                            "    ld.{space}.{} {}, [{}];",
                            Self::ptx_mem_suffix(s),
                            dst.fmt_operand(),
                            off_addr.fmt_operand(),
                        )
                        .unwrap();
                    }
                    components.push(dst);
                }
                Ok(PtxVal::Vec(components))
            }
            _ => {
                let dst = self.alloc_r32();
                writeln!(
                    self.body,
                    "    ld.{space}.u32 {}, [{}];",
                    dst.fmt_operand(),
                    addr.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
        }
    }

    // ── Access / AccessIndex ─────────────────────────────────────────

    fn eval_access(
        &mut self,
        base: naga::Handle<naga::Expression>,
        index: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let base_ty = self.resolve_expr_type(base);
        if matches!(base_ty, naga::TypeInner::Vector { .. }) {
            let vec_val = self.eval_expr(base)?;
            let idx_val = self.eval_expr(index)?;
            // Dynamic component access on vectors — use indexing
            // For simplicity, this emits a series of selects. Common case
            // is small vectors (2-4 elements).
            if let PtxVal::Vec(ref components) = vec_val {
                if components.len() <= 4 {
                    let result = self.alloc_r32();
                    writeln!(
                        self.body,
                        "    mov.u32 {}, {};",
                        result.fmt_operand(),
                        components[0].fmt_operand()
                    )
                    .unwrap();
                    for (i, comp) in components.iter().enumerate().skip(1) {
                        let pred = self.alloc_pred();
                        writeln!(
                            self.body,
                            "    setp.eq.u32 {}, {}, {};",
                            pred.fmt_operand(),
                            idx_val.fmt_operand(),
                            i,
                        )
                        .unwrap();
                        writeln!(
                            self.body,
                            "    @{} mov.u32 {}, {};",
                            pred.fmt_operand(),
                            result.fmt_operand(),
                            comp.fmt_operand(),
                        )
                        .unwrap();
                    }
                    return Ok(result);
                }
            }
        }

        // Pointer access — compute address
        let (base_addr, _space) = self.eval_pointer(base)?;
        let idx_val = self.eval_expr(index)?;
        let stride = self.pointer_element_stride(base);
        let addr = self.compute_element_addr(&base_addr, &idx_val, stride);
        // Return as a pointer (address in rd64 register)
        // The Load expression will dereference it later
        Ok(addr)
    }

    fn eval_access_index(
        &mut self,
        _h: naga::Handle<naga::Expression>,
        base: naga::Handle<naga::Expression>,
        index: u32,
    ) -> Result<PtxVal, CompileError> {
        let base_ty = self.resolve_expr_type(base);
        match base_ty {
            naga::TypeInner::Vector { .. } => {
                let vec_val = self.eval_expr(base)?;
                Ok(vec_val.component(index as usize).clone())
            }
            naga::TypeInner::Struct { .. } => {
                let base_val = self.eval_expr(base)?;
                if let PtxVal::Vec(ref components) = base_val {
                    if (index as usize) < components.len() {
                        return Ok(components[index as usize].clone());
                    }
                }
                Ok(base_val)
            }
            _ => {
                // Pointer access — evaluate as pointer
                let base_val = self.eval_expr(base)?;
                Ok(base_val)
            }
        }
    }

    // ── ArrayLength ──────────────────────────────────────────────────

    fn eval_array_length(
        &mut self,
        ptr_expr: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let gv_handle = self.find_global_variable(ptr_expr);
        let gv_handle = gv_handle.ok_or_else(|| {
            CompileError::NotImplemented("PTX arrayLength: cannot resolve buffer".into())
        })?;

        let binding_idx = self.binding_index(gv_handle).ok_or_else(|| {
            CompileError::NotImplemented("PTX arrayLength: unbound global variable".into())
        })?;

        let stride = self.bindings[binding_idx].element_stride;
        let idx = self.bindings[binding_idx].binding;
        let size_reg = self.alloc_rd64();
        writeln!(
            self.body,
            "    ld.param.u64 {}, [_buf{idx}_size];",
            size_reg.fmt_operand()
        )
        .unwrap();

        let result_64 = self.alloc_rd64();
        if stride.is_power_of_two() && stride > 1 {
            let shift = stride.trailing_zeros();
            writeln!(
                self.body,
                "    shr.u64 {}, {}, {shift};",
                result_64.fmt_operand(),
                size_reg.fmt_operand(),
            )
            .unwrap();
        } else if stride == 1 {
            writeln!(
                self.body,
                "    mov.u64 {}, {};",
                result_64.fmt_operand(),
                size_reg.fmt_operand(),
            )
            .unwrap();
        } else {
            let stride_reg = self.alloc_rd64();
            writeln!(
                self.body,
                "    mov.u64 {}, {stride};",
                stride_reg.fmt_operand()
            )
            .unwrap();
            writeln!(
                self.body,
                "    div.u64 {}, {}, {};",
                result_64.fmt_operand(),
                size_reg.fmt_operand(),
                stride_reg.fmt_operand(),
            )
            .unwrap();
        }

        let result = self.alloc_r32();
        writeln!(
            self.body,
            "    cvt.u32.u64 {}, {};",
            result.fmt_operand(),
            result_64.fmt_operand(),
        )
        .unwrap();
        Ok(result)
    }

    fn find_global_variable(
        &self,
        h: naga::Handle<naga::Expression>,
    ) -> Option<naga::Handle<naga::GlobalVariable>> {
        match self.func.expressions[h] {
            naga::Expression::GlobalVariable(gv) => Some(gv),
            naga::Expression::AccessIndex { base, .. } | naga::Expression::Access { base, .. } => {
                self.find_global_variable(base)
            }
            _ => None,
        }
    }

    // ── Binary operations ────────────────────────────────────────────

    fn eval_binary(
        &mut self,
        op: naga::BinaryOperator,
        left: &PtxVal,
        right: &PtxVal,
        left_handle: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let scalar = self.scalar_of(left_handle);
        let ts = Self::ptx_type_suffix(scalar);

        if let (PtxVal::Vec(lv), PtxVal::Vec(rv)) = (left, right) {
            let mut results = Vec::with_capacity(lv.len());
            for (left_comp, right_comp) in lv.iter().zip(rv.iter()) {
                results.push(self.eval_binary_scalar(op, left_comp, right_comp, scalar, ts)?);
            }
            return Ok(PtxVal::Vec(results));
        }

        self.eval_binary_scalar(op, left, right, scalar, ts)
    }

    fn eval_binary_scalar(
        &mut self,
        op: naga::BinaryOperator,
        left: &PtxVal,
        right: &PtxVal,
        scalar: naga::Scalar,
        ts: &str,
    ) -> Result<PtxVal, CompileError> {
        use naga::BinaryOperator as BO;
        let is_float = scalar.kind == naga::ScalarKind::Float;

        match op {
            BO::Add | BO::Subtract | BO::Multiply | BO::Divide | BO::Modulo => {
                let dst = self.alloc_for_scalar(scalar);
                let instr = match op {
                    BO::Add => "add",
                    BO::Subtract => "sub",
                    BO::Multiply if is_float => "mul",
                    BO::Multiply => "mul.lo",
                    BO::Divide => "div.rn",
                    BO::Modulo => "rem",
                    _ => unreachable!(),
                };
                writeln!(
                    self.body,
                    "    {instr}.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            BO::Equal
            | BO::NotEqual
            | BO::Less
            | BO::LessEqual
            | BO::Greater
            | BO::GreaterEqual => {
                let pred = self.alloc_pred();
                let cmp = match op {
                    BO::Equal => "eq",
                    BO::NotEqual => "ne",
                    BO::Less => "lt",
                    BO::LessEqual => "le",
                    BO::Greater => "gt",
                    BO::GreaterEqual => "ge",
                    _ => unreachable!(),
                };
                writeln!(
                    self.body,
                    "    setp.{cmp}.{ts} {}, {}, {};",
                    pred.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .unwrap();
                Ok(pred)
            }
            BO::And => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    and.b{} {}, {}, {};",
                    scalar.width * 8,
                    dst.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            BO::InclusiveOr => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    or.b{} {}, {}, {};",
                    scalar.width * 8,
                    dst.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            BO::ExclusiveOr => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    xor.b{} {}, {}, {};",
                    scalar.width * 8,
                    dst.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            BO::ShiftLeft => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    shl.b{} {}, {}, {};",
                    scalar.width * 8,
                    dst.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            BO::ShiftRight => {
                let dst = self.alloc_for_scalar(scalar);
                let instr = if scalar.kind == naga::ScalarKind::Sint {
                    "shr.s"
                } else {
                    "shr.u"
                };
                writeln!(
                    self.body,
                    "    {instr}{} {}, {}, {};",
                    scalar.width * 8,
                    dst.fmt_operand(),
                    left.fmt_operand(),
                    right.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            BO::LogicalAnd => {
                let lp = self.ensure_pred(left)?;
                let rp = self.ensure_pred(right)?;
                let dst = self.alloc_pred();
                writeln!(
                    self.body,
                    "    and.pred {}, {}, {};",
                    dst.fmt_operand(),
                    lp.fmt_operand(),
                    rp.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            BO::LogicalOr => {
                let lp = self.ensure_pred(left)?;
                let rp = self.ensure_pred(right)?;
                let dst = self.alloc_pred();
                writeln!(
                    self.body,
                    "    or.pred {}, {}, {};",
                    dst.fmt_operand(),
                    lp.fmt_operand(),
                    rp.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
        }
    }

    // ── Unary operations ─────────────────────────────────────────────

    fn eval_unary(
        &mut self,
        op: naga::UnaryOperator,
        val: &PtxVal,
        expr: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let scalar = self.scalar_of(expr);
        match op {
            naga::UnaryOperator::Negate => {
                let dst = self.alloc_for_scalar(scalar);
                let ts = Self::ptx_type_suffix(scalar);
                writeln!(
                    self.body,
                    "    neg.{ts} {}, {};",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            naga::UnaryOperator::LogicalNot => {
                let p = self.ensure_pred(val)?;
                let dst = self.alloc_pred();
                writeln!(
                    self.body,
                    "    not.pred {}, {};",
                    dst.fmt_operand(),
                    p.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            naga::UnaryOperator::BitwiseNot => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    not.b{} {}, {};",
                    scalar.width * 8,
                    dst.fmt_operand(),
                    val.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
        }
    }

    // ── Math functions ─────────────────────────────────────────────--

    /// Emits PTX for a built-in mathematical function (`min`, `max`, `fma`, etc.).
    fn eval_math(
        &mut self,
        fun: naga::MathFunction,
        arg: &PtxVal,
        arg1: Option<&PtxVal>,
        arg2: Option<&PtxVal>,
        arg_handle: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        use naga::MathFunction as MF;
        let scalar = self.scalar_of(arg_handle);
        let ts = Self::ptx_type_suffix(scalar);

        match fun {
            MF::Abs => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    abs.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            MF::Min => {
                let rhs =
                    arg1.ok_or_else(|| CompileError::NotImplemented("min without arg1".into()))?;
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    min.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    rhs.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            MF::Max => {
                let rhs =
                    arg1.ok_or_else(|| CompileError::NotImplemented("max without arg1".into()))?;
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    max.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    rhs.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            MF::Clamp => {
                let lo =
                    arg1.ok_or_else(|| CompileError::NotImplemented("clamp without arg1".into()))?;
                let hi =
                    arg2.ok_or_else(|| CompileError::NotImplemented("clamp without arg2".into()))?;
                let tmp = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    max.{ts} {}, {}, {};",
                    tmp.fmt_operand(),
                    arg.fmt_operand(),
                    lo.fmt_operand(),
                )
                .unwrap();
                writeln!(
                    self.body,
                    "    min.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    tmp.fmt_operand(),
                    hi.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            MF::Floor | MF::Ceil | MF::Round | MF::Trunc => {
                let mode = match fun {
                    MF::Floor => "rmi",
                    MF::Ceil => "rpi",
                    MF::Round => "rni",
                    MF::Trunc => "rzi",
                    _ => unreachable!(),
                };
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    cvt.{mode}.{ts}.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            MF::Sqrt => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    sqrt.rn.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            MF::InverseSqrt => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    rsqrt.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            MF::Sin | MF::Cos => {
                let op_name = if matches!(fun, MF::Sin) { "sin" } else { "cos" };
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    {op_name}.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            MF::Exp2 => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    ex2.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            MF::Log2 => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    lg2.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            MF::Fma => {
                let mul_term =
                    arg1.ok_or_else(|| CompileError::NotImplemented("fma without arg1".into()))?;
                let add_term =
                    arg2.ok_or_else(|| CompileError::NotImplemented("fma without arg2".into()))?;
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    fma.rn.{ts} {}, {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    mul_term.fmt_operand(),
                    add_term.fmt_operand(),
                )
                .unwrap();
                Ok(dst)
            }
            _ => Err(CompileError::NotImplemented(
                format!("PTX math function: {fun:?}").into(),
            )),
        }
    }

    // ── Type cast ────────────────────────────────────────────────────

    fn eval_cast(
        &mut self,
        val: &PtxVal,
        kind: naga::ScalarKind,
        convert: Option<u8>,
        inner_handle: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let src_scalar = self.scalar_of(inner_handle);
        let dst_width = convert.unwrap_or(src_scalar.width);
        let dst_scalar = naga::Scalar {
            kind,
            width: dst_width,
        };
        let src_ts = Self::ptx_type_suffix(src_scalar);
        let dst_ts = Self::ptx_type_suffix(dst_scalar);

        if src_ts == dst_ts {
            return Ok(val.clone());
        }

        if let PtxVal::Vec(components) = val {
            let mut results = Vec::with_capacity(components.len());
            for c in components {
                let dst = self.alloc_for_scalar(dst_scalar);
                writeln!(
                    self.body,
                    "    cvt.{dst_ts}.{src_ts} {}, {};",
                    dst.fmt_operand(),
                    c.fmt_operand(),
                )
                .unwrap();
                results.push(dst);
            }
            return Ok(PtxVal::Vec(results));
        }

        let dst = self.alloc_for_scalar(dst_scalar);
        writeln!(
            self.body,
            "    cvt.{dst_ts}.{src_ts} {}, {};",
            dst.fmt_operand(),
            val.fmt_operand(),
        )
        .unwrap();
        Ok(dst)
    }

    // ── Predicate conversion ─────────────────────────────────────────

    fn ensure_pred(&mut self, val: &PtxVal) -> Result<PtxVal, CompileError> {
        match val {
            PtxVal::Pred(_) => Ok(val.clone()),
            PtxVal::R32(_) => {
                let p = self.alloc_pred();
                writeln!(
                    self.body,
                    "    setp.ne.u32 {}, {}, 0;",
                    p.fmt_operand(),
                    val.fmt_operand(),
                )
                .unwrap();
                Ok(p)
            }
            _ => Err(CompileError::NotImplemented(
                "PTX: cannot convert to predicate".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ptx_write_42() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = 42u;
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).unwrap();
        assert!(ptx.contains(".version 8.7"));
        assert!(ptx.contains(".target sm_120"));
        assert!(ptx.contains("main_kernel"));
        assert!(ptx.contains("_buf0_ptr"));
        assert!(ptx.contains("st.global"));
        assert!(ptx.contains("42"));
        assert_eq!(result.format, BinaryFormat::Ptx);
        assert_eq!(result.info.local_size, [64, 1, 1]);
    }

    #[test]
    fn ptx_copy_ab() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read> src: array<u32>;

@group(0) @binding(1)
var<storage, read_write> dst: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    dst[gid.x] = src[gid.x];
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).unwrap();
        assert!(ptx.contains("_buf0_ptr"));
        assert!(ptx.contains("_buf1_ptr"));
        assert!(ptx.contains("ld.global"));
        assert!(ptx.contains("st.global"));
    }

    #[test]
    fn ptx_array_length() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> buf: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let len = arrayLength(&buf);
    buf[0] = len;
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).unwrap();
        assert!(ptx.contains("_buf0_size"));
        assert!(ptx.contains("shr.u64") || ptx.contains("div.u64"));
    }

    #[test]
    fn ptx_num_workgroups() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main(@builtin(num_workgroups) nwg: vec3<u32>) {
    out[0] = nwg.x;
    out[1] = nwg.y;
    out[2] = nwg.z;
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).unwrap();
        assert!(ptx.contains("%nctaid.x"));
        assert!(ptx.contains("%nctaid.y"));
        assert!(ptx.contains("%nctaid.z"));
    }
}
