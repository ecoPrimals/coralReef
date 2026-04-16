// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{NvClass, classes};

#[test]
fn nv_class_key_constants_match_expected_values() {
    assert_eq!(NvClass::VOLTA_COMPUTE_A.0, 0xC3C0);
    assert_eq!(NvClass::AMPERE_COMPUTE_A.0, 0xC6C0);
    assert_eq!(NvClass::HOPPER_COMPUTE_A.0, 0xCBC0);
    assert_eq!(NvClass::MAXWELL_COMPUTE_B.0, 0xB1C0);
    assert_eq!(NvClass::PASCAL_COMPUTE.0, 0xC0C0);
    assert_eq!(NvClass::KEPLER_COMPUTE_A.0, 0xA0C0);
}

#[test]
fn nv_class_display_shows_name_and_hex() {
    let s = format!("{}", NvClass::VOLTA_COMPUTE_A);
    assert!(s.contains("Volta"));
    assert!(s.contains("C3C0"));
}

#[test]
fn nv_class_from_u32() {
    let v: u32 = NvClass::VOLTA_COMPUTE_A.into();
    assert_eq!(v, 0xC3C0);
}

#[test]
fn submodule_constants_backward_compatible() {
    assert_eq!(classes::clc3c0::VOLTA_COMPUTE_A, 0xC3C0);
    assert_eq!(classes::clc6c0::AMPERE_COMPUTE_A, 0xC6C0);
    assert_eq!(classes::clcbc0::HOPPER_COMPUTE_A, 0xCBC0);
    assert_eq!(classes::clb1c0::MAXWELL_COMPUTE_B, 0xB1C0);
}

#[test]
fn nv_class_display_all_variants() {
    assert!(format!("{}", NvClass::KEPLER_SPH).contains("Kepler SPH"));
    assert!(format!("{}", NvClass::KEPLER_COMPUTE_A).contains("Kepler Compute A"));
    assert!(format!("{}", NvClass::MAXWELL_COMPUTE_A).contains("Maxwell Compute A"));
    assert!(format!("{}", NvClass::MAXWELL_COMPUTE_B).contains("Maxwell Compute B"));
    assert!(format!("{}", NvClass::PASCAL_COMPUTE).contains("Pascal Compute"));
    assert!(format!("{}", NvClass::VOLTA_COMPUTE_A).contains("Volta Compute A"));
    assert!(format!("{}", NvClass::AMPERE_COMPUTE_A).contains("Ampere Compute A"));
    assert!(format!("{}", NvClass::HOPPER_COMPUTE_A).contains("Hopper Compute A"));
    assert!(format!("{}", NvClass::BLACKWELL_COMPUTE).contains("Blackwell Compute"));
    assert!(format!("{}", NvClass::FERMI_DMA_COPY).contains("Fermi DMA Copy"));
    assert!(format!("{}", NvClass(0xDEAD)).contains("Unknown"));
}

#[test]
#[expect(
    clippy::assertions_on_constants,
    reason = "compile-time validation of header constants"
)]
fn qmdv00_06_field_ranges_within_max_bit() {
    use classes::cla0c0::qmd;
    assert!(qmd::QMDV00_06_CTA_RASTER_WIDTH.end <= qmd::QMDV00_06_MAX_BIT + 1);
    assert!(qmd::QMDV00_06_CTA_RASTER_HEIGHT.end <= qmd::QMDV00_06_MAX_BIT + 1);
    assert!(qmd::QMDV00_06_CTA_THREAD_DIMENSION0.end <= qmd::QMDV00_06_MAX_BIT + 1);
    assert!(qmd::QMDV00_06_REGISTER_COUNT.end <= qmd::QMDV00_06_MAX_BIT + 1);
}

#[test]
fn qmdv00_06_critical_fields_non_overlapping() {
    use classes::cla0c0::qmd;
    fn disjoint(a: std::ops::Range<usize>, b: std::ops::Range<usize>) -> bool {
        a.end <= b.start || b.end <= a.start
    }
    assert!(disjoint(
        qmd::QMDV00_06_QMD_MAJOR_VERSION,
        qmd::QMDV00_06_QMD_VERSION
    ));
    assert!(disjoint(
        qmd::QMDV00_06_CTA_RASTER_WIDTH,
        qmd::QMDV00_06_CTA_RASTER_HEIGHT
    ));
    assert!(disjoint(
        qmd::QMDV00_06_CTA_THREAD_DIMENSION0,
        qmd::QMDV00_06_CTA_THREAD_DIMENSION1
    ));
}

