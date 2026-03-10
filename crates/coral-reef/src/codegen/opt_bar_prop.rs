// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

#![allow(clippy::wildcard_imports)]

use super::debug::{DEBUG, GetDebugFlags};
use super::ir::*;

use coral_reef_stubs::bitset::BitSet;
use coral_reef_stubs::fxhash::FxHashMap;

struct PhiMap {
    phi_ssa: FxHashMap<Phi, Vec<SSAValue>>,
    ssa_phi: FxHashMap<SSAValue, Phi>,
}

impl PhiMap {
    pub fn new() -> Self {
        Self {
            ssa_phi: FxHashMap::default(),
            phi_ssa: FxHashMap::default(),
        }
    }

    fn add_phi_srcs(&mut self, op: &OpPhiSrcs) {
        for (phi, src) in op.srcs.iter() {
            if let SrcRef::SSA(ssa) = &src.reference {
                assert!(ssa.comps() == 1);
                let phi_srcs = self.phi_ssa.entry(*phi).or_default();
                phi_srcs.push(ssa[0]);
            }
        }
    }

    fn add_phi_dsts(&mut self, op: &OpPhiDsts) {
        for (phi, dst) in op.dsts.iter() {
            if let Dst::SSA(ssa) = dst {
                assert!(ssa.comps() == 1);
                self.ssa_phi.insert(ssa[0], *phi);
            }
        }
    }

    fn get_phi(&self, ssa: &SSAValue) -> Option<&Phi> {
        self.ssa_phi.get(ssa)
    }

    fn phi_srcs(&self, idx: &Phi) -> &[SSAValue] {
        static EMPTY: [SSAValue; 0] = [];
        self.phi_ssa
            .get(idx)
            .map(|v| v.as_slice())
            .unwrap_or(&EMPTY)
    }
}

#[derive(Default)]
struct BarPropPass {
    ssa_map: FxHashMap<SSAValue, SSAValue>,
    phi_is_bar: BitSet<Phi>,
    phi_is_not_bar: BitSet<Phi>,
}

impl BarPropPass {
    pub fn new() -> Self {
        Self::default()
    }

    fn add_copy(&mut self, dst: SSAValue, src: SSAValue) {
        assert!(dst.file() == RegFile::Bar || src.file() == RegFile::Bar);
        self.ssa_map.insert(dst, src);
    }

    fn is_bar(&self, ssa: &SSAValue) -> bool {
        ssa.file() == RegFile::Bar || self.ssa_map.contains_key(ssa)
    }

    fn map_bar<'a>(&'a self, ssa: &'a SSAValue) -> Option<&'a SSAValue> {
        let mut ssa = ssa;
        let mut last_bar = None;
        loop {
            let Some(mapped) = self.ssa_map.get(ssa) else {
                break;
            };

            if mapped.file() == RegFile::Bar {
                last_bar = Some(mapped);
            }
            ssa = mapped;
        }

        last_bar
    }

    fn phi_can_be_bar_recur(&mut self, phi_map: &PhiMap, seen: &mut BitSet<Phi>, phi: Phi) -> bool {
        if self.phi_is_not_bar.contains(phi) {
            return false;
        }

        if seen.contains(phi) {
            // If we've hit a cycle, that's not a fail
            return true;
        }

        seen.insert(phi);

        for src_ssa in phi_map.phi_srcs(&phi) {
            if self.is_bar(src_ssa) {
                continue;
            }

            if let Some(src_phi) = phi_map.get_phi(src_ssa) {
                if self.phi_can_be_bar_recur(phi_map, seen, *src_phi) {
                    continue;
                }
            }

            self.phi_is_not_bar.insert(phi);
            return false;
        }

        true
    }

    fn add_phi_recur(
        &mut self,
        ssa_alloc: &mut SSAValueAllocator,
        phi_map: &PhiMap,
        needs_bar: &mut BitSet<Phi>,
        phi: Phi,
        ssa: SSAValue,
    ) {
        if !needs_bar.contains(phi) {
            return;
        }

        let bar = ssa_alloc.alloc(RegFile::Bar);
        self.ssa_map.insert(ssa, bar);
        self.phi_is_bar.insert(phi);
        needs_bar.remove(phi);

        for src_ssa in phi_map.phi_srcs(&phi) {
            if let Some(src_phi) = phi_map.get_phi(src_ssa) {
                self.add_phi_recur(ssa_alloc, phi_map, needs_bar, *src_phi, *src_ssa);
            }
        }
    }

    fn try_add_phi(
        &mut self,
        ssa_alloc: &mut SSAValueAllocator,
        phi_map: &PhiMap,
        phi: Phi,
        ssa: SSAValue,
    ) {
        if self.phi_is_bar.contains(phi) {
            return;
        }

        let mut seen = BitSet::<Phi>::new(super::PHI_BITSET_CAPACITY);
        if self.phi_can_be_bar_recur(phi_map, &mut seen, phi) {
            self.add_phi_recur(ssa_alloc, phi_map, &mut seen, phi, ssa);
            assert!(seen.is_empty());
        }
    }

