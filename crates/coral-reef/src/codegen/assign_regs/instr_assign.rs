// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

#![allow(clippy::wildcard_imports)]

use super::super::ir::*;
use super::*;

fn instr_remap_srcs_file(instr: &mut Instr, ra: &mut VecRegAllocator) {
    // Collect vector sources first since those may silently pin some of our
    // scalar sources.
    for src in instr.srcs_mut() {
        if let Some(ssa) = src_ssa_ref(src) {
            if ssa.file() == ra.file() && ssa.len() > 1 {
                let reg = ra.collect_vector(ssa);
                src_set_reg(src, reg);
            }
        }
    }

    if let PredRef::SSA(pred) = instr.pred.predicate {
        if pred.file() == ra.file() {
            instr.pred.predicate = ra.collect_vector(&[pred]).into();
        }
    }

    for src in instr.srcs_mut() {
        if let Some(ssa) = src_ssa_ref(src) {
            if ssa.file() == ra.file() && ssa.len() == 1 {
                let reg = ra.collect_vector(ssa);
                src_set_reg(src, reg);
            }
        }
    }
}

fn instr_alloc_scalar_dsts_file(
    instr: &mut Instr,
    ip: usize,
    sum: &SSAUseMap,
    phi_webs: &mut PhiWebs,
    ra: &mut RegAllocator,
) {
    for dst in instr.dsts_mut() {
        if let Dst::SSA(ssa) = dst {
            if ssa.file() == ra.file() {
                assert!(ssa.comps() == 1);
                let reg = ra.alloc_scalar(ip, sum, phi_webs, ssa[0]);
                *dst = RegRef::new(ra.file(), reg, 1).into();
            }
        }
    }
}

pub(super) fn instr_assign_regs_file(
    instr: &mut Instr,
    ip: usize,
    sum: &SSAUseMap,
    phi_webs: &mut PhiWebs,
    killed: &KillSet,
    pcopy: &mut OpParCopy,
    ra: &mut RegAllocator,
) {
    struct VecDst {
        dst_idx: usize,
        comps: u8,
        killed: Option<SSARef>,
        reg: u32,
    }

    let mut vec_dsts = Vec::new();
    let mut vec_dst_comps = 0;
    for (i, dst) in instr.dsts().iter().enumerate() {
        if let Dst::SSA(ssa) = dst {
            if ssa.file() == ra.file() && ssa.comps() > 1 {
                vec_dsts.push(VecDst {
                    dst_idx: i,
                    comps: ssa.comps(),
                    killed: None,
                    reg: u32::MAX,
                });
                vec_dst_comps += ssa.comps();
            }
        }
    }

    // No vector destinations is the easy case
    if vec_dst_comps == 0 {
        let mut vra = VecRegAllocator::new(ra);
        instr_remap_srcs_file(instr, &mut vra);
        vra.free_killed(killed);
        vra.finish(pcopy);
        instr_alloc_scalar_dsts_file(instr, ip, sum, phi_webs, ra);
        return;
    }

    // Predicates can't be vectors.  This lets us ignore instr.pred in our
    // analysis for the cases below. Only the easy case above needs to care
    // about them.
    assert!(!ra.file().is_predicate());

    let mut avail = killed.set.clone();
    let mut killed_vecs = Vec::new();
    for src in instr.srcs() {
        if let Some(vec) = src_ssa_ref(src) {
            if vec.len() > 1 {
                let mut vec_killed = true;
                for ssa in vec {
                    if ssa.file() != ra.file() || !avail.contains(ssa) {
                        vec_killed = false;
                        break;
                    }
                }
                if vec_killed {
                    for ssa in vec {
                        avail.remove(ssa);
                    }
                    killed_vecs.push(SSARef::new(vec));
                }
            }
        }
    }

    vec_dsts.sort_by_key(|v| v.comps);
    killed_vecs.sort_by_key(|v| v.comps());

    let mut next_dst_reg = 0;
    let mut vec_dsts_map_to_killed_srcs = true;
    let mut could_trivially_allocate = true;
    for vec_dst in vec_dsts.iter_mut().rev() {
        while let Some(src) = killed_vecs.pop() {
            if src.comps() >= vec_dst.comps {
                vec_dst.killed = Some(src);
                break;
            }
        }
        if vec_dst.killed.is_none() {
            vec_dsts_map_to_killed_srcs = false;
        }

        let align = vec_dst.comps.next_power_of_two();
        if let Some(reg) = ra.try_find_unused_reg_range(next_dst_reg, vec_dst.comps, align, 0) {
            vec_dst.reg = reg;
            next_dst_reg = reg + u32::from(vec_dst.comps);
        } else {
            could_trivially_allocate = false;
        }
    }

    if vec_dsts_map_to_killed_srcs {
        let mut vra = VecRegAllocator::new(ra);
        instr_remap_srcs_file(instr, &mut vra);

        for vec_dst in &mut vec_dsts {
            let src_vec = vec_dst.killed.as_ref().unwrap();
            vec_dst.reg = vra.try_get_vec_reg(src_vec).unwrap();
        }

        vra.free_killed(killed);

        for vec_dst in vec_dsts {
            let dst = &mut instr.dsts_mut()[vec_dst.dst_idx];
            *dst = vra
                .assign_pin_vec_reg(dst.as_ssa().unwrap(), vec_dst.reg)
                .into();
        }

        vra.finish(pcopy);

        instr_alloc_scalar_dsts_file(instr, ip, sum, phi_webs, ra);
    } else if could_trivially_allocate {
        let mut vra = VecRegAllocator::new(ra);
        for vec_dst in vec_dsts {
            let dst = &mut instr.dsts_mut()[vec_dst.dst_idx];
            *dst = vra
                .assign_pin_vec_reg(dst.as_ssa().unwrap(), vec_dst.reg)
                .into();
        }

        instr_remap_srcs_file(instr, &mut vra);
        vra.free_killed(killed);
        vra.finish(pcopy);
        instr_alloc_scalar_dsts_file(instr, ip, sum, phi_webs, ra);
    } else {
        let mut vra = VecRegAllocator::new(ra);
        instr_remap_srcs_file(instr, &mut vra);

        // Allocate vector destinations first so we have the most freedom.
        // Scalar destinations can fill in holes.
        for dst in instr.dsts_mut() {
            if let Dst::SSA(ssa) = dst {
                if ssa.file() == vra.file() && ssa.comps() > 1 {
                    *dst = vra.alloc_vector(ssa).into();
                }
            }
        }

        vra.free_killed(killed);
        vra.finish(pcopy);

        instr_alloc_scalar_dsts_file(instr, ip, sum, phi_webs, ra);
    }
}

impl PerRegFile<RegAllocator> {
    pub fn assign_reg(&mut self, ssa: SSAValue, reg: RegRef) {
        assert!(reg.file() == ssa.file());
        assert!(reg.comps() == 1);
        self[ssa.file()].assign_reg(ssa, reg.base_idx());
    }

    pub fn free_killed(&mut self, killed: &KillSet) {
        for ssa in killed.iter() {
            self[ssa.file()].free_ssa(*ssa);
        }
    }
}
