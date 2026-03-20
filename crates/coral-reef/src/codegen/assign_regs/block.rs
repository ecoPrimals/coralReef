// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

use super::super::debug::{DEBUG, GetDebugFlags};
use super::super::ir::*;
use super::super::liveness::BlockLiveness;
use super::*;

use coral_reef_stubs::fxhash::FxHashMap;

pub(super) struct AssignRegsBlock {
    pub(super) ra: PerRegFile<RegAllocator>,
    pcopy_tmp_gprs: u8,
    live_in: Vec<LiveValue>,
    phi_out: FxHashMap<Phi, SrcRef>,
    block_idx: usize,
}

impl AssignRegsBlock {
    pub(super) fn new(reg_count: &PerRegFile<u32>, pcopy_tmp_gprs: u8, block_idx: usize) -> Self {
        Self {
            ra: PerRegFile::new_with(|file| RegAllocator::new(file, reg_count[file])),
            pcopy_tmp_gprs,
            live_in: Vec::new(),
            phi_out: FxHashMap::default(),
            block_idx,
        }
    }

    fn get_scalar(&self, ssa: SSAValue) -> RegRef {
        let ra = &self.ra[ssa.file()];
        let reg = ra.try_get_reg(ssa).unwrap_or_else(|| {
            let known: Vec<_> = ra.ssa_reg.keys().collect();
            crate::codegen::ice!(
                "Unknown SSA value {ssa:?} (file={:?}) in block {}. Allocated SSAs: {known:?}",
                ssa.file(),
                self.block_idx,
            );
        });
        RegRef::new(ssa.file(), reg, 1)
    }

    fn alloc_scalar(
        &mut self,
        ip: usize,
        sum: &SSAUseMap,
        phi_webs: &mut PhiWebs,
        ssa: SSAValue,
    ) -> RegRef {
        let ra = &mut self.ra[ssa.file()];
        let reg = ra.alloc_scalar(ip, sum, phi_webs, ssa);
        RegRef::new(ssa.file(), reg, 1)
    }

    fn pin_vector(&mut self, reg: RegRef) {
        let ra = &mut self.ra[reg.file()];
        for c in 0..reg.comps() {
            ra.pin_reg(reg.comp(c).base_idx());
        }
    }

    fn try_coalesce(&mut self, ssa: SSAValue, src: &Src) -> bool {
        if !src.is_unmodified() {
            return false;
        }
        let SrcRef::Reg(src_reg) = src.reference else {
            return false;
        };
        debug_assert!(src_reg.comps() == 1);

        if src_reg.file() != ssa.file() {
            return false;
        }

        let ra = &mut self.ra[src_reg.file()];
        if ra.reg_is_used(src_reg.base_idx()) {
            return false;
        }

        ra.assign_reg(ssa, src_reg.base_idx());
        true
    }

    pub(super) fn pre_alloc_back_edge_live_in(&mut self, live_in_values: &[SSAValue]) {
        for &ssa in live_in_values {
            let raf = &mut self.ra[ssa.file()];
            if raf.try_get_reg(ssa).is_some() {
                continue;
            }
            let reg = raf
                .try_find_unused_reg_range(0, 1, 1, 0)
                .expect("no free register for back-edge live-in");
            raf.assign_reg(ssa, reg);
            self.live_in.push(LiveValue {
                live_ref: LiveRef::SSA(ssa),
                reg_ref: RegRef::new(ssa.file(), reg, 1),
            });
        }
        self.live_in.sort();
    }

    fn pcopy_tmp(&self) -> Option<RegRef> {
        if self.pcopy_tmp_gprs > 0 {
            Some(RegRef::new(
                RegFile::GPR,
                self.ra[RegFile::GPR].reg_count,
                self.pcopy_tmp_gprs,
            ))
        } else {
            None
        }
    }