#[test]
fn qmdv00_06_constant_buffer_accessors() {
    use classes::cla0c0::qmd;
    let r0 = qmd::QMDV00_06_CONSTANT_BUFFER_ADDR_LOWER(0);
    assert_eq!(r0, 1536..1568);
    let r1 = qmd::QMDV00_06_CONSTANT_BUFFER_ADDR_UPPER(0);
    assert_eq!(r1, 1568..1576);
    let r2 = qmd::QMDV00_06_CONSTANT_BUFFER_SIZE(1);
    assert_eq!(r2, 1640..1657);
    let r3 = qmd::QMDV00_06_CONSTANT_BUFFER_VALID(2);
    assert_eq!(r3, 1721..1722);
}

#[test]
fn qmdv02_01_constant_buffer_accessors() {
    use classes::clc0c0::qmd;
    let r0 = qmd::QMDV02_01_CONSTANT_BUFFER_ADDR_LOWER(0);
    assert_eq!(r0, 1536..1568);
    let r1 = qmd::QMDV02_01_CONSTANT_BUFFER_SIZE_SHIFTED4(1);
    assert_eq!(r1, 1640..1657);
    let r2 = qmd::QMDV02_01_CONSTANT_BUFFER_VALID(2);
    assert_eq!(r2, 1721..1722);
}

#[test]
fn qmdv02_02_constant_buffer_accessors() {
    use classes::clc3c0::qmd;
    let r0 = qmd::QMDV02_02_CONSTANT_BUFFER_ADDR_LOWER(0);
    assert_eq!(r0, 1536..1568);
    let r1 = qmd::QMDV02_02_CONSTANT_BUFFER_SIZE_SHIFTED4(1);
    assert_eq!(r1, 1640..1657);
}

#[test]
fn qmdv03_00_constant_buffer_accessors() {
    use classes::clc6c0::qmd;
    let r0 = qmd::QMDV03_00_CONSTANT_BUFFER_ADDR_LOWER(0);
    assert_eq!(r0, 1536..1568);
    let r1 = qmd::QMDV03_00_CONSTANT_BUFFER_VALID(3);
    assert_eq!(r1, 1785..1786);
}

#[test]
fn qmdv04_00_version_and_constant_buffer_accessors() {
    use classes::clcbc0::qmd;
    assert_eq!(qmd::QMDV04_00_MAX_BIT, 3071);
    let r0 = qmd::QMDV04_00_CONSTANT_BUFFER_ADDR_LOWER_SHIFTED6(0);
    assert_eq!(r0, 2048..2074);
    let r1 = qmd::QMDV04_00_CONSTANT_BUFFER_ADDR_UPPER_SHIFTED6(0);
    assert_eq!(r1, 2074..2091);
    let r2 = qmd::QMDV04_00_CONSTANT_BUFFER_SIZE_SHIFTED4(1);
    assert_eq!(r2, 2155..2172);
    let r3 = qmd::QMDV04_00_CONSTANT_BUFFER_VALID(2);
    assert_eq!(r3, 2236..2237);
}

#[test]
fn qmdv05_00_version_and_constant_buffer_accessors() {
    use classes::clcdc0::qmd;
    assert_eq!(qmd::QMDV05_00_MAX_BIT, 3071);
    let r0 = qmd::QMDV05_00_CONSTANT_BUFFER_ADDR_LOWER_SHIFTED6(0);
    assert_eq!(r0, 2048..2074);
    let r1 = qmd::QMDV05_00_CONSTANT_BUFFER_VALID(1);
    assert_eq!(r1, 2172..2173);
}

#[test]
fn sph_field_constants_defined() {
    use classes::cla097::sph;
    assert_eq!(sph::SPH_TYPE_COMPUTE, 1);
    assert_eq!(sph::SPH_TYPE_VERTEX, 2);
    assert_eq!(sph::SPH_TYPE_FRAGMENT, 5);
    assert_eq!(sph::SPH_VERSION_SM70, 3);
    assert_eq!(sph::NUM_GPRS_OFFSET, 64);
    assert_eq!(sph::NUM_GPRS_WIDTH, 8);
    assert_eq!(sph::NUM_BARRIERS_OFFSET, 144);
    assert_eq!(sph::NUM_BARRIERS_WIDTH, 5);
    assert_eq!(sph::SHARED_MEM_OFFSET, 149);
    assert_eq!(sph::SHARED_MEM_WIDTH, 11);
}

#[test]
fn kepler_qmd_shared_mem_and_l1_fields() {
    use classes::cla0c0::qmd;
    const {
        assert!(qmd::QMDV00_06_SHARED_MEMORY_SIZE.end <= qmd::QMDV00_06_MAX_BIT + 1);
        assert!(qmd::QMDV00_06_L1_CONFIGURATION.end <= qmd::QMDV00_06_MAX_BIT + 1);
    };
    assert_eq!(
        qmd::QMDV00_06_L1_CONFIGURATION_DIRECTLY_ADDRESSABLE_MEMORY_SIZE_48KB,
        3
    );
}

