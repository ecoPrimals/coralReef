// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unit tests for SM20 control-flow encoders (`control.rs`).

use super::super::encoder::*;
use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Label, LabelAllocator, OpBar, OpBra, OpBrk, OpCont, OpExit, OpKill, OpNop, OpPBk, OpPCnt,
    OpSSy, OpSync,
};

fn sm20_encoder() -> SM20Encoder<'static> {
    let sm: &'static ShaderModel20 = Box::leak(Box::new(ShaderModel20::new(20)));
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(FxHashMap::default()));
    SM20Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
    }
}

fn unit(e: &SM20Encoder<'_>) -> u64 {
    e.get_field(0..3)
}

fn opcode_byte(e: &SM20Encoder<'_>) -> u64 {
    e.get_field(58..64)
}

fn label_0() -> Label {
    LabelAllocator::new().alloc()
}

fn encoder_with_label() -> (SM20Encoder<'static>, Label) {
    let sm: &'static ShaderModel20 = Box::leak(Box::new(ShaderModel20::new(20)));
    let label = label_0();
    let mut labels_map = FxHashMap::default();
    labels_map.insert(label, 64_usize);
    let labels: &'static FxHashMap<Label, usize> = Box::leak(Box::new(labels_map));
    let e = SM20Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 2],
    };
    (e, label)
}

#[test]
fn op_bra_exec_unit_and_opcode() {
    let (mut e, label) = encoder_with_label();
    OpBra {
        target: label,
        cond: true.into(),
    }
    .encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Exec as u64);
    assert_eq!(opcode_byte(&e), 0x10);
}

#[test]
fn op_bra_relative_offset_field() {
    let (mut e, label) = encoder_with_label();
    OpBra {
        target: label,
        cond: true.into(),
    }
    .encode(&mut e);
    let rel: u64 = e.get_field(26..50);
    let expected: u64 = 64 - 0 - 8;
    assert_eq!(rel, expected, "relative offset should be target - ip - 8");
}

#[test]
fn op_ssy_exec_unit_and_opcode() {
    let (mut e, label) = encoder_with_label();
    OpSSy { target: label }.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Exec as u64);
    assert_eq!(opcode_byte(&e), 0x18);
}

#[test]
fn op_sync_move_unit_and_sync_bit() {
    let mut e = sm20_encoder();
    OpSync { target: label_0() }.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0x10);
    assert!(e.get_bit(4), "sync bit 4 should be set on SM20");
}

#[test]
fn op_brk_exec_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpBrk { target: label_0() }.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Exec as u64);
    assert_eq!(opcode_byte(&e), 0x2a);
}

#[test]
fn op_pbk_exec_unit_and_opcode() {
    let (mut e, label) = encoder_with_label();
    OpPBk { target: label }.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Exec as u64);
    assert_eq!(opcode_byte(&e), 0x1a);
}

#[test]
fn op_cont_exec_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpCont { target: label_0() }.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Exec as u64);
    assert_eq!(opcode_byte(&e), 0x2c);
}

#[test]
fn op_pcnt_exec_unit_and_opcode() {
    let (mut e, label) = encoder_with_label();
    OpPCnt { target: label }.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Exec as u64);
    assert_eq!(opcode_byte(&e), 0x1c);
}

#[test]
fn op_exit_exec_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpExit {}.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Exec as u64);
    assert_eq!(opcode_byte(&e), 0x20);
}

#[test]
fn op_bar_move_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpBar {}.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0x14);
}

#[test]
fn op_kill_exec_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpKill {}.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Exec as u64);
    assert_eq!(opcode_byte(&e), 0x26);
}

#[test]
fn op_nop_move_unit_and_opcode() {
    let mut e = sm20_encoder();
    OpNop { label: None }.encode(&mut e);
    assert_eq!(unit(&e), SM20Unit::Move as u64);
    assert_eq!(opcode_byte(&e), 0x10);
    assert!(!e.get_bit(4), "nop should NOT have sync bit set");
}

#[test]
fn op_nop_vs_sync_differ_only_by_sync_bit() {
    let mut e_nop = sm20_encoder();
    OpNop { label: None }.encode(&mut e_nop);

    let mut e_sync = sm20_encoder();
    OpSync { target: label_0() }.encode(&mut e_sync);

    assert_eq!(unit(&e_nop), unit(&e_sync), "same unit");
    assert_eq!(opcode_byte(&e_nop), opcode_byte(&e_sync), "same opcode");
    assert!(!e_nop.get_bit(4));
    assert!(e_sync.get_bit(4));
}
