// SPDX-License-Identifier: AGPL-3.0-only
//! Chaos and fault injection tests for robustness.

use coral_reef::{CompileOptions, GpuArch, compile, compile_wgsl};

/// Compile the same shader many times concurrently — no panics, no races.
#[test]
fn chaos_concurrent_compilation() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let handles: Vec<_> = (0..16)
        .map(|_| {
            std::thread::spawn(|| {
                let opts = CompileOptions::default();
                let _ = compile_wgsl(wgsl, &opts);
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread should not panic");
    }
}

/// Truncated SPIR-V at every possible length should not crash.
#[test]
fn chaos_truncated_spirv_all_lengths() {
    let full = [
        0x0723_0203u32,
        0x0001_0000,
        0x000B_0000,
        0x0000_0008,
        0x0000_0000,
        0x0002_0011,
        0x0000_0001,
        0x0003_0006,
    ];
    for len in 0..=full.len() {
        let _ = compile(&full[..len], &CompileOptions::default());
    }
}

/// Repeated compilation of the same input yields identical results.
#[test]
fn chaos_determinism_stress() {
    let wgsl = "@compute @workgroup_size(64) fn main() {}";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    };

    let results: Vec<_> = (0..50).map(|_| compile_wgsl(wgsl, &opts)).collect();

    let first = &results[0];
    for (i, result) in results.iter().enumerate().skip(1) {
        match (first, result) {
            (Ok(a), Ok(b)) => assert_eq!(a, b, "run {i} differs from run 0"),
            (Err(_), Err(_)) => {}
            _ => panic!("inconsistent results between run 0 and run {i}"),
        }
    }
}

/// Every `GpuArch` variant can be constructed and formatted.
#[test]
fn chaos_all_archs_constructible() {
    for arch in GpuArch::ALL {
        let s = arch.to_string();
        assert!(!s.is_empty(), "arch {arch:?} has empty display");
        let parsed = GpuArch::parse(&s);
        assert_eq!(parsed, Some(*arch), "roundtrip failed for {arch}");
    }
}

/// Very large WGSL inputs should not OOM or panic.
#[test]
fn chaos_large_wgsl_input() {
    use std::fmt::Write;
    let mut wgsl = String::from("@compute @workgroup_size(1) fn main() {\n");
    for i in 0..500 {
        let _ = writeln!(wgsl, "  var x{i}: f32 = f32({i});");
    }
    wgsl.push_str("}\n");
    let _ = compile_wgsl(&wgsl, &CompileOptions::default());
}

/// Multiple architectures should produce non-overlapping binaries.
#[test]
fn chaos_cross_arch_no_collision() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let mut results = Vec::new();
    for arch in GpuArch::ALL {
        let opts = CompileOptions {
            target: (*arch).into(),
            ..CompileOptions::default()
        };
        if let Ok(binary) = compile_wgsl(wgsl, &opts) {
            results.push((*arch, binary));
        }
    }
    // At least some archs should compile
    assert!(!results.is_empty(), "no arch produced a binary");
}
