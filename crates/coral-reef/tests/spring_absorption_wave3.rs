// SPDX-License-Identifier: AGPL-3.0-only
#![cfg(feature = "naga")]
//! Spring absorption wave 3 — hotSpring v0.6.25 + healthSpring v14.
//!
//! New domains: fluid dynamics, pharmacology, ecology.
//! Provenance: hotSpring lattice/physics/md, healthSpring health shaders.
//! Date: March 10, 2026.

use coral_reef::{AmdArch, CompileOptions, GpuArch, GpuTarget, compile_wgsl, compile_wgsl_full};

fn sm70_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn rdna2_opts() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        ..CompileOptions::default()
    }
}

// ---------------------------------------------------------------------------
// hotSpring — Euler HLL f64 (1D compressible Euler equations, HLL Riemann solver)
// Source: hotSpring/barracuda/src/physics/shaders/euler_hll_f64.wgsl
// Domain: Fluid dynamics (new domain for coralReef)
// Exercises: f64 arithmetic, sqrt, abs, min/max, conditional branches
// ---------------------------------------------------------------------------

#[test]
fn corpus_euler_hll_f64_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/euler_hll_f64.wgsl");
    let result = compile_wgsl_full(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

#[test]
fn corpus_euler_hll_f64_rdna2() {
    let wgsl = include_str!("fixtures/wgsl/corpus/euler_hll_f64.wgsl");
    let result = compile_wgsl_full(wgsl, &rdna2_opts());
    assert!(result.is_ok(), "RDNA2: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

// ---------------------------------------------------------------------------
// hotSpring — Deformed Potentials f64 (Skyrme mean-field + Coulomb)
// Source: hotSpring/barracuda/src/physics/shaders/deformed_potentials_f64.wgsl
// Domain: Nuclear physics (priority shader from spring review)
// Exercises: f64, pow, sqrt, complex array indexing, 3D grid computation
// ---------------------------------------------------------------------------

#[test]
fn corpus_deformed_potentials_f64_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/deformed_potentials_f64.wgsl");
    let result = compile_wgsl_full(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

#[test]
fn corpus_deformed_potentials_f64_rdna2() {
    let wgsl = include_str!("fixtures/wgsl/corpus/deformed_potentials_f64.wgsl");
    let result = compile_wgsl_full(wgsl, &rdna2_opts());
    assert!(result.is_ok(), "RDNA2: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

// ---------------------------------------------------------------------------
// hotSpring — Verlet neighbor list build (cell list construction)
// Source: hotSpring/barracuda/src/md/shaders/verlet_build.wgsl
// Domain: Molecular dynamics
// Exercises: f64, PBC, nested loops, cell list iteration
// ---------------------------------------------------------------------------

#[test]
fn corpus_verlet_build_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/verlet_build.wgsl");
    let result = compile_wgsl(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().is_empty());
}

#[test]
fn corpus_verlet_build_rdna2() {
    let wgsl = include_str!("fixtures/wgsl/corpus/verlet_build.wgsl");
    let result = compile_wgsl(wgsl, &rdna2_opts());
    assert!(result.is_ok(), "RDNA2: {}", result.unwrap_err());
    assert!(!result.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// hotSpring — Verlet displacement check (skin distance tracking)
// Source: hotSpring/barracuda/src/md/shaders/verlet_check_displacement.wgsl
// Domain: Molecular dynamics
// Exercises: f64, atomics (atomicMax u32), sqrt, workgroup barrier
// ---------------------------------------------------------------------------

#[test]
fn corpus_verlet_check_displacement_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/verlet_check_displacement.wgsl");
    let result = compile_wgsl(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().is_empty());
}

#[test]
fn corpus_verlet_check_displacement_rdna2() {
    let wgsl = include_str!("fixtures/wgsl/corpus/verlet_check_displacement.wgsl");
    let result = compile_wgsl(wgsl, &rdna2_opts());
    assert!(result.is_ok(), "RDNA2: {}", result.unwrap_err());
    assert!(!result.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// healthSpring — Population PK Monte Carlo (pharmacokinetics)
// Source: healthSpring/barracuda/shaders/health/population_pk_f64.wgsl
// Domain: Pharmacology (new spring for coralReef!)
// Exercises: f64, PRNG (Wang hash + xorshift32), exp via f32 cast
// ---------------------------------------------------------------------------

#[test]
fn corpus_population_pk_f64_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/population_pk_f64.wgsl");
    let result = compile_wgsl(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().is_empty());
}

#[test]
fn corpus_population_pk_f64_rdna2() {
    let wgsl = include_str!("fixtures/wgsl/corpus/population_pk_f64.wgsl");
    let result = compile_wgsl(wgsl, &rdna2_opts());
    assert!(result.is_ok(), "RDNA2: {}", result.unwrap_err());
    assert!(!result.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// healthSpring — Hill dose-response (sigmoid pharmacology model)
// Source: healthSpring/barracuda/shaders/health/hill_dose_response_f64.wgsl
// Domain: Pharmacology
// Exercises: f64, exp/log via f32 cast, pow pattern
// ---------------------------------------------------------------------------

#[test]
fn corpus_hill_dose_response_f64_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/hill_dose_response_f64.wgsl");
    let result = compile_wgsl(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().is_empty());
}

#[test]
fn corpus_hill_dose_response_f64_rdna2() {
    let wgsl = include_str!("fixtures/wgsl/corpus/hill_dose_response_f64.wgsl");
    let result = compile_wgsl(wgsl, &rdna2_opts());
    assert!(result.is_ok(), "RDNA2: {}", result.unwrap_err());
    assert!(!result.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// healthSpring — Shannon/Simpson diversity (ecology/epidemiology)
// Source: healthSpring/barracuda/shaders/health/diversity_f64.wgsl
// Domain: Ecology / Health
// Exercises: f64, log via f32 cast, reduction pattern
// ---------------------------------------------------------------------------

#[test]
fn corpus_diversity_f64_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/diversity_f64.wgsl");
    let result = compile_wgsl(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().is_empty());
}

#[test]
fn corpus_diversity_f64_rdna2() {
    let wgsl = include_str!("fixtures/wgsl/corpus/diversity_f64.wgsl");
    let result = compile_wgsl(wgsl, &rdna2_opts());
    assert!(result.is_ok(), "RDNA2: {}", result.unwrap_err());
    assert!(!result.unwrap().is_empty());
}
