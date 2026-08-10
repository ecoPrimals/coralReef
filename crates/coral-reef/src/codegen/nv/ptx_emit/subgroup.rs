// SPDX-License-Identifier: AGPL-3.0-or-later
//! PTX subgroup/warp operations — ballot, collective, gather, scan.

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::PtxVal;

impl PtxEmitter<'_> {
    pub(super) fn emit_subgroup_ballot(
        &mut self,
        result: naga::Handle<naga::Expression>,
        predicate: Option<naga::Handle<naga::Expression>>,
    ) -> Result<(), CompileError> {
        let pred_op = if let Some(pred_h) = predicate {
            let p = self.eval_expr(pred_h)?;
            self.ensure_pred(&p)?.fmt_operand()
        } else {
            "1".to_string()
        };
        let dst = self.alloc_r32();
        writeln_ptx!(
            self.body,
            "    vote.sync.ballot.b32 {}, {pred_op}, 0xFFFFFFFF;",
            dst.fmt_operand(),
        );
        self.values.insert(result, dst);
        Ok(())
    }

    pub(super) fn emit_subgroup_collective(
        &mut self,
        op: naga::SubgroupOperation,
        collective_op: naga::CollectiveOperation,
        argument: naga::Handle<naga::Expression>,
        result: naga::Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let val = self.eval_expr(argument)?;
        let val_scalar = self.scalar_of(argument);
        let dst = self.alloc_for_scalar(val_scalar);
        let type_suffix = Self::ptx_atom_type(val_scalar);

        match collective_op {
            naga::CollectiveOperation::Reduce => {
                let reduce_op = match op {
                    naga::SubgroupOperation::All => Some("and"),
                    naga::SubgroupOperation::Any => Some("or"),
                    naga::SubgroupOperation::Add => Some("add"),
                    naga::SubgroupOperation::Min => Some("min"),
                    naga::SubgroupOperation::Max => Some("max"),
                    naga::SubgroupOperation::And => Some("and"),
                    naga::SubgroupOperation::Or => Some("or"),
                    naga::SubgroupOperation::Xor => Some("xor"),
                    naga::SubgroupOperation::Mul => None,
                };
                if let Some(op_str) = reduce_op {
                    writeln_ptx!(
                        self.body,
                        "    redux.sync.{op_str}.{type_suffix} {}, {}, 0xFFFFFFFF;",
                        dst.fmt_operand(),
                        val.fmt_operand(),
                    );
                } else {
                    let scan_op = Self::scan_op_str(op, val_scalar)?;
                    self.emit_warp_scan(&val, &dst, type_suffix, scan_op, false, op, val_scalar);
                }
            }
            naga::CollectiveOperation::InclusiveScan | naga::CollectiveOperation::ExclusiveScan => {
                let scan_op = Self::scan_op_str(op, val_scalar)?;
                self.emit_warp_scan(
                    &val,
                    &dst,
                    type_suffix,
                    scan_op,
                    collective_op == naga::CollectiveOperation::ExclusiveScan,
                    op,
                    val_scalar,
                );
            }
        }
        self.values.insert(result, dst);
        Ok(())
    }

    pub(super) fn emit_subgroup_gather(
        &mut self,
        mode: naga::GatherMode,
        argument: naga::Handle<naga::Expression>,
        result: naga::Handle<naga::Expression>,
    ) -> Result<(), CompileError> {
        let val = self.eval_expr(argument)?;
        let val_scalar = self.scalar_of(argument);
        let dst = self.alloc_for_scalar(val_scalar);

        match mode {
            naga::GatherMode::BroadcastFirst => {
                writeln_ptx!(
                    self.body,
                    "    shfl.sync.idx.b32 {}, {}, 0, 0x1f1f, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                );
            }
            naga::GatherMode::Broadcast(idx_h) => {
                let idx = self.eval_expr(idx_h)?;
                writeln_ptx!(
                    self.body,
                    "    shfl.sync.idx.b32 {}, {}, {}, 0x1f1f, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                    idx.fmt_operand(),
                );
            }
            naga::GatherMode::ShuffleDown(offset_h) => {
                let offset = self.eval_expr(offset_h)?;
                writeln_ptx!(
                    self.body,
                    "    shfl.sync.down.b32 {}, {}, {}, 0x1f, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                    offset.fmt_operand(),
                );
            }
            naga::GatherMode::ShuffleUp(offset_h) => {
                let offset = self.eval_expr(offset_h)?;
                writeln_ptx!(
                    self.body,
                    "    shfl.sync.up.b32 {}, {}, {}, 0, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                    offset.fmt_operand(),
                );
            }
            naga::GatherMode::ShuffleXor(mask_h) => {
                let mask = self.eval_expr(mask_h)?;
                writeln_ptx!(
                    self.body,
                    "    shfl.sync.bfly.b32 {}, {}, {}, 0x1f, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                    mask.fmt_operand(),
                );
            }
            naga::GatherMode::Shuffle(idx_h) => {
                let idx = self.eval_expr(idx_h)?;
                writeln_ptx!(
                    self.body,
                    "    shfl.sync.idx.b32 {}, {}, {}, 0x1f1f, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                    idx.fmt_operand(),
                );
            }
            naga::GatherMode::QuadBroadcast(idx_h) => {
                let idx = self.eval_expr(idx_h)?;
                writeln_ptx!(
                    self.body,
                    "    shfl.sync.idx.b32 {}, {}, {}, 0x0003, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                    idx.fmt_operand(),
                );
            }
            naga::GatherMode::QuadSwap(_direction) => {
                writeln_ptx!(
                    self.body,
                    "    shfl.sync.bfly.b32 {}, {}, 1, 0x03, 0xFFFFFFFF;",
                    dst.fmt_operand(),
                    val.fmt_operand(),
                );
            }
        }

        self.values.insert(result, dst);
        Ok(())
    }

    pub(super) fn scan_op_str(
        op: naga::SubgroupOperation,
        scalar: naga::Scalar,
    ) -> Result<&'static str, CompileError> {
        let is_float = scalar.kind == naga::ScalarKind::Float;
        Ok(match op {
            naga::SubgroupOperation::Add => "add",
            naga::SubgroupOperation::Mul => {
                if is_float {
                    "mul"
                } else {
                    "mul.lo"
                }
            }
            naga::SubgroupOperation::Min => "min",
            naga::SubgroupOperation::Max => "max",
            naga::SubgroupOperation::And => "and",
            naga::SubgroupOperation::Or => "or",
            naga::SubgroupOperation::Xor => "xor",
            _ => {
                return Err(CompileError::NotImplemented(
                    format!("PTX scan op: {op:?}").into(),
                ));
            }
        })
    }

    pub(super) fn scan_identity(op: naga::SubgroupOperation, scalar: naga::Scalar) -> &'static str {
        match op {
            naga::SubgroupOperation::Add
            | naga::SubgroupOperation::Or
            | naga::SubgroupOperation::Xor => "0",
            naga::SubgroupOperation::Mul => "1",
            naga::SubgroupOperation::And => "0xFFFFFFFF",
            naga::SubgroupOperation::Min => {
                if scalar.kind == naga::ScalarKind::Float {
                    "0x7F800000" // +inf as f32 bits
                } else {
                    "0x7FFFFFFF" // i32 max
                }
            }
            naga::SubgroupOperation::Max => {
                if scalar.kind == naga::ScalarKind::Float {
                    "0xFF800000" // -inf as f32 bits
                } else {
                    "0x80000000" // i32 min
                }
            }
            _ => "0",
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "scan emission needs all parameters"
    )]
    pub(super) fn emit_warp_scan(
        &mut self,
        val: &PtxVal,
        dst: &PtxVal,
        type_suffix: &str,
        scan_op: &str,
        exclusive: bool,
        op: naga::SubgroupOperation,
        scalar: naga::Scalar,
    ) {
        let tmp = self.alloc_for_scalar(scalar);
        let pred = self.alloc_pred();

        writeln_ptx!(
            self.body,
            "    mov.{type_suffix} {}, {};",
            dst.fmt_operand(),
            val.fmt_operand(),
        );

        for offset in [1u32, 2, 4, 8, 16] {
            writeln_ptx!(
                self.body,
                "    shfl.sync.up.b32 {}|{}, {}, {offset}, 0, 0xFFFFFFFF;",
                tmp.fmt_operand(),
                pred.fmt_operand(),
                dst.fmt_operand(),
            );
            writeln_ptx!(
                self.body,
                "    @{} {scan_op}.{type_suffix} {}, {}, {};",
                pred.fmt_operand(),
                dst.fmt_operand(),
                dst.fmt_operand(),
                tmp.fmt_operand(),
            );
        }

        if exclusive {
            writeln_ptx!(
                self.body,
                "    shfl.sync.up.b32 {}|{}, {}, 1, 0, 0xFFFFFFFF;",
                tmp.fmt_operand(),
                pred.fmt_operand(),
                dst.fmt_operand(),
            );
            let identity = Self::scan_identity(op, scalar);
            writeln_ptx!(
                self.body,
                "    selp.{type_suffix} {}, {}, {identity}, {};",
                dst.fmt_operand(),
                tmp.fmt_operand(),
                pred.fmt_operand(),
            );
        }
    }
}
