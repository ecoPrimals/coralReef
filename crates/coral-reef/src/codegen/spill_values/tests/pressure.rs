// SPDX-License-Identifier: AGPL-3.0-only
//! Single-block register pressure tests: GPR, UGPR, Pred chains with varying limits.

use super::*;

#[test]
fn test_spill_values_ugpr_with_high_pressure() {
    let mut func = make_function_with_many_ugprs(15);
    let mut info = ShaderInfo {
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
    };
    func.to_cssa();
    func.spill_values(RegFile::UGPR, 4, &mut info).unwrap();
    assert!(!func.blocks[0].instrs.is_empty());
}

#[test]
fn test_spill_values_preserves_semantics() {
    let mut func = make_function_with_many_gprs(8);
    let mut info = ShaderInfo {
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
    };
    func.to_cssa();
    func.spill_values(RegFile::GPR, 4, &mut info).unwrap();
    assert!(!func.blocks[0].instrs.is_empty());
    let last = func.blocks[0].instrs.last().unwrap();
    assert!(matches!(last.op, Op::Exit(_)));
}

#[test]
fn test_spill_values_pred_with_high_pressure() {
    let mut func = make_function_with_many_preds(8);
    let mut info = ShaderInfo {
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
    };
    func.to_cssa();
    func.spill_values(RegFile::Pred, 4, &mut info).unwrap();
    assert!(!func.blocks[0].instrs.is_empty());
}

/// Very low limit with high pressure exercises spill cost/selection paths.
#[test]
fn test_spill_values_very_low_limit() {
    let mut func = make_function_with_many_gprs(20);
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, 2, &mut info).unwrap();
    let last = func.blocks[0].instrs.last().unwrap();
    assert!(matches!(last.op, Op::Exit(_)));
}

/// High limit: no spilling needed; exercises early-exit paths.
#[test]
fn test_spill_values_no_spill_needed() {
    let mut func = make_function_with_many_gprs(4);
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, 64, &mut info).unwrap();
    assert_eq!(info.spills_to_mem, 0);
    assert_eq!(info.fills_from_mem, 0);
}

/// `limit == 1` is the most aggressive GPR bound for the main spill path.
#[test]
fn test_spill_values_gpr_limit_one() {
    let mut func = make_function_with_many_gprs(24);
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, LIMIT_ONE_GPR, &mut info)
        .unwrap();
    assert_eq!(func.blocks.len(), 1);
    assert!(matches!(
        func.blocks[0].instrs.last().unwrap().op,
        Op::Exit(_)
    ));
}

#[test]
fn test_spill_values_extreme_gpr_pressure_many_defs() {
    let mut func = make_function_with_many_gprs(SPILL_STRESS_MANY_DEFS);
    let mut info = default_shader_info();
    func.to_cssa();
    func.spill_values(RegFile::GPR, LIMIT_TWO_GPR, &mut info)
        .expect("spill_values should succeed with a long copy chain and low GPR limit");
    assert_eq!(func.blocks.len(), 1);
    assert!(matches!(
        func.blocks[0].instrs.last().expect("non-empty block").op,
        Op::Exit(_)
    ));
}
