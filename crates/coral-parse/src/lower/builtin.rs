// SPDX-License-Identifier: AGPL-3.0-only
//! Built-in variable lowering → S2R system register reads.

use super::{sys_regs, FuncLowerer};
use crate::ast::BuiltIn;
use coral_reef::codegen::ir::*;
use coral_reef::error::CompileError;

impl FuncLowerer<'_, '_> {
    pub(crate) fn lower_builtin(&mut self, builtin: BuiltIn) -> Result<SSARef, CompileError> {
        match builtin {
            BuiltIn::GlobalInvocationId => self.lower_global_invocation_id(),
            BuiltIn::LocalInvocationId => self.lower_vec3_s2r(
                sys_regs::SR_TID_X, sys_regs::SR_TID_Y, sys_regs::SR_TID_Z,
            ),
            BuiltIn::WorkGroupId => self.lower_vec3_s2r(
                sys_regs::SR_CTAID_X, sys_regs::SR_CTAID_Y, sys_regs::SR_CTAID_Z,
            ),
            BuiltIn::NumWorkGroups => self.lower_vec3_s2r(
                sys_regs::SR_NCTAID_X, sys_regs::SR_NCTAID_Y, sys_regs::SR_NCTAID_Z,
            ),
            BuiltIn::LocalInvocationIndex => {
                // Linearized: tid.x + tid.y * wg_size.x + tid.z * wg_size.x * wg_size.y
                let tid_x = self.alloc_ssa(RegFile::GPR);
                let tid_y = self.alloc_ssa(RegFile::GPR);
                let tid_z = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpS2R { dst: tid_x.into(), idx: sys_regs::SR_TID_X }));
                self.push_instr(Instr::new(OpS2R { dst: tid_y.into(), idx: sys_regs::SR_TID_Y }));
                self.push_instr(Instr::new(OpS2R { dst: tid_z.into(), idx: sys_regs::SR_TID_Z }));

                let wx = self.workgroup_size[0];
                let wy = self.workgroup_size[1];

                // y_contrib = tid.y * wg_size.x
                let y_contrib = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpIMul {
                    dst: y_contrib.into(),
                    srcs: [Src::from(tid_y), Src::new_imm_u32(wx)],
                    signed: [false, false],
                    high: false,
                }));
                // z_contrib = tid.z * (wg_size.x * wg_size.y)
                let z_contrib = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpIMul {
                    dst: z_contrib.into(),
                    srcs: [Src::from(tid_z), Src::new_imm_u32(wx * wy)],
                    signed: [false, false],
                    high: false,
                }));

                let tmp = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpIAdd2 {
                    dsts: [tmp.into(), Dst::None],
                    srcs: [Src::from(tid_x), Src::from(y_contrib)],
                }));
                let result = self.alloc_ssa(RegFile::GPR);
                self.push_instr(Instr::new(OpIAdd2 {
                    dsts: [result.into(), Dst::None],
                    srcs: [Src::from(tmp), Src::from(z_contrib)],
                }));
                Ok(result.into())
            }
            BuiltIn::WorkGroupSize => {
                let dst = self.alloc_ssa_vec(RegFile::GPR, 3);
                self.push_instr(Instr::new(OpCopy {
                    dst: dst[0].into(),
                    src: Src::new_imm_u32(self.workgroup_size[0]),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: dst[1].into(),
                    src: Src::new_imm_u32(self.workgroup_size[1]),
                }));
                self.push_instr(Instr::new(OpCopy {
                    dst: dst[2].into(),
                    src: Src::new_imm_u32(self.workgroup_size[2]),
                }));
                Ok(dst)
            }
            _ => {
                Err(CompileError::InvalidInput(
                    format!("unsupported builtin: {:?}", builtin).into(),
                ))
            }
        }
    }

    fn lower_vec3_s2r(&mut self, x: u8, y: u8, z: u8) -> Result<SSARef, CompileError> {
        let dst = self.alloc_ssa_vec(RegFile::GPR, 3);
        self.push_instr(Instr::new(OpS2R { dst: dst[0].into(), idx: x }));
        self.push_instr(Instr::new(OpS2R { dst: dst[1].into(), idx: y }));
        self.push_instr(Instr::new(OpS2R { dst: dst[2].into(), idx: z }));
        Ok(dst)
    }

    fn lower_global_invocation_id(&mut self) -> Result<SSARef, CompileError> {
        let tid = self.lower_vec3_s2r(
            sys_regs::SR_TID_X, sys_regs::SR_TID_Y, sys_regs::SR_TID_Z,
        )?;
        let ctaid = self.lower_vec3_s2r(
            sys_regs::SR_CTAID_X, sys_regs::SR_CTAID_Y, sys_regs::SR_CTAID_Z,
        )?;

        let result = self.alloc_ssa_vec(RegFile::GPR, 3);
        for i in 0..3 {
            let wg_scaled = self.alloc_ssa(RegFile::GPR);
            self.push_instr(Instr::new(OpIMul {
                dst: wg_scaled.into(),
                srcs: [Src::from(ctaid[i]), Src::new_imm_u32(self.workgroup_size[i])],
                signed: [false, false],
                high: false,
            }));
            self.push_instr(Instr::new(OpIAdd2 {
                dsts: [result[i].into(), Dst::None],
                srcs: [Src::from(tid[i]), Src::from(wg_scaled)],
            }));
        }
        Ok(result)
    }
}
