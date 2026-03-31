// SPDX-License-Identifier: AGPL-3.0-only
//! Cooperative workgroup scheduling for the `CoralIR` interpreter.
//!
//! Models GPU barrier semantics: all invocations within a workgroup advance to
//! each `workgroupBarrier()` before any proceeds past it. Per-thread write
//! buffers ensure a thread sees its own shared-memory writes while preventing
//! cross-thread visibility until the next barrier.

use std::collections::HashMap;

use coral_reef::codegen::ir::{Phi, Shader};

use super::mem_ops::write_u32_to_shared;
use super::{CpuError, InvocationCtx, OpEffect, RegValue, eval_op, eval_pred};

/// Per-invocation execution state, preserved across barrier yields.
pub(super) struct InvocationState {
    ctx: InvocationCtx,
    regs: HashMap<u32, RegValue>,
    phi_state: HashMap<Phi, RegValue>,
    block_idx: usize,
    instr_idx: usize,
    completed: bool,
}

enum StepResult {
    Barrier,
    Completed,
}

/// Execute one workgroup with cooperative barrier scheduling.
pub(super) fn execute_workgroup(
    shader: &Shader<'_>,
    buffers: &mut [Vec<u8>],
    shared_mem: &mut [u8],
    workgroup_id: [u32; 3],
    num_workgroups: [u32; 3],
    workgroup_size: [u32; 3],
) -> Result<(), CpuError> {
    if shader.functions.is_empty() {
        return Ok(());
    }

    let mk = |tx, ty, tz| InvocationState {
        ctx: InvocationCtx {
            workgroup_id,
            local_id: [tx, ty, tz],
            num_workgroups,
            workgroup_size,
        },
        regs: HashMap::new(),
        phi_state: HashMap::new(),
        block_idx: 0,
        instr_idx: 0,
        completed: false,
    };
    let mut invocations: Vec<_> = (0..workgroup_size[2])
        .flat_map(|tz| {
            (0..workgroup_size[1])
                .flat_map(move |ty| (0..workgroup_size[0]).map(move |tx| mk(tx, ty, tz)))
        })
        .collect();

    let mut snapshot = shared_mem.to_vec();
    let mut write_bufs: Vec<HashMap<usize, u32>> = vec![HashMap::new(); invocations.len()];

    loop {
        let mut any_active = false;
        let mut all_at_barrier = true;

        for (idx, inv) in invocations.iter_mut().enumerate() {
            if inv.completed {
                continue;
            }
            any_active = true;
            match step_invocation(
                shader,
                buffers,
                shared_mem,
                &snapshot,
                &mut write_bufs[idx],
                inv,
            )? {
                StepResult::Barrier => {}
                StepResult::Completed => {
                    inv.completed = true;
                    all_at_barrier = false;
                }
            }
        }

        if !any_active {
            break;
        }
        if all_at_barrier {
            for wb in &mut write_bufs {
                for (&off, &val) in &*wb {
                    write_u32_to_shared(shared_mem, off, val);
                }
                wb.clear();
            }
            snapshot.copy_from_slice(shared_mem);
        } else if invocations.iter().all(|i| i.completed) {
            break;
        }
    }

    Ok(())
}

/// Run one invocation until it hits a barrier or completes.
fn step_invocation(
    shader: &Shader<'_>,
    buffers: &mut [Vec<u8>],
    shared_mem: &mut [u8],
    shared_snapshot: &[u8],
    shared_writes: &mut HashMap<usize, u32>,
    state: &mut InvocationState,
) -> Result<StepResult, CpuError> {
    let func = &shader.functions[0];

    let label_to_block: HashMap<String, usize> = func
        .blocks
        .iter()
        .enumerate()
        .map(|(i, bb)| (format!("{}", bb.label), i))
        .collect();

    while state.block_idx < func.blocks.len() {
        let bb = &func.blocks[state.block_idx];
        let mut next_block = Some(state.block_idx + 1);

        for instr in bb.instrs.iter().skip(state.instr_idx) {
            state.instr_idx += 1;

            if instr.pred.is_false() {
                continue;
            }
            if !instr.pred.is_true() && !eval_pred(&instr.pred, &state.regs) {
                continue;
            }
            match eval_op(
                &instr.op,
                &mut state.regs,
                &mut state.phi_state,
                buffers,
                shared_mem,
                shared_snapshot,
                shared_writes,
                &state.ctx,
                &label_to_block,
            )? {
                OpEffect::Continue => {}
                OpEffect::Branch(target_idx) => {
                    next_block = Some(target_idx);
                    break;
                }
                OpEffect::Exit => {
                    return Ok(StepResult::Completed);
                }
                OpEffect::Barrier => {
                    return Ok(StepResult::Barrier);
                }
            }
        }

        match next_block {
            Some(idx) if idx < func.blocks.len() => {
                state.block_idx = idx;
                state.instr_idx = 0;
            }
            _ => return Ok(StepResult::Completed),
        }
    }

    Ok(StepResult::Completed)
}
