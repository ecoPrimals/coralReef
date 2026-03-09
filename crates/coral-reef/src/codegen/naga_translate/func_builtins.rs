// SPDX-License-Identifier: AGPL-3.0-only
#![allow(clippy::wildcard_imports)]
use super::super::ir::*;
use super::func::FuncTranslator;
use crate::error::CompileError;

use super::sys_regs;

impl<'a, 'b> FuncTranslator<'a, 'b> {
    pub(super) fn read_sys_reg(&mut self, idx: u8) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        self.push_instr(Instr::new(OpS2R {
            dst: dst.into(),
            idx,
        }));
        dst
    }

    pub(super) fn resolve_builtin(
        &mut self,
        builtin: naga::BuiltIn,
    ) -> Result<SSARef, CompileError> {
        match builtin {
            naga::BuiltIn::GlobalInvocationId => {
                let tid_x = self.read_sys_reg(sys_regs::SR_TID_X);
                let tid_y = self.read_sys_reg(sys_regs::SR_TID_Y);
                let tid_z = self.read_sys_reg(sys_regs::SR_TID_Z);
                let ctaid_x = self.read_sys_reg(sys_regs::SR_CTAID_X);
                let ctaid_y = self.read_sys_reg(sys_regs::SR_CTAID_Y);
                let ctaid_z = self.read_sys_reg(sys_regs::SR_CTAID_Z);

                let wg = self.workgroup_size;

                let gid = self.alloc_ssa_vec(RegFile::GPR, 3);

                let tmp_x =
                    self.emit_imad(ctaid_x.into(), Src::new_imm_u32(wg[0]), tid_x.into());
                self.push_instr(Instr::new(OpCopy {
                    dst: gid[0].into(),
                    src: tmp_x.into(),
                }));
                let tmp_y =
                    self.emit_imad(ctaid_y.into(), Src::new_imm_u32(wg[1]), tid_y.into());
                self.push_instr(Instr::new(OpCopy {
                    dst: gid[1].into(),
                    src: tmp_y.into(),
                }));
                let tmp_z =
                    self.emit_imad(ctaid_z.into(), Src::new_imm_u32(wg[2]), tid_z.into());
                self.push_instr(Instr::new(OpCopy {
                    dst: gid[2].into(),
                    src: tmp_z.into(),
                }));
                Ok(gid)
            }
            naga::BuiltIn::LocalInvocationId => {
                let v = self.alloc_ssa_vec(RegFile::GPR, 3);
                let tid_x = self.read_sys_reg(sys_regs::SR_TID_X);
                let tid_y = self.read_sys_reg(sys_regs::SR_TID_Y);
                let tid_z = self.read_sys_reg(sys_regs::SR_TID_Z);
                self.push_instr(Instr::new(OpCopy {
                    dst: v[0].into(),
                    src: tid_x.into(),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: v[1].into(),
                    src: tid_y.into(),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: v[2].into(),
                    src: tid_z.into(),
                }));
                Ok(v)
            }
            naga::BuiltIn::WorkGroupId => {
                let v = self.alloc_ssa_vec(RegFile::GPR, 3);
                let x = self.read_sys_reg(sys_regs::SR_CTAID_X);
                let y = self.read_sys_reg(sys_regs::SR_CTAID_Y);
                let z = self.read_sys_reg(sys_regs::SR_CTAID_Z);
                self.push_instr(Instr::new(OpCopy {
                    dst: v[0].into(),
                    src: x.into(),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: v[1].into(),
                    src: y.into(),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: v[2].into(),
                    src: z.into(),
                }));
                Ok(v)
            }
            naga::BuiltIn::NumWorkGroups => {
                let v = self.alloc_ssa_vec(RegFile::GPR, 3);
                let x = self.read_sys_reg(sys_regs::SR_NCTAID_X);
                let y = self.read_sys_reg(sys_regs::SR_NCTAID_Y);
                let z = self.read_sys_reg(sys_regs::SR_NCTAID_Z);
                self.push_instr(Instr::new(OpCopy {
                    dst: v[0].into(),
                    src: x.into(),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: v[1].into(),
                    src: y.into(),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: v[2].into(),
                    src: z.into(),
                }));
                Ok(v)
            }
            naga::BuiltIn::LocalInvocationIndex => {
                let tid_x = self.read_sys_reg(sys_regs::SR_TID_X);
                let tid_y = self.read_sys_reg(sys_regs::SR_TID_Y);
                let tid_z = self.read_sys_reg(sys_regs::SR_TID_Z);

                let wg = self.workgroup_size;

                let yz = self.emit_imad(tid_z.into(), Src::new_imm_u32(wg[1]), tid_y.into());
                let idx = self.emit_imad(yz.into(), Src::new_imm_u32(wg[0]), tid_x.into());
                let v = self.alloc_ssa_vec(RegFile::GPR, 1);
                self.push_instr(Instr::new(OpCopy {
                    dst: v[0].into(),
                    src: idx.into(),
                }));
                Ok(v)
            }
            naga::BuiltIn::SubgroupInvocationId => {
                let lane = self.read_sys_reg(sys_regs::SR_LANEID);
                let v = self.alloc_ssa_vec(RegFile::GPR, 1);
                self.push_instr(Instr::new(OpCopy {
                    dst: v[0].into(),
                    src: lane.into(),
                }));
                Ok(v)
            }
            other => Err(CompileError::NotImplemented(
                format!("builtin {other:?} not yet supported").into(),
            )),
        }
    }

    pub(super) fn emit_imad(&mut self, a: Src, b: Src, c: Src) -> SSAValue {
        let dst = self.alloc_ssa(RegFile::GPR);
        if self.sm.sm() >= 70 {
            self.push_instr(Instr::new(OpIMad {
                dst: dst.into(),
                srcs: [a, b, c],
                signed: false,
            }));
        } else {
            let tmp = self.alloc_ssa(RegFile::GPR);
            self.push_instr(Instr::new(OpIMul {
                dst: tmp.into(),
                srcs: [a, b],
                signed: [false; 2],
                high: false,
            }));
            self.push_instr(Instr::new(OpIAdd2 {
                dst: dst.into(),
                srcs: [tmp.into(), c],
                carry_out: Dst::None,
            }));
        }
        dst
    }
}
