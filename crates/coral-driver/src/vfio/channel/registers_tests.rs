// SPDX-License-Identifier: AGPL-3.0-or-later

use super::falcon;
use super::pbdma;
use super::pccsr;
use super::pfifo;
use super::pri;
use super::{INSTANCE_IOVA, PD0_IOVA, PT_ENTRIES, TARGET_SYS_MEM_COHERENT};

#[test]
fn pfifo_runlist_stride() {
    assert_eq!(pfifo::runlist_base(0), 0x2270);
    assert_eq!(pfifo::runlist_base(1), 0x2280);
    assert_eq!(pfifo::runlist_submit(2), 0x2294);
}

#[test]
fn gv100_runlist_base_and_submit_encode() {
    let iova = 0x1_2345_6789_ABCD_u64;
    assert_eq!(pfifo::gv100_runlist_base_value(iova), (iova >> 12) as u32);
    let entry_count = 7_u32;
    let expected_submit = ((iova >> 44) as u32) | (entry_count << 16);
    assert_eq!(
        pfifo::gv100_runlist_submit_value(iova, entry_count),
        expected_submit
    );
}

#[test]
fn pfifo_intr_bits() {
    assert_eq!(pfifo::INTR_BIT8 | pfifo::INTR_CHSW_ERROR, 0x0001_0100);
    assert_ne!(pfifo::INTR_RL_COMPLETE, 0);
    assert_ne!(pfifo::INTR_PBDMA, 0);
    assert_eq!(pfifo::INTR_RL_COMPLETE & pfifo::INTR_PBDMA, 0);
}

#[test]
fn pbdma_stride_and_intr() {
    assert_eq!(pbdma::base(0), 0x4_0000);
    assert_eq!(pbdma::base(1), 0x4_2000);
    assert_eq!(pbdma::intr(0), 0x4_0108);
    assert_eq!(pbdma::intr(1), 0x4_2108);
}

#[test]
fn pccsr_inst_channel_and_status() {
    assert_eq!(pccsr::inst(0), 0x80_0000);
    assert_eq!(pccsr::channel(0), 0x80_0004);
    let ch = (0x5 << 24) | 1;
    assert_eq!(pccsr::status(ch), 0x5);
    assert_eq!(pccsr::status_name(ch), "ON_PBDMA");
}

#[test]
fn pccsr_channel_flags() {
    assert_eq!(pccsr::inst(3), 0x80_0018);
    assert_eq!(
        pccsr::CHANNEL_ENABLE_SET & pccsr::CHANNEL_ENABLE_CLR,
        0,
        "enable set/clear are distinct bits"
    );
}

#[test]
fn pri_error_classifiers() {
    assert!(pri::is_pri_error(0xBADF_1234));
    assert!(pri::is_pri_timeout(0xBAD0_0200));
    assert!(pri::is_pri_access_error(0xBADF_0000));
    assert!(!pri::is_pri_error(0x1234_5678));
}

#[test]
fn pri_decode_and_domain_names() {
    assert_eq!(
        pri::decode_pri_error(0xBADF_1100),
        "FBPA power-gated (BLCG/SLCG)"
    );
    assert_eq!(pri::decode_pri_error(0xBAD0_0200), "PBUS timeout");
    assert_eq!(pri::domain_name(0x200), "PMC");
    assert_eq!(pri::domain_name(0x2000), "PFIFO");
    assert_eq!(pri::domain_name(0x100_C80), "PFB_NISO/MMU");
    assert_eq!(pri::domain_name(0xFF_FFFF), "UNKNOWN");
}

#[test]
fn falcon_hwcfg_sizes() {
    let hwcfg = (3 << 9) | 5;
    assert_eq!(falcon::imem_size_bytes(hwcfg), 5 * 256);
    assert_eq!(falcon::dmem_size_bytes(hwcfg), 3 * 256);
    assert_eq!(falcon::CPUCTL_STARTCPU | falcon::CPUCTL_IINVAL, 0x3);
}

#[test]
fn channel_iova_and_target_constants() {
    assert_eq!(INSTANCE_IOVA, 0x3000);
    assert_eq!(PD0_IOVA, 0x8000);
    assert_eq!(TARGET_SYS_MEM_COHERENT, 2);
    assert_eq!(PT_ENTRIES, 512);
}
