// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use crate::codegen::ir::{OpF64Rcp, OpF64Sqrt, Pred};

fn make_sm70() -> ShaderModelInfo {
    ShaderModelInfo::new(70, 64)
}

#[test]
fn test_newton_f64_constants() {
    const F64_NEG_HALF: u32 = 0xBFE0_0000;
    let f64_neg_half = f64::from_bits(u64::from(F64_NEG_HALF) << 32);
    assert!((f64_neg_half - (-0.5)).abs() < 1e-10);

    const F64_ONE_HALF: u32 = 0x3FF8_0000;
    let f64_one_half = f64::from_bits(u64::from(F64_ONE_HALF) << 32);
    assert!((f64_one_half - 1.5).abs() < 1e-10);

    const F64_TWO: u32 = 0x4000_0000;
    let f64_two = f64::from_bits(u64::from(F64_TWO) << 32);
    assert!((f64_two - 2.0).abs() < 1e-10);
}

#[test]
fn test_f64_sqrt_lowering_uses_rsq64h() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let x = alloc.alloc_vec(RegFile::GPR, 2);
    let dst = alloc.alloc_vec(RegFile::GPR, 2);

    let op = OpF64Sqrt {
        dst: dst.into(),
        src: Src::from(x),
    };
    let instr = Instr::new(op);
    let result = super::super::lower_instr(instr, &mut alloc, &sm);

    let MappedInstrs::Many(seq) = result else {
        panic!("Expected Many instructions");
    };
    let has_rsq64h = seq
        .iter()
        .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rsq64H));
    assert!(has_rsq64h, "sqrt lowering must use MUFU.Rsq64H seed");
}

#[test]
fn test_f64_rcp_lowering_uses_rcp64h() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let x = alloc.alloc_vec(RegFile::GPR, 2);
    let dst = alloc.alloc_vec(RegFile::GPR, 2);

    let op = OpF64Rcp {
        dst: dst.into(),
        src: Src::from(x),
    };
    let instr = Instr::new(op);
    let result = super::super::lower_instr(instr, &mut alloc, &sm);

    let MappedInstrs::Many(seq) = result else {
        panic!("Expected Many instructions");
    };
    let has_rcp64h = seq
        .iter()
        .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rcp64H));
    assert!(has_rcp64h, "rcp lowering must use MUFU.Rcp64H seed");
}

#[test]
fn test_f64_sqrt_lowering_two_newton_iterations() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let x = alloc.alloc_vec(RegFile::GPR, 2);
    let dst = alloc.alloc_vec(RegFile::GPR, 2);

    let op = OpF64Sqrt {
        dst: dst.into(),
        src: Src::from(x),
    };
    let instr = Instr::new(op);
    let result = super::super::lower_instr(instr, &mut alloc, &sm);

    let MappedInstrs::Many(seq) = result else {
        panic!("Expected Many instructions");
    };
    let dfma_count = seq.iter().filter(|i| matches!(i.op, Op::DFma(_))).count();
    assert!(
        dfma_count >= 2,
        "sqrt uses 2 Newton iterations (2 DFMA each)"
    );
}

#[test]
fn test_f64_sqrt_lowering_produces_dfma_sequence() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let x = alloc.alloc_vec(RegFile::GPR, 2);
    let dst = alloc.alloc_vec(RegFile::GPR, 2);

    let op = OpF64Sqrt {
        dst: dst.into(),
        src: Src::from(x),
    };
    let instr = Instr::new(op);
    let result = super::super::lower_instr(instr, &mut alloc, &sm);

    let MappedInstrs::Many(seq) = result else {
        panic!("Expected Many instructions");
    };
    assert!(seq.len() > 10, "sqrt should expand to many instructions");
    let has_transcendental = seq.iter().any(|i| matches!(i.op, Op::Transcendental(_)));
    let has_dfma = seq.iter().any(|i| matches!(i.op, Op::DFma(_)));
    let has_dmul = seq.iter().any(|i| matches!(i.op, Op::DMul(_)));
    assert!(has_transcendental, "sqrt lowering should use MUFU.Rsq64H");
    assert!(has_dfma, "sqrt lowering should use DFMA");
    assert!(has_dmul, "sqrt lowering should use DMul");
}

#[test]
fn test_f64_rcp_lowering_produces_dfma_sequence() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let x = alloc.alloc_vec(RegFile::GPR, 2);
    let dst = alloc.alloc_vec(RegFile::GPR, 2);

    let op = OpF64Rcp {
        dst: dst.into(),
        src: Src::from(x),
    };
    let instr = Instr::new(op);
    let result = super::super::lower_instr(instr, &mut alloc, &sm);

    let MappedInstrs::Many(seq) = result else {
        panic!("Expected Many instructions");
    };
    assert!(seq.len() > 5, "rcp should expand to multiple instructions");
    let has_transcendental = seq.iter().any(|i| matches!(i.op, Op::Transcendental(_)));
    let has_dmul = seq.iter().any(|i| matches!(i.op, Op::DMul(_)));
    assert!(has_transcendental, "rcp lowering should use MUFU.Rcp64H");
    assert!(has_dmul, "rcp lowering should use DMul");
}

