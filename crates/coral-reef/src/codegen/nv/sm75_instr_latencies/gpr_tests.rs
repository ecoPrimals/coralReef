use super::RegLatencySM75::{self, *};

const WRITERS: &[RegLatencySM75] = &[
    CoupledDisp64,
    CoupledDisp,
    CoupledAlu,
    CoupledFMA,
    IMADLo,
    IMADWideLower,
    IMADWideUpper,
    RedirectedFP64,
    RedirectedFP16,
    RedirectedHMMA_884_F16(0),
    RedirectedHMMA_884_F16(2),
    RedirectedHMMA_884_F32(0),
    RedirectedHMMA_884_F32(2),
    RedirectedHMMA_1688,
    RedirectedHMMA_16816,
    IMMA(0),
    IMMA(2),
    Decoupled,
    BMov,
    GuardPredicate,
];
const READERS: &[RegLatencySM75] = &[
    CoupledDisp64,
    CoupledDisp,
    CoupledAlu,
    CoupledFMA,
    IMADLo,
    IMADWideAB,
    IMADWideLower,
    IMADWideUpper,
    RedirectedFP64,
    RedirectedFP16,
    RedirectedHMMA_884_F16(0),
    RedirectedHMMA_884_F16(2),
    RedirectedHMMA_884_F32(0),
    RedirectedHMMA_884_F32(2),
    RedirectedHMMA_1688,
    RedirectedHMMA_16816,
    IMMA(0),
    IMMA(2),
    Decoupled,
    DecoupledOther,
];
const WAW_CATS: &[RegLatencySM75] = &[
    CoupledDisp64,
    CoupledDisp,
    CoupledAlu,
    CoupledFMA,
    IMADLo,
    IMADWideLower,
    IMADWideUpper,
    RedirectedFP64,
    RedirectedFP16,
    RedirectedHMMA_884_F16(0),
    RedirectedHMMA_884_F32(0),
    RedirectedHMMA_1688,
    RedirectedHMMA_16816,
    IMMA(0),
    IMMA(2),
    Decoupled,
    BMov,
];
const WAR_WRITERS: &[RegLatencySM75] = &[
    CoupledDisp64,
    CoupledDisp,
    CoupledAlu,
    CoupledFMA,
    IMADLo,
    IMADWideLower,
    IMADWideUpper,
    RedirectedFP64,
    RedirectedFP16,
    RedirectedHMMA_884_F16(0),
    RedirectedHMMA_884_F32(0),
    RedirectedHMMA_1688,
    RedirectedHMMA_16816,
    IMMA(0),
    Decoupled,
    BMov,
];
const PRED_WRITERS: &[RegLatencySM75] = &[
    CoupledAlu,
    CoupledFMA,
    IMADLo,
    Decoupled,
    RedirectedFP64,
    RedirectedFP16,
];
const PRED_READERS: &[RegLatencySM75] = &[
    CoupledDisp,
    CoupledAlu,
    CoupledFMA,
    IMADLo,
    Decoupled,
    RedirectedFP64,
    RedirectedFP16,
];
const PRED_WAW_CATS: &[RegLatencySM75] = &[
    CoupledDisp,
    CoupledAlu,
    CoupledFMA,
    IMADLo,
    IMADWideLower,
    IMADWideUpper,
    RedirectedFP64,
    RedirectedFP16,
    Decoupled,
];
const PRED_WAR_READERS: &[RegLatencySM75] = &[
    CoupledAlu,
    CoupledFMA,
    IMADLo,
    IMADWideUpper,
    IMADWideLower,
    RedirectedFP64,
    RedirectedFP16,
    Decoupled,
    CoupledDisp64,
];
const PRED_WAR_WRITERS: &[RegLatencySM75] = &[
    CoupledDisp,
    CoupledAlu,
    CoupledFMA,
    IMADLo,
    IMADWideUpper,
    IMADWideLower,
    RedirectedFP64,
    RedirectedFP16,
    Decoupled,
];

#[test]
fn raw_latency_all_pairs() {
    for w in WRITERS {
        for r in READERS {
            let lat = RegLatencySM75::read_after_write(*w, *r);
            assert!(lat >= 1, "RAW({w:?}, {r:?}) = {lat}, expected >= 1");
        }
    }
}

#[test]
fn waw_latency_all_pairs() {
    for w1 in WAW_CATS {
        for w2 in WAW_CATS {
            for has_pred in [false, true] {
                let lat = RegLatencySM75::write_after_write(*w1, *w2, has_pred);
                assert!(lat >= 1, "WAW({w1:?}, {w2:?}, pred={has_pred}) = {lat}");
            }
        }
    }
}

#[test]
fn war_latency_all_pairs() {
    for r in READERS {
        for w in WAR_WRITERS {
            let lat = RegLatencySM75::write_after_read(*r, *w);
            assert!(lat >= 1, "WAR({r:?}, {w:?}) = {lat}");
        }
    }
}

#[test]
fn pred_raw_latency_pairs() {
    for w in PRED_WRITERS {
        for r in PRED_READERS {
            let lat = RegLatencySM75::pred_read_after_write(*w, *r);
            assert!(lat >= 1, "pred_RAW({w:?}, {r:?}) = {lat}");
        }
    }
}

#[test]
fn pred_waw_latency_pairs() {
    for w1 in PRED_WAW_CATS {
        for w2 in PRED_WAW_CATS {
            for has_pred in [false, true] {
                let lat = RegLatencySM75::pred_write_after_write(*w1, *w2, has_pred);
                assert!(
                    lat >= 1,
                    "pred_WAW({w1:?}, {w2:?}, pred={has_pred}) = {lat}"
                );
            }
        }
    }
}

#[test]
fn pred_war_latency_pairs() {
    for r in PRED_WAR_READERS {
        for w in PRED_WAR_WRITERS {
            let lat = RegLatencySM75::pred_write_after_read(*r, *w);
            assert!(lat >= 1, "pred_WAR({r:?}, {w:?}) = {lat}");
        }
    }
}