#[test]
fn pascal_qmd_sm_global_caching_and_program_offset() {
    use classes::clc0c0::qmd;
    const {
        assert!(qmd::QMDV02_01_SM_GLOBAL_CACHING_ENABLE.end <= qmd::QMDV02_01_MAX_BIT + 1);
        assert!(qmd::QMDV02_01_PROGRAM_OFFSET.end <= qmd::QMDV02_01_MAX_BIT + 1);
    };
}

#[test]
fn blackwell_qmd_type_and_program_address_fields() {
    use classes::clcdc0::qmd;
    const {
        assert!(qmd::QMDV05_00_QMD_TYPE.end <= qmd::QMDV05_00_MAX_BIT + 1);
        assert!(qmd::QMDV05_00_PROGRAM_ADDRESS_LOWER_SHIFTED4.end <= qmd::QMDV05_00_MAX_BIT + 1);
        assert!(qmd::QMDV05_00_PROGRAM_ADDRESS_UPPER_SHIFTED4.end <= qmd::QMDV05_00_MAX_BIT + 1);
    };
}

#[test]
fn hopper_qmd_grid_fields_cover_header_region() {
    use classes::clcbc0::qmd;
    const {
        assert!(qmd::QMDV04_00_GRID_WIDTH.end <= 64);
        assert!(qmd::QMDV04_00_GRID_HEIGHT.start >= 32);
        assert!(qmd::QMDV04_00_QMD_MAJOR_VERSION.end <= qmd::QMDV04_00_MAX_BIT + 1);
    };
}

#[test]
fn qmd_version_fields_defined() {
    use classes::cla0c0::qmd as q06;
    use classes::clc0c0::qmd as q21;
    use classes::clc3c0::qmd as q22;
    use classes::clc6c0::qmd as q30;
    use classes::clcbc0::qmd as q40;
    use classes::clcdc0::qmd as q50;
    assert_eq!(q06::QMDV00_06_QMD_MAJOR_VERSION, 0..4);
    assert_eq!(q06::QMDV00_06_QMD_VERSION, 4..8);
    assert_eq!(q21::QMDV02_01_QMD_MAJOR_VERSION, 0..4);
    assert_eq!(q22::QMDV02_02_QMD_VERSION, 4..8);
    assert_eq!(q30::QMDV03_00_QMD_VERSION, 4..8);
    assert_eq!(q40::QMDV04_00_QMD_MAJOR_VERSION, 68..72);
    assert_eq!(q40::QMDV04_00_QMD_VERSION, 64..68);
    assert_eq!(q50::QMDV05_00_QMD_MAJOR_VERSION, 68..72);
    assert_eq!(q50::QMDV05_00_QMD_VERSION, 64..68);
}

/// Maps each named GPU generation used in tooling to its primary compute [`NvClass`].
/// Turing and Ada do not introduce distinct class constants in this stub; Ada aligns with Ampere.
#[test]
fn nv_class_chip_generations_all_variants() {
    let cases: &[(&str, NvClass, u32)] = &[
        ("kepler_sph", NvClass::KEPLER_SPH, 0xA097),
        ("kepler_compute", NvClass::KEPLER_COMPUTE_A, 0xA0C0),
        ("maxwell_compute_a", NvClass::MAXWELL_COMPUTE_A, 0xB0C0),
        ("maxwell_compute_b", NvClass::MAXWELL_COMPUTE_B, 0xB1C0),
        ("pascal", NvClass::PASCAL_COMPUTE, 0xC0C0),
        ("volta", NvClass::VOLTA_COMPUTE_A, 0xC3C0),
        (
            "turing_same_class_as_pascal_stub",
            NvClass::PASCAL_COMPUTE,
            0xC0C0,
        ),
        ("ampere", NvClass::AMPERE_COMPUTE_A, 0xC6C0),
        (
            "ada_same_class_as_ampere_stub",
            NvClass::AMPERE_COMPUTE_A,
            0xC6C0,
        ),
        ("hopper", NvClass::HOPPER_COMPUTE_A, 0xCBC0),
        ("blackwell", NvClass::BLACKWELL_COMPUTE, 0xCDC0),
        ("fermi_dma", NvClass::FERMI_DMA_COPY, 0x90B5),
    ];
    for (label, class, raw) in cases {
        assert_eq!(class.0, *raw, "{label}");
        let s = format!("{class}");
        assert!(
            s.contains(&format!("{:04X}", raw & 0xFFFF)),
            "{label} display should contain class hex"
        );
        let v: u32 = (*class).into();
        assert_eq!(v, *raw, "{label} Into<u32>");
    }
}

