// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt::Write as _;

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::PtxVal;

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
            naga::Expression::ImageLoad {
                image,
                coordinate,
                array_index: _,
                sample: _,
                level: _,
            } => self.eval_image_load(image, coordinate),
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
    ) -> Result<PtxVal, CompileError> {
        let naga::Expression::GlobalVariable(gv_handle) = self.func.expressions[image] else {
            return Err(CompileError::NotImplemented(
                "ImageLoad from non-global image".into(),
            ));
        };
        let surf_idx = self.surface_index(gv_handle).ok_or_else(|| {
            CompileError::InvalidInput(
                "ImageLoad source is not a recognized surface binding".into(),
            )
        })?;
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
}
