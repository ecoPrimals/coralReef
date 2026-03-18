// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

//! Legalization helper functions and the [`LegalizeBuildHelpers`] trait.
//!
//! These are the building blocks used by architecture-specific legalization
//! code (SM70, RDNA2, …) via the [`super::LegalizeBuilder`].

use super::super::debug::{DEBUG, GetDebugFlags};
use super::super::ir::*;

pub fn src_is_upred_reg(src: &Src) -> bool {
    match &src.reference {
        SrcRef::True | SrcRef::False => false,
        SrcRef::SSA(ssa) => {
            assert!(ssa.comps() == 1);
            match ssa[0].file() {
                RegFile::Pred => false,
                RegFile::UPred => true,
                _ => super::super::ice!("ICE: Not a predicate source"),
            }
        }
        SrcRef::Reg(_) => super::super::ice!("ICE: Not in SSA form"),
        _ => super::super::ice!("ICE: Not a predicate source"),
    }
}

pub fn src_is_reg(src: &Src, reg_file: RegFile) -> bool {
    match &src.reference {
        SrcRef::Zero => true,
        SrcRef::True | SrcRef::False => {
            matches!(reg_file, RegFile::Pred | RegFile::UPred)
        }
        SrcRef::SSA(ssa) => ssa.file() == reg_file,
        SrcRef::Imm32(_) | SrcRef::CBuf(_) => false,
        SrcRef::Reg(_) => super::super::ice!("ICE: Not in SSA form"),
    }
}

pub fn swap_srcs_if_not_reg(x: &mut Src, y: &mut Src, reg_file: RegFile) -> bool {
    if !src_is_reg(x, reg_file) && src_is_reg(y, reg_file) {
        std::mem::swap(x, y);
        true
    } else {
        false
    }
}

fn src_is_imm(src: &Src) -> bool {
    matches!(src.reference, SrcRef::Imm32(_))
}

pub enum PadValue {
    Zero,
    #[expect(dead_code, reason = "variant reserved for completeness / future use")]
    Undefined,
}

pub trait LegalizeBuildHelpers: SSABuilder {
    fn copy_ssa(&mut self, ssa: &mut SSAValue, reg_file: RegFile) {
        let tmp = self.alloc_ssa(reg_file);
        self.copy_to(tmp.into(), (*ssa).into());
        *ssa = tmp;
    }

    fn copy_ssa_ref(&mut self, vec: &mut SSARef, reg_file: RegFile) {
        for ssa in &mut vec[..] {
            self.copy_ssa(ssa, reg_file);
        }
    }

    fn copy_pred_ssa_if_uniform(&mut self, ssa: &mut SSAValue) {
        match ssa.file() {
            RegFile::Pred => (),
            RegFile::UPred => self.copy_ssa(ssa, RegFile::Pred),
            _ => super::super::ice!("ICE: Not a predicate value"),
        }
    }

    fn copy_pred_if_upred(&mut self, pred: &mut Pred) {
        match &mut pred.predicate {
            PredRef::None => (),
            PredRef::SSA(ssa) => {
                self.copy_pred_ssa_if_uniform(ssa);
            }
            PredRef::Reg(_) => super::super::ice!("ICE: Not in SSA form"),
        }
    }

    fn copy_src_if_upred(&mut self, src: &mut Src) {
        match &mut src.reference {
            SrcRef::True | SrcRef::False => (),
            SrcRef::SSA(ssa) => {
                assert!(ssa.comps() == 1);
                self.copy_pred_ssa_if_uniform(&mut ssa[0]);
            }
            SrcRef::Reg(_) => super::super::ice!("ICE: Not in SSA form"),
            _ => super::super::ice!("ICE: Not a predicate source"),
        }
    }

