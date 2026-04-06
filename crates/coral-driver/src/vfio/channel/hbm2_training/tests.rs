// SPDX-License-Identifier: AGPL-3.0-or-later
//! HBM2 training unit tests.

use super::constants::volta_hbm2;
use super::types::{DramReady, LinkTrained, PhyUp, Untrained, Verified};
use super::types::{FbpaOffset, Hbm2TrainingError, LtcOffset, TrainingLog};

#[test]
fn phase_debug_names() {
    assert_eq!(format!("{:?}", Untrained), "Untrained");
    assert_eq!(format!("{:?}", PhyUp), "PhyUp");
    assert_eq!(format!("{:?}", LinkTrained), "LinkTrained");
    assert_eq!(format!("{:?}", DramReady), "DramReady");
    assert_eq!(format!("{:?}", Verified), "Verified");
}

#[test]
fn training_error_display() {
    let err = Hbm2TrainingError {
        phase: "enable_phy",
        detail: "no FBPA alive".into(),
        register_snapshot: vec![(0x9A0000, 0xDEAD_DEAD)],
    };
    assert!(err.to_string().contains("enable_phy"));
    assert!(err.to_string().contains("no FBPA alive"));
}

#[test]
fn fbpa_offset_newtype_prevents_mixup() {
    let fbpa = FbpaOffset(0x04);
    let ltc = LtcOffset(0x04);
    assert_eq!(fbpa.0, ltc.0);
}

#[test]
fn volta_fbpa_reg_calculation() {
    assert_eq!(volta_hbm2::fbpa_reg(0, FbpaOffset(0x04)), 0x9A0004);
    assert_eq!(volta_hbm2::fbpa_reg(1, FbpaOffset(0x04)), 0x9A4004);
    assert_eq!(volta_hbm2::fbpa_reg(3, FbpaOffset(0x80)), 0x9AC080);
}

#[test]
fn training_log_counts_writes() {
    let mut log = TrainingLog::default();
    log.log_write(0x200, 0xFFFF_FFFF, 0);
    log.log_read(0x200, 0x5FEC_DFF1);
    log.log_write(0x9A0000, 0x42, 0);
    assert_eq!(log.write_count(), 2);
}