#[test]
fn qmd_indexed_lookups_cover_multiple_indices() {
    use classes::cla0c0::qmd as q06;
    use classes::clc0c0::qmd as q21;
    use classes::clc3c0::qmd as q22;
    use classes::clc6c0::qmd as q30;
    use classes::clcbc0::qmd as q40;
    use classes::clcdc0::qmd as q50;
    for idx in [0usize, 1, 4, 7] {
        let base = 1536 + idx * 64;
        assert_eq!(
            q06::QMDV00_06_CONSTANT_BUFFER_ADDR_LOWER(idx),
            base..base + 32
        );
        assert_eq!(
            q06::QMDV00_06_CONSTANT_BUFFER_ADDR_UPPER(idx),
            base + 32..base + 40
        );
        assert_eq!(
            q06::QMDV00_06_CONSTANT_BUFFER_SIZE(idx),
            base + 40..base + 57
        );
        assert_eq!(
            q06::QMDV00_06_CONSTANT_BUFFER_VALID(idx),
            base + 57..base + 58
        );

        assert_eq!(
            q21::QMDV02_01_CONSTANT_BUFFER_ADDR_LOWER(idx),
            base..base + 32
        );
        assert_eq!(
            q21::QMDV02_01_CONSTANT_BUFFER_SIZE_SHIFTED4(idx),
            base + 40..base + 57
        );
        assert_eq!(
            q21::QMDV02_01_CONSTANT_BUFFER_VALID(idx),
            base + 57..base + 58
        );

        assert_eq!(
            q22::QMDV02_02_CONSTANT_BUFFER_ADDR_LOWER(idx),
            base..base + 32
        );
        assert_eq!(
            q22::QMDV02_02_CONSTANT_BUFFER_SIZE_SHIFTED4(idx),
            base + 40..base + 57
        );
        assert_eq!(
            q22::QMDV02_02_CONSTANT_BUFFER_VALID(idx),
            base + 57..base + 58
        );

        assert_eq!(
            q30::QMDV03_00_CONSTANT_BUFFER_ADDR_LOWER(idx),
            base..base + 32
        );
        assert_eq!(
            q30::QMDV03_00_CONSTANT_BUFFER_SIZE_SHIFTED4(idx),
            base + 40..base + 57
        );
        assert_eq!(
            q30::QMDV03_00_CONSTANT_BUFFER_VALID(idx),
            base + 57..base + 58
        );

        let base_h = 2048 + idx * 64;
        assert_eq!(
            q40::QMDV04_00_CONSTANT_BUFFER_ADDR_LOWER_SHIFTED6(idx),
            base_h..base_h + 26
        );
        assert_eq!(
            q40::QMDV04_00_CONSTANT_BUFFER_ADDR_UPPER_SHIFTED6(idx),
            base_h + 26..base_h + 43
        );
        assert_eq!(
            q40::QMDV04_00_CONSTANT_BUFFER_SIZE_SHIFTED4(idx),
            base_h + 43..base_h + 60
        );
        assert_eq!(
            q40::QMDV04_00_CONSTANT_BUFFER_VALID(idx),
            base_h + 60..base_h + 61
        );

        assert_eq!(
            q50::QMDV05_00_CONSTANT_BUFFER_ADDR_LOWER_SHIFTED6(idx),
            base_h..base_h + 26
        );
        assert_eq!(
            q50::QMDV05_00_CONSTANT_BUFFER_ADDR_UPPER_SHIFTED6(idx),
            base_h + 26..base_h + 43
        );
        assert_eq!(
            q50::QMDV05_00_CONSTANT_BUFFER_SIZE_SHIFTED4(idx),
            base_h + 43..base_h + 60
        );
        assert_eq!(
            q50::QMDV05_00_CONSTANT_BUFFER_VALID(idx),
            base_h + 60..base_h + 61
        );
    }
}

#[test]
fn qmd_volta_ampere_extended_fields_within_max_bit() {
    use classes::clc3c0::qmd as q22;
    use classes::clc6c0::qmd as q30;
    let max22 = q22::QMDV02_02_MAX_BIT + 1;
    let max30 = q30::QMDV03_00_MAX_BIT + 1;
    assert!(q22::QMDV02_02_PROGRAM_ADDRESS_UPPER.end <= max22);
    assert!(q22::QMDV02_02_MIN_SM_CONFIG_SHARED_MEM_SIZE.end <= max22);
    assert!(q30::QMDV03_00_PROGRAM_ADDRESS_UPPER.end <= max30);
    assert!(q30::QMDV03_00_TARGET_SM_CONFIG_SHARED_MEM_SIZE.end <= max30);
}
