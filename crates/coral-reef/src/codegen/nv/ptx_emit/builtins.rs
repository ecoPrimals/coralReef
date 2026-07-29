// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::PtxVal;

impl PtxEmitter<'_> {
    pub(super) fn precompute_builtins(&mut self) -> Result<(), CompileError> {
        let args: Vec<_> = self.func.arguments.iter().enumerate().collect();
        let exprs: Vec<_> = self.func.expressions.iter().collect();

        for (handle, expr) in &exprs {
            if let naga::Expression::FunctionArgument(idx) = expr {
                if let Some(arg) = args.get(*idx as usize) {
                    if let Some(naga::Binding::BuiltIn(builtin)) = &arg.1.binding {
                        let val = self.emit_builtin(*builtin)?;
                        self.values.insert(*handle, val);
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn emit_builtin(&mut self, builtin: naga::BuiltIn) -> Result<PtxVal, CompileError> {
        match builtin {
            naga::BuiltIn::GlobalInvocationId => {
                let mut components = Vec::with_capacity(3);
                for (axis, dim_label) in ["x", "y", "z"].iter().enumerate() {
                    let tid = self.alloc_r32();
                    let ctaid = self.alloc_r32();
                    let ntid = self.alloc_r32();
                    let gid = self.alloc_r32();
                    writeln_ptx!(
                        self.body,
                        "    mov.u32 {}, %tid.{dim_label};",
                        tid.fmt_operand()
                    );
                    if self.workgroup_size[axis] == 1 {
                        writeln_ptx!(
                            self.body,
                            "    mov.u32 {}, %ctaid.{dim_label};",
                            gid.fmt_operand()
                        );
                    } else {
                        writeln_ptx!(
                            self.body,
                            "    mov.u32 {}, %ctaid.{dim_label};",
                            ctaid.fmt_operand()
                        );
                        writeln_ptx!(
                            self.body,
                            "    mov.u32 {}, %ntid.{dim_label};",
                            ntid.fmt_operand()
                        );
                        writeln_ptx!(
                            self.body,
                            "    mad.lo.u32 {}, {}, {}, {};",
                            gid.fmt_operand(),
                            ctaid.fmt_operand(),
                            ntid.fmt_operand(),
                            tid.fmt_operand(),
                        );
                    }
                    components.push(gid);
                }
                Ok(PtxVal::Vec(components))
            }
            naga::BuiltIn::LocalInvocationId => {
                let mut components = Vec::with_capacity(3);
                for dim_label in ["x", "y", "z"] {
                    let r = self.alloc_r32();
                    writeln_ptx!(
                        self.body,
                        "    mov.u32 {}, %tid.{dim_label};",
                        r.fmt_operand()
                    );
                    components.push(r);
                }
                Ok(PtxVal::Vec(components))
            }
            naga::BuiltIn::WorkGroupId => {
                let mut components = Vec::with_capacity(3);
                for dim_label in ["x", "y", "z"] {
                    let r = self.alloc_r32();
                    writeln_ptx!(
                        self.body,
                        "    mov.u32 {}, %ctaid.{dim_label};",
                        r.fmt_operand()
                    );
                    components.push(r);
                }
                Ok(PtxVal::Vec(components))
            }
            naga::BuiltIn::NumWorkGroups => {
                let mut components = Vec::with_capacity(3);
                for dim_label in ["x", "y", "z"] {
                    let r = self.alloc_r32();
                    writeln_ptx!(
                        self.body,
                        "    mov.u32 {}, %nctaid.{dim_label};",
                        r.fmt_operand()
                    );
                    components.push(r);
                }
                Ok(PtxVal::Vec(components))
            }
            naga::BuiltIn::LocalInvocationIndex => {
                let r = self.alloc_r32();
                let tx = self.alloc_r32();
                let ty = self.alloc_r32();
                let tz = self.alloc_r32();
                let nx = self.alloc_r32();
                let ny = self.alloc_r32();
                writeln_ptx!(self.body, "    mov.u32 {}, %tid.x;", tx.fmt_operand());
                writeln_ptx!(self.body, "    mov.u32 {}, %tid.y;", ty.fmt_operand());
                writeln_ptx!(self.body, "    mov.u32 {}, %tid.z;", tz.fmt_operand());
                writeln_ptx!(self.body, "    mov.u32 {}, %ntid.x;", nx.fmt_operand());
                writeln_ptx!(self.body, "    mov.u32 {}, %ntid.y;", ny.fmt_operand());
                let tmp = self.alloc_r32();
                // index = tx + ty * nx + tz * nx * ny
                writeln_ptx!(
                    self.body,
                    "    mad.lo.u32 {}, {}, {}, {};",
                    r.fmt_operand(),
                    ty.fmt_operand(),
                    nx.fmt_operand(),
                    tx.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    mul.lo.u32 {}, {}, {};",
                    tmp.fmt_operand(),
                    nx.fmt_operand(),
                    ny.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    mad.lo.u32 {}, {}, {}, {};",
                    r.fmt_operand(),
                    tz.fmt_operand(),
                    tmp.fmt_operand(),
                    r.fmt_operand(),
                );
                Ok(r)
            }
            naga::BuiltIn::SubgroupInvocationId => {
                let r = self.alloc_r32();
                writeln_ptx!(self.body, "    mov.u32 {}, %laneid;", r.fmt_operand());
                Ok(r)
            }
            naga::BuiltIn::SubgroupSize => {
                let r = self.alloc_r32();
                writeln_ptx!(self.body, "    mov.u32 {}, 32;", r.fmt_operand());
                Ok(r)
            }
            naga::BuiltIn::WorkGroupSize => {
                let mut components = Vec::with_capacity(3);
                for dim_label in ["x", "y", "z"] {
                    let r = self.alloc_r32();
                    writeln_ptx!(
                        self.body,
                        "    mov.u32 {}, %ntid.{dim_label};",
                        r.fmt_operand()
                    );
                    components.push(r);
                }
                Ok(PtxVal::Vec(components))
            }
            naga::BuiltIn::NumSubgroups => {
                let nx = self.alloc_r32();
                let ny = self.alloc_r32();
                let nz = self.alloc_r32();
                let total = self.alloc_r32();
                let result = self.alloc_r32();
                writeln_ptx!(self.body, "    mov.u32 {}, %ntid.x;", nx.fmt_operand());
                writeln_ptx!(self.body, "    mov.u32 {}, %ntid.y;", ny.fmt_operand());
                writeln_ptx!(self.body, "    mov.u32 {}, %ntid.z;", nz.fmt_operand());
                writeln_ptx!(
                    self.body,
                    "    mul.lo.u32 {}, {}, {};",
                    total.fmt_operand(),
                    nx.fmt_operand(),
                    ny.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    mul.lo.u32 {}, {}, {};",
                    total.fmt_operand(),
                    total.fmt_operand(),
                    nz.fmt_operand(),
                );
                let rounded = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    add.u32 {}, {}, 31;",
                    rounded.fmt_operand(),
                    total.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    shr.u32 {}, {}, 5;",
                    result.fmt_operand(),
                    rounded.fmt_operand(),
                );
                Ok(result)
            }
            naga::BuiltIn::SubgroupId => {
                let tx = self.alloc_r32();
                let ty = self.alloc_r32();
                let tz = self.alloc_r32();
                let nx = self.alloc_r32();
                let ny = self.alloc_r32();
                let linear = self.alloc_r32();
                let result = self.alloc_r32();
                writeln_ptx!(self.body, "    mov.u32 {}, %tid.x;", tx.fmt_operand());
                writeln_ptx!(self.body, "    mov.u32 {}, %tid.y;", ty.fmt_operand());
                writeln_ptx!(self.body, "    mov.u32 {}, %tid.z;", tz.fmt_operand());
                writeln_ptx!(self.body, "    mov.u32 {}, %ntid.x;", nx.fmt_operand());
                writeln_ptx!(self.body, "    mov.u32 {}, %ntid.y;", ny.fmt_operand());
                let tmp = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    mad.lo.u32 {}, {}, {}, {};",
                    linear.fmt_operand(),
                    ty.fmt_operand(),
                    nx.fmt_operand(),
                    tx.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    mul.lo.u32 {}, {}, {};",
                    tmp.fmt_operand(),
                    nx.fmt_operand(),
                    ny.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    mad.lo.u32 {}, {}, {}, {};",
                    linear.fmt_operand(),
                    tz.fmt_operand(),
                    tmp.fmt_operand(),
                    linear.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    shr.u32 {}, {}, 5;",
                    result.fmt_operand(),
                    linear.fmt_operand(),
                );
                Ok(result)
            }
            other => Err(CompileError::NotImplemented(
                format!("PTX builtin: {other:?}").into(),
            )),
        }
    }

    pub(super) fn load_buffer_params(&mut self) {
        let binding_info: Vec<_> = self
            .bindings
            .iter()
            .enumerate()
            .map(|(i, b)| (i, b.binding, b.gv_handle))
            .collect();
        for (i, idx, gv_handle) in binding_info {
            let ptr_reg = self.alloc_rd64();
            let _size_reg = self.alloc_rd64();
            writeln_ptx!(
                self.body,
                "    ld.param.u64 {}, [_buf{idx}_ptr];",
                ptr_reg.fmt_operand()
            );
            writeln_ptx!(
                self.body,
                "    ld.param.u64 {}, [_buf{idx}_size];",
                _size_reg.fmt_operand()
            );
            self.gv_ptr_regs.insert(gv_handle, (ptr_reg, i));
        }
    }
}