    fn assign_regs_instr(
        &mut self,
        mut instr: Instr,
        ip: usize,
        sum: &SSAUseMap,
        phi_webs: &mut PhiWebs,
        srcs_killed: &KillSet,
        dsts_killed: &KillSet,
        pcopy: &mut OpParCopy,
    ) -> Option<Instr> {
        match &mut instr.op {
            Op::Undef(undef) => {
                if let Dst::SSA(ssa) = &undef.dst {
                    assert!(ssa.comps() == 1);
                    self.alloc_scalar(ip, sum, phi_webs, ssa[0]);
                }
                assert!(srcs_killed.is_empty());
                self.ra.free_killed(dsts_killed);
                None
            }
            Op::PhiSrcs(op) => {
                for (id, src) in op.srcs.iter() {
                    assert!(src.is_unmodified());
                    if let Some(ssa) = src_ssa_ref(src) {
                        assert!(ssa.len() == 1);
                        let reg = self.get_scalar(ssa[0]);
                        self.phi_out.insert(*id, reg.into());
                    } else {
                        self.phi_out.insert(*id, src.reference.clone());
                    }
                }
                assert!(dsts_killed.is_empty());
                None
            }
            Op::PhiDsts(op) => {
                assert!(instr.pred.is_true());

                for (phi, dst) in op.dsts.iter() {
                    if let Dst::SSA(ssa) = dst {
                        assert!(ssa.comps() == 1);
                        let reg = self.alloc_scalar(ip, sum, phi_webs, ssa[0]);
                        self.live_in.push(LiveValue {
                            live_ref: LiveRef::Phi(*phi),
                            reg_ref: reg,
                        });
                    }
                }
                assert!(srcs_killed.is_empty());
                self.ra.free_killed(dsts_killed);

                None
            }
            Op::Break(op) => {
                for src in op.srcs_as_mut_slice() {
                    if let Some(ssa) = src_ssa_ref(src) {
                        assert!(ssa.len() == 1);
                        let reg = self.get_scalar(ssa[0]);
                        src_set_reg(src, reg);
                    }
                }

                self.ra.free_killed(srcs_killed);

                if let Dst::SSA(ssa) = &op.bar_out {
                    let reg = *op
                        .bar_in()
                        .reference
                        .as_reg()
                        .expect("bar_in must be register after RA");
                    self.ra.assign_reg(ssa[0], reg);
                    op.bar_out = reg.into();
                }

                self.ra.free_killed(dsts_killed);

                Some(instr)
            }
            Op::BSSy(op) => {
                for src in op.srcs_as_mut_slice() {
                    if let Some(ssa) = src_ssa_ref(src) {
                        assert!(ssa.len() == 1);
                        let reg = self.get_scalar(ssa[0]);
                        src_set_reg(src, reg);
                    }
                }

                self.ra.free_killed(srcs_killed);

                if let Dst::SSA(ssa) = &op.bar_out {
                    let reg = *op
                        .bar_in()
                        .reference
                        .as_reg()
                        .expect("bar_in must be register after RA");
                    self.ra.assign_reg(ssa[0], reg);
                    op.bar_out = reg.into();
                }

                self.ra.free_killed(dsts_killed);

                Some(instr)
            }
            Op::Copy(copy) => {
                if let Some(ssa) = src_ssa_ref(&copy.src) {
                    // This may be a Cbuf::BindlessSSA source so we need to
                    // support vectors because cbuf handles are vec2s. However,
                    // since we only have a single scalar destination, we can
                    // just allocate and free killed up-front.
                    let ra = &mut self.ra[ssa.file()];
                    let mut vra = VecRegAllocator::new(ra);
                    let reg = vra.collect_vector(ssa);
                    vra.free_killed(srcs_killed);
                    vra.finish(pcopy);
                    src_set_reg(&mut copy.src, reg);
                }

                let mut del_copy = false;
                if let Dst::SSA(dst_vec) = &mut copy.dst {
                    debug_assert!(dst_vec.comps() == 1);
                    let dst_ssa = &dst_vec[0];

                    if self.try_coalesce(*dst_ssa, &copy.src) {
                        del_copy = true;
                    } else {
                        copy.dst = self.alloc_scalar(ip, sum, phi_webs, *dst_ssa).into();
                    }
                }

                self.ra.free_killed(dsts_killed);

                if del_copy { None } else { Some(instr) }
            }
            Op::Pin(_) | Op::Unpin(_) => {
                assert!(instr.pred.is_true());

                let (src, dst) = match &instr.op {
                    Op::Pin(pin) => (&pin.src, &pin.dst),
                    Op::Unpin(unpin) => (&unpin.src, &unpin.dst),
                    _ => unreachable!(),
                };

                // These basically act as a vector version of OpCopy except that
                // they only work on SSA values and we pin the destination if
                // it's OpPin.
                let src_vec = src.as_ssa().expect("Pin/Unpin src must be SSA value");
                let dst_vec = dst.as_ssa().expect("Pin/Unpin dst must be SSA value");
                assert!(src_vec.comps() == dst_vec.comps());

                if srcs_killed.len() == usize::from(src_vec.comps())
                    && src_vec.file() == dst_vec.file()
                {
                    let ra = &mut self.ra[src_vec.file()];
                    let mut vra = VecRegAllocator::new(ra);
                    let reg = vra.collect_vector(src_vec);
                    vra.finish(pcopy);
                    for c in 0..src_vec.comps() {
                        let c_reg = ra.free_ssa(src_vec[usize::from(c)]);
                        debug_assert!(c_reg == reg.comp(c).base_idx());
                        ra.assign_reg(dst_vec[usize::from(c)], c_reg);
                    }

                    if matches!(&instr.op, Op::Pin(_)) {
                        self.pin_vector(reg);
                    }
                    self.ra.free_killed(dsts_killed);

                    None
                } else {
                    // Otherwise, turn into a parallel copy
                    //
                    // We can always allocate the destination first in this
                    // case.
                    assert!(dst_vec.comps() > 1 || srcs_killed.is_empty());

                    let dst_ra = &mut self.ra[dst_vec.file()];
                    let mut vra = VecRegAllocator::new(dst_ra);
                    let dst_reg = vra.alloc_vector(dst_vec);
                    vra.finish(pcopy);

                    let mut pin_copy = OpParCopy::new();
                    for c in 0..dst_reg.comps() {
                        let src_reg = self.get_scalar(src_vec[usize::from(c)]);
                        pin_copy.push(dst_reg.comp(c).into(), src_reg.into());
                    }

                    if matches!(&instr.op, Op::Pin(_)) {
                        self.pin_vector(dst_reg);
                    }
                    self.ra.free_killed(srcs_killed);
                    self.ra.free_killed(dsts_killed);

                    Some(Instr::new(pin_copy))
                }
            }
            Op::ParCopy(pcopy) => {
                for (_, src) in pcopy.dsts_srcs.iter_mut() {
                    if let Some(src_vec) = src_ssa_ref(src) {
                        debug_assert!(src_vec.len() == 1);
                        let reg = self.get_scalar(src_vec[0]);
                        src_set_reg(src, reg);
                    }
                }

                self.ra.free_killed(srcs_killed);

                // Try to coalesce destinations into sources, if possible
                pcopy.dsts_srcs.retain(|dst, src| match dst {
                    Dst::None => false,
                    Dst::SSA(dst_vec) => {
                        debug_assert!(dst_vec.comps() == 1);
                        !self.try_coalesce(dst_vec[0], src)
                    }
                    Dst::Reg(_) => true,
                });

                for (dst, _) in pcopy.dsts_srcs.iter_mut() {
                    if let Dst::SSA(dst_vec) = dst {
                        debug_assert!(dst_vec.comps() == 1);
                        *dst = self.alloc_scalar(ip, sum, phi_webs, dst_vec[0]).into();
                    }
                }

                self.ra.free_killed(dsts_killed);

                pcopy.tmp = self.pcopy_tmp();
                if pcopy.is_empty() { None } else { Some(instr) }
            }
            Op::RegOut(out) => {
                for src in &mut out.srcs {
                    if let Some(src_vec) = src_ssa_ref(src) {
                        debug_assert!(src_vec.len() == 1);
                        let reg = self.get_scalar(src_vec[0]);
                        src_set_reg(src, reg);
                    }
                }

                self.ra.free_killed(srcs_killed);
                assert!(dsts_killed.is_empty());

                // This should be the last instruction and its sources should
                // be the last free GPRs.
                debug_assert!(self.ra[RegFile::GPR].used_reg_count() == 0);

                for (i, src) in out.srcs.drain(..).enumerate() {
                    let reg = u32::try_from(i).expect("RegOut index must fit in u32");
                    let dst = RegRef::new(RegFile::GPR, reg, 1);
                    pcopy.push(dst.into(), src);
                }

                None
            }
            _ => {
                for file in self.ra.values_mut() {
                    instr_assign_regs_file(&mut instr, ip, sum, phi_webs, srcs_killed, pcopy, file);
                }
                self.ra.free_killed(dsts_killed);
                Some(instr)
            }
        }
    }

