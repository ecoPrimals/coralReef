// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn test_sm_numbers() {
    assert_eq!(NvArch::Sm35.sm(), 35);
    assert_eq!(NvArch::Sm70.sm(), 70);
    assert_eq!(NvArch::Sm75.sm(), 75);
    assert_eq!(NvArch::Sm80.sm(), 80);
    assert_eq!(NvArch::Sm86.sm(), 86);
    assert_eq!(NvArch::Sm89.sm(), 89);
    assert_eq!(NvArch::Sm120.sm(), 120);
}

#[test]
fn test_gpu_target_vendor() {
    let nv = GpuTarget::Nvidia(NvArch::Sm70);
    assert_eq!(nv.vendor(), "nvidia");
    let amd = GpuTarget::Amd(AmdArch::Rdna2);
    assert_eq!(amd.vendor(), "amd");
    let intel = GpuTarget::Intel(IntelArch::XeHpg);
    assert_eq!(intel.vendor(), "intel");
}

#[test]
fn test_gpu_target_default_is_nvidia() {
    let t = GpuTarget::default();
    assert!(t.as_nvidia().is_some());
    assert_eq!(t.as_nvidia(), Some(NvArch::Sm70));
}

#[test]
fn test_gpu_target_from_nv_arch() {
    let t: GpuTarget = NvArch::Sm89.into();
    assert_eq!(t, GpuTarget::Nvidia(NvArch::Sm89));
}

#[test]
fn test_gpu_target_display() {
    assert_eq!(GpuTarget::Nvidia(NvArch::Sm70).to_string(), "sm_70");
    assert_eq!(GpuTarget::Amd(AmdArch::Rdna2).to_string(), "rdna2");
    assert_eq!(GpuTarget::Amd(AmdArch::Rdna3).to_string(), "rdna3");
    assert_eq!(GpuTarget::Intel(IntelArch::XeHpg).to_string(), "xe_hpg");
}

#[test]
fn test_gpu_arch_alias_works() {
    let a: GpuArch = GpuArch::Sm70;
    assert_eq!(a.sm(), 70);
}

#[test]
fn test_nv_arch_parse() {
    assert_eq!(NvArch::parse("sm_35"), Some(NvArch::Sm35));
    assert_eq!(NvArch::parse("sm_70"), Some(NvArch::Sm70));
    assert_eq!(NvArch::parse("sm89"), Some(NvArch::Sm89));
    assert_eq!(NvArch::parse("sm_120"), Some(NvArch::Sm120));
    assert_eq!(NvArch::parse("sm120"), Some(NvArch::Sm120));
    assert_eq!(NvArch::parse("rdna3"), None);
}

#[test]
fn test_nv_arch_roundtrip() {
    for &arch in NvArch::ALL {
        let s = arch.to_string();
        assert_eq!(NvArch::parse(&s), Some(arch));
    }
}

#[test]
fn test_fast_fp64() {
    assert!(NvArch::Sm35.has_fast_fp64());
    assert!(NvArch::Sm70.has_fast_fp64());
    assert!(!NvArch::Sm75.has_fast_fp64());
    assert!(NvArch::Sm80.has_fast_fp64());
    assert!(!NvArch::Sm86.has_fast_fp64());
    assert!(!NvArch::Sm89.has_fast_fp64());
    assert!(!NvArch::Sm120.has_fast_fp64());
}

#[test]
fn test_shared_mem() {
    assert_eq!(NvArch::Sm35.max_shared_mem(), 49_152);
    assert_eq!(NvArch::Sm70.max_shared_mem(), 49_152);
    assert_eq!(NvArch::Sm80.max_shared_mem(), 102_400);
    assert_eq!(NvArch::Sm120.max_shared_mem(), 102_400);
}

#[test]
fn test_unwrap_helpers() {
    let nv = GpuTarget::Nvidia(NvArch::Sm80);
    assert!(nv.as_nvidia().is_some());
    assert!(nv.as_amd().is_none());
    assert!(nv.as_intel().is_none());

    let amd = GpuTarget::Amd(AmdArch::Rdna4);
    assert!(amd.as_nvidia().is_none());
    assert!(amd.as_amd().is_some());
}

#[test]
fn test_amd_arch_parse() {
    assert_eq!(AmdArch::parse("gcn5"), Some(AmdArch::Gcn5));
    assert_eq!(AmdArch::parse("vega"), Some(AmdArch::Gcn5));
    assert_eq!(AmdArch::parse("gfx906"), Some(AmdArch::Gcn5));
    assert_eq!(AmdArch::parse("rdna2"), Some(AmdArch::Rdna2));
    assert_eq!(AmdArch::parse("gfx1030"), Some(AmdArch::Rdna2));
    assert_eq!(AmdArch::parse("rdna3"), Some(AmdArch::Rdna3));
    assert_eq!(AmdArch::parse("gfx1100"), Some(AmdArch::Rdna3));
    assert_eq!(AmdArch::parse("rdna4"), Some(AmdArch::Rdna4));
    assert_eq!(AmdArch::parse("sm_70"), None);
}

