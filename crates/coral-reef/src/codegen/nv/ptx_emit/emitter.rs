// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt::Write as _;

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::{
    BufferBinding, ImageDim, PtxVal, SharedVar, SurfaceBinding, TexChannelType, TexelFormat,
    TextureBinding,
};

#[allow(
    clippy::elidable_lifetime_names,
    reason = "Shared lifetime ties module + entry point in Self; elided impl body breaks constructor"
)]
impl<'a> PtxEmitter<'a> {
    pub(super) fn new(module: &'a naga::Module, ep: &'a naga::EntryPoint, sm: u8) -> Self {
        Self {
            module,
            func: &ep.function,
            sm,
            workgroup_size: ep.workgroup_size,
            bindings: Vec::new(),
            surfaces: Vec::new(),
            textures: Vec::new(),
            shared_vars: Vec::new(),
            r32_next: 0,
            rd64_next: 0,
            pred_next: 0,
            label_next: 0,
            values: std::collections::HashMap::new(),
            locals: std::collections::HashMap::new(),
            gv_ptr_regs: std::collections::HashMap::new(),
            body: String::with_capacity(4096),
            shared_mem_bytes: 0,
            barrier_count: 0,
            inline_depth: 0,
            inline_return_val: None,
        }
    }

    pub(super) fn alloc_r32(&mut self) -> PtxVal {
        let id = self.r32_next;
        self.r32_next += 1;
        PtxVal::R32(id)
    }

    pub(super) fn alloc_rd64(&mut self) -> PtxVal {
        let id = self.rd64_next;
        self.rd64_next += 1;
        PtxVal::Rd64(id)
    }

    pub(super) fn alloc_pred(&mut self) -> PtxVal {
        let id = self.pred_next;
        self.pred_next += 1;
        PtxVal::Pred(id)
    }

    pub(super) fn alloc_label(&mut self) -> u32 {
        let id = self.label_next;
        self.label_next += 1;
        id
    }

    pub(super) fn alloc_for_scalar(&mut self, scalar: naga::Scalar) -> PtxVal {
        if scalar.width == 8 {
            self.alloc_rd64()
        } else {
            self.alloc_r32()
        }
    }