    pub(super) fn first_pass<BL: BlockLiveness>(
        &mut self,
        b: &mut BasicBlock,
        bl: &BL,
        pred_ras: &[&PerRegFile<RegAllocator>],
        phi_webs: &mut PhiWebs,
    ) {
        // Populate live-in from ALL (forward) predecessors' register files.
        // Multi-predecessor blocks (e.g. if/else merge) may have SSA values
        // in only one predecessor — we must check them all.
        for pred_ra in pred_ras {
            for (raf, pred_raf) in self.ra.values_mut().zip(pred_ra.values()) {
                for (ssa, reg) in &pred_raf.ssa_reg {
                    if bl.is_live_in(ssa) && raf.try_get_reg(*ssa).is_none() {
                        raf.assign_reg(*ssa, *reg);
                        if pred_raf.reg_is_pinned(*reg) {
                            raf.pin_reg(*reg);
                        }
                        self.live_in.push(LiveValue {
                            live_ref: LiveRef::SSA(*ssa),
                            reg_ref: RegRef::new(raf.file(), *reg, 1),
                        });
                    }
                }
            }
        }

        let sum = SSAUseMap::for_block(b);

        let mut instrs = Vec::new();
        let mut srcs_killed = KillSet::new();
        let mut dsts_killed = KillSet::new();

        for (ip, instr) in b.instrs.drain(..).enumerate() {
            // Build up the kill set
            srcs_killed.clear();
            if let PredRef::SSA(ssa) = &instr.pred.predicate {
                if !bl.is_live_after_ip(ssa, ip) {
                    srcs_killed.insert(*ssa);
                }
            }
            for src in instr.srcs() {
                for ssa in src.iter_ssa() {
                    if !bl.is_live_after_ip(ssa, ip) {
                        srcs_killed.insert(*ssa);
                    }
                }
            }

            dsts_killed.clear();
            for dst in instr.dsts() {
                if let Dst::SSA(vec) = dst {
                    for ssa in vec.iter() {
                        if !bl.is_live_after_ip(ssa, ip) {
                            dsts_killed.insert(*ssa);
                        }
                    }
                }
            }

            let mut pcopy = OpParCopy::new();
            pcopy.tmp = self.pcopy_tmp();

            let instr = self.assign_regs_instr(
                instr,
                ip,
                &sum,
                phi_webs,
                &srcs_killed,
                &dsts_killed,
                &mut pcopy,
            );

            if !pcopy.is_empty() {
                if DEBUG.annotate() {
                    instrs.push(Instr::new(OpAnnotate {
                        annotation: "generated by assign_regs".into(),
                    }));
                }
                if !b.uniform {
                    for dst in pcopy.dsts_as_slice() {
                        if let Dst::Reg(reg) = dst {
                            assert!(!reg.is_uniform());
                        }
                    }
                }
                instrs.push(Instr::new(pcopy));
            }

            if let Some(instr) = instr {
                instrs.push(instr);
            }
        }

        // Update phi_webs with the registers assigned in this block
        for ra in self.ra.values() {
            for (ssa, reg) in &ra.ssa_reg {
                phi_webs.set(*ssa, *reg);
            }
        }

        // Sort live-in to maintain determinism
        self.live_in.sort();

        b.instrs = instrs;
    }