#[test]
fn test_amd_arch_roundtrip() {
    for &arch in AmdArch::ALL {
        let s = arch.to_string();
        assert_eq!(AmdArch::parse(&s), Some(arch));
    }
}

#[test]
fn test_amd_arch_properties() {
    assert_eq!(AmdArch::Gcn5.gfx_major(), 9);
    assert_eq!(AmdArch::Rdna2.gfx_major(), 10);
    assert_eq!(AmdArch::Rdna3.gfx_major(), 11);
    assert_eq!(AmdArch::Rdna4.gfx_major(), 12);
    assert!(AmdArch::Rdna2.has_native_f64());
    assert!(AmdArch::Gcn5.has_native_f64());
    assert_eq!(AmdArch::Gcn5.f64_rate_divisor(), 4);
    assert_eq!(AmdArch::Rdna2.f64_rate_divisor(), 16);
    assert_eq!(AmdArch::Gcn5.max_vgprs(), 256);
    assert_eq!(AmdArch::Rdna2.max_vgprs(), 256);
    assert_eq!(AmdArch::Gcn5.max_sgprs(), 102);
    assert_eq!(AmdArch::Rdna2.max_sgprs(), 106);
    assert_eq!(AmdArch::Gcn5.default_wave_size(), 64);
    assert_eq!(AmdArch::Rdna2.default_wave_size(), 32);
    assert!(AmdArch::Gcn5.supports_wave64());
    assert!(AmdArch::Rdna2.supports_wave64());
    assert_eq!(AmdArch::Gcn5.max_lds(), 65_536);
    assert!(!AmdArch::Gcn5.has_flat_offset());
    assert!(AmdArch::Rdna2.has_flat_offset());
}

#[test]
fn test_nv_arch_hw_properties() {
    for &arch in NvArch::ALL {
        assert!(arch.has_dfma(), "{arch} should support DFMA");
    }

    assert_eq!(NvArch::Sm70.warp_size(), 32);
    assert_eq!(NvArch::Sm89.warp_size(), 32);

    assert_eq!(NvArch::Sm70.sm_version(), 70);
    assert_eq!(NvArch::Sm89.sm_version(), 89);

    assert!(NvArch::Sm70.max_reg_count() > 0);
    assert!(NvArch::Sm89.max_reg_count() >= NvArch::Sm70.max_reg_count());

    assert!(NvArch::Sm70.max_warps_per_sm() > 0);
    assert!(NvArch::Sm70.total_reg_file() > 0);
}

#[test]
fn test_nv_arch_fromstr_error() {
    let result: Result<NvArch, _> = "not_a_gpu".parse();
    assert!(result.is_err());
    let err_msg = result.unwrap_err();
    assert!(err_msg.contains("unknown"));
}

#[test]
fn test_amd_arch_fromstr_error() {
    let result: Result<AmdArch, _> = "not_a_gpu".parse();
    assert!(result.is_err());
    let err_msg = result.unwrap_err();
    assert!(err_msg.contains("unknown"));
}

#[test]
fn test_gpu_target_from_amd() {
    let t: GpuTarget = AmdArch::Rdna3.into();
    assert_eq!(t, GpuTarget::Amd(AmdArch::Rdna3));
    assert!(t.as_amd().is_some());
}

#[test]
fn test_gpu_target_from_intel() {
    let t: GpuTarget = IntelArch::XeHpg.into();
    assert_eq!(t, GpuTarget::Intel(IntelArch::XeHpg));
    assert!(t.as_intel().is_some());
}

#[test]
fn test_intel_display() {
    assert_eq!(IntelArch::XeHpg.to_string(), "xe_hpg");
    assert_eq!(IntelArch::Dg2Alchemist.to_string(), "dg2_alchemist");
    assert_eq!(IntelArch::Xe2Hpg.to_string(), "xe2_hpg");
    assert_eq!(IntelArch::XeLpg.to_string(), "xe_lpg");
}

#[test]
fn test_intel_arch_all() {
    assert_eq!(IntelArch::ALL.len(), 4);
    assert!(IntelArch::ALL.contains(&IntelArch::Dg2Alchemist));
    assert!(IntelArch::ALL.contains(&IntelArch::XeLpg));
}

#[test]
fn test_nv_arch_parse_both_formats() {
    assert_eq!(NvArch::parse("sm_70"), Some(NvArch::Sm70));
    assert_eq!(NvArch::parse("sm70"), Some(NvArch::Sm70));
}

