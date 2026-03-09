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