#[test]
fn test_f64_sqrt_lowering_direct() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let x = alloc.alloc_vec(RegFile::GPR, 2);
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    let op = OpF64Sqrt {
        dst: dst.into(),
        src: Src::from(x),
    };
    let seq = lower_f64_sqrt(&op, Pred::from(true), &mut alloc, &sm);
    assert!(
        seq.len() >= 15,
        "sqrt direct lowering should produce ~20+ instructions"
    );
    let has_transcendental = seq
        .iter()
        .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rsq64H));
    assert!(has_transcendental);
}

#[test]
fn test_f64_rcp_lowering_direct() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let x = alloc.alloc_vec(RegFile::GPR, 2);
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    let op = OpF64Rcp {
        dst: dst.into(),
        src: Src::from(x),
    };
    let seq = lower_f64_rcp(&op, Pred::from(true), &mut alloc, &sm);
    assert!(
        seq.len() >= 10,
        "rcp direct lowering should produce >= 10 instructions"
    );
    let has_transcendental = seq
        .iter()
        .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rcp64H));
    assert!(has_transcendental);
}

#[test]
fn test_f64_rcp_lowering_two_newton_iterations() {
    let sm = make_sm70();
    let mut alloc = SSAValueAllocator::new();
    let x = alloc.alloc_vec(RegFile::GPR, 2);
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    let op = OpF64Rcp {
        dst: dst.into(),
        src: Src::from(x),
    };
    let seq = lower_f64_rcp(&op, Pred::from(true), &mut alloc, &sm);
    let dadd_count = seq.iter().filter(|i| matches!(i.op, Op::DAdd(_))).count();
    let dmul_count = seq.iter().filter(|i| matches!(i.op, Op::DMul(_))).count();
    assert!(
        dmul_count >= 3,
        "rcp has 2 iterations: x*y0, x*y1, y1*factor2"
    );
    assert!(dadd_count >= 2, "rcp computes 2 - x*y0 and 2 - x*y1");
}

fn make_sm120() -> ShaderModelInfo {
    ShaderModelInfo::new(120, 64)
}

#[test]
fn test_f64_rcp_sm120_uses_f2f_not_rcp64h() {
    let sm = make_sm120();
    let mut alloc = SSAValueAllocator::new();
    let x = alloc.alloc_vec(RegFile::GPR, 2);
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    let op = OpF64Rcp {
        dst: dst.into(),
        src: Src::from(x),
    };
    let seq = lower_f64_rcp(&op, Pred::from(true), &mut alloc, &sm);

    let has_rcp64h = seq
        .iter()
        .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rcp64H));
    assert!(!has_rcp64h, "SM120 must NOT use MUFU.RCP64H");

    let has_f2f = seq.iter().any(|i| matches!(&i.op, Op::F2F(_)));
    assert!(has_f2f, "SM120 should use F2F for f64↔f32 conversion");

    let has_rcp_f32 = seq
        .iter()
        .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rcp));
    assert!(has_rcp_f32, "SM120 should use MUFU.RCP (f32) as seed");
}

#[test]
fn test_f64_sqrt_sm120_uses_f2f_not_rsq64h() {
    let sm = make_sm120();
    let mut alloc = SSAValueAllocator::new();
    let x = alloc.alloc_vec(RegFile::GPR, 2);
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    let op = OpF64Sqrt {
        dst: dst.into(),
        src: Src::from(x),
    };
    let seq = lower_f64_sqrt(&op, Pred::from(true), &mut alloc, &sm);

    let has_rsq64h = seq
        .iter()
        .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rsq64H));
    assert!(!has_rsq64h, "SM120 must NOT use MUFU.RSQ64H");

    let has_f2f = seq.iter().any(|i| matches!(&i.op, Op::F2F(_)));
    assert!(has_f2f, "SM120 should use F2F for f64↔f32 conversion");

    let has_rsq_f32 = seq
        .iter()
        .any(|i| matches!(&i.op, Op::Transcendental(m) if m.op == TranscendentalOp::Rsq));
    assert!(has_rsq_f32, "SM120 should use MUFU.RSQ (f32) as seed");
}

#[test]
fn test_f64_rcp_sm120_has_newton_iterations() {
    let sm = make_sm120();
    let mut alloc = SSAValueAllocator::new();
    let x = alloc.alloc_vec(RegFile::GPR, 2);
    let dst = alloc.alloc_vec(RegFile::GPR, 2);
    let op = OpF64Rcp {
        dst: dst.into(),
        src: Src::from(x),
    };
    let seq = lower_f64_rcp(&op, Pred::from(true), &mut alloc, &sm);
    let dmul_count = seq.iter().filter(|i| matches!(i.op, Op::DMul(_))).count();
    assert!(
        dmul_count >= 3,
        "SM120 rcp should have at least 2 NR iterations (3+ DMul)"
    );
}
