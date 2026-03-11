// SPDX-License-Identifier: AGPL-3.0-only
//! SPIR-V roundtrip tests — WGSL → naga → SPIR-V → `compile()`.
//!
//! Verifies the SPIR-V frontend path by converting existing WGSL fixtures
//! to SPIR-V via naga, then feeding the result through coralReef's
//! `compile()` (SPIR-V entry point).  No binary fixture files needed.
//!
//! The curated subset covers: f32 ALU, f64, control flow, shared memory,
//! atomics, transcendentals, and struct buffer patterns.

use coral_reef::{CompileOptions, GpuArch, compile};
use std::time::Instant;

fn sm70_f64_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn wgsl_to_spirv(wgsl: &str) -> Vec<u32> {
    let module = naga::front::wgsl::parse_str(wgsl).expect("WGSL should parse");
    let caps = naga::valid::Capabilities::FLOAT64;
    let mut validator = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps);
    let info = validator.validate(&module).expect("module should validate");
    naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
        .expect("SPIR-V emission should succeed")
}

macro_rules! spirv_roundtrip_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let wgsl = include_str!($file);
            let spirv = wgsl_to_spirv(wgsl);
            assert!(
                spirv.len() > 4,
                "SPIR-V should be non-trivial, got {} words",
                spirv.len()
            );
            let start = Instant::now();
            let r = compile(&spirv, &sm70_f64_opts());
            let elapsed = start.elapsed();
            assert!(
                r.is_ok(),
                "{} SPIR-V roundtrip failed ({elapsed:?}): {:?}",
                stringify!($name),
                r.err()
            );
            let bin = r.unwrap();
            assert!(
                !bin.is_empty(),
                "{} produced empty binary",
                stringify!($name)
            );
            eprintln!(
                "  {} — {} SPIR-V words → {} bytes, {elapsed:?}",
                stringify!($name),
                spirv.len(),
                bin.len()
            );
        }
    };
    ($name:ident, $file:expr, ignore = $reason:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            let wgsl = include_str!($file);
            let spirv = wgsl_to_spirv(wgsl);
            let r = compile(&spirv, &sm70_f64_opts());
            assert!(
                r.is_ok(),
                "{} SPIR-V roundtrip failed: {:?}",
                stringify!($name),
                r.err()
            );
        }
    };
}

// ===========================================================================
// Compiler-owned WGSL fixtures — SPIR-V roundtrip
// ===========================================================================

// f32 ALU: float add, mul, FMA
spirv_roundtrip_test!(
    spirv_rt_alu_float_fma,
    "fixtures/wgsl/sm70_alu_float_fma.wgsl"
);

// Integer ALU: signed integer ops
spirv_roundtrip_test!(
    spirv_rt_alu_int_signed,
    "fixtures/wgsl/sm70_alu_int_signed.wgsl"
);

// Control flow: branches, loops, barrier
spirv_roundtrip_test!(
    spirv_rt_control_branches,
    "fixtures/wgsl/sm70_control_branches_loops_barrier.wgsl"
);

// Data types: vec2/vec3/vec4 operations
spirv_roundtrip_test!(
    spirv_rt_data_vectors,
    "fixtures/wgsl/data_vec2_vec3_vec4.wgsl"
);

// Shared memory + storage types
spirv_roundtrip_test!(
    spirv_rt_memory_shared,
    "fixtures/wgsl/memory_shared_storage_types.wgsl"
);

// ===========================================================================
// Corpus fixtures — SPIR-V roundtrip (f32 and f64)
// ===========================================================================

// f64 lattice QCD: SU(3) gauge force — heavy f64 staple arithmetic
spirv_roundtrip_test!(
    spirv_rt_su3_gauge_force,
    "fixtures/wgsl/corpus/su3_gauge_force_f64.wgsl"
);

// f64 molecular dynamics: velocity-Verlet half kick
spirv_roundtrip_test!(
    spirv_rt_vv_half_kick,
    "fixtures/wgsl/corpus/vv_half_kick_f64.wgsl"
);

// f32 ODE solver: RK4 parallel with complex control flow
spirv_roundtrip_test!(
    spirv_rt_rk4_parallel,
    "fixtures/wgsl/corpus/rk4_parallel.wgsl"
);

// f32 HMM Viterbi: argmax, logsumexp, exp/log codegen
spirv_roundtrip_test!(
    spirv_rt_hmm_viterbi,
    "fixtures/wgsl/corpus/hmm_viterbi.wgsl"
);

// f64 Anderson localization: xoshiro PRNG + transfer matrix
spirv_roundtrip_test!(
    spirv_rt_anderson_lyapunov,
    "fixtures/wgsl/corpus/anderson_lyapunov_f64.wgsl"
);
