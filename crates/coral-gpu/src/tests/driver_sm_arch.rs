// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! `driver::sm_to_nvarch` mapping (Linux).

#[cfg(target_os = "linux")]
#[test]
fn sm_to_nvarch_maps_known_versions() {
    use coral_reef::NvArch;

    assert_eq!(crate::driver::sm_to_nvarch(35), NvArch::Sm35);
    assert_eq!(crate::driver::sm_to_nvarch(37), NvArch::Sm35);
    for sm in 70_u32..=89 {
        let arch = crate::driver::sm_to_nvarch(sm);
        let expected = match sm {
            75 => NvArch::Sm75,
            80 => NvArch::Sm80,
            86 => NvArch::Sm86,
            89 => NvArch::Sm89,
            _ => NvArch::Sm70,
        };
        assert_eq!(arch, expected, "SM {sm}");
    }
    assert_eq!(crate::driver::sm_to_nvarch(99), NvArch::Sm70);
    assert_eq!(crate::driver::sm_to_nvarch(120), NvArch::Sm120);
}