    pub(super) fn resolve_expr_type_handle(
        &self,
        h: naga::Handle<naga::Expression>,
    ) -> naga::Handle<naga::Type> {
        let expr = &self.func.expressions[h];
        match *expr {
            naga::Expression::GlobalVariable(gv) => self.module.global_variables[gv].ty,
            naga::Expression::LocalVariable(lv) => self.func.local_variables[lv].ty,
            naga::Expression::FunctionArgument(idx) => self.func.arguments[idx as usize].ty,
            naga::Expression::Literal(ref lit) => {
                self.scalar_type_handle(crate::codegen::naga_translate::lit_scalar(lit))
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
                    crate::codegen::ice!("module has no types");
                },
                |(h, _)| h,
            ),
        }
    }

    pub(super) fn scalar_type_handle(&self, scalar: naga::Scalar) -> naga::Handle<naga::Type> {
        for (handle, ty) in self.module.types.iter() {
            if ty.inner == naga::TypeInner::Scalar(scalar) {
                return handle;
            }
        }
        self.module
            .types
            .iter()
            .next()
            .expect("module has at least one type handle for scalar fallback")
            .0
    }

    pub(super) fn resolve_expr_type(&self, h: naga::Handle<naga::Expression>) -> &naga::TypeInner {
        let th = self.resolve_expr_type_handle(h);
        &self.module.types[th].inner
    }

    pub(super) fn inner_type(&self, th: naga::Handle<naga::Type>) -> &naga::TypeInner {
        &self.module.types[th].inner
    }

    pub(super) fn scalar_of(&self, h: naga::Handle<naga::Expression>) -> naga::Scalar {
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

    pub(super) fn ptx_type_suffix(scalar: naga::Scalar) -> &'static str {
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

    pub(super) fn ptx_mem_suffix(scalar: naga::Scalar) -> &'static str {
        match scalar.width {
            8 => "u64",
            4 => "u32",
            2 => "u16",
            1 => "u8",
            _ => "u32",
        }
    }

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

    pub(super) fn element_stride_of(&self, ty_handle: naga::Handle<naga::Type>) -> u32 {
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

    fn collect_surfaces(&mut self) {
        let mut surfaces = Vec::new();
        for (handle, gv) in self.module.global_variables.iter() {
            if gv.space != naga::AddressSpace::Handle {
                continue;
            }
            let ty_inner = &self.module.types[gv.ty].inner;
            if let naga::TypeInner::Image { dim, class, .. } = *ty_inner {
                let binding_idx = gv.binding.as_ref().map_or(0, |b| b.binding);
                let image_dim = match dim {
                    naga::ImageDimension::D1 => ImageDim::D1,
                    naga::ImageDimension::D2 | naga::ImageDimension::Cube => ImageDim::D2,
                    naga::ImageDimension::D3 => ImageDim::D3,
                };
                let texel_format = match class {
                    naga::ImageClass::Storage { format, .. } => match format {
                        naga::StorageFormat::R8Unorm
                        | naga::StorageFormat::R8Snorm
                        | naga::StorageFormat::R8Uint
                        | naga::StorageFormat::R8Sint => TexelFormat::R8,
                        naga::StorageFormat::R16Uint
                        | naga::StorageFormat::R16Sint
                        | naga::StorageFormat::R16Float
                        | naga::StorageFormat::R16Unorm
                        | naga::StorageFormat::R16Snorm => TexelFormat::R16,
                        naga::StorageFormat::R32Uint
                        | naga::StorageFormat::R32Sint
                        | naga::StorageFormat::R32Float => TexelFormat::R32,
                        naga::StorageFormat::Rg8Unorm
                        | naga::StorageFormat::Rg8Snorm
                        | naga::StorageFormat::Rg8Uint
                        | naga::StorageFormat::Rg8Sint => TexelFormat::Rg8,
                        naga::StorageFormat::Rg16Uint
                        | naga::StorageFormat::Rg16Sint
                        | naga::StorageFormat::Rg16Float
                        | naga::StorageFormat::Rg16Unorm
                        | naga::StorageFormat::Rg16Snorm => TexelFormat::Rg16,
                        naga::StorageFormat::Rg32Uint
                        | naga::StorageFormat::Rg32Sint
                        | naga::StorageFormat::Rg32Float => TexelFormat::Rg32,
                        naga::StorageFormat::Rgba8Unorm
                        | naga::StorageFormat::Rgba8Snorm
                        | naga::StorageFormat::Rgba8Uint
                        | naga::StorageFormat::Rgba8Sint => TexelFormat::Rgba8,
                        naga::StorageFormat::Bgra8Unorm => TexelFormat::Bgra8,
                        naga::StorageFormat::Rgba16Uint
                        | naga::StorageFormat::Rgba16Sint
                        | naga::StorageFormat::Rgba16Float
                        | naga::StorageFormat::Rgba16Unorm
                        | naga::StorageFormat::Rgba16Snorm => TexelFormat::Rgba16,
                        naga::StorageFormat::Rgba32Uint
                        | naga::StorageFormat::Rgba32Sint
                        | naga::StorageFormat::Rgba32Float => TexelFormat::Rgba32,
                        _ => TexelFormat::Rgba32,
                    },
                    _ => TexelFormat::Rgba32,
                };
                surfaces.push(SurfaceBinding {
                    binding: binding_idx,
                    gv_handle: handle,
                    dim: image_dim,
                    texel_format,
                });
            }
        }
        surfaces.sort_by_key(|s| s.binding);
        self.surfaces = surfaces;
    }

    fn collect_textures(&mut self) {
        let mut textures = Vec::new();
        for (handle, gv) in self.module.global_variables.iter() {
            if gv.space != naga::AddressSpace::Handle {
                continue;
            }
            let ty_inner = &self.module.types[gv.ty].inner;
            if let naga::TypeInner::Image { dim, class, .. } = *ty_inner {
                let (channel_type, is_depth) = match class {
                    naga::ImageClass::Sampled { kind, .. } => {
                        let ct = match kind {
                            naga::ScalarKind::Float => TexChannelType::F32,
                            naga::ScalarKind::Sint => TexChannelType::S32,
                            naga::ScalarKind::Uint => TexChannelType::U32,
                            _ => TexChannelType::F32,
                        };
                        (ct, false)
                    }
                    naga::ImageClass::Depth { .. } => (TexChannelType::F32, true),
                    naga::ImageClass::Storage { .. } | naga::ImageClass::External => continue,
                };
                let binding_idx = gv.binding.as_ref().map_or(0, |b| b.binding);
                let image_dim = match dim {
                    naga::ImageDimension::D1 => ImageDim::D1,
                    naga::ImageDimension::D2 | naga::ImageDimension::Cube => ImageDim::D2,
                    naga::ImageDimension::D3 => ImageDim::D3,
                };
                textures.push(TextureBinding {
                    binding: binding_idx,
                    gv_handle: handle,
                    dim: image_dim,
                    channel_type,
                    is_depth,
                });
            }
        }
        textures.sort_by_key(|t| t.binding);
        self.textures = textures;
    }

    fn write_surface_decls(&self, out: &mut String) {
        for (i, surf) in self.surfaces.iter().enumerate() {
            writeln!(out, ".global .surfref _surf{i};").expect("write to String");
            let _ = surf;
        }
        if !self.surfaces.is_empty() {
            writeln!(out).expect("write to String");
        }
    }

    fn write_texture_decls(&self, out: &mut String) {
        for (i, tex) in self.textures.iter().enumerate() {
            writeln!(out, ".global .texref _tex{i};").expect("write to String");
            let _ = tex;
        }
        if !self.textures.is_empty() {
            writeln!(out).expect("write to String");
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

    pub(super) fn binding_index(&self, gv: naga::Handle<naga::GlobalVariable>) -> Option<usize> {
        self.bindings.iter().position(|b| b.gv_handle == gv)
    }

    pub(super) fn surface_index(&self, gv: naga::Handle<naga::GlobalVariable>) -> Option<usize> {
        self.surfaces.iter().position(|s| s.gv_handle == gv)
    }

    pub(super) fn texture_index(&self, gv: naga::Handle<naga::GlobalVariable>) -> Option<usize> {
        self.textures.iter().position(|t| t.gv_handle == gv)
    }

    pub(super) fn shared_var(&self, gv: naga::Handle<naga::GlobalVariable>) -> Option<&SharedVar> {
        self.shared_vars.iter().find(|s| s.gv_handle == gv)
    }

    pub(super) fn emit(&mut self) -> Result<String, CompileError> {
        self.collect_bindings();
        self.collect_surfaces();
        self.collect_textures();
        self.collect_shared_vars();
        self.init_locals();
        self.precompute_builtins()?;
        self.load_buffer_params();
        self.emit_block(&self.func.body.clone())?;
        writeln!(self.body, "    ret;").expect("write to String");

        let mut out = String::with_capacity(512 + self.body.len());
        self.write_header(&mut out);
        self.write_surface_decls(&mut out);
        self.write_texture_decls(&mut out);
        self.write_params(&mut out);
        writeln!(out, "{{").expect("write to String");
        self.write_reg_decls(&mut out);
        self.write_shared_decls(&mut out);
        out.push_str(&self.body);
        writeln!(out, "}}").expect("write to String");
        Ok(out)
    }

    fn write_header(&self, out: &mut String) {
        writeln!(out, ".version 8.7").expect("write to String");
        writeln!(out, ".target sm_{}", self.sm).expect("write to String");
        writeln!(out, ".address_size 64").expect("write to String");
        writeln!(out).expect("write to String");
    }

    fn write_params(&self, out: &mut String) {
        writeln!(out, ".visible .entry main_kernel(").expect("write to String");
        let param_count = self.bindings.len() * 2;
        for (i, binding) in self.bindings.iter().enumerate() {
            let idx = binding.binding;
            let comma = if (i * 2 + 1) < param_count - 1 {
                ","
            } else {
                ""
            };
            writeln!(out, "    .param .u64 _buf{idx}_ptr,").expect("write to String");
            writeln!(out, "    .param .u64 _buf{idx}_size{comma}").expect("write to String");
        }
        writeln!(out, ")").expect("write to String");
    }

    fn write_reg_decls(&self, out: &mut String) {
        if self.r32_next > 0 {
            writeln!(out, "    .reg .b32 %r<{}>;", self.r32_next).expect("write to String");
        }
        if self.rd64_next > 0 {
            writeln!(out, "    .reg .b64 %rd<{}>;", self.rd64_next).expect("write to String");
        }
        if self.pred_next > 0 {
            writeln!(out, "    .reg .pred %p<{}>;", self.pred_next).expect("write to String");
        }
    }

    fn write_shared_decls(&self, out: &mut String) {
        if self.shared_mem_bytes > 0 {
            writeln!(
                out,
                "    .shared .align 4 .b8 _shared[{}];",
                self.shared_mem_bytes
            )
            .expect("write to String");
        }
    }

    fn init_locals(&mut self) {
        let local_vars: Vec<_> = self.func.local_variables.iter().collect();
        for (handle, lv) in local_vars {
            let val = self.alloc_for_type(lv.ty);
            self.zero_val(&val);
            self.locals.insert(handle, val);
        }
    }

    pub(super) fn alloc_for_type(&mut self, ty: naga::Handle<naga::Type>) -> PtxVal {
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

    pub(super) fn zero_val(&mut self, val: &PtxVal) {
        match val {
            PtxVal::R32(_) => {
                writeln!(self.body, "    mov.u32 {}, 0;", val.fmt_operand())
                    .expect("write to String");
            }
            PtxVal::Rd64(_) => {
                writeln!(self.body, "    mov.u64 {}, 0;", val.fmt_operand())
                    .expect("write to String");
            }
            PtxVal::Pred(_) => {
                writeln!(self.body, "    setp.eq.u32 {}, 0, 0;", val.fmt_operand())
                    .expect("write to String");
            }
            PtxVal::Vec(v) => {
                for c in v {
                    self.zero_val(c);
                }
            }
        }
    }
}