    pub(super) fn second_pass(&self, target: &Self, b: &mut BasicBlock) {
        let mut pcopy = OpParCopy::new();
        pcopy.tmp = self.pcopy_tmp();

        for lv in &target.live_in {
            let src = match lv.live_ref {
                LiveRef::SSA(ssa) => {
                    let raf = &self.ra[ssa.file()];
                    if let Some(reg) = raf.try_get_reg(ssa) {
                        SrcRef::from(RegRef::new(ssa.file(), reg, 1))
                    } else {
                        // Value only reachable via back-edge predecessor;
                        // that predecessor's second_pass provides the copy.
                        continue;
                    }
                }
                LiveRef::Phi(phi) => self
                    .phi_out
                    .get(&phi)
                    .expect("phi must have been assigned in first pass")
                    .clone(),
            };
            let dst = lv.reg_ref;
            if let SrcRef::Reg(src_reg) = src {
                if dst == src_reg {
                    continue;
                }
            }
            pcopy.push(dst.into(), src.into());
        }

        if !pcopy.is_empty() {
            let ann = OpAnnotate {
                annotation: "generated by assign_regs".into(),
            };
            if b.branch().is_some() {
                b.instrs.insert(b.instrs.len() - 1, Instr::new(ann));
                b.instrs.insert(b.instrs.len() - 1, Instr::new(pcopy));
            } else {
                b.instrs.push(Instr::new(ann));
                b.instrs.push(Instr::new(pcopy));
            }
        }
    }
}
