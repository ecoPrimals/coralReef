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
//! - `stress_virial_f64` — hotSpring MD off-diagonal stress tensor. Used by
//!   wetSpring for mechanical property validation.
//!
//! ## Iteration 17 absorptions
//!
//! - CG linear algebra suite (`cg_compute_alpha_f64`, `cg_compute_beta_f64`,
//!   `cg_update_p_f64`, `cg_update_xr_f64`, `complex_dot_re_f64`) — hotSpring
//!   lattice QCD conjugate gradient kernels. Test scalar + vector CG paths.
//!
//! - Yukawa variants (`yukawa_force_verlet_f64`, `yukawa_force_celllist_indirect_f64`)
//!   — hotSpring MD neighbor-list variants. Exercise `round()`, indirect u32 indexing.
//!
//! - `su3_momentum_update_f64`, `vacf_batch_f64`, `su3_flow_accumulate_f64` —
//!   hotSpring additional lattice/MD kernels.
//!
//! - `xoshiro128ss` — neuralSpring PRNG. Exercises bitwise rotl/shift/xor path.
//!
//! - `hmm_viterbi`, `hmm_backward_log` — neuralSpring HMM log-domain. Exercises
//!   argmax, logsumexp, exp/log codegen.
//!
//! - `pairwise_hamming`, `pairwise_jaccard` — neuralSpring distance metrics.
//!   Integer diff counting and set-based distance.
//!
//! - `rk45_adaptive` — neuralSpring adaptive ODE. `pow()` + scratch buffers.
//!
//! - `matrix_correlation` — neuralSpring shared-memory Pearson correlation.
//!
//! - `stencil_cooperation`, `spatial_payoff` — neuralSpring game-theory stencils.
//!
//! - `swarm_nn_forward` — neuralSpring batch NN forward with integer argmax.
//!
//! ## Iteration 21 absorptions
//!
//! ### hotSpring (9 new + 6 wired)
//!
//! - `spin_orbit_pack_f64`, `batched_hfb_density_f64`, `batched_hfb_energy_f64`
//!   — nuclear physics (HFB, spin-orbit). Self-contained f64.
//! - `esn_readout`, `esn_reservoir_update` — MD Echo State Network (f32, tanh).
//! - `su3_kinetic_energy_f64`, `su3_link_update_f64`, `staggered_fermion_force_f64`,
//!   `dirac_staggered_f64` — lattice QCD (SU(3) dynamics, fermion force, Dirac).
//! - Wired existing fixtures: `xpay_f64`, `yukawa_force_f64`, `wilson_action_f64`,
//!   `polyakov_loop_f64`, `vv_kick_drift_f64`, `lattice_init_f64`.
//!
//! ### neuralSpring (17 new + 6 wired)
//!
//! - coralForge Evoformer (df64 auto-prepended): `torsion_angles_f64`,
//!   `triangle_mul_outgoing_f64`, `triangle_mul_incoming_f64`,
//!   `triangle_attention_f64`, `outer_product_mean_f64`,
//!   `msa_row_attention_scores_f64`, `msa_col_attention_scores_f64`,
//!   `attention_apply_f64`, `ipa_scores_f64`, `backbone_update_f64`.
//! - Bio/evolution (f32): `hill_gate`, `batch_fitness_eval`, `multi_obj_fitness`,
//!   `swarm_nn_scores`, `locus_variance`, `head_split`, `head_concat`.
//! - Wired existing fixtures: `batch_ipr`, `wright_fisher_step`,
//!   `logsumexp_reduce`, `chi_squared_f64`, `pairwise_l2`, `linear_regression`.
//!
//! ### Retired
//!
//! - `local_elementwise_f64` — removed (airSpring v0.7.2 retired this shader;
//!   upstream replacement is `batched_elementwise_f64` in barraCuda).

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
            let src = include_str!(concat!("fixtures/wgsl/corpus/", $file));
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
            let src = include_str!(concat!("fixtures/wgsl/corpus/", $file));
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
wgsl_compile_test!(corpus_su3_gauge_force_f64, "su3_gauge_force_f64.wgsl");

// Lattice: Wilson plaquette (f64, complex SU(3) math, multiple loops)
wgsl_compile_test!(corpus_wilson_plaquette_f64, "wilson_plaquette_f64.wgsl");

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

