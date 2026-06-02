// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt::Write as _;

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::{ImageDim, PtxVal};

impl PtxEmitter<'_> {
    pub(super) fn eval_expr(
        &mut self,
        h: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
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
                let second = arg1.map(|eh| self.eval_expr(eh)).transpose()?;
                let third = arg2.map(|eh| self.eval_expr(eh)).transpose()?;
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
                } else if let Some(sv) = self.shared_var(gv) {
                    let offset = sv.offset;
                    let addr = self.alloc_rd64();
                    writeln!(self.body, "    mov.u64 {}, _shared;", addr.fmt_operand())
                        .expect("write to String");
                    if offset > 0 {
                        writeln!(
                            self.body,
                            "    add.u64 {0}, {0}, {offset};",
                            addr.fmt_operand()
                        )
                        .expect("write to String");
                    }
                    Ok(addr)
                } else {
                    tracing::debug!("unbound global variable — allocating zero register");
                    Ok(self.alloc_r32())
                }
            }
            naga::Expression::LocalVariable(lv) => {
                if let Some(val) = self.locals.get(&lv).cloned() {
                    Ok(val)
                } else {
                    tracing::debug!("unbound local variable — allocating zero register");
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
                .expect("write to String");
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
                    .expect("write to String");
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
                    Ok(components
                        .into_iter()
                        .next()
                        .expect("swizzle with size 1 has one component"))
                } else {
                    Ok(PtxVal::Vec(components))
                }
            }
            naga::Expression::SubgroupOperationResult { ty } => {
                let scalar = match self.module.types[ty].inner {
                    naga::TypeInner::Scalar(s) => s,
                    _ => naga::Scalar::U32,
                };
                Ok(self.alloc_for_scalar(scalar))
            }
            naga::Expression::SubgroupBallotResult => Ok(self.alloc_r32()),
            naga::Expression::ImageSample {
                image,
                sampler: _,
                gather,
                coordinate,
                array_index,
                offset: _,
                level,
                depth_ref,
                ..
            } => self.eval_image_sample(image, coordinate, array_index, level, depth_ref, gather),
            naga::Expression::ImageLoad {
                image,
                coordinate,
                array_index,
                sample: _,
                level,
            } => self.eval_image_load(image, coordinate, array_index, level),
            naga::Expression::ImageQuery { image, query } => self.eval_image_query(image, query),
            naga::Expression::RayQueryProceedResult => self.eval_ray_query_proceed_result(),
            naga::Expression::RayQueryGetIntersection { query, committed } => {
                self.eval_ray_query_get_intersection(query, committed)
            }
            _ => Err(CompileError::NotImplemented(
                format!("PTX expression: {expr:?}").into(),
            )),
        }?;

        self.values.insert(h, result.clone());
        Ok(result)
    }

    fn eval_image_load(
        &mut self,
        image: naga::Handle<naga::Expression>,
        coordinate: naga::Handle<naga::Expression>,
        array_index: Option<naga::Handle<naga::Expression>>,
        level: Option<naga::Handle<naga::Expression>>,
    ) -> Result<PtxVal, CompileError> {
        let naga::Expression::GlobalVariable(gv_handle) = self.func.expressions[image] else {
            return Err(CompileError::NotImplemented(
                "ImageLoad from non-global image".into(),
            ));
        };

        if let Some(surf_idx) = self.surface_index(gv_handle) {
            return self.eval_surface_load(surf_idx, coordinate);
        }

        if let Some(tex_idx) = self.texture_index(gv_handle) {
            return self.eval_texture_load(tex_idx, coordinate, array_index, level);
        }

        Err(CompileError::InvalidInput(
            "ImageLoad source is not a recognized surface or texture binding".into(),
        ))
    }

    fn eval_surface_load(
        &mut self,
        surf_idx: usize,
        coordinate: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let dim_suffix = self.surfaces[surf_idx].dim.ptx_suffix();
        let type_suffix = self.surfaces[surf_idx].texel_format.ptx_type();
        let comp_count = self.surfaces[surf_idx].texel_format.component_count();

        let coord = self.eval_expr(coordinate)?;

        let coord_str = match &coord {
            super::types::PtxVal::Vec(components) if components.len() >= 2 => {
                format!(
                    "{{{}, {}}}",
                    components[0].fmt_operand(),
                    components[1].fmt_operand()
                )
            }
            _ => format!("{{{}}}", coord.fmt_operand()),
        };

        let dst_components: Vec<PtxVal> = (0..comp_count).map(|_| self.alloc_r32()).collect();
        let dst_str = dst_components
            .iter()
            .map(PtxVal::fmt_operand)
            .collect::<Vec<_>>()
            .join(", ");

        writeln!(
            self.body,
            "    suld.b.{dim_suffix}.{type_suffix}.zero {{{dst_str}}}, [_surf{surf_idx}, {coord_str}];",
        )
        .expect("write to String");

        if comp_count == 1 {
            Ok(dst_components.into_iter().next().expect("component exists"))
        } else {
            Ok(PtxVal::Vec(dst_components))
        }
    }

    fn eval_texture_load(
        &mut self,
        tex_idx: usize,
        coordinate: naga::Handle<naga::Expression>,
        array_index: Option<naga::Handle<naga::Expression>>,
        level: Option<naga::Handle<naga::Expression>>,
    ) -> Result<PtxVal, CompileError> {
        let dim = self.textures[tex_idx].dim;
        let dim_suffix = dim.ptx_suffix();

        let coord = self.eval_expr(coordinate)?;
        let array_val = array_index.map(|ai| self.eval_expr(ai)).transpose()?;
        let coord_str = self.format_tex_coord(&coord, array_val.as_ref(), dim);

        let lod_str = if let Some(lod_handle) = level {
            let lod_val = self.eval_expr(lod_handle)?;
            lod_val.fmt_operand()
        } else {
            String::from("0")
        };

        let dst = [
            self.alloc_r32(),
            self.alloc_r32(),
            self.alloc_r32(),
            self.alloc_r32(),
        ];
        let dst_str = format!(
            "{{{}, {}, {}, {}}}",
            dst[0].fmt_operand(),
            dst[1].fmt_operand(),
            dst[2].fmt_operand(),
            dst[3].fmt_operand(),
        );

        writeln!(
            self.body,
            "    tld.b.{dim_suffix}.v4.s32.f32 {dst_str}, [_tex{tex_idx}, {coord_str}], {lod_str};",
        )
        .expect("write to String");

        Ok(PtxVal::Vec(dst.to_vec()))
    }

    fn eval_image_sample(
        &mut self,
        image: naga::Handle<naga::Expression>,
        coordinate: naga::Handle<naga::Expression>,
        array_index: Option<naga::Handle<naga::Expression>>,
        level: naga::SampleLevel,
        depth_ref: Option<naga::Handle<naga::Expression>>,
        gather: Option<naga::SwizzleComponent>,
    ) -> Result<PtxVal, CompileError> {
        let naga::Expression::GlobalVariable(gv_handle) = self.func.expressions[image] else {
            return Err(CompileError::NotImplemented(
                "ImageSample from non-global image".into(),
            ));
        };
        let tex_idx = self.texture_index(gv_handle).ok_or_else(|| {
            CompileError::InvalidInput(
                "ImageSample source is not a recognized texture binding".into(),
            )
        })?;
        let dim = self.textures[tex_idx].dim;
        let channel_type = self.textures[tex_idx].channel_type;
        let is_depth = self.textures[tex_idx].is_depth;
        let dim_suffix = dim.ptx_suffix();
        let ret_type = channel_type.ptx_suffix();

        let coord = self.eval_expr(coordinate)?;
        let array_val = array_index.map(|ai| self.eval_expr(ai)).transpose()?;
        let coord_str = self.format_tex_coord(&coord, array_val.as_ref(), dim);

        if let Some(component) = gather {
            return self.eval_texture_gather(tex_idx, &coord_str, component, channel_type);
        }

        if let (true, Some(ref_expr)) = (is_depth, depth_ref) {
            return self.eval_depth_compare_sample(
                tex_idx,
                &coord,
                array_val.as_ref(),
                dim,
                &level,
                ref_expr,
            );
        }

        let dst_components: Vec<PtxVal> = (0..4).map(|_| self.alloc_r32()).collect();
        let dst_str = dst_components
            .iter()
            .map(PtxVal::fmt_operand)
            .collect::<Vec<_>>()
            .join(", ");

        match level {
            naga::SampleLevel::Auto | naga::SampleLevel::Zero => {
                writeln!(
                    self.body,
                    "    tex.{dim_suffix}.v4.{ret_type}.{ret_type} {{{dst_str}}}, [_tex{tex_idx}, {coord_str}];",
                )
                .expect("write to String");
            }
            naga::SampleLevel::Exact(lod_expr) => {
                let lod = self.eval_expr(lod_expr)?;
                writeln!(
                    self.body,
                    "    tex.level.{dim_suffix}.v4.{ret_type}.{ret_type} {{{dst_str}}}, [_tex{tex_idx}, {coord_str}], {lod_op};",
                    lod_op = lod.fmt_operand(),
                )
                .expect("write to String");
            }
            naga::SampleLevel::Bias(bias_expr) => {
                let bias = self.eval_expr(bias_expr)?;
                writeln!(
                    self.body,
                    "    tex.level.{dim_suffix}.v4.{ret_type}.{ret_type} {{{dst_str}}}, [_tex{tex_idx}, {coord_str}], {bias_op};",
                    bias_op = bias.fmt_operand(),
                )
                .expect("write to String");
            }
            naga::SampleLevel::Gradient { x, y } => {
                let grad_x = self.eval_expr(x)?;
                let grad_y = self.eval_expr(y)?;
                let grad_x_str = self.format_tex_coord(&grad_x, None, dim);
                let grad_y_str = self.format_tex_coord(&grad_y, None, dim);
                writeln!(
                    self.body,
                    "    tex.grad.{dim_suffix}.v4.{ret_type}.{ret_type} {{{dst_str}}}, [_tex{tex_idx}, {coord_str}], {grad_x_str}, {grad_y_str};",
                )
                .expect("write to String");
            }
        }

        Ok(PtxVal::Vec(dst_components))
    }

    /// Emit a `tex.level.compare.{dim}.f32.f32` instruction for depth
    /// texture comparison (shadow sampling). Returns a scalar f32 result
    /// (0.0 or 1.0) from the hardware comparison unit.
    fn eval_depth_compare_sample(
        &mut self,
        tex_idx: usize,
        coord: &PtxVal,
        array_val: Option<&PtxVal>,
        dim: ImageDim,
        level: &naga::SampleLevel,
        ref_expr: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        let ref_val = self.eval_expr(ref_expr)?;
        let dst = self.alloc_r32();
        let dim_suffix = dim.ptx_suffix();

        let compare_coord = self.format_depth_compare_coord(coord, array_val, &ref_val, dim);

        match level {
            naga::SampleLevel::Auto | naga::SampleLevel::Zero => {
                writeln!(
                    self.body,
                    "    tex.level.compare.{dim_suffix}.f32.f32 {dst_op}, [_tex{tex_idx}, {compare_coord}], 0.0;",
                    dst_op = dst.fmt_operand(),
                )
                .expect("write to String");
            }
            naga::SampleLevel::Exact(lod_expr) => {
                let lod = self.eval_expr(*lod_expr)?;
                writeln!(
                    self.body,
                    "    tex.level.compare.{dim_suffix}.f32.f32 {dst_op}, [_tex{tex_idx}, {compare_coord}], {lod_op};",
                    dst_op = dst.fmt_operand(),
                    lod_op = lod.fmt_operand(),
                )
                .expect("write to String");
            }
            naga::SampleLevel::Bias(_) | naga::SampleLevel::Gradient { .. } => {
                return Err(CompileError::NotImplemented(
                    "depth comparison with bias/gradient not supported in PTX".into(),
                ));
            }
        }

        Ok(dst)
    }

    /// Format coordinate tuple for depth comparison: appends the reference
    /// value as the last coordinate component per PTX ISA convention.
    /// For 2D: `{s, t, ref}`, for cube: `{x, y, z, ref}`.
    fn format_depth_compare_coord(
        &self,
        coord: &PtxVal,
        array_val: Option<&PtxVal>,
        ref_val: &PtxVal,
        dim: ImageDim,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(layer) = array_val {
            parts.push(layer.fmt_operand());
        }

        match coord {
            PtxVal::Vec(components) => {
                let needed = dim.coord_components();
                for c in components.iter().take(needed) {
                    parts.push(c.fmt_operand());
                }
            }
            scalar => {
                parts.push(scalar.fmt_operand());
            }
        }

        parts.push(ref_val.fmt_operand());
        format!("{{{}}}", parts.join(", "))
    }

    fn format_tex_coord(
        &self,
        coord: &PtxVal,
        array_val: Option<&PtxVal>,
        dim: ImageDim,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(layer) = array_val {
            parts.push(layer.fmt_operand());
        }

        match coord {
            PtxVal::Vec(components) => {
                let needed = dim.coord_components();
                for c in components.iter().take(needed) {
                    parts.push(c.fmt_operand());
                }
            }
            scalar => {
                parts.push(scalar.fmt_operand());
            }
        }

        format!("{{{}}}", parts.join(", "))
    }

    fn eval_texture_gather(
        &mut self,
        tex_idx: usize,
        coord_str: &str,
        component: naga::SwizzleComponent,
        channel_type: super::types::TexChannelType,
    ) -> Result<PtxVal, CompileError> {
        let comp_suffix = match component {
            naga::SwizzleComponent::X => "r",
            naga::SwizzleComponent::Y => "g",
            naga::SwizzleComponent::Z => "b",
            naga::SwizzleComponent::W => "a",
        };
        let ret_type = channel_type.ptx_suffix();

        let dst_components: Vec<PtxVal> = (0..4).map(|_| self.alloc_r32()).collect();
        let dst_str = dst_components
            .iter()
            .map(PtxVal::fmt_operand)
            .collect::<Vec<_>>()
            .join(", ");

        writeln!(
            self.body,
            "    tld4.{comp_suffix}.2d.v4.{ret_type}.{ret_type} {{{dst_str}}}, [_tex{tex_idx}, {coord_str}];",
        )
        .expect("write to String");

        Ok(PtxVal::Vec(dst_components))
    }

    fn eval_image_query(
        &mut self,
        image: naga::Handle<naga::Expression>,
        query: naga::ImageQuery,
    ) -> Result<PtxVal, CompileError> {
        let naga::Expression::GlobalVariable(gv_handle) = self.func.expressions[image] else {
            return Err(CompileError::NotImplemented(
                "ImageQuery on non-global image".into(),
            ));
        };

        if let Some(tex_idx) = self.texture_index(gv_handle) {
            return self.eval_texture_query(tex_idx, query);
        }

        let surf_idx = self.surface_index(gv_handle).ok_or_else(|| {
            CompileError::InvalidInput("ImageQuery source is not a recognized binding".into())
        })?;
        let dim = self.surfaces[surf_idx].dim;

        match query {
            naga::ImageQuery::Size { .. } => {
                let width = self.alloc_r32();
                writeln!(
                    self.body,
                    "    suq.width.b32 {}, [_surf{surf_idx}];",
                    width.fmt_operand(),
                )
                .expect("write to String");

                match dim {
                    ImageDim::D1 | ImageDim::A1d => Ok(width),
                    ImageDim::D2 | ImageDim::Cube | ImageDim::A2d | ImageDim::Acube => {
                        let height = self.alloc_r32();
                        writeln!(
                            self.body,
                            "    suq.height.b32 {}, [_surf{surf_idx}];",
                            height.fmt_operand(),
                        )
                        .expect("write to String");
                        Ok(PtxVal::Vec(vec![width, height]))
                    }
                    ImageDim::D3 => {
                        let height = self.alloc_r32();
                        let depth = self.alloc_r32();
                        writeln!(
                            self.body,
                            "    suq.height.b32 {}, [_surf{surf_idx}];",
                            height.fmt_operand(),
                        )
                        .expect("write to String");
                        writeln!(
                            self.body,
                            "    suq.depth.b32 {}, [_surf{surf_idx}];",
                            depth.fmt_operand(),
                        )
                        .expect("write to String");
                        Ok(PtxVal::Vec(vec![width, height, depth]))
                    }
                }
            }
            naga::ImageQuery::NumLevels => Err(CompileError::NotImplemented(
                "ImageQuery::NumLevels on surface (use texref for mipmap queries)".into(),
            )),
            naga::ImageQuery::NumLayers => {
                let dst = self.alloc_r32();
                writeln!(
                    self.body,
                    "    suq.array_size.b32 {}, [_surf{surf_idx}];",
                    dst.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            naga::ImageQuery::NumSamples => Err(CompileError::NotImplemented(
                "ImageQuery::NumSamples (not supported in PTX for surfaces or textures)".into(),
            )),
        }
    }

    fn eval_texture_query(
        &mut self,
        tex_idx: usize,
        query: naga::ImageQuery,
    ) -> Result<PtxVal, CompileError> {
        let dim = self.textures[tex_idx].dim;
        match query {
            naga::ImageQuery::Size { .. } => {
                let width = self.alloc_r32();
                writeln!(
                    self.body,
                    "    txq.width.b32 {}, [_tex{tex_idx}];",
                    width.fmt_operand(),
                )
                .expect("write to String");
                match dim {
                    ImageDim::D1 | ImageDim::A1d => Ok(width),
                    ImageDim::D2 | ImageDim::Cube | ImageDim::A2d | ImageDim::Acube => {
                        let height = self.alloc_r32();
                        writeln!(
                            self.body,
                            "    txq.height.b32 {}, [_tex{tex_idx}];",
                            height.fmt_operand(),
                        )
                        .expect("write to String");
                        Ok(PtxVal::Vec(vec![width, height]))
                    }
                    ImageDim::D3 => {
                        let height = self.alloc_r32();
                        let depth = self.alloc_r32();
                        writeln!(
                            self.body,
                            "    txq.height.b32 {}, [_tex{tex_idx}];",
                            height.fmt_operand(),
                        )
                        .expect("write to String");
                        writeln!(
                            self.body,
                            "    txq.depth.b32 {}, [_tex{tex_idx}];",
                            depth.fmt_operand(),
                        )
                        .expect("write to String");
                        Ok(PtxVal::Vec(vec![width, height, depth]))
                    }
                }
            }
            naga::ImageQuery::NumLevels => {
                let dst = self.alloc_r32();
                writeln!(
                    self.body,
                    "    txq.num_mip_levels.b32 {}, [_tex{tex_idx}];",
                    dst.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            naga::ImageQuery::NumLayers => {
                let dst = self.alloc_r32();
                writeln!(
                    self.body,
                    "    txq.array_size.b32 {}, [_tex{tex_idx}];",
                    dst.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            naga::ImageQuery::NumSamples => Err(CompileError::NotImplemented(
                "ImageQuery::NumSamples (not supported in PTX for textures)".into(),
            )),
        }
    }

    /// The `RayQueryProceedResult` expression reads the boolean result
    /// produced by the most recent `Proceed` statement. Since `emit_ray_query_proceed`
    /// already inserts the predicate into `self.values` via the `result` handle,
    /// this expression should have been resolved from cache. If we reach here,
    /// allocate a default-false predicate as a fallback.
    fn eval_ray_query_proceed_result(&mut self) -> Result<PtxVal, CompileError> {
        let p = self.alloc_pred();
        writeln!(self.body, "    setp.eq.u32 {}, 0, 1;", p.fmt_operand()).expect("write to String");
        Ok(p)
    }

    /// Read intersection data from a ray query. Returns a struct-like
    /// `PtxVal::Vec` with the `RayIntersection` fields:
    /// kind(u32), t(f32), instance_custom_data(u32), instance_index(u32),
    /// sbt_record_offset(u32), geometry_index(u32), primitive_index(u32),
    /// barycentrics(vec2<f32>), front_face(u32).
    ///
    /// The `committed` flag selects between the committed (closest) hit
    /// or the current candidate intersection.
    fn eval_ray_query_get_intersection(
        &mut self,
        query: naga::Handle<naga::Expression>,
        committed: bool,
    ) -> Result<PtxVal, CompileError> {
        let qh = self
            .ray_queries
            .get(&query)
            .map_or(PtxVal::Rd64(0), |s| s.query_handle.clone());

        let kind = self.alloc_r32();
        let t = self.alloc_r32();
        let instance_custom_data = self.alloc_r32();
        let instance_index = self.alloc_r32();
        let sbt_record_offset = self.alloc_r32();
        let geometry_index = self.alloc_r32();
        let primitive_index = self.alloc_r32();
        let bary_x = self.alloc_r32();
        let bary_y = self.alloc_r32();
        let front_face = self.alloc_r32();

        // RT core intersection query: emit calls to driver-resolved builtins.
        // These symbols (_rt_*) are provided by the NVIDIA driver at JIT time
        // when the shader runs on hardware with RT cores (SM75+).
        let committed_flag = u32::from(committed);
        writeln!(
            self.body,
            "    call ({kind}), _rt_query_get_intersection_kind, ({qh}, {cf});",
            kind = kind.fmt_operand(),
            qh = qh.fmt_operand(),
            cf = committed_flag,
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    call ({t}), _rt_query_get_intersection_t, ({qh}, {cf});",
            t = t.fmt_operand(),
            qh = qh.fmt_operand(),
            cf = committed_flag,
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    call ({v}), _rt_query_get_intersection_instance_custom_index, ({qh}, {cf});",
            v = instance_custom_data.fmt_operand(),
            qh = qh.fmt_operand(),
            cf = committed_flag,
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    call ({v}), _rt_query_get_intersection_instance_id, ({qh}, {cf});",
            v = instance_index.fmt_operand(),
            qh = qh.fmt_operand(),
            cf = committed_flag,
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    call ({v}), _rt_query_get_intersection_sbt_offset, ({qh}, {cf});",
            v = sbt_record_offset.fmt_operand(),
            qh = qh.fmt_operand(),
            cf = committed_flag,
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    call ({v}), _rt_query_get_intersection_geometry_index, ({qh}, {cf});",
            v = geometry_index.fmt_operand(),
            qh = qh.fmt_operand(),
            cf = committed_flag,
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    call ({v}), _rt_query_get_intersection_primitive_index, ({qh}, {cf});",
            v = primitive_index.fmt_operand(),
            qh = qh.fmt_operand(),
            cf = committed_flag,
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    call ({bx}, {by}), _rt_query_get_intersection_barycentrics, ({qh}, {cf});",
            bx = bary_x.fmt_operand(),
            by = bary_y.fmt_operand(),
            qh = qh.fmt_operand(),
            cf = committed_flag,
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    call ({v}), _rt_query_get_intersection_front_face, ({qh}, {cf});",
            v = front_face.fmt_operand(),
            qh = qh.fmt_operand(),
            cf = committed_flag,
        )
        .expect("write to String");

        Ok(PtxVal::Vec(vec![
            kind,
            t,
            instance_custom_data,
            instance_index,
            sbt_record_offset,
            geometry_index,
            primitive_index,
            PtxVal::Vec(vec![bary_x, bary_y]),
            front_face,
        ]))
    }
}