#[test]
fn test_amd_arch_parse_case_insensitive() {
    assert_eq!(AmdArch::parse("RDNA2"), Some(AmdArch::Rdna2));
    assert_eq!(AmdArch::parse("gfx1031"), Some(AmdArch::Rdna2));
}

#[test]
fn test_nv_arch_has_transcendental_64h() {
    assert!(
        !NvArch::Sm35.has_transcendental_64h(),
        "Kepler: no 64-bit MUFU"
    );
    assert!(
        !NvArch::Sm120.has_transcendental_64h(),
        "Blackwell: RCP64H/RSQ64H broken"
    );
    for &arch in &[
        NvArch::Sm70,
        NvArch::Sm75,
        NvArch::Sm80,
        NvArch::Sm86,
        NvArch::Sm89,
    ] {
        assert!(
            arch.has_transcendental_64h(),
            "{arch} should have 64h seeds"
        );
    }
}

#[test]
fn test_nv_arch_max_warps_per_sm_variants() {
    assert_eq!(NvArch::Sm70.max_warps_per_sm(), 64);
    assert_eq!(NvArch::Sm75.max_warps_per_sm(), 32);
    assert_eq!(NvArch::Sm89.max_warps_per_sm(), 48);
}

#[test]
fn test_gpu_target_vendor_display() {
    let nv = GpuTarget::Nvidia(NvArch::Sm70);
    assert_eq!(format!("{nv}"), "sm_70");
    let amd = GpuTarget::Amd(AmdArch::Rdna3);
    assert_eq!(format!("{amd}"), "rdna3");
}

#[test]
fn test_amd_arch_gfx_variants() {
    assert_eq!(AmdArch::parse("gfx1032"), Some(AmdArch::Rdna2));
    assert_eq!(AmdArch::parse("gfx1101"), Some(AmdArch::Rdna3));
    assert_eq!(AmdArch::parse("gfx1102"), Some(AmdArch::Rdna3));
    assert_eq!(AmdArch::parse("gfx1200"), Some(AmdArch::Rdna4));
}

#[test]
fn test_nv_arch_fromstr_valid() {
    let arch: NvArch = "sm_70".parse().unwrap();
    assert_eq!(arch, NvArch::Sm70);
}

#[test]
fn test_amd_arch_fromstr_valid() {
    let arch: AmdArch = "rdna2".parse().unwrap();
    assert_eq!(arch, AmdArch::Rdna2);
}

// ---------------------------------------------------------------------------
// CompileTarget tests
// ---------------------------------------------------------------------------

#[test]
fn test_compile_target_gpu() {
    let ct = CompileTarget::from(GpuTarget::Nvidia(NvArch::Sm70));
    assert!(ct.is_gpu());
    assert!(!ct.is_cpu());
    assert!(!ct.is_npu());
    assert_eq!(ct.execution_model(), "simt");
    assert_eq!(ct.target_class(), "gpu");
}

#[test]
fn test_compile_target_cpu() {
    let ct = CompileTarget::Cpu(CpuArch::X86_64);
    assert!(!ct.is_gpu());
    assert!(ct.is_cpu());
    assert_eq!(ct.execution_model(), "sequential");
    assert_eq!(ct.to_string(), "x86_64");
}

#[test]
fn test_compile_target_npu() {
    let ct = CompileTarget::Npu(NpuTarget::Akida);
    assert!(!ct.is_gpu());
    assert!(ct.is_npu());
    assert_eq!(ct.execution_model(), "dataflow");
    assert_eq!(ct.to_string(), "akida");
}

#[test]
fn test_compile_target_default() {
    let ct = CompileTarget::default();
    assert!(ct.is_gpu());
    assert_eq!(ct.as_gpu(), Some(GpuTarget::default()));
}

#[test]
fn test_compile_target_from_nv() {
    let ct = CompileTarget::from(NvArch::Sm89);
    assert_eq!(ct, CompileTarget::Gpu(GpuTarget::Nvidia(NvArch::Sm89)));
}

#[test]
fn test_compile_target_from_amd() {
    let ct = CompileTarget::from(AmdArch::Rdna3);
    assert_eq!(ct, CompileTarget::Gpu(GpuTarget::Amd(AmdArch::Rdna3)));
}

#[test]
fn test_compile_target_from_cpu() {
    let ct = CompileTarget::from(CpuArch::Aarch64);
    assert_eq!(ct, CompileTarget::Cpu(CpuArch::Aarch64));
}

#[test]
fn test_compile_target_from_npu() {
    let ct = CompileTarget::from(NpuTarget::GenericDataflow);
    assert_eq!(ct, CompileTarget::Npu(NpuTarget::GenericDataflow));
}
