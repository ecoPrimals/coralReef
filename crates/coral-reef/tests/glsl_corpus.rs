// SPDX-License-Identifier: AGPL-3.0-only
#![cfg(feature = "naga")]
//! GLSL compute shader corpus — tests the GLSL frontend path.
//!
//! These are coralReef-owned fixtures (not spring copies) designed to exercise
//! the naga GLSL frontend → IR → codegen pipeline across distinct patterns:
//! ALU, control flow, shared memory, transcendentals, and buffer I/O.
//!
//! Each shader is compiled for SM70 to verify the GLSL path produces valid
//! native binaries identical in quality to the WGSL path.

use coral_reef::{CompileOptions, GpuArch, compile_glsl};
use std::time::Instant;

fn sm70_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        ..CompileOptions::default()
    }
}

macro_rules! glsl_compile_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let src = include_str!(concat!("fixtures/glsl/", $file));
            let start = Instant::now();
            let r = compile_glsl(src, &sm70_opts());
            let elapsed = start.elapsed();
            assert!(
                r.is_ok(),
                "{} failed to compile for SM70 ({elapsed:?}): {:?}",
                $file,
                r.err()
            );
            let bin = r.unwrap();
            assert!(!bin.is_empty(), "{} produced empty binary", $file);
            eprintln!("  {} — {} bytes, {elapsed:?}", $file, bin.len());
        }
    };
    ($name:ident, $file:expr, ignore = $reason:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            let src = include_str!(concat!("fixtures/glsl/", $file));
            let start = Instant::now();
            let r = compile_glsl(src, &sm70_opts());
            let elapsed = start.elapsed();
            assert!(
                r.is_ok(),
                "{} failed to compile for SM70 ({elapsed:?}): {:?}",
                $file,
                r.err()
            );
            let bin = r.unwrap();
            assert!(!bin.is_empty(), "{} produced empty binary", $file);
            eprintln!("  {} — {} bytes, {elapsed:?}", $file, bin.len());
        }
    };
}

// ===========================================================================
// GLSL 450 compute shader corpus
// ===========================================================================

// Integer + float ALU, FMA, type conversions, bitwise ops
glsl_compile_test!(glsl_basic_alu, "basic_alu.comp");

// Loops, branches, barriers, break, shared memory
glsl_compile_test!(glsl_control_flow, "control_flow.comp");

// Parallel workgroup reduction via shared memory
glsl_compile_test!(glsl_shared_reduction, "shared_reduction.comp");

// exp, log, sin, cos, sqrt, pow, abs, min, max, clamp, floor, ceil, fract, sign, mix
glsl_compile_test!(glsl_transcendentals, "transcendentals.comp");

// Multiple SSBO bindings, struct layout, vec3/vec4, early return
glsl_compile_test!(glsl_buffer_rw, "buffer_rw.comp");