    fn copy_src_if_not_same_file(&mut self, src: &mut Src) {
        let SrcRef::SSA(vec) = &mut src.reference else {
            return;
        };

        if vec.comps() == 1 {
            return;
        }

        let mut all_same = true;
        let file = vec[0].file();
        for i in 1..vec.comps() {
            let c_file = vec[usize::from(i)].file();
            if c_file != file {
                debug_assert!(c_file.to_warp() == file.to_warp());
                all_same = false;
            }
        }

        if !all_same {
            self.copy_ssa_ref(vec, file.to_warp());
        }
    }

    fn align_reg(&mut self, src: &mut Src, n_comps: usize, pad_value: PadValue) {
        debug_assert!(!matches!(src.reference, SrcRef::Reg(_)));
        let SrcRef::SSA(ref old_val) = src.reference else {
            return;
        };
        assert!(old_val.len() <= n_comps);
        assert!(src.is_unmodified());

        let pad_fn = || {
            Some(match pad_value {
                PadValue::Zero => self.copy(0.into()),
                PadValue::Undefined => self.undef(),
            })
        };

        let ssa_vals: Vec<_> = old_val
            .iter()
            .copied()
            .chain(std::iter::from_fn(pad_fn))
            .take(n_comps)
            .collect();

        let val = SSARef::try_from(ssa_vals).expect("Cannot create SSARef");
        src.reference = val.into();
    }

    fn copy_alu_src(&mut self, src: &mut Src, reg_file: RegFile, src_type: SrcType) {
        let val = match src_type {
            SrcType::GPR
            | SrcType::ALU
            | SrcType::F32
            | SrcType::F16
            | SrcType::F16v2
            | SrcType::I32
            | SrcType::B32 => self.alloc_ssa_vec(reg_file, 1),
            SrcType::F64 => self.alloc_ssa_vec(reg_file, 2),
            SrcType::Pred => self.alloc_ssa_vec(reg_file, 1),
            _ => super::super::ice!("ICE: Unknown source type"),
        };

        if DEBUG.annotate() {
            self.push_instr(Instr::new(OpAnnotate {
                annotation: "copy generated by legalizer".into(),
            }));
        }

        let old_src_ref = std::mem::replace(&mut src.reference, val.clone().into());
        if val.comps() == 1 {
            self.copy_to(val[0].into(), old_src_ref.into());
        } else {
            match old_src_ref {
                SrcRef::Imm32(u) => {
                    self.copy_to(val[0].into(), 0.into());
                    self.copy_to(val[1].into(), u.into());
                }
                SrcRef::CBuf(cb) => {
                    self.copy_to(val[0].into(), cb.clone().into());
                    self.copy_to(val[1].into(), cb.offset(4).into());
                }
                SrcRef::SSA(vec) => {
                    assert!(vec.comps() == 2);
                    self.copy_to(val[0].into(), vec[0].into());
                    self.copy_to(val[1].into(), vec[1].into());
                }
                _ => super::super::ice!("ICE: Invalid 64-bit SrcRef"),
            }
        }
    }

    fn copy_alu_src_if_not_reg(&mut self, src: &mut Src, reg_file: RegFile, src_type: SrcType) {
        if !src_is_reg(src, reg_file) {
            self.copy_alu_src(src, reg_file, src_type);
        }
    }

    fn copy_alu_src_if_not_reg_or_imm(
        &mut self,
        src: &mut Src,
        reg_file: RegFile,
        src_type: SrcType,
    ) {
        if !src_is_reg(src, reg_file) && !matches!(&src.reference, SrcRef::Imm32(_)) {
            self.copy_alu_src(src, reg_file, src_type);
        }
    }

    fn copy_alu_src_if_pred(&mut self, src: &mut Src, reg_file: RegFile, src_type: SrcType) {
        let is_pred = match &src.reference {
            SrcRef::True | SrcRef::False => true,
            SrcRef::SSA(ssa) => matches!(ssa.file(), RegFile::Pred | RegFile::UPred),
            _ => false,
        };
        if is_pred {
            self.copy_alu_src(src, reg_file, src_type);
        }
    }

