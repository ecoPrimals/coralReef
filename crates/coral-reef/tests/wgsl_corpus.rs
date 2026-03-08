// SPDX-License-Identifier: AGPL-3.0-only
//! WGSL cross-spring corpus — shaders from all ecosystem springs.
//!
//! Each shader is compiled for SM70 (Volta) with f64 software lowering enabled.
//! Provenance tracks which spring originated and evolved each shader — several
//! shaders were first written in one domain then absorbed across springs.
//!
//! ## Provenance
//!
//! | Domain | Spring | Path |
//! |--------|--------|------|
//! | Lattice QCD | hotSpring | barracuda/src/lattice/shaders/ |
//! | Molecular dynamics | hotSpring | barracuda/src/md/shaders/ |
//! | Nuclear physics | hotSpring | barracuda/src/physics/shaders/ |
//! | Condensed matter | groundSpring | metalForge/shaders/ |
//! | ML / attention | neuralSpring | metalForge/shaders/ |
//! | Hydrology / env | airSpring | barracuda/src/shaders/ |
//!
//! ## Cross-spring evolution notes
//!
//! - `gelu_f64`, `layer_norm_f64`, `softmax_f64`, `sdpa_scores_f64` — neuralSpring
//!   coralForge attention primitives, evolved from hotSpring precision patterns (FMA,
//!   Kahan summation). Used by neuralSpring for Evoformer/IPA and by wetSpring for
//!   bio-statistical pipelines (DF64 dispatch).
//!
//! - `kl_divergence_f64` — neuralSpring statistical primitive. Absorbed by wetSpring
//!   for cross-entropy validation and by groundSpring for Anderson model fitness.
//!
//! - `rk4_parallel` — neuralSpring ODE solver (f32). Complex control flow with
//!   loops and temporaries — exercises scheduling and register allocation hard.
//!
//! - `dielectric_mermin_f64`, `bcs_bisection_f64` — hotSpring plasma/nuclear
//!   precision shaders. Demonstrate stable W(z) asymptotic and cancellation-safe
//!   BCS v² formula. Referenced by wetSpring precision gap analysis.
//!
//! - `su3_gauge_force_f64` — hotSpring lattice QCD. SU(3) matrix ops with complex
//!   traceless anti-Hermitian projection. Heavy f64 with staple sums.
//!
//! - `anderson_lyapunov_f64` — groundSpring condensed matter. xoshiro128** PRNG,
//!   transfer matrix multiplication, uniform buffer bindings. Referenced by
//!   neuralSpring for disorder sweep validation.
//!
//! - `local_elementwise_f64` — airSpring hydrology domain ops (SCS-CN, Stewart,
//!   Makkink, Turc, Hamon, Blaney-Criddle). Simple elementwise f64 — good
//!   baseline for compilation correctness.
//!
//! - `stress_virial_f64` — hotSpring MD off-diagonal stress tensor. Used by
//!   wetSpring for mechanical property validation.

use coral_reef::{CompileOptions, GpuArch, compile_wgsl};
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

macro_rules! wgsl_compile_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let src = include_str!(concat!("fixtures/wgsl/", $file));
            let start = Instant::now();
            let r = compile_wgsl(src, &sm70_f64_opts());
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
            let src = include_str!(concat!("fixtures/wgsl/", $file));
            let start = Instant::now();
            let r = compile_wgsl(src, &sm70_f64_opts());
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
// hotSpring — Lattice QCD (precision shaders)
// ===========================================================================

// Lattice: axpy (f64, storage buffers, simple loop)
wgsl_compile_test!(corpus_axpy_f64, "axpy_f64.wgsl");

// Lattice: conjugate gradient kernels (f64, shared memory, barriers)
wgsl_compile_test!(corpus_cg_kernels_f64, "cg_kernels_f64.wgsl");

// Lattice: sum reduction (f64, shared memory, barriers, uniform)
wgsl_compile_test!(corpus_sum_reduce_f64, "sum_reduce_f64.wgsl");

// Lattice: SU(3) gauge force — heavy f64 staple sum + TA projection
wgsl_compile_test!(
    corpus_su3_gauge_force_f64,
    "su3_gauge_force_f64.wgsl",
    ignore = "register allocator: unknown SSA value in GPR file (var array liveness)"
);

// Lattice: Wilson plaquette (f64, complex SU(3) math, multiple loops)
wgsl_compile_test!(
    corpus_wilson_plaquette_f64,
    "wilson_plaquette_f64.wgsl",
    ignore = "scheduler PerRegFile live_in mismatch in loop-carried phi"
);

// ===========================================================================
// hotSpring — Molecular dynamics
// ===========================================================================

// MD: velocity-Verlet half kick (f64, simple arithmetic)
wgsl_compile_test!(corpus_vv_half_kick_f64, "vv_half_kick_f64.wgsl");

// MD: kinetic energy (f64, reduction-style)
wgsl_compile_test!(corpus_kinetic_energy_f64, "kinetic_energy_f64.wgsl");

// MD: Berendsen thermostat (f64, sqrt, uniform)
wgsl_compile_test!(corpus_berendsen_f64, "berendsen_f64.wgsl");

// MD: off-diagonal stress tensor σ_xy (f64 virial)
// Cross-spring: used by wetSpring for mechanical property validation
wgsl_compile_test!(corpus_stress_virial_f64, "stress_virial_f64.wgsl");

