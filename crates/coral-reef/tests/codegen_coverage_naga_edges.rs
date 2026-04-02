// SPDX-License-Identifier: AGPL-3.0-only
#![cfg(feature = "naga")]
//! Targeted WGSL fixtures for `naga_translate` codegen paths: `translate_switch`,
//! `emit_atomic`, `translate_array_length`, and additional f64 builtins.
//!
//! Atomic **subtract** is omitted here: it currently triggers `opt_copy_prop` assertion
//! failures (see `codegen_coverage_wgsl_pipeline.rs` atomics section).
//!
//! Complements `codegen_coverage_targeted.rs` (register pressure) and corpus tests.

use coral_reef::{CompileOptions, GpuArch, compile_wgsl_full};

fn sm70_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

#[test]
fn corpus_naga_edge_switch_u32_i32_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/naga_edge_switch_u32_i32.wgsl");
    let result = compile_wgsl_full(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

#[test]
fn corpus_naga_edge_atomics_add_min_max_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/naga_edge_atomics_add_min_max.wgsl");
    let result = compile_wgsl_full(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

#[test]
fn corpus_naga_edge_atomics_bitwise_exchange_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/naga_edge_atomics_bitwise_exchange.wgsl");
    let result = compile_wgsl_full(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

#[test]
fn corpus_naga_edge_atomic_compare_exchange_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/naga_edge_atomic_compare_exchange.wgsl");
    let result = compile_wgsl_full(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

#[test]
fn corpus_naga_edge_array_length_runtime_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/naga_edge_array_length_runtime.wgsl");
    let result = compile_wgsl_full(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

#[test]
fn corpus_naga_edge_f64_more_builtins_sm70() {
    let wgsl = include_str!("fixtures/wgsl/corpus/naga_edge_f64_more_builtins.wgsl");
    let result = compile_wgsl_full(wgsl, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}