    fn copy_alu_src_if_imm(&mut self, src: &mut Src, reg_file: RegFile, src_type: SrcType) {
        if src_is_imm(src) {
            self.copy_alu_src(src, reg_file, src_type);
        }
    }

    fn copy_alu_src_if_ineg_imm(&mut self, src: &mut Src, reg_file: RegFile, src_type: SrcType) {
        assert!(src_type == SrcType::I32);
        if src_is_imm(src) && src.modifier.is_ineg() {
            self.copy_alu_src(src, reg_file, src_type);
        }
    }

    fn copy_alu_src_if_both_not_reg(
        &mut self,
        src1: &Src,
        src2: &mut Src,
        reg_file: RegFile,
        src_type: SrcType,
    ) {
        if !src_is_reg(src1, reg_file) && !src_is_reg(src2, reg_file) {
            self.copy_alu_src(src2, reg_file, src_type);
        }
    }

    fn copy_alu_src_and_lower_fmod(&mut self, src: &mut Src, reg_file: RegFile, src_type: SrcType) {
        match src_type {
            SrcType::F16 | SrcType::F16v2 => {
                let val = self.alloc_ssa(reg_file);
                let old_src = std::mem::replace(src, val.into());
                self.push_op(OpHAdd2 {
                    dst: val.into(),
                    srcs: [Src::ZERO.fneg(), old_src],
                    saturate: false,
                    ftz: false,
                    f32: false,
                });
            }
            SrcType::F32 => {
                let val = self.alloc_ssa(reg_file);
                let old_src = std::mem::replace(src, val.into());
                self.push_op(OpFAdd {
                    dst: val.into(),
                    srcs: [Src::ZERO.fneg(), old_src],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                });
            }
            SrcType::F64 => {
                let val = self.alloc_ssa_vec(reg_file, 2);
                let old_src = std::mem::replace(src, val.clone().into());
                self.push_op(OpDAdd {
                    dst: val.into(),
                    srcs: [Src::ZERO.fneg(), old_src],
                    rnd_mode: FRndMode::NearestEven,
                });
            }
            _ => super::super::ice!("ICE: Invalid ffabs srouce type"),
        }
    }

    fn copy_alu_src_and_lower_ineg(&mut self, src: &mut Src, reg_file: RegFile, src_type: SrcType) {
        assert!(src_type == SrcType::I32);
        let val = self.alloc_ssa(reg_file);
        let old_src = std::mem::replace(src, val.into());
        if self.sm() >= 70 {
            self.push_op(OpIAdd3 {
                dsts: [val.into(), Dst::None, Dst::None],
                srcs: [Src::ZERO, old_src, Src::ZERO],
            });
        } else {
            self.push_op(OpIAdd2 {
                dsts: [val.into(), Dst::None],
                srcs: [Src::ZERO, old_src],
            });
        }
    }

    fn copy_alu_src_if_fabs(&mut self, src: &mut Src, reg_file: RegFile, src_type: SrcType) {
        if src.modifier.has_fabs() {
            self.copy_alu_src_and_lower_fmod(src, reg_file, src_type);
        }
    }

    fn copy_alu_src_if_i20_overflow(
        &mut self,
        src: &mut Src,
        reg_file: RegFile,
        src_type: SrcType,
    ) {
        if src.as_imm_not_i20().is_some() {
            self.copy_alu_src(src, reg_file, src_type);
        }
    }

    fn copy_alu_src_if_f20_overflow(
        &mut self,
        src: &mut Src,
        reg_file: RegFile,
        src_type: SrcType,
    ) {
        if src.as_imm_not_f20().is_some() {
            self.copy_alu_src(src, reg_file, src_type);
        }
    }

    fn copy_ssa_ref_if_uniform(&mut self, ssa_ref: &mut SSARef) {
        for ssa in &mut ssa_ref[..] {
            if ssa.is_uniform() {
                let warp = self.alloc_ssa(ssa.file().to_warp());
                self.copy_to(warp.into(), (*ssa).into());
                *ssa = warp;
            }
        }
    }
}
