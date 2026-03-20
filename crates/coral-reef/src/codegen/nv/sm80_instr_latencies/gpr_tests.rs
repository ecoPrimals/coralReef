// SPDX-License-Identifier: AGPL-3.0-only
use super::RegLatencySM80;
use RegLatencySM80::*;

fn valid_writers() -> Vec<RegLatencySM80> {
    vec![
        CoupledAlu,
        CoupledDisp64,
        CoupledFMA,
        IMADWideWriteDL,
        IMADWideWriteDH,
        FP16,
        FP16_Alu,
        FP16_F32,
        HFMA2_MMA,
        RedirectedFP64,
        Clmad,
        IMMA_88,
        MMA_1x_collect,
        MMA_2x_collect,
        DMMA,
        Cbu,
        Decoupled,
        DecoupledAgu,
    ]
}

fn valid_readers() -> Vec<RegLatencySM80> {
    vec![
        CoupledAlu,
        CoupledFMA,
        IMADWideReadAB,
        IMADWideReadCL,
        IMADWideReadCH,
        FP16,
        FP16_F32,
        HFMA2_MMA,
        RedirectedFP64,
        Clmad,
        IMMA_88,
        MMA_1x_collect,
        MMA_2x_collect,
        DMMA,
        Decoupled,
        DecoupledAgu,
    ]
}

fn waw_valid_writers() -> Vec<RegLatencySM80> {
    vec![
        CoupledAlu,
        CoupledDisp64,
        CoupledFMA,
        IMADWideWriteDL,
        IMADWideWriteDH,
        FP16,
        FP16_Alu,
        FP16_F32,
        HFMA2_MMA,
        RedirectedFP64,
        Clmad,
        IMMA_88,
        MMA_1x_collect,
        MMA_2x_collect,
        DMMA,
    ]
}

fn war_valid_writers() -> Vec<RegLatencySM80> {
    vec![
        CoupledAlu,
        CoupledDisp64,
        CoupledFMA,
        IMADWideWriteDL,
        IMADWideWriteDH,
        FP16,
        FP16_Alu,
        FP16_F32,
        HFMA2_MMA,
        RedirectedFP64,
        Clmad,
        IMMA_88,
        MMA_1x_collect,
        MMA_2x_collect,
        DMMA,
        Decoupled,
        DecoupledAgu,
    ]
}

#[test]
fn raw_latency_all_pairs() {
    for w in valid_writers() {
        for r in valid_readers() {
            let lat = RegLatencySM80::read_after_write(w, r);
            assert!(lat >= 1, "RAW({w:?}, {r:?}) = {lat}");
        }
    }
}

#[test]
fn waw_latency_all_pairs() {
    for w1 in waw_valid_writers() {
        for w2 in waw_valid_writers() {
            for has_pred in [false, true] {
                let lat = RegLatencySM80::write_after_write(w1, w2, has_pred);
                assert!(lat >= 1, "WAW({w1:?}, {w2:?}, pred={has_pred}) = {lat}");
            }
        }
    }
}

#[test]
fn war_latency_all_pairs() {
    for r in valid_readers() {
        for w in war_valid_writers() {
            let lat = RegLatencySM80::write_after_read(r, w);
            assert!(lat >= 1, "WAR({r:?}, {w:?}) = {lat}");
        }
    }
}

#[test]
fn op_category_sm80_representative() {
    use crate::codegen::ir::{
        Dst, FloatType, HmmaSize, ImmaSize, IntType, MemAccess, MemAddrType, MemEvictionPriority,
        MemOrder, MemScope, MemSpace, MemType, OffsetStride, Op, OpCS2R, OpDFma, OpHAdd2, OpHmma,
        OpImma, OpLd, OpMov, OpNop, RegFile, RegRef, Src,
    };
    use RegLatencySM80::*;

    fn gpr() -> Dst {
        Dst::Reg(RegRef::new(RegFile::GPR, 0, 1))
    }

    assert!(matches!(
        RegLatencySM80::op_category(
            &Op::Mov(Box::new(OpMov {
                dst: gpr(),
                src: Src::ZERO,
                quad_lanes: 0xf,
            })),
            false,
            0
        ),
        CoupledAlu
    ));

    assert!(matches!(
        RegLatencySM80::op_category(&Op::Nop(OpNop { label: None }), false, 0),
        CoupledDisp64
    ));

    assert!(matches!(
        RegLatencySM80::op_category(
            &Op::DFma(Box::new(OpDFma {
                dst: gpr(),
                srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
                rnd_mode: crate::codegen::ir::FRndMode::NearestEven,
            })),
            false,
            0
        ),
        RedirectedFP64
    ));

    assert!(matches!(
        RegLatencySM80::op_category(
            &Op::HAdd2(Box::new(OpHAdd2 {
                dst: gpr(),
                srcs: [Src::ZERO, Src::ZERO],
                saturate: false,
                ftz: false,
                f32: false,
            })),
            false,
            0
        ),
        FP16
    ));

    assert!(matches!(
        RegLatencySM80::op_category(
            &Op::Hmma(Box::new(OpHmma {
                dst: gpr(),
                mat_size: HmmaSize::M16N8K8,
                src_type: FloatType::F16,
                dst_type: FloatType::F32,
                srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
            })),
            false,
            0
        ),
        MMA_1x_collect
    ));

    assert!(matches!(
        RegLatencySM80::op_category(
            &Op::Imma(Box::new(OpImma {
                dst: gpr(),
                mat_size: ImmaSize::M16N8K16,
                src_types: [IntType::I8, IntType::I8],
                saturate: false,
                srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
            })),
            false,
            0
        ),
        MMA_1x_collect
    ));

    assert!(matches!(
        RegLatencySM80::op_category(
            &Op::CS2R(Box::new(OpCS2R {
                dst: Dst::Reg(RegRef::new(RegFile::GPR, 0, 2)),
                idx: 0,
            })),
            false,
            0
        ),
        CoupledDisp64
    ));

    assert!(matches!(
        RegLatencySM80::op_category(
            &Op::Ld(Box::new(OpLd {
                dst: gpr(),
                addr: Src::ZERO,
                offset: 0,
                stride: OffsetStride::X1,
                access: MemAccess {
                    mem_type: MemType::B32,
                    space: MemSpace::Global(MemAddrType::A32),
                    order: MemOrder::Strong(MemScope::CTA),
                    eviction_priority: MemEvictionPriority::Normal,
                },
            })),
            false,
            0
        ),
        DecoupledAgu
    ));
}
