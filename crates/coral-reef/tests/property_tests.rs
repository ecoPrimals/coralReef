// SPDX-License-Identifier: AGPL-3.0-only
#![cfg(feature = "naga")]
//! Property-based tests for the coral-reef compiler.

use coral_reef::{CompileOptions, GpuArch, compile, compile_wgsl};
use proptest::prelude::*;

fn arb_gpu_arch() -> impl Strategy<Value = GpuArch> {
    prop_oneof![
        Just(GpuArch::Sm70),
        Just(GpuArch::Sm75),
        Just(GpuArch::Sm80),
        Just(GpuArch::Sm86),
        Just(GpuArch::Sm89),
    ]
}

fn arb_opt_level() -> impl Strategy<Value = u32> {
    0..=3u32
}

proptest! {
    /// Any random bytes interpreted as SPIR-V should not crash the compiler.
    #[test]
    fn fuzz_compile_never_panics(
        data in proptest::collection::vec(any::<u32>(), 0..256),
        arch in arb_gpu_arch(),
        opt in arb_opt_level(),
    ) {
        let opts = CompileOptions {
            target: arch.into(),
            opt_level: opt,
            debug_info: false,
            fp64_software: true,
            ..CompileOptions::default()
        };
        let _ = compile(&data, &opts);
    }

    /// Random WGSL strings should not crash the compiler.
    #[test]
    fn fuzz_compile_wgsl_never_panics(
        wgsl in proptest::string::string_regex("[@a-zA-Z0-9_ (){};\n]{1,200}").unwrap(),
        arch in arb_gpu_arch(),
    ) {
        let opts = CompileOptions {
            target: arch.into(),
            ..CompileOptions::default()
        };
        let _ = compile_wgsl(&wgsl, &opts);
    }

    /// Valid SPIR-V always produces the same binary for the same inputs.
    #[test]
    fn compile_is_deterministic(arch in arb_gpu_arch(), opt in arb_opt_level()) {
        let wgsl = "@compute @workgroup_size(1) fn main() {}";
        let opts = CompileOptions {
            target: arch.into(),
            opt_level: opt,
            debug_info: false,
            fp64_software: true,
            ..CompileOptions::default()
        };
        let r1 = compile_wgsl(wgsl, &opts);
        let r2 = compile_wgsl(wgsl, &opts);
        match (r1, r2) {
            (Ok(b1), Ok(b2)) => prop_assert_eq!(b1, b2),
            (Err(_), Err(_)) => {} // both fail is fine
            _ => prop_assert!(false, "inconsistent results"),
        }
    }

    /// CompileOptions default values are stable.
    #[test]
    fn default_options_are_stable(_seed in 0..100u32) {
        let a = CompileOptions::default();
        let b = CompileOptions::default();
        prop_assert_eq!(a.opt_level, b.opt_level);
        prop_assert_eq!(a.fp64_software, b.fp64_software);
        prop_assert_eq!(a.debug_info, b.debug_info);
    }

    /// GpuArch display/parse roundtrip.
    #[test]
    fn gpu_arch_display_roundtrip(arch in arb_gpu_arch()) {
        let s = arch.to_string();
        let parsed = GpuArch::parse(&s);
        prop_assert_eq!(parsed, Some(arch));
    }
}
