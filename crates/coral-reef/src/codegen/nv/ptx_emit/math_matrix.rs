// SPDX-License-Identifier: AGPL-3.0-or-later
//! PTX matrix operations — transpose, determinant, inverse.
//!
//! Split from `math_pack.rs` for cohesion: pack/unpack are data-format
//! conversions, while these are linear algebra primitives requiring
//! multi-instruction sequences (cofactor expansion, adjugate).

use std::fmt::Write;

use super::PtxEmitter;
use super::types::PtxVal;
use crate::error::CompileError;

impl PtxEmitter<'_> {
    /// `transpose(mat)` — swap rows/columns by element permutation.
    pub(super) fn emit_transpose(&self, arg: PtxVal) -> Result<PtxVal, CompileError> {
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
    pub(super) fn emit_determinant(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
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
    pub(super) fn emit_matrix_inverse(&mut self, arg: PtxVal) -> Result<PtxVal, CompileError> {
        let PtxVal::Vec(cols) = &arg else {
            return Err(CompileError::NotImplemented("inverse of non-matrix".into()));
        };
        let n = cols.len();
        match n {
            2 => self.emit_inverse2x2(cols),
            3 => self.emit_inverse3x3(cols),
            _ => Err(CompileError::NotImplemented(
                format!("inverse for {n}x{n} matrix").into(),
            )),
        }
    }

    // ─── Matrix helpers ─────────────────────────────────────────────

    pub(super) fn get_mat_elem(&self, cols: &[PtxVal], col: usize, row: usize) -> PtxVal {
        match &cols[col] {
            PtxVal::Vec(elems) => elems[row].clone(),
            scalar => scalar.clone(),
        }
    }

    pub(super) fn emit_fmul(&mut self, a: &PtxVal, b: &PtxVal) -> PtxVal {
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

    pub(super) fn emit_fsub(&mut self, a: &PtxVal, b: &PtxVal) -> PtxVal {
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

    pub(super) fn emit_fadd(&mut self, a: &PtxVal, b: &PtxVal) -> PtxVal {
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

    pub(super) fn emit_fma_f32(&mut self, a: &PtxVal, b: &PtxVal, c: &PtxVal) -> PtxVal {
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

        let m00_p = self.emit_fmul(&a11, &a22);
        let m00_n = self.emit_fmul(&a12, &a21);
        let m00 = self.emit_fsub(&m00_p, &m00_n);

        let m01_p = self.emit_fmul(&a10, &a22);
        let m01_n = self.emit_fmul(&a12, &a20);
        let m01 = self.emit_fsub(&m01_p, &m01_n);

        let m02_p = self.emit_fmul(&a10, &a21);
        let m02_n = self.emit_fmul(&a11, &a20);
        let m02 = self.emit_fsub(&m02_p, &m02_n);

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

    /// inverse(3x3) via cofactor matrix / determinant.
    ///
    /// inv(M) = adj(M) / det(M), where adj(M)[i][j] = cofactor(M, j, i).
    fn emit_inverse3x3(&mut self, cols: &[PtxVal]) -> Result<PtxVal, CompileError> {
        let det = self.emit_det3x3(cols)?;
        let rcp = self.alloc_r32();
        writeln!(
            self.body,
            "    rcp.approx.f32 {}, {};",
            rcp.fmt_operand(),
            det.fmt_operand(),
        )
        .expect("write to String");

        let mut result_cols: Vec<PtxVal> = Vec::with_capacity(3);
        for col in 0..3_usize {
            let mut result_row: Vec<PtxVal> = Vec::with_capacity(3);
            for row in 0..3_usize {
                let cofactor = self.emit_cofactor3x3(cols, row, col)?;
                let scaled = self.emit_fmul(&cofactor, &rcp);
                result_row.push(scaled);
            }
            result_cols.push(PtxVal::Vec(result_row));
        }
        Ok(PtxVal::Vec(result_cols))
    }

    /// Cofactor(M, i, j) = (-1)^(i+j) * det(minor(M, i, j))
    fn emit_cofactor3x3(
        &mut self,
        cols: &[PtxVal],
        exclude_row: usize,
        exclude_col: usize,
    ) -> Result<PtxVal, CompileError> {
        let mut minor_cols: Vec<PtxVal> = Vec::with_capacity(2);
        for c in 0..3_usize {
            if c == exclude_col {
                continue;
            }
            let mut minor_col: Vec<PtxVal> = Vec::with_capacity(2);
            for r in 0..3_usize {
                if r == exclude_row {
                    continue;
                }
                minor_col.push(self.get_mat_elem(cols, c, r));
            }
            minor_cols.push(PtxVal::Vec(minor_col));
        }
        let det_minor = self.emit_det2x2(&minor_cols)?;
        if (exclude_row + exclude_col) % 2 == 0 {
            Ok(det_minor)
        } else {
            let neg = self.alloc_r32();
            writeln!(
                self.body,
                "    neg.f32 {}, {};",
                neg.fmt_operand(),
                det_minor.fmt_operand(),
            )
            .expect("write to String");
            Ok(neg)
        }
    }
}
