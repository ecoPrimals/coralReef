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
            70..=74 => NvArch::Sm70,
            75 => NvArch::Sm75,
            76..=79 => NvArch::Sm75,
            80..=85 => NvArch::Sm80,
            86..=88 => NvArch::Sm86,
            89 => NvArch::Sm89,
            _ => unreachable!("loop iterates 70..=89 only"),
        };
        assert_eq!(arch, expected, "SM {sm}");
    }
    assert_eq!(crate::driver::sm_to_nvarch(99), NvArch::Sm70);
    assert_eq!(crate::driver::sm_to_nvarch(120), NvArch::Sm120);
}

/// SM integers that are not mapped to a dedicated [`NvArch`] use a documented
/// fallback — see `driver::sm_to_nvarch`.
#[test]
fn sm_to_nvarch_fallbacks_match_table() {
    use coral_reef::NvArch;

    assert_eq!(crate::driver::sm_to_nvarch(50), NvArch::Sm35);
    assert_eq!(crate::driver::sm_to_nvarch(60), NvArch::Sm75);
    assert_eq!(crate::driver::sm_to_nvarch(90), NvArch::Sm89);
    assert_eq!(crate::driver::sm_to_nvarch(100), NvArch::Sm120);
    for sm in [91_u32, 99, 101] {
        assert_eq!(crate::driver::sm_to_nvarch(sm), NvArch::Sm70, "SM {sm}");
    }
}

#[test]
fn sm_to_nvarch_explicit_known_sm_values() {
    use coral_reef::NvArch;

    assert_eq!(crate::driver::sm_to_nvarch(35), NvArch::Sm35);
    assert_eq!(crate::driver::sm_to_nvarch(37), NvArch::Sm35);
    assert_eq!(crate::driver::sm_to_nvarch(70), NvArch::Sm70);
    assert_eq!(crate::driver::sm_to_nvarch(75), NvArch::Sm75);
    assert_eq!(crate::driver::sm_to_nvarch(80), NvArch::Sm80);
    assert_eq!(crate::driver::sm_to_nvarch(86), NvArch::Sm86);
    assert_eq!(crate::driver::sm_to_nvarch(89), NvArch::Sm89);
    assert_eq!(crate::driver::sm_to_nvarch(120), NvArch::Sm120);
}