// Physics: BCS bisection root-finding — cancellation-safe v² formula.
// abs_f64 inlined (was preamble-injected by hotSpring ShaderTemplate).
wgsl_compile_test!(corpus_bcs_bisection_f64, "bcs_bisection_f64.wgsl");

// Physics: HFB Hamiltonian (f64, complex math, many registers)
wgsl_compile_test!(
    corpus_batched_hfb_hamiltonian_f64,
    "batched_hfb_hamiltonian_f64.wgsl"
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
wgsl_compile_test!(corpus_sigmoid_f64, "sigmoid_f64.wgsl");

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
// hotSpring — CG linear algebra (Iteration 17 absorption)
// ===========================================================================

// CG scalar: α = rz / pAp (single-thread f64, 2 storage bindings)
wgsl_compile_test!(corpus_cg_compute_alpha_f64, "cg_compute_alpha_f64.wgsl");

// CG scalar: β = rz_new / rz_old (single-thread f64)
wgsl_compile_test!(corpus_cg_compute_beta_f64, "cg_compute_beta_f64.wgsl");

// CG vector: p = r + β·p (workgroup_size(64), f64 AXPY variant)
wgsl_compile_test!(corpus_cg_update_p_f64, "cg_update_p_f64.wgsl");

// CG vector: x += α·p, r -= α·ap (6 bindings, dual update)
wgsl_compile_test!(corpus_cg_update_xr_f64, "cg_update_xr_f64.wgsl");

// Complex dot product Re(a·conj(b)) for n_pairs (f64, integer indexing)
wgsl_compile_test!(corpus_complex_dot_re_f64, "complex_dot_re_f64.wgsl");

// ===========================================================================
// hotSpring — Yukawa MD variants (Iteration 17 absorption)
// ===========================================================================

// Yukawa force with Verlet neighbor list (f64, round, u32 neighbor list)
wgsl_compile_test!(
    corpus_yukawa_force_verlet_f64,
    "yukawa_force_verlet_f64.wgsl"
);

// Yukawa cell-list force with indirect sorted_indices (f64, u32 indexing)
wgsl_compile_test!(
    corpus_yukawa_force_celllist_indirect_f64,
    "yukawa_force_celllist_indirect_f64.wgsl"
);

// ===========================================================================
// hotSpring — Lattice QCD additional (Iteration 17 absorption)
// ===========================================================================

// SU(3) momentum update: P += dt·F (f64, 18-element loop per link)
wgsl_compile_test!(
    corpus_su3_momentum_update_f64,
    "su3_momentum_update_f64.wgsl"
);

// Batched VACF: v(t0)·v(t) over multiple time origins (f64, uniform struct)
wgsl_compile_test!(corpus_vacf_batch_f64, "vacf_batch_f64.wgsl");

// Gradient flow K-buffer accumulate: K = α·K + Z (f64 AXPY-like)
wgsl_compile_test!(
    corpus_su3_flow_accumulate_f64,
    "su3_flow_accumulate_f64.wgsl"
);

// ===========================================================================
// neuralSpring — PRNG / bitwise (Iteration 17 absorption)
// ===========================================================================

// Xoshiro128** PRNG: rotl, shift, xor (pure u32/f32, PRNG codegen path)
wgsl_compile_test!(corpus_xoshiro128ss, "xoshiro128ss.wgsl");

// ===========================================================================
// neuralSpring — HMM / log-domain (Iteration 17 absorption)
// ===========================================================================

// HMM Viterbi decoding: argmax over log-transition + emission (f32, u32)
wgsl_compile_test!(corpus_hmm_viterbi, "hmm_viterbi.wgsl");

// HMM backward: logsumexp reduction over j states (f32, exp, log)
wgsl_compile_test!(corpus_hmm_backward_log, "hmm_backward_log.wgsl");

// ===========================================================================
// neuralSpring — Distance / set metrics (Iteration 17 absorption)
// ===========================================================================

// Pairwise Hamming distance: integer diff count between u32 sequences
wgsl_compile_test!(corpus_pairwise_hamming, "pairwise_hamming.wgsl");

// Pairwise Jaccard distance: 1 - intersection/union from PA matrix (f32)
wgsl_compile_test!(corpus_pairwise_jaccard, "pairwise_jaccard.wgsl");

// ===========================================================================
// neuralSpring — Adaptive ODE / stencil (Iteration 17 absorption)
// ===========================================================================

// Adaptive Dormand-Prince RK45 with Hill RHS (f32, pow, scratch buffer)
wgsl_compile_test!(corpus_rk45_adaptive, "rk45_adaptive.wgsl");

// Pearson correlation via workgroup reduction (shared memory, f32)
wgsl_compile_test!(corpus_matrix_correlation, "matrix_correlation.wgsl");

// Fermi imitation dynamics on 2D grid (u32 strategies, Moore stencil)
wgsl_compile_test!(corpus_stencil_cooperation, "stencil_cooperation.wgsl");

// Spatial prisoner's dilemma payoff stencil (u32/f32, Moore neighborhood)
wgsl_compile_test!(corpus_spatial_payoff, "spatial_payoff.wgsl");

// Batch NN forward: 1→4→5 sigmoid layers + argmax (f32, u32 actions)
wgsl_compile_test!(corpus_swarm_nn_forward, "swarm_nn_forward.wgsl");

// ===========================================================================
// hotSpring — Existing fixtures wired for corpus (Iteration 21)
// ===========================================================================

// Lattice: BLAS xpay x = a*x + y (f64, simple loop)
wgsl_compile_test!(corpus_xpay_f64, "xpay_f64.wgsl");

// Lattice: Yukawa all-pairs force (f64, N² loop)
wgsl_compile_test!(corpus_yukawa_force_f64, "yukawa_force_f64.wgsl");

// Lattice: Wilson action (f64, plaquette sum → S = β·Σ(1-Re Tr U))
wgsl_compile_test!(corpus_wilson_action_f64, "wilson_action_f64.wgsl");

// Lattice: Polyakov loop (f64, temporal link product)
wgsl_compile_test!(corpus_polyakov_loop_f64, "polyakov_loop_f64.wgsl");

// MD: velocity-Verlet kick+drift fused (f64, leapfrog)
wgsl_compile_test!(corpus_vv_kick_drift_f64, "vv_kick_drift_f64.wgsl");

// Lattice: lattice initialization (f64, SU(3) identity/near-identity)
wgsl_compile_test!(corpus_lattice_init_f64, "lattice_init_f64.wgsl");

// ===========================================================================
// neuralSpring — Existing fixtures wired for corpus (Iteration 21)
// ===========================================================================

// Anderson IPR: inverse participation ratio (f32, spectral analysis)
wgsl_compile_test!(corpus_batch_ipr, "batch_ipr.wgsl");

// Wright-Fisher step: population genetics drift (f32, stochastic)
wgsl_compile_test!(corpus_wright_fisher_step, "wright_fisher_step.wgsl");

// Log-sum-exp reduction: HMM log-domain (f32, stable logsumexp)
wgsl_compile_test!(corpus_logsumexp_reduce, "logsumexp_reduce.wgsl");

// Chi-squared test: goodness-of-fit (f64, statistical test)
wgsl_compile_test!(corpus_chi_squared_f64, "chi_squared_f64.wgsl");

// Pairwise L2 distance: MODES novelty search (f32, distance matrix)
wgsl_compile_test!(corpus_pairwise_l2, "pairwise_l2.wgsl");

// Linear regression: OLS normal equations (f32, stats)
wgsl_compile_test!(corpus_linear_regression, "linear_regression.wgsl");

// ===========================================================================
// hotSpring — New absorption: nuclear/physics (Iteration 21)
// ===========================================================================

// Nuclear: spin-orbit interaction packing (f64, simple struct ops)
wgsl_compile_test!(corpus_spin_orbit_pack_f64, "spin_orbit_pack_f64.wgsl");

// Nuclear: HFB density matrix (f64, BCS occupation, sqrt)
wgsl_compile_test!(
    corpus_batched_hfb_density_f64,
    "batched_hfb_density_f64.wgsl"
);

// Nuclear: HFB total energy (f64, SEMF + pairing + deformation)
wgsl_compile_test!(corpus_batched_hfb_energy_f64, "batched_hfb_energy_f64.wgsl");

// ===========================================================================
// hotSpring — New absorption: MD / ESN (Iteration 21)
// ===========================================================================

// MD: Echo State Network readout (f32, linear output layer)
wgsl_compile_test!(corpus_esn_readout, "esn_readout.wgsl");

// MD: ESN reservoir update (f32, tanh activation, sparse recurrence)
wgsl_compile_test!(corpus_esn_reservoir_update, "esn_reservoir_update.wgsl");

// ===========================================================================
// hotSpring — New absorption: Lattice QCD (Iteration 21)
// ===========================================================================

// Lattice: SU(3) kinetic energy Tr(P†P) (f64, 18-element inner product)
wgsl_compile_test!(corpus_su3_kinetic_energy_f64, "su3_kinetic_energy_f64.wgsl");

// Lattice: SU(3) link update via Cayley (f64, 3×3 matrix inverse, exp)
wgsl_compile_test!(corpus_su3_link_update_f64, "su3_link_update_f64.wgsl");

// Lattice: staggered fermion force dS_F/dU (f64, outer product, TA projection)
wgsl_compile_test!(
    corpus_staggered_fermion_force_f64,
    "staggered_fermion_force_f64.wgsl"
);

// Lattice: staggered Dirac operator D_stag·ψ (f64, SU(3)×color-vector)
wgsl_compile_test!(corpus_dirac_staggered_f64, "dirac_staggered_f64.wgsl");

// ===========================================================================
// neuralSpring — New absorption: bio/evolution (Iteration 21)
// ===========================================================================

// Hill gate: signal integration (f32, Hill function, pow)
wgsl_compile_test!(corpus_hill_gate, "hill_gate.wgsl");

// EA batch fitness: NK landscape evaluation (f32, fma, bitwise)
wgsl_compile_test!(corpus_batch_fitness_eval, "batch_fitness_eval.wgsl");

// Multi-objective Pareto fitness (f32, sqrt, fma)
wgsl_compile_test!(corpus_multi_obj_fitness, "multi_obj_fitness.wgsl");

// Swarm NN scores: score path for swarm robotics (f32, sigmoid, exp)
wgsl_compile_test!(corpus_swarm_nn_scores, "swarm_nn_scores.wgsl");

// Meta-population locus variance / FST (f32, allele frequency stats)
wgsl_compile_test!(corpus_locus_variance, "locus_variance.wgsl");

// MHA head split: reshape (B,S,D) → (B,S,H,Dh) (f32, pure indexing)
wgsl_compile_test!(corpus_head_split, "head_split.wgsl");

// MHA head concat: reshape (B,S,H,Dh) → (B,S,D) (f32, pure indexing)
wgsl_compile_test!(corpus_head_concat, "head_concat.wgsl");

// ===========================================================================
// neuralSpring — New absorption: coralForge Evoformer (df64, Iteration 21)
// Df64/df64_* usage auto-prepended by coralReef prepare_wgsl()
// ===========================================================================

// Evoformer: torsion angle dihedrals (df64, atan2, vector ops)
wgsl_compile_test!(corpus_torsion_angles_f64, "torsion_angles_f64.wgsl");

// Evoformer: triangle multiplication outgoing (df64, Algorithm 11)
wgsl_compile_test!(
    corpus_triangle_mul_outgoing_f64,
    "triangle_mul_outgoing_f64.wgsl"
);

// Evoformer: triangle multiplication incoming (df64, Algorithm 12)
wgsl_compile_test!(
    corpus_triangle_mul_incoming_f64,
    "triangle_mul_incoming_f64.wgsl"
);

// Evoformer: triangle attention (df64, softmax+gate, Algorithm 13)
wgsl_compile_test!(corpus_triangle_attention_f64, "triangle_attention_f64.wgsl");

// MSA: outer product mean accumulation (df64)
wgsl_compile_test!(corpus_outer_product_mean_f64, "outer_product_mean_f64.wgsl");

// MSA: row attention scores (df64, Q·K^T / sqrt(d))
wgsl_compile_test!(
    corpus_msa_row_attention_scores_f64,
    "msa_row_attention_scores_f64.wgsl"
);

// MSA: column attention scores (df64, transposed attention)
wgsl_compile_test!(
    corpus_msa_col_attention_scores_f64,
    "msa_col_attention_scores_f64.wgsl"
);

// Attention: output application V·attn_weights (df64)
wgsl_compile_test!(corpus_attention_apply_f64, "attention_apply_f64.wgsl");

// IPA: invariant point attention scores (df64, frame transforms)
wgsl_compile_test!(corpus_ipa_scores_f64, "ipa_scores_f64.wgsl");

// IPA: backbone frame update (df64, quaternion-like rotation)
wgsl_compile_test!(corpus_backbone_update_f64, "backbone_update_f64.wgsl");