// MD: Yukawa force with cell list (f64, complex pointer patterns)
wgsl_compile_test!(
    corpus_yukawa_force_celllist_f64,
    "yukawa_force_celllist_f64.wgsl"
);

// MD: radial distribution function (f64, atomicAdd)
wgsl_compile_test!(corpus_rdf_histogram_f64, "rdf_histogram_f64.wgsl");

// MD: VACF dot product v(t0)·v(t) (f64, per-particle dot, uniform struct)
wgsl_compile_test!(corpus_vacf_dot_f64, "vacf_dot_f64.wgsl");

// MD: Verlet reference copy — positions → ref_positions (f64, simple memory ops)
wgsl_compile_test!(corpus_verlet_copy_ref, "verlet_copy_ref.wgsl");

// ===========================================================================
// hotSpring — Nuclear physics (cancellation-safe precision)
// ===========================================================================

// Physics: Mermin dielectric function ε(k,ω) — stable W(z) asymptotic
// Cross-spring: referenced by wetSpring precision gap analysis
wgsl_compile_test!(
    corpus_dielectric_mermin_f64,
    "dielectric_mermin_f64.wgsl",
    ignore = "uses Complex64 struct type defined in separate include"
);

// Physics: BCS bisection root-finding — cancellation-safe v² formula
wgsl_compile_test!(
    corpus_bcs_bisection_f64,
    "bcs_bisection_f64.wgsl",
    ignore = "uses abs_f64 helper defined in separate include"
);

// Physics: HFB Hamiltonian (f64, complex math, many registers)
wgsl_compile_test!(
    corpus_batched_hfb_hamiltonian_f64,
    "batched_hfb_hamiltonian_f64.wgsl",
    ignore = "Pred→GPR coercion incomplete: ISetP encoder receives predicate in ALU source"
);

// Physics: SEMF batch (f64, f64 -> f32 cast)
wgsl_compile_test!(corpus_semf_batch_f64, "semf_batch_f64.wgsl");

// Physics: chi-squared batch (f64, pow/log transcendentals)
wgsl_compile_test!(corpus_chi2_batch_f64, "chi2_batch_f64.wgsl");

// ===========================================================================
// groundSpring — Condensed matter
// ===========================================================================

// Anderson localization (f32 variant, uniform struct, loops, PRNG)
wgsl_compile_test!(corpus_anderson_lyapunov_f32, "anderson_lyapunov_f32.wgsl");

// Anderson localization (f64, xoshiro128**, transfer matrix, uniform bindings)
// Cross-spring: referenced by neuralSpring for disorder sweep validation
wgsl_compile_test!(corpus_anderson_lyapunov_f64, "anderson_lyapunov_f64.wgsl");

// ===========================================================================
// neuralSpring — ML / attention primitives (coralForge)
// Cross-spring: evolved from hotSpring precision patterns (FMA, Kahan);
//   absorbed by wetSpring for bio-statistical DF64 dispatch pipelines
// ===========================================================================

// GELU activation (df64 core streaming — preamble auto-prepended)
wgsl_compile_test!(corpus_gelu_f64, "gelu_f64.wgsl");

// Layer normalization (df64 core streaming — preamble auto-prepended)
wgsl_compile_test!(corpus_layer_norm_f64, "layer_norm_f64.wgsl");

// Row-wise softmax (df64 core streaming, pass 2 of 3-pass SDPA)
wgsl_compile_test!(corpus_softmax_f64, "softmax_f64.wgsl");

// Scaled dot-product attention QK^T/sqrt(d_k) (df64, pass 1 of 3-pass SDPA)
wgsl_compile_test!(corpus_sdpa_scores_f64, "sdpa_scores_f64.wgsl");

// Sigmoid activation (df64 core streaming — preamble auto-prepended)
// Compiles through naga + IR lowering but hits RA SSA tracking on loop-carried phi
wgsl_compile_test!(
    corpus_sigmoid_f64,
    "sigmoid_f64.wgsl",
    ignore = "RA SSA tracking: loop-carried phi live_in mismatch in exp_df64 branches"
);

// KL divergence (f64, fused log-ratio + sum)
// Cross-spring: absorbed by wetSpring for cross-entropy validation,
//   groundSpring for Anderson model fitness
wgsl_compile_test!(corpus_kl_divergence_f64, "kl_divergence_f64.wgsl");

// Mean reduction (f32, single-workgroup, population fitness aggregation)
wgsl_compile_test!(corpus_mean_reduce, "mean_reduce.wgsl");

// Parallel RK4 integrator (f32, complex control flow, ODE solver)
// Cross-spring: exercises loop scheduling + register pressure
wgsl_compile_test!(corpus_rk4_parallel, "rk4_parallel.wgsl");

// ===========================================================================
// airSpring — Hydrology / environmental science
// ===========================================================================

// Local elementwise f64 — 6 domain ops (SCS-CN, Stewart, Makkink, Turc,
// Hamon, Blaney-Criddle). Switch lowered, but var_storage slot indexing
// incomplete for function-call inlining within switch cases.
wgsl_compile_test!(
    corpus_local_elementwise_f64,
    "local_elementwise_f64.wgsl",
    ignore = "var_storage slot overflow: switch case body stores to inlined function locals"
);