    fn run(&mut self, f: &mut Function) {
        let mut phi_map = PhiMap::new();
        let mut phis_want_bar = Vec::new();
        for b in &f.blocks {
            for instr in &b.instrs {
                match &instr.op {
                    Op::PhiSrcs(op) => {
                        phi_map.add_phi_srcs(op);
                    }
                    Op::PhiDsts(op) => {
                        phi_map.add_phi_dsts(op);
                    }
                    Op::BMov(op) => {
                        assert!(!op.clear);
                        assert!(op.src.is_unmodified());
                        let Some(dst) = op.dst.as_ssa() else { continue };
                        let Some(src) = op.src.as_ssa() else { continue };
                        assert!(dst.comps() == 1 && src.comps() == 1);

                        self.add_copy(dst[0], src[0]);

                        if let Some(phi) = phi_map.get_phi(&src[0]) {
                            phis_want_bar.push((*phi, src[0]));
                        }
                    }
                    _ => (),
                }
            }
        }

        for (phi, ssa) in phis_want_bar.into_iter() {
            self.try_add_phi(&mut f.ssa_alloc, &phi_map, phi, ssa);
        }

        f.map_instrs(|mut instr, _| {
            match &mut instr.op {
                Op::PhiSrcs(op) => {
                    for (idx, src) in op.srcs.iter_mut() {
                        if self.phi_is_bar.contains(*idx) {
                            // Barrier immediates don't exist
                            let Some(ssa) = src.as_ssa() else { continue };
                            let Some(bar) = self.map_bar(&ssa[0]) else {
                                continue;
                            };
                            *src = (*bar).into();
                        }
                    }
                    MappedInstrs::One(instr)
                }
                Op::PhiDsts(op) => {
                    let mut bmovs = Vec::new();
                    for (idx, dst) in op.dsts.iter_mut() {
                        if self.phi_is_bar.contains(*idx) {
                            let Some(ssa) = dst.as_ssa() else { continue };
                            let ssa = ssa.clone();
                            let Some(bar) = self.ssa_map.get(&ssa[0]) else {
                                continue;
                            };
                            *dst = (*bar).into();

                            // On the off chance that someone still wants the
                            // GPR version of this barrier, insert an OpBMov to
                            // copy into the GPR.  DCE will clean it up if it's
                            // never used.
                            bmovs.push(Instr::new(OpBMov {
                                dst: ssa.into(),
                                src: (*bar).into(),
                                clear: false,
                            }));
                        }
                    }
                    if bmovs.is_empty() {
                        MappedInstrs::One(instr)
                    } else {
                        if DEBUG.annotate() {
                            bmovs.insert(
                                0,
                                Instr::new(OpAnnotate {
                                    annotation: "generated by opt_bar_prop".into(),
                                }),
                            );
                        }
                        bmovs.insert(1, instr);
                        MappedInstrs::Many(bmovs)
                    }
                }
                _ => {
                    let src_types = instr.src_types();
                    for (i, src) in instr.srcs_mut().iter_mut().enumerate() {
                        if src_types[i] != SrcType::Bar {
                            continue;
                        }
                        if let SrcRef::SSA(ssa) = &src.reference {
                            if let Some(bar) = self.map_bar(&ssa[0]) {
                                *src = (*bar).into();
                            }
                        }
                    }
                    MappedInstrs::One(instr)
                }
            }
        });
    }
}

impl Shader<'_> {
    pub fn opt_bar_prop(&mut self) {
        for f in &mut self.functions {
            BarPropPass::new().run(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        BasicBlock, ComputeShaderInfo, Function, Instr, LabelAllocator, Op, OpBMov, OpBSync,
        OpExit, PhiAllocator, SSAValueAllocator, Shader, ShaderInfo, ShaderIoInfo, ShaderStageInfo,
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
    fn test_bar_prop_basic() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let bar_ssa = ssa_alloc.alloc(RegFile::Bar);
        let gpr_ssa = ssa_alloc.alloc(RegFile::GPR);
        let mut shader = make_shader_with_function(
            vec![
                Instr::new(OpBMov {
                    dst: gpr_ssa.into(),
                    src: bar_ssa.into(),
                    clear: false,
                }),
                Instr::new(OpBSync {
                    srcs: [gpr_ssa.into(), true.into()],
                }),
                Instr::new(OpExit {}),
            ],
            ssa_alloc,
        );

        shader.opt_bar_prop();

        let bsync = &shader.functions[0].blocks[0].instrs[1];
        let Op::BSync(op) = &bsync.op else {
            panic!("expected BSync");
        };
        let SrcRef::SSA(ssa) = &op.bar().reference else {
            panic!("expected SSA bar source");
        };
        assert_eq!(ssa[0], bar_ssa, "bar should be propagated to original bar");
    }

    #[test]
    fn test_bar_prop_multiple_bsync_same_bar() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let bar_ssa = ssa_alloc.alloc(RegFile::Bar);
        let gpr_ssa = ssa_alloc.alloc(RegFile::GPR);
        let mut shader = make_shader_with_function(
            vec![
                Instr::new(OpBMov {
                    dst: gpr_ssa.into(),
                    src: bar_ssa.into(),
                    clear: false,
                }),
                Instr::new(OpBSync {
                    srcs: [gpr_ssa.into(), true.into()],
                }),
                Instr::new(OpBSync {
                    srcs: [gpr_ssa.into(), true.into()],
                }),
                Instr::new(OpExit {}),
            ],
            ssa_alloc,
        );

        shader.opt_bar_prop();

        for i in [1, 2] {
            let bsync = &shader.functions[0].blocks[0].instrs[i];
            let Op::BSync(op) = &bsync.op else {
                panic!("expected BSync");
            };
            let SrcRef::SSA(ssa) = &op.bar().reference else {
                panic!("expected SSA bar source");
            };
            assert_eq!(ssa[0], bar_ssa);
        }
    }
}
