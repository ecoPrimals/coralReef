// SPDX-License-Identifier: AGPL-3.0-or-later
//! PTX pack/unpack, matrix, and struct-returning math builtins.
//!
//! Handles naga `MathFunction` variants that the main `math.rs` and
//! `math_ext.rs` don't cover: Pack/Unpack (10 variants), Transpose,
//! Determinant, Inverse, and struct-returning Modf/Frexp fixes.

use naga::MathFunction as MF;

use super::PtxEmitter;
use super::types::PtxVal;
use crate::error::CompileError;

impl PtxEmitter<'_> {
    /// Evaluate pack/unpack and matrix math builtins.
    ///
    /// Returns `Ok(Some(val))` if handled, `Ok(None)` if not our domain.
    pub(super) fn eval_math_pack(
        &mut self,
        fun: MF,
        arg: &PtxVal,
    ) -> Result<Option<PtxVal>, CompileError> {
        let arg = arg.clone();
        match fun {
            MF::Pack4x8unorm => Ok(Some(self.emit_pack4x8_unorm(arg)?)),
            MF::Pack4x8snorm => Ok(Some(self.emit_pack4x8_snorm(arg)?)),
            MF::Unpack4x8unorm => Ok(Some(self.emit_unpack4x8_unorm(arg)?)),
            MF::Unpack4x8snorm => Ok(Some(self.emit_unpack4x8_snorm(arg)?)),
            MF::Pack2x16float => Ok(Some(self.emit_pack2x16_float(arg)?)),
            MF::Unpack2x16float => Ok(Some(self.emit_unpack2x16_float(arg)?)),
            MF::Pack2x16unorm => Ok(Some(self.emit_pack2x16_unorm(arg)?)),
            MF::Pack2x16snorm => Ok(Some(self.emit_pack2x16_snorm(arg)?)),
            MF::Unpack2x16unorm => Ok(Some(self.emit_unpack2x16_unorm(arg)?)),
            MF::Unpack2x16snorm => Ok(Some(self.emit_unpack2x16_snorm(arg)?)),
            MF::Transpose => Ok(Some(self.emit_transpose(arg)?)),
            MF::Determinant => Ok(Some(self.emit_determinant(arg)?)),
            MF::Inverse => Ok(Some(self.emit_matrix_inverse(arg)?)),
            _ => Ok(None),
        }
    }

    // ─── Pack4x8 ───────────────────────────────────────────────────

    /// `pack4x8unorm(vec4<f32>) -> u32`
    /// Each f32 component: clamp [0,1], scale by 255, convert to u8, pack.
    fn emit_pack4x8_unorm(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let components = self.extract_vec_components(&arg, 4);
        let dst = self.alloc_r32();
        writeln!(self.body, "    mov.u32 {}, 0;", dst.fmt_operand()).expect("write to String");
        for (i, comp) in components.iter().enumerate() {
            let clamped = self.alloc_r32();
            let scaled = self.alloc_r32();
            let byte = self.alloc_r32();
            writeln!(
                self.body,
                "    max.f32 {}, {}, 0f00000000;",
                clamped.fmt_operand(),
                comp.fmt_operand(),
            )
            .expect("write to String");
            writeln!(
                self.body,
                "    min.f32 {}, {}, 0f3F800000;",
                clamped.fmt_operand(),
                clamped.fmt_operand(),
            )
            .expect("write to String");
            writeln!(
                self.body,
                "    mul.f32 {}, {}, 0f437F0000;",
                scaled.fmt_operand(),
                clamped.fmt_operand(),
            )
            .expect("write to String");
            writeln!(
                self.body,
                "    cvt.rni.u32.f32 {}, {};",
                byte.fmt_operand(),
                scaled.fmt_operand(),
            )
            .expect("write to String");
            if i > 0 {
                writeln!(
                    self.body,
                    "    shl.b32 {}, {}, {};",
                    byte.fmt_operand(),
                    byte.fmt_operand(),
                    i * 8,
                )
                .expect("write to String");
            }
            writeln!(
                self.body,
                "    or.b32 {}, {}, {};",
                dst.fmt_operand(),
                dst.fmt_operand(),
                byte.fmt_operand(),
            )
            .expect("write to String");
        }
        Ok(dst)
    }

    /// `pack4x8snorm(vec4<f32>) -> u32`
    fn emit_pack4x8_snorm(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let components = self.extract_vec_components(&arg, 4);
        let dst = self.alloc_r32();
        writeln!(self.body, "    mov.u32 {}, 0;", dst.fmt_operand()).expect("write to String");
        for (i, comp) in components.iter().enumerate() {
            let clamped = self.alloc_r32();
            let scaled = self.alloc_r32();
            let byte_s = self.alloc_r32();
            let byte_u = self.alloc_r32();
            // clamp to [-1, 1]
            writeln!(
                self.body,
                "    max.f32 {}, {}, 0fBF800000;",
                clamped.fmt_operand(),
                comp.fmt_operand(),
            )
            .expect("write to String");
            writeln!(
                self.body,
                "    min.f32 {}, {}, 0f3F800000;",
                clamped.fmt_operand(),
                clamped.fmt_operand(),
            )
            .expect("write to String");
            // scale by 127
            writeln!(
                self.body,
                "    mul.f32 {}, {}, 0f42FE0000;",
                scaled.fmt_operand(),
                clamped.fmt_operand(),
            )
            .expect("write to String");
            writeln!(
                self.body,
                "    cvt.rni.s32.f32 {}, {};",
                byte_s.fmt_operand(),
                scaled.fmt_operand(),
            )
            .expect("write to String");
            // mask to 8 bits
            writeln!(
                self.body,
                "    and.b32 {}, {}, 0xFF;",
                byte_u.fmt_operand(),
                byte_s.fmt_operand(),
            )
            .expect("write to String");
            if i > 0 {
                writeln!(
                    self.body,
                    "    shl.b32 {}, {}, {};",
                    byte_u.fmt_operand(),
                    byte_u.fmt_operand(),
                    i * 8,
                )
                .expect("write to String");
            }
            writeln!(
                self.body,
                "    or.b32 {}, {}, {};",
                dst.fmt_operand(),
                dst.fmt_operand(),
                byte_u.fmt_operand(),
            )
            .expect("write to String");
        }
        Ok(dst)
    }

    /// `unpack4x8unorm(u32) -> vec4<f32>`
    fn emit_unpack4x8_unorm(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let mut components = Vec::with_capacity(4);
        for i in 0..4u32 {
            let byte = self.alloc_r32();
            let shifted = if i > 0 {
                let s = self.alloc_r32();
                writeln!(
                    self.body,
                    "    shr.u32 {}, {}, {};",
                    s.fmt_operand(),
                    arg.fmt_operand(),
                    i * 8,
                )
                .expect("write to String");
                s
            } else {
                arg.clone()
            };
            writeln!(
                self.body,
                "    and.b32 {}, {}, 0xFF;",
                byte.fmt_operand(),
                shifted.fmt_operand(),
            )
            .expect("write to String");
            let f = self.alloc_r32();
            writeln!(
                self.body,
                "    cvt.rn.f32.u32 {}, {};",
                f.fmt_operand(),
                byte.fmt_operand(),
            )
            .expect("write to String");
            let scaled = self.alloc_r32();
            // 1.0/255.0 = 0x3B808081
            writeln!(
                self.body,
                "    mul.f32 {}, {}, 0f3B808081;",
                scaled.fmt_operand(),
                f.fmt_operand(),
            )
            .expect("write to String");
            components.push(scaled);
        }
        Ok(PtxVal::Vec(components))
    }

    /// `unpack4x8snorm(u32) -> vec4<f32>`
    fn emit_unpack4x8_snorm(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let mut components = Vec::with_capacity(4);
        for i in 0..4u32 {
            let byte = self.alloc_r32();
            let shifted = if i > 0 {
                let s = self.alloc_r32();
                writeln!(
                    self.body,
                    "    shr.u32 {}, {}, {};",
                    s.fmt_operand(),
                    arg.fmt_operand(),
                    i * 8,
                )
                .expect("write to String");
                s
            } else {
                arg.clone()
            };
            // Extract byte and sign-extend from i8 to i32
            writeln!(
                self.body,
                "    and.b32 {}, {}, 0xFF;",
                byte.fmt_operand(),
                shifted.fmt_operand(),
            )
            .expect("write to String");
            let sign_ext = self.alloc_r32();
            writeln!(
                self.body,
                "    cvt.s32.s8 {}, {};",
                sign_ext.fmt_operand(),
                byte.fmt_operand(),
            )
            .expect("write to String");
            let f = self.alloc_r32();
            writeln!(
                self.body,
                "    cvt.rn.f32.s32 {}, {};",
                f.fmt_operand(),
                sign_ext.fmt_operand(),
            )
            .expect("write to String");
            let scaled = self.alloc_r32();
            // max(val / 127.0, -1.0)
            // 1.0/127.0 = 0x3C010204
            writeln!(
                self.body,
                "    mul.f32 {}, {}, 0f3C010204;",
                scaled.fmt_operand(),
                f.fmt_operand(),
            )
            .expect("write to String");
            let clamped = self.alloc_r32();
            writeln!(
                self.body,
                "    max.f32 {}, {}, 0fBF800000;",
                clamped.fmt_operand(),
                scaled.fmt_operand(),
            )
            .expect("write to String");
            components.push(clamped);
        }
        Ok(PtxVal::Vec(components))
    }

    // ─── Pack2x16 float ────────────────────────────────────────────

    /// `pack2x16float(vec2<f32>) -> u32`
    fn emit_pack2x16_float(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let components = self.extract_vec_components(&arg, 2);
        let lo = self.alloc_r32();
        let hi = self.alloc_r32();
        // Convert f32 to f16 (as u16 in a u32 register)
        writeln!(
            self.body,
            "    cvt.rn.f16.f32 {}, {};",
            lo.fmt_operand(),
            components[0].fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    cvt.rn.f16.f32 {}, {};",
            hi.fmt_operand(),
            components[1].fmt_operand(),
        )
        .expect("write to String");
        // Pack: dst = (hi << 16) | (lo & 0xFFFF)
        let dst = self.alloc_r32();
        writeln!(
            self.body,
            "    and.b32 {}, {}, 0xFFFF;",
            lo.fmt_operand(),
            lo.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    shl.b32 {}, {}, 16;",
            hi.fmt_operand(),
            hi.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    or.b32 {}, {}, {};",
            dst.fmt_operand(),
            lo.fmt_operand(),
            hi.fmt_operand(),
        )
        .expect("write to String");
        Ok(dst)
    }

    /// `unpack2x16float(u32) -> vec2<f32>`
    fn emit_unpack2x16_float(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let lo_bits = self.alloc_r32();
        let hi_bits = self.alloc_r32();
        writeln!(
            self.body,
            "    and.b32 {}, {}, 0xFFFF;",
            lo_bits.fmt_operand(),
            arg.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    shr.u32 {}, {}, 16;",
            hi_bits.fmt_operand(),
            arg.fmt_operand(),
        )
        .expect("write to String");
        let lo_f32 = self.alloc_r32();
        let hi_f32 = self.alloc_r32();
        writeln!(
            self.body,
            "    cvt.f32.f16 {}, {};",
            lo_f32.fmt_operand(),
            lo_bits.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    cvt.f32.f16 {}, {};",
            hi_f32.fmt_operand(),
            hi_bits.fmt_operand(),
        )
        .expect("write to String");
        Ok(PtxVal::Vec(vec![lo_f32, hi_f32]))
    }

    // ─── Pack2x16 unorm/snorm ──────────────────────────────────────

    /// `pack2x16unorm(vec2<f32>) -> u32`
    fn emit_pack2x16_unorm(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let components = self.extract_vec_components(&arg, 2);
        let dst = self.alloc_r32();
        writeln!(self.body, "    mov.u32 {}, 0;", dst.fmt_operand()).expect("write to String");
        for (i, comp) in components.iter().enumerate() {
            let clamped = self.alloc_r32();
            let scaled = self.alloc_r32();
            let half = self.alloc_r32();
            writeln!(
                self.body,
                "    max.f32 {}, {}, 0f00000000;",
                clamped.fmt_operand(),
                comp.fmt_operand(),
            )
            .expect("write to String");
            writeln!(
                self.body,
                "    min.f32 {}, {}, 0f3F800000;",
                clamped.fmt_operand(),
                clamped.fmt_operand(),
            )
            .expect("write to String");
            // 65535.0 = 0x477FFF00
            writeln!(
                self.body,
                "    mul.f32 {}, {}, 0f477FFF00;",
                scaled.fmt_operand(),
                clamped.fmt_operand(),
            )
            .expect("write to String");
            writeln!(
                self.body,
                "    cvt.rni.u32.f32 {}, {};",
                half.fmt_operand(),
                scaled.fmt_operand(),
            )
            .expect("write to String");
            if i > 0 {
                writeln!(
                    self.body,
                    "    shl.b32 {}, {}, 16;",
                    half.fmt_operand(),
                    half.fmt_operand(),
                )
                .expect("write to String");
            }
            writeln!(
                self.body,
                "    or.b32 {}, {}, {};",
                dst.fmt_operand(),
                dst.fmt_operand(),
                half.fmt_operand(),
            )
            .expect("write to String");
        }
        Ok(dst)
    }

    /// `pack2x16snorm(vec2<f32>) -> u32`
    fn emit_pack2x16_snorm(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let components = self.extract_vec_components(&arg, 2);
        let dst = self.alloc_r32();
        writeln!(self.body, "    mov.u32 {}, 0;", dst.fmt_operand()).expect("write to String");
        for (i, comp) in components.iter().enumerate() {
            let clamped = self.alloc_r32();
            let scaled = self.alloc_r32();
            let half_s = self.alloc_r32();
            let half_u = self.alloc_r32();
            writeln!(
                self.body,
                "    max.f32 {}, {}, 0fBF800000;",
                clamped.fmt_operand(),
                comp.fmt_operand(),
            )
            .expect("write to String");
            writeln!(
                self.body,
                "    min.f32 {}, {}, 0f3F800000;",
                clamped.fmt_operand(),
                clamped.fmt_operand(),
            )
            .expect("write to String");
            // 32767.0 = 0x46FFFE00
            writeln!(
                self.body,
                "    mul.f32 {}, {}, 0f46FFFE00;",
                scaled.fmt_operand(),
                clamped.fmt_operand(),
            )
            .expect("write to String");
            writeln!(
                self.body,
                "    cvt.rni.s32.f32 {}, {};",
                half_s.fmt_operand(),
                scaled.fmt_operand(),
            )
            .expect("write to String");
            writeln!(
                self.body,
                "    and.b32 {}, {}, 0xFFFF;",
                half_u.fmt_operand(),
                half_s.fmt_operand(),
            )
            .expect("write to String");
            if i > 0 {
                writeln!(
                    self.body,
                    "    shl.b32 {}, {}, 16;",
                    half_u.fmt_operand(),
                    half_u.fmt_operand(),
                )
                .expect("write to String");
            }
            writeln!(
                self.body,
                "    or.b32 {}, {}, {};",
                dst.fmt_operand(),
                dst.fmt_operand(),
                half_u.fmt_operand(),
            )
            .expect("write to String");
        }
        Ok(dst)
    }

    /// `unpack2x16unorm(u32) -> vec2<f32>`
    fn emit_unpack2x16_unorm(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let lo = self.alloc_r32();
        let hi = self.alloc_r32();
        writeln!(
            self.body,
            "    and.b32 {}, {}, 0xFFFF;",
            lo.fmt_operand(),
            arg.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    shr.u32 {}, {}, 16;",
            hi.fmt_operand(),
            arg.fmt_operand(),
        )
        .expect("write to String");
        let lo_f = self.alloc_r32();
        let hi_f = self.alloc_r32();
        writeln!(
            self.body,
            "    cvt.rn.f32.u32 {}, {};",
            lo_f.fmt_operand(),
            lo.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    cvt.rn.f32.u32 {}, {};",
            hi_f.fmt_operand(),
            hi.fmt_operand(),
        )
        .expect("write to String");
        let lo_scaled = self.alloc_r32();
        let hi_scaled = self.alloc_r32();
        // 1.0/65535.0 = 0x37800080
        writeln!(
            self.body,
            "    mul.f32 {}, {}, 0f37800080;",
            lo_scaled.fmt_operand(),
            lo_f.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    mul.f32 {}, {}, 0f37800080;",
            hi_scaled.fmt_operand(),
            hi_f.fmt_operand(),
        )
        .expect("write to String");
        Ok(PtxVal::Vec(vec![lo_scaled, hi_scaled]))
    }

    /// `unpack2x16snorm(u32) -> vec2<f32>`
    fn emit_unpack2x16_snorm(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let lo = self.alloc_r32();
        let hi = self.alloc_r32();
        writeln!(
            self.body,
            "    and.b32 {}, {}, 0xFFFF;",
            lo.fmt_operand(),
            arg.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    shr.u32 {}, {}, 16;",
            hi.fmt_operand(),
            arg.fmt_operand(),
        )
        .expect("write to String");
        let lo_ext = self.alloc_r32();
        let hi_ext = self.alloc_r32();
        writeln!(
            self.body,
            "    cvt.s32.s16 {}, {};",
            lo_ext.fmt_operand(),
            lo.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    cvt.s32.s16 {}, {};",
            hi_ext.fmt_operand(),
            hi.fmt_operand(),
        )
        .expect("write to String");
        let lo_f = self.alloc_r32();
        let hi_f = self.alloc_r32();
        writeln!(
            self.body,
            "    cvt.rn.f32.s32 {}, {};",
            lo_f.fmt_operand(),
            lo_ext.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    cvt.rn.f32.s32 {}, {};",
            hi_f.fmt_operand(),
            hi_ext.fmt_operand(),
        )
        .expect("write to String");
        let lo_scaled = self.alloc_r32();
        let hi_scaled = self.alloc_r32();
        // 1.0/32767.0 = 0x38000100
        writeln!(
            self.body,
            "    mul.f32 {}, {}, 0f38000100;",
            lo_scaled.fmt_operand(),
            lo_f.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    mul.f32 {}, {}, 0f38000100;",
            hi_scaled.fmt_operand(),
            hi_f.fmt_operand(),
        )
        .expect("write to String");
        let lo_clamped = self.alloc_r32();
        let hi_clamped = self.alloc_r32();
        writeln!(
            self.body,
            "    max.f32 {}, {}, 0fBF800000;",
            lo_clamped.fmt_operand(),
            lo_scaled.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    max.f32 {}, {}, 0fBF800000;",
            hi_clamped.fmt_operand(),
            hi_scaled.fmt_operand(),
        )
        .expect("write to String");
        Ok(PtxVal::Vec(vec![lo_clamped, hi_clamped]))
    }

    // ─── Matrix ops ────────────────────────────────────────────────

    /// `transpose(mat)` — swap rows/columns by element permutation.
    fn emit_transpose(&self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let PtxVal::Vec(cols) = arg else {
            return Ok(arg);
        };
        let ncols = cols.len();
        let nrows = match &cols[0] {
            PtxVal::Vec(r) => r.len(),
            _ => 1,
        };
        let mut result_cols = Vec::with_capacity(nrows);
        for r in 0..nrows {
            let mut row = Vec::with_capacity(ncols);
            for col in &cols {
                match col {
                    PtxVal::Vec(elems) => row.push(elems[r].clone()),
                    scalar => row.push(scalar.clone()),
                }
            }
            result_cols.push(PtxVal::Vec(row));
        }
        Ok(PtxVal::Vec(result_cols))
    }

    /// `determinant(mat)` — 2x2, 3x3, or 4x4 determinant via cofactor expansion.
    fn emit_determinant(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let PtxVal::Vec(cols) = &arg else {
            return Ok(arg);
        };
        let n = cols.len();
        match n {
            2 => self.emit_det2x2(cols),
            3 => self.emit_det3x3(cols),
            4 => self.emit_det4x4(cols),
            _ => Err(CompileError::NotImplemented(
                format!("determinant for {n}x{n} matrix").into(),
            )),
        }
    }

    /// `inverse(mat)` — matrix inverse via adjugate / determinant.
    fn emit_matrix_inverse(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let PtxVal::Vec(cols) = &arg else {
            return Err(CompileError::NotImplemented("inverse of non-matrix".into()));
        };
        let n = cols.len();
        match n {
            2 => self.emit_inverse2x2(cols),
            _ => Err(CompileError::NotImplemented(
                format!("inverse for {n}x{n} matrix").into(),
            )),
        }
    }

    // ─── Helpers ───────────────────────────────────────────────────

    fn extract_vec_components(&self, val: &PtxVal, expected: usize) -> Vec<PtxVal> {
        match val {
            PtxVal::Vec(v) => {
                let mut out = v.clone();
                while out.len() < expected {
                    out.push(PtxVal::R32(0));
                }
                out
            }
            scalar => {
                let mut out = vec![scalar.clone()];
                while out.len() < expected {
                    out.push(PtxVal::R32(0));
                }
                out
            }
        }
    }

    fn get_mat_elem(&self, cols: &[PtxVal], col: usize, row: usize) -> PtxVal {
        match &cols[col] {
            PtxVal::Vec(elems) => elems[row].clone(),
            scalar => scalar.clone(),
        }
    }

    fn emit_fmul(&mut self, a: &PtxVal, b: &PtxVal) -> PtxVal {
        let dst = self.alloc_r32();
        writeln!(
            self.body,
            "    mul.f32 {}, {}, {};",
            dst.fmt_operand(),
            a.fmt_operand(),
            b.fmt_operand(),
        )
        .expect("write to String");
        dst
    }

    fn emit_fsub(&mut self, a: &PtxVal, b: &PtxVal) -> PtxVal {
        let dst = self.alloc_r32();
        writeln!(
            self.body,
            "    sub.f32 {}, {}, {};",
            dst.fmt_operand(),
            a.fmt_operand(),
            b.fmt_operand(),
        )
        .expect("write to String");
        dst
    }

    fn emit_fadd(&mut self, a: &PtxVal, b: &PtxVal) -> PtxVal {
        let dst = self.alloc_r32();
        writeln!(
            self.body,
            "    add.f32 {}, {}, {};",
            dst.fmt_operand(),
            a.fmt_operand(),
            b.fmt_operand(),
        )
        .expect("write to String");
        dst
    }

    fn emit_fma_f32(&mut self, a: &PtxVal, b: &PtxVal, c: &PtxVal) -> PtxVal {
        let dst = self.alloc_r32();
        writeln!(
            self.body,
            "    fma.rn.f32 {}, {}, {}, {};",
            dst.fmt_operand(),
            a.fmt_operand(),
            b.fmt_operand(),
            c.fmt_operand(),
        )
        .expect("write to String");
        dst
    }

    /// det(2x2) = a*d - b*c
    fn emit_det2x2(&mut self, cols: &[PtxVal]) -> Result<PtxVal, CompileError> {
        let a = self.get_mat_elem(cols, 0, 0);
        let b = self.get_mat_elem(cols, 1, 0);
        let c = self.get_mat_elem(cols, 0, 1);
        let d = self.get_mat_elem(cols, 1, 1);
        let ad = self.emit_fmul(&a, &d);
        let bc = self.emit_fmul(&b, &c);
        Ok(self.emit_fsub(&ad, &bc))
    }

    /// det(3x3) by cofactor expansion along first row.
    fn emit_det3x3(&mut self, cols: &[PtxVal]) -> Result<PtxVal, CompileError> {
        let a00 = self.get_mat_elem(cols, 0, 0);
        let a01 = self.get_mat_elem(cols, 1, 0);
        let a02 = self.get_mat_elem(cols, 2, 0);
        let a10 = self.get_mat_elem(cols, 0, 1);
        let a11 = self.get_mat_elem(cols, 1, 1);
        let a12 = self.get_mat_elem(cols, 2, 1);
        let a20 = self.get_mat_elem(cols, 0, 2);
        let a21 = self.get_mat_elem(cols, 1, 2);
        let a22 = self.get_mat_elem(cols, 2, 2);

        // M00 = a11*a22 - a12*a21
        let m00_p = self.emit_fmul(&a11, &a22);
        let m00_n = self.emit_fmul(&a12, &a21);
        let m00 = self.emit_fsub(&m00_p, &m00_n);

        // M01 = a10*a22 - a12*a20
        let m01_p = self.emit_fmul(&a10, &a22);
        let m01_n = self.emit_fmul(&a12, &a20);
        let m01 = self.emit_fsub(&m01_p, &m01_n);

        // M02 = a10*a21 - a11*a20
        let m02_p = self.emit_fmul(&a10, &a21);
        let m02_n = self.emit_fmul(&a11, &a20);
        let m02 = self.emit_fsub(&m02_p, &m02_n);

        // det = a00*M00 - a01*M01 + a02*M02
        let t0 = self.emit_fmul(&a00, &m00);
        let t1 = self.emit_fmul(&a01, &m01);
        let t2 = self.emit_fmul(&a02, &m02);
        let d01 = self.emit_fsub(&t0, &t1);
        Ok(self.emit_fadd(&d01, &t2))
    }

    /// det(4x4) by cofactor expansion along first row, using 3x3 sub-determinants.
    fn emit_det4x4(&mut self, cols: &[PtxVal]) -> Result<PtxVal, CompileError> {
        let mut result: Option<PtxVal> = None;
        for j in 0..4 {
            let cofactor_elem = self.get_mat_elem(cols, j, 0);
            let mut sub_cols = Vec::with_capacity(3);
            for (jj, col) in cols.iter().enumerate() {
                if jj == j {
                    continue;
                }
                let mut sub_col = Vec::with_capacity(3);
                for row in 1..4 {
                    sub_col.push(self.get_mat_elem(std::slice::from_ref(col), 0, row));
                }
                sub_cols.push(PtxVal::Vec(sub_col));
            }
            let minor = self.emit_det3x3(&sub_cols)?;
            let contrib = self.emit_fmul(&cofactor_elem, &minor);
            result = Some(match result {
                None => contrib,
                Some(acc) => {
                    if j % 2 == 0 {
                        self.emit_fadd(&acc, &contrib)
                    } else {
                        self.emit_fsub(&acc, &contrib)
                    }
                }
            });
        }
        Ok(result.expect("4x4 matrix has columns"))
    }

    /// inverse(2x2) = (1/det) * [[d, -b], [-c, a]]
    fn emit_inverse2x2(&mut self, cols: &[PtxVal]) -> Result<PtxVal, CompileError> {
        let a = self.get_mat_elem(cols, 0, 0);
        let b = self.get_mat_elem(cols, 1, 0);
        let c = self.get_mat_elem(cols, 0, 1);
        let d = self.get_mat_elem(cols, 1, 1);

        let det = self.emit_det2x2(cols)?;
        let rcp = self.alloc_r32();
        writeln!(
            self.body,
            "    rcp.approx.f32 {}, {};",
            rcp.fmt_operand(),
            det.fmt_operand(),
        )
        .expect("write to String");

        let neg_b = self.alloc_r32();
        let neg_c = self.alloc_r32();
        writeln!(
            self.body,
            "    neg.f32 {}, {};",
            neg_b.fmt_operand(),
            b.fmt_operand(),
        )
        .expect("write to String");
        writeln!(
            self.body,
            "    neg.f32 {}, {};",
            neg_c.fmt_operand(),
            c.fmt_operand(),
        )
        .expect("write to String");

        let r00 = self.emit_fmul(&d, &rcp);
        let r01 = self.emit_fmul(&neg_b, &rcp);
        let r10 = self.emit_fmul(&neg_c, &rcp);
        let r11 = self.emit_fmul(&a, &rcp);

        Ok(PtxVal::Vec(vec![
            PtxVal::Vec(vec![r00, r10]),
            PtxVal::Vec(vec![r01, r11]),
        ]))
    }
}

use std::fmt::Write;
