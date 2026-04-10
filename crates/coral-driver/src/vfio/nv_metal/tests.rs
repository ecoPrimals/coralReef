// SPDX-License-Identifier: AGPL-3.0-or-later

use super::NvVoltaIdentity;

fn boot0_from_arch(arch_code: u16, rev: u8) -> u32 {
    (u32::from(arch_code) << 20) | u32::from(rev)
}

#[test]
fn nv_volta_identity_from_boot0_table() {
    struct Case {
        boot0: u32,
        want_chip: &'static str,
        want_arch: &'static str,
        want_impl: u8,
        want_rev: u8,
    }

    let cases = [
        // GV100-class (Titan V / Tesla V100): same BOOT0 arch nibble; revision may vary.
        Case {
            boot0: boot0_from_arch(0x140, 0xA1),
            want_chip: "GV100",
            want_arch: "Volta",
            want_impl: 0x40,
            want_rev: 0xA1,
        },
        Case {
            boot0: boot0_from_arch(0x140, 0x00),
            want_chip: "GV100",
            want_arch: "Volta",
            want_impl: 0x40,
            want_rev: 0x00,
        },
        // Turing: implementation nibble encoded in low byte of arch_code (see `from_boot0`).
        Case {
            boot0: boot0_from_arch(0x162, 0x01),
            want_chip: "TU62",
            want_arch: "Turing",
            want_impl: 0x62,
            want_rev: 0x01,
        },
        Case {
            boot0: boot0_from_arch(0x164, 0x02),
            want_chip: "TU64",
            want_arch: "Turing",
            want_impl: 0x64,
            want_rev: 0x02,
        },
        Case {
            boot0: boot0_from_arch(0x166, 0x03),
            want_chip: "TU66",
            want_arch: "Turing",
            want_impl: 0x66,
            want_rev: 0x03,
        },
        // GA102-class (Ampere)
        Case {
            boot0: boot0_from_arch(0x172, 0x04),
            want_chip: "GA72",
            want_arch: "Ampere",
            want_impl: 0x72,
            want_rev: 0x04,
        },
        // AD102-class (Ada Lovelace)
        Case {
            boot0: boot0_from_arch(0x192, 0x05),
            want_chip: "AD92",
            want_arch: "Ada",
            want_impl: 0x92,
            want_rev: 0x05,
        },
        // Unknown architecture code (must fit in 9-bit `arch_code` after `& 0x1FF`).
        Case {
            boot0: boot0_from_arch(0x100, 0x06),
            want_chip: "NV100",
            want_arch: "Unknown(0x100)",
            want_impl: 0x00,
            want_rev: 0x06,
        },
        // All zeros / all ones
        Case {
            boot0: 0x0000_0000,
            want_chip: "NV000",
            want_arch: "Unknown(0x0)",
            want_impl: 0x00,
            want_rev: 0x00,
        },
        Case {
            boot0: 0xFFFF_FFFF,
            want_chip: "NV1FF",
            want_arch: "Unknown(0x1ff)",
            want_impl: 0xFF,
            want_rev: 0xFF,
        },
    ];

    for c in cases {
        let id = NvVoltaIdentity::from_boot0(c.boot0);
        assert_eq!(id.boot0, c.boot0, "boot0 round-trip");
        assert_eq!(id.chip_impl, c.want_impl, "chip_impl for {:#x}", c.boot0);
        assert_eq!(id.chip_rev, c.want_rev, "chip_rev for {:#x}", c.boot0);
        assert_eq!(
            id.chip_name_str, c.want_chip,
            "chip name for {:#x}",
            c.boot0
        );
        assert_eq!(id.arch_name, c.want_arch, "arch for {:#x}", c.boot0);
    }
}
