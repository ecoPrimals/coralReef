// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

#![allow(clippy::wildcard_imports)]

use super::const_tracker::ConstTracker;
use super::debug::{DEBUG, GetDebugFlags};
use super::ir::*;
use super::liveness::{BlockLiveness, Liveness, SimpleLiveness};

use coral_reef_stubs::fxhash::{FxHashMap, FxHashSet};

pub fn src_is_upred_reg(src: &Src) -> bool {
    match &src.reference {
        SrcRef::True | SrcRef::False => false,
        SrcRef::SSA(ssa) => {
            assert!(ssa.comps() == 1);
            match ssa[0].file() {
                RegFile::Pred => false,
                RegFile::UPred => true,
                _ => super::ice!("ICE: Not a predicate source"),
            }
        }
        SrcRef::Reg(_) => super::ice!("ICE: Not in SSA form"),
        _ => super::ice!("ICE: Not a predicate source"),
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
        SrcRef::Reg(_) => super::ice!("ICE: Not in SSA form"),
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
    #[allow(dead_code, reason = "variant reserved for completeness / future use")]
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
            _ => super::ice!("ICE: Not a predicate value"),
        }
    }

    fn copy_pred_if_upred(&mut self, pred: &mut Pred) {
        match &mut pred.predicate {
            PredRef::None => (),
            PredRef::SSA(ssa) => {
                self.copy_pred_ssa_if_uniform(ssa);
            }
            PredRef::Reg(_) => super::ice!("ICE: Not in SSA form"),
        }
    }

    fn copy_src_if_upred(&mut self, src: &mut Src) {
        match &mut src.reference {
            SrcRef::True | SrcRef::False => (),
            SrcRef::SSA(ssa) => {
                assert!(ssa.comps() == 1);
                self.copy_pred_ssa_if_uniform(&mut ssa[0]);
            }
            SrcRef::Reg(_) => super::ice!("ICE: Not in SSA form"),
            _ => super::ice!("ICE: Not a predicate source"),
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

        // Pad the given ssa_ref with either undefined or zero
        let ssa_vals: Vec<_> = old_val
            .iter()
            .copied()
            .chain(std::iter::from_fn(pad_fn))
            .take(n_comps)
            .collect();

        // Collect it in a new ssa_ref and replace it with the original.
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
            _ => super::ice!("ICE: Unknown source type"),
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
                    // Immediates go in the top bits
                    self.copy_to(val[0].into(), 0.into());
                    self.copy_to(val[1].into(), u.into());
                }
                SrcRef::CBuf(cb) => {
                    // CBufs load 8B
                    self.copy_to(val[0].into(), cb.clone().into());
                    self.copy_to(val[1].into(), cb.offset(4).into());
                }
                SrcRef::SSA(vec) => {
                    assert!(vec.comps() == 2);
                    self.copy_to(val[0].into(), vec[0].into());
                    self.copy_to(val[1].into(), vec[1].into());
                }
                _ => super::ice!("ICE: Invalid 64-bit SrcRef"),
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
            _ => super::ice!("ICE: Invalid ffabs srouce type"),
        }
    }

    fn copy_alu_src_and_lower_ineg(&mut self, src: &mut Src, reg_file: RegFile, src_type: SrcType) {
        assert!(src_type == SrcType::I32);
        let val = self.alloc_ssa(reg_file);
        let old_src = std::mem::replace(src, val.into());
        if self.sm() >= 70 {
            self.push_op(OpIAdd3 {
                srcs: [Src::ZERO, old_src, Src::ZERO],
                overflow: [Dst::None, Dst::None],
                dst: val.into(),
            });
        } else {
            self.push_op(OpIAdd2 {
                dst: val.into(),
                carry_out: Dst::None,
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

pub struct LegalizeBuilder<'a> {
    b: SSAInstrBuilder<'a>,
    const_tracker: &'a mut ConstTracker,
}

impl<'a> LegalizeBuilder<'a> {
    fn new(
        sm: &'a dyn ShaderModel,
        alloc: &'a mut SSAValueAllocator,
        const_tracker: &'a mut ConstTracker,
    ) -> Self {
        LegalizeBuilder {
            b: SSAInstrBuilder::new(sm, alloc),
            const_tracker,
        }
    }

    pub fn into_vec(self) -> Vec<Instr> {
        self.b.into_vec()
    }

    #[allow(dead_code, reason = "legalize API reserved for future use")]
    pub fn into_mapped_instrs(self) -> MappedInstrs {
        self.b.into_mapped_instrs()
    }
}

impl<'a> Builder for LegalizeBuilder<'a> {
    fn push_instr(&mut self, instr: Instr) -> &mut Instr {
        self.b.push_instr(instr)
    }

    fn sm(&self) -> u8 {
        self.b.sm()
    }

    fn copy_to(&mut self, dst: Dst, mut src: Src) {
        if let Some(ssa_ref) = src.as_ssa() {
            if let &[ssa_value] = &ssa_ref[..] {
                if let Some(new_src) = self.const_tracker.get(&ssa_value) {
                    src = new_src.clone();
                }
            }
        }
        self.b.copy_to(dst, src);
    }
}

impl<'a> SSABuilder for LegalizeBuilder<'a> {
    fn alloc_ssa(&mut self, file: RegFile) -> SSAValue {
        self.b.alloc_ssa(file)
    }

    fn alloc_ssa_vec(&mut self, file: RegFile, comps: u8) -> SSARef {
        self.b.alloc_ssa_vec(file, comps)
    }
}

impl LegalizeBuildHelpers for LegalizeBuilder<'_> {}

fn legalize_instr(
    sm: &dyn ShaderModel,
    b: &mut LegalizeBuilder,
    bl: &impl BlockLiveness,
    block_uniform: bool,
    pinned: &FxHashSet<SSARef>,
    ip: usize,
    instr: &mut Instr,
) -> Result<(), crate::CompileError> {
    // Handle a few no-op cases up-front
    match &instr.op {
        Op::Annotate(_) => {
            // OpAnnotate does nothing.  There's nothing to legalize.
            return Ok(());
        }
        Op::Undef(_)
        | Op::PhiSrcs(_)
        | Op::PhiDsts(_)
        | Op::Pin(_)
        | Op::Unpin(_)
        | Op::RegOut(_) => {
            // These are implemented by RA and can take pretty much anything
            // you can throw at them.
            debug_assert!(instr.pred.is_true());
            return Ok(());
        }
        Op::Copy(_) => {
            // OpCopy is implemented in a lowering pass and can handle anything
            return Ok(());
        }
        Op::SrcBar(_) => {
            // This is turned into a nop by calc_instr_deps
            return Ok(());
        }
        Op::Swap(_) | Op::ParCopy(_) => {
            // These are generated by RA and should not exist yet
            super::ice!("ICE: Unsupported instruction");
        }
        _ => (),
    }

    if !instr.is_uniform() {
        b.copy_pred_if_upred(&mut instr.pred);
    }

    let src_types = instr.src_types();
    for (i, src) in instr.srcs_mut().iter_mut().enumerate() {
        if matches!(src.reference, SrcRef::Imm32(_)) {
            // Fold modifiers on Imm32 sources whenever possible.  Not all
            // instructions suppport modifiers and immediates at the same time.
            // But leave Zero sources alone as we don't want to make things
            // immediates that could just be rZ.
            if let Some(u) = src.as_u32(src_types[i]) {
                *src = u.into();
            }
        }
        b.copy_src_if_not_same_file(src);

        if !block_uniform {
            // In non-uniform control-flow, we can't collect uniform vectors so
            // we need to insert copies to warp regs which we can collect.
            match &mut src.reference {
                SrcRef::SSA(vec) => {
                    if vec.is_uniform() && vec.comps() > 1 && !pinned.contains(vec) {
                        b.copy_ssa_ref(vec, vec.file().to_warp());
                    }
                }
                SrcRef::CBuf(CBufRef {
                    buf: CBuf::BindlessSSA(handle),
                    ..
                }) => assert!(pinned.contains(&SSARef::new(handle))),
                _ => (),
            }
        }
    }

    // OpBreak and OpBSsy impose additional RA constraints
    let mut legalize_break_bssy = |bar_in: &mut Src, bar_out: &mut Dst| {
        let bar_in_ssa = bar_in
            .reference
            .as_ssa()
            .expect("bar_in source must be SSA value");
        if !bar_out.is_none() && bl.is_live_after_ip(&bar_in_ssa[0], ip) {
            let gpr = b.bmov_to_gpr(bar_in.clone());
            let tmp = b.bmov_to_bar(gpr.into());
            *bar_in = tmp.into();
        }
    };
    match &mut instr.op {
        Op::Break(op) => legalize_break_bssy(&mut op.bar_in, &mut op.bar_out),
        Op::BSSy(op) => legalize_break_bssy(&mut op.bar_in, &mut op.bar_out),
        _ => (),
    }

    sm.legalize_op(b, &mut instr.op)?;

    let mut vec_src_map: FxHashMap<SSARef, SSARef> = FxHashMap::default();
    let mut vec_comps: FxHashSet<_> = FxHashSet::default();
    for src in instr.srcs_mut() {
        if let SrcRef::SSA(vec) = &src.reference {
            if vec.comps() == 1 {
                continue;
            }

            // If the same vector shows up twice in one instruction, that's
            // okay. Just make it look the same as the previous source we
            // fixed up.
            if let Some(new_vec) = vec_src_map.get(vec) {
                src.reference = new_vec.clone().into();
                continue;
            }

            let mut new_vec = vec.clone();
            for c in 0..vec.comps() {
                let ssa = vec[usize::from(c)];
                // If the same SSA value shows up in multiple non-identical
                // vector sources or as multiple components in the same
                // source, we need to make a copy so it can get assigned to
                // multiple different registers.
                if vec_comps.contains(&ssa) {
                    let copy = b.alloc_ssa(ssa.file());
                    b.copy_to(copy.into(), ssa.into());
                    new_vec[usize::from(c)] = copy;
                } else {
                    vec_comps.insert(ssa);
                }
            }

            vec_src_map.insert(vec.clone(), new_vec.clone());
            src.reference = new_vec.into();
        }
    }
    Ok(())
}

impl Shader<'_> {
    /// Legalize IR for the target shader model.
    ///
    /// # Errors
    ///
    /// Returns `CompileError::UnsupportedArch` if the shader model is not supported.
    pub fn legalize(&mut self) -> Result<(), crate::CompileError> {
        let sm = self.sm;
        for f in &mut self.functions {
            let live = SimpleLiveness::for_function(f);
            let mut pinned: FxHashSet<_> = FxHashSet::default();
            let mut const_tracker = ConstTracker::new();

            for (bi, b) in f.blocks.iter_mut().enumerate() {
                let bl = live.block_live(bi);
                let bu = b.uniform;

                let mut instrs = Vec::new();
                for (ip, mut instr) in b.instrs.drain(..).enumerate() {
                    match &instr.op {
                        Op::Pin(pin) => {
                            if let Dst::SSA(ssa) = &pin.dst {
                                pinned.insert(ssa.clone());
                            }
                        }
                        Op::Copy(copy) => {
                            const_tracker.add_copy(copy);
                        }
                        _ => (),
                    }

                    let mut b = LegalizeBuilder::new(sm, &mut f.ssa_alloc, &mut const_tracker);
                    legalize_instr(sm, &mut b, bl, bu, &pinned, ip, &mut instr)?;
                    b.push_instr(instr);
                    instrs.append(&mut b.into_vec());
                }
                b.instrs = instrs;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        BasicBlock, ComputeShaderInfo, FRndMode, Function, Instr, LabelAllocator, OpCopy, OpExit,
        OpFAdd, OpRegOut, PhiAllocator, RegFile, SSAValueAllocator, Shader, ShaderInfo,
        ShaderIoInfo, ShaderModelInfo, ShaderStageInfo, Src, SrcMod, SrcRef, SrcSwizzle,
    };
    use coral_reef_stubs::cfg::CFGBuilder;

    fn make_shader_with_function(
        instrs: Vec<Instr>,
        ssa_alloc: SSAValueAllocator,
    ) -> Shader<'static> {
        let sm = Box::leak(Box::new(ShaderModelInfo::new(70, 64)));
        let mut label_alloc = LabelAllocator::new();
        let mut cfg_builder = CFGBuilder::new();
        let block = BasicBlock {
            label: label_alloc.alloc(),
            uniform: false,
            instrs,
        };
        cfg_builder.add_block(block);
        let function = Function {
            ssa_alloc,
            phi_alloc: PhiAllocator::new(),
            blocks: cfg_builder.build(),
        };
        Shader {
            sm,
            info: ShaderInfo {
                max_warps_per_sm: 0,
                gpr_count: 0,
                control_barrier_count: 0,
                instr_count: 0,
                static_cycle_count: 0,
                spills_to_mem: 0,
                fills_from_mem: 0,
                spills_to_reg: 0,
                fills_from_reg: 0,
                shared_local_mem_size: 0,
                max_crs_depth: 0,
                uses_global_mem: false,
                writes_global_mem: false,
                uses_fp64: false,
                stage: ShaderStageInfo::Compute(ComputeShaderInfo {
                    local_size: [1, 1, 1],
                    shared_mem_size: 0,
                }),
                io: ShaderIoInfo::None,
            },
            functions: vec![function],
            fma_policy: crate::FmaPolicy::default(),
        }
    }

    #[test]
    fn test_src_is_reg_zero_true_false() {
        assert!(src_is_reg(&Src::ZERO, RegFile::GPR));
        assert!(src_is_reg(&true.into(), RegFile::Pred));
        assert!(src_is_reg(&false.into(), RegFile::Pred));
        assert!(!src_is_reg(&true.into(), RegFile::GPR));
        assert!(!src_is_reg(&false.into(), RegFile::GPR));
    }

    #[test]
    fn test_src_is_reg_imm32_cbuf() {
        assert!(!src_is_reg(&Src::new_imm_u32(42), RegFile::GPR));
    }

    #[test]
    fn test_src_is_reg_ssa_matching_file() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let gpr = ssa_alloc.alloc(RegFile::GPR);
        let pred = ssa_alloc.alloc(RegFile::Pred);
        assert!(src_is_reg(&gpr.into(), RegFile::GPR));
        assert!(src_is_reg(&pred.into(), RegFile::Pred));
        assert!(!src_is_reg(&gpr.into(), RegFile::Pred));
        assert!(!src_is_reg(&pred.into(), RegFile::GPR));
    }

    #[test]
    fn test_src_is_upred_reg() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let upred = ssa_alloc.alloc(RegFile::UPred);
        let src = Src {
            reference: SrcRef::SSA(upred.into()),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        };
        assert!(src_is_upred_reg(&src));
    }

    #[test]
    fn test_src_is_upred_reg_false_for_pred() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let pred = ssa_alloc.alloc(RegFile::Pred);
        let src = Src {
            reference: SrcRef::SSA(pred.into()),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        };
        assert!(!src_is_upred_reg(&src));
    }

    #[test]
    fn test_swap_srcs_if_not_reg_swaps_when_x_imm_y_reg() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let gpr = ssa_alloc.alloc(RegFile::GPR);
        let mut x = Src::new_imm_u32(1);
        let mut y = gpr.into();
        assert!(swap_srcs_if_not_reg(&mut x, &mut y, RegFile::GPR));
        assert!(matches!(x.reference, SrcRef::SSA(_)));
        assert!(matches!(y.reference, SrcRef::Imm32(1)));
    }

    #[test]
    fn test_swap_srcs_if_not_reg_no_swap_when_both_reg() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let gpr_a = ssa_alloc.alloc(RegFile::GPR);
        let gpr_b = ssa_alloc.alloc(RegFile::GPR);
        let mut x = gpr_a.into();
        let mut y = gpr_b.into();
        assert!(!swap_srcs_if_not_reg(&mut x, &mut y, RegFile::GPR));
    }

    #[test]
    fn test_legalize_preserves_simple_copy() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let dst = ssa_alloc.alloc(RegFile::GPR);
        let mut shader = make_shader_with_function(
            vec![
                Instr::new(OpCopy {
                    dst: dst.into(),
                    src: Src::ZERO,
                }),
                Instr::new(OpRegOut {
                    srcs: vec![dst.into()],
                }),
                Instr::new(OpExit {}),
            ],
            ssa_alloc,
        );
        let result = shader.legalize();
        assert!(result.is_ok());
        let block = &shader.functions[0].blocks[0];
        assert!(!block.instrs.is_empty());
    }

    #[test]
    fn test_legalize_preserves_fadd() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let dst_a = ssa_alloc.alloc(RegFile::GPR);
        let dst_b = ssa_alloc.alloc(RegFile::GPR);
        let mut shader = make_shader_with_function(
            vec![
                Instr::new(OpCopy {
                    dst: dst_a.into(),
                    src: Src::new_imm_u32(1),
                }),
                Instr::new(OpFAdd {
                    dst: dst_b.into(),
                    srcs: [dst_a.into(), dst_a.into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }),
                Instr::new(OpRegOut {
                    srcs: vec![dst_b.into()],
                }),
                Instr::new(OpExit {}),
            ],
            ssa_alloc,
        );
        let result = shader.legalize();
        assert!(result.is_ok());
    }
}
