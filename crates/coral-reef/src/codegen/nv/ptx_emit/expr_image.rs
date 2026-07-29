// SPDX-License-Identifier: AGPL-3.0-or-later
//! PTX image/texture/surface evaluation — load, sample, query operations.

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::{ImageDim, PtxVal};

impl PtxEmitter<'_> {
    pub(super) fn eval_image_load(
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

        writeln_ptx!(
            self.body,
            "    suld.b.{dim_suffix}.{type_suffix}.zero {{{dst_str}}}, [_surf{surf_idx}, {coord_str}];",
        );

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

        writeln_ptx!(
            self.body,
            "    tld.b.{dim_suffix}.v4.s32.f32 {dst_str}, [_tex{tex_idx}, {coord_str}], {lod_str};",
        );

        Ok(PtxVal::Vec(dst.to_vec()))
    }

    pub(super) fn eval_image_sample(
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
                writeln_ptx!(
                    self.body,
                    "    tex.{dim_suffix}.v4.{ret_type}.{ret_type} {{{dst_str}}}, [_tex{tex_idx}, {coord_str}];",
                );
            }
            naga::SampleLevel::Exact(lod_expr) => {
                let lod = self.eval_expr(lod_expr)?;
                writeln_ptx!(
                    self.body,
                    "    tex.level.{dim_suffix}.v4.{ret_type}.{ret_type} {{{dst_str}}}, [_tex{tex_idx}, {coord_str}], {lod_op};",
                    lod_op = lod.fmt_operand(),
                );
            }
            naga::SampleLevel::Bias(bias_expr) => {
                let bias = self.eval_expr(bias_expr)?;
                writeln_ptx!(
                    self.body,
                    "    tex.level.{dim_suffix}.v4.{ret_type}.{ret_type} {{{dst_str}}}, [_tex{tex_idx}, {coord_str}], {bias_op};",
                    bias_op = bias.fmt_operand(),
                );
            }
            naga::SampleLevel::Gradient { x, y } => {
                let grad_x = self.eval_expr(x)?;
                let grad_y = self.eval_expr(y)?;
                let grad_x_str = self.format_tex_coord(&grad_x, None, dim);
                let grad_y_str = self.format_tex_coord(&grad_y, None, dim);
                writeln_ptx!(
                    self.body,
                    "    tex.grad.{dim_suffix}.v4.{ret_type}.{ret_type} {{{dst_str}}}, [_tex{tex_idx}, {coord_str}], {grad_x_str}, {grad_y_str};",
                );
            }
        }

        Ok(PtxVal::Vec(dst_components))
    }

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
                writeln_ptx!(
                    self.body,
                    "    tex.level.compare.{dim_suffix}.f32.f32 {dst_op}, [_tex{tex_idx}, {compare_coord}], 0.0;",
                    dst_op = dst.fmt_operand(),
                );
            }
            naga::SampleLevel::Exact(lod_expr) => {
                let lod = self.eval_expr(*lod_expr)?;
                writeln_ptx!(
                    self.body,
                    "    tex.level.compare.{dim_suffix}.f32.f32 {dst_op}, [_tex{tex_idx}, {compare_coord}], {lod_op};",
                    dst_op = dst.fmt_operand(),
                    lod_op = lod.fmt_operand(),
                );
            }
            naga::SampleLevel::Bias(_) | naga::SampleLevel::Gradient { .. } => {
                return Err(CompileError::NotImplemented(
                    "depth comparison with bias/gradient not supported in PTX".into(),
                ));
            }
        }

        Ok(dst)
    }

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

    pub(super) fn format_tex_coord(
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

        writeln_ptx!(
            self.body,
            "    tld4.{comp_suffix}.2d.v4.{ret_type}.{ret_type} {{{dst_str}}}, [_tex{tex_idx}, {coord_str}];",
        );

        Ok(PtxVal::Vec(dst_components))
    }

    pub(super) fn eval_image_query(
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
                writeln_ptx!(
                    self.body,
                    "    suq.width.b32 {}, [_surf{surf_idx}];",
                    width.fmt_operand()
                );
                match dim {
                    ImageDim::D1 | ImageDim::A1d => Ok(width),
                    ImageDim::D2 | ImageDim::Cube | ImageDim::A2d | ImageDim::Acube => {
                        let height = self.alloc_r32();
                        writeln_ptx!(
                            self.body,
                            "    suq.height.b32 {}, [_surf{surf_idx}];",
                            height.fmt_operand()
                        );
                        Ok(PtxVal::Vec(vec![width, height]))
                    }
                    ImageDim::D3 => {
                        let height = self.alloc_r32();
                        let depth = self.alloc_r32();
                        writeln_ptx!(
                            self.body,
                            "    suq.height.b32 {}, [_surf{surf_idx}];",
                            height.fmt_operand()
                        );
                        writeln_ptx!(
                            self.body,
                            "    suq.depth.b32 {}, [_surf{surf_idx}];",
                            depth.fmt_operand()
                        );
                        Ok(PtxVal::Vec(vec![width, height, depth]))
                    }
                }
            }
            naga::ImageQuery::NumLevels => Err(CompileError::NotImplemented(
                "ImageQuery::NumLevels on surface (use texref for mipmap queries)".into(),
            )),
            naga::ImageQuery::NumLayers => {
                let dst = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    suq.array_size.b32 {}, [_surf{surf_idx}];",
                    dst.fmt_operand()
                );
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
                writeln_ptx!(
                    self.body,
                    "    txq.width.b32 {}, [_tex{tex_idx}];",
                    width.fmt_operand()
                );
                match dim {
                    ImageDim::D1 | ImageDim::A1d => Ok(width),
                    ImageDim::D2 | ImageDim::Cube | ImageDim::A2d | ImageDim::Acube => {
                        let height = self.alloc_r32();
                        writeln_ptx!(
                            self.body,
                            "    txq.height.b32 {}, [_tex{tex_idx}];",
                            height.fmt_operand()
                        );
                        Ok(PtxVal::Vec(vec![width, height]))
                    }
                    ImageDim::D3 => {
                        let height = self.alloc_r32();
                        let depth = self.alloc_r32();
                        writeln_ptx!(
                            self.body,
                            "    txq.height.b32 {}, [_tex{tex_idx}];",
                            height.fmt_operand()
                        );
                        writeln_ptx!(
                            self.body,
                            "    txq.depth.b32 {}, [_tex{tex_idx}];",
                            depth.fmt_operand()
                        );
                        Ok(PtxVal::Vec(vec![width, height, depth]))
                    }
                }
            }
            naga::ImageQuery::NumLevels => {
                let dst = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    txq.num_mip_levels.b32 {}, [_tex{tex_idx}];",
                    dst.fmt_operand()
                );
                Ok(dst)
            }
            naga::ImageQuery::NumLayers => {
                let dst = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    txq.array_size.b32 {}, [_tex{tex_idx}];",
                    dst.fmt_operand()
                );
                Ok(dst)
            }
            naga::ImageQuery::NumSamples => Err(CompileError::NotImplemented(
                "ImageQuery::NumSamples (not supported in PTX for textures)".into(),
            )),
        }
    }
}
