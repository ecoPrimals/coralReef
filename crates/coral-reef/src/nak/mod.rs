// Copyright © 2023 Collabora, Ltd.
// SPDX-License-Identifier: MIT

// NAK-derived shader compiler code — these allows reflect patterns from upstream Mesa
// that would be unsafe to mass-refactor during the porting process.
#![allow(
    clippy::match_same_arms,        // ISA encoding tables have intentionally explicit arms for clarity
    clippy::trivially_copy_pass_by_ref, // NAK API uses &self consistently for trait compatibility
    clippy::needless_pass_by_value, // NAK pass functions take ownership by design
    clippy::cast_possible_wrap,     // GPU register fields use intentional u32↔i32 reinterpretation
    clippy::cast_possible_truncation, // GPU encoding fields are known-width
    clippy::cast_sign_loss,         // Intentional bit-pattern casts in encoding
    clippy::doc_markdown,           // NAK doc references (SSA, GPR, etc.) are domain terms
    clippy::uninlined_format_args,  // Will be fixed incrementally
    clippy::too_many_arguments,     // Compiler passes have inherently many parameters
    clippy::similar_names,          // NAK uses conventional GPU register names
    clippy::module_name_repetitions, // Standard NAK module naming
    clippy::unused_self,            // Trait implementations
    clippy::missing_panics_doc,     // NAK internal functions
    clippy::struct_excessive_bools, // NAK option structs
    clippy::many_single_char_names, // Texture/encoding code uses x,y,z,a,b,c,d,o
    clippy::redundant_else,        // NAK control flow patterns
    clippy::explicit_deref_methods, // SSAValueArray Deref usage
    clippy::len_zero,              // NAK uses len() > 0 patterns
    clippy::upper_case_acronyms,   // SSA, GPR, etc. are domain terms
    clippy::if_not_else,           // NAK dominance/CFG logic
    clippy::explicit_into_iter_loop, // NAK block iteration
    clippy::wrong_self_convention,  // to_cssa mutates in place by design
    clippy::too_many_lines,         // NAK compiler passes are inherently large
    clippy::redundant_closure_for_method_calls, // NAK iteration patterns
    clippy::collapsible_else_if,    // NAK control flow clarity
    clippy::needless_range_loop,    // NAK uses index for multi-array access
    clippy::stable_sort_primitive,  // NAK sort semantics
    clippy::used_underscore_binding, // NAK debug assertions
    clippy::manual_assert,          // NAK uses if/panic for custom messages
    clippy::struct_field_names,     // NAK IR: Src.src_ref, Dst.dst_ref etc.
    clippy::verbose_bit_mask,       // NAK encoding bit masks
    clippy::range_plus_one,         // NAK loop patterns
    clippy::float_cmp,              // NAK constant folding uses exact float cmp
    clippy::cast_lossless,          // NAK u32/u64 conversions
    clippy::bool_to_int_with_if,    // NAK carry/overflow handling
    clippy::items_after_statements, // NAK use statements in match arms
    clippy::write_with_newline,     // NAK program formatting
    clippy::manual_range_contains,   // NAK address range checks
    clippy::elidable_lifetime_names, // NAK impl blocks
    clippy::borrow_deref_ref,      // NAK iteration patterns
    clippy::manual_let_else,       // NAK control flow style
    clippy::single_match,          // NAK match patterns
    clippy::cast_precision_loss,   // NAK loop depth as f32
    clippy::collapsible_if,        // NAK scheduling logic
    clippy::needless_return,       // NAK control flow style
    clippy::absurd_extreme_comparisons, // NAK num_uses comparison
    clippy::redundant_closure,     // NAK iterator patterns
    clippy::useless_conversion,    // NAK extend/into_iter
    clippy::from_iter_instead_of_collect, // NAK LiveSet construction
    clippy::match_like_matches_macro, // NAK match style
    clippy::partialeq_to_none,     // NAK Option checks
    clippy::map_unwrap_or,         // NAK optional handling
    clippy::unnecessary_wraps,     // Pipeline returns Result for API
    clippy::question_mark,         // NAK optional style
)]

mod api;
mod assign_regs;
mod builder;
mod calc_instr_deps;
mod const_tracker;
pub(crate) mod debug;
pub(crate) mod ir;
mod legalize;
mod lower_f64;
mod liveness;
mod lower_copy_swap;
mod lower_par_copies;
mod opt_bar_prop;
mod opt_copy_prop;
mod opt_crs;
mod opt_dce;
mod opt_instr_sched_common;
mod opt_instr_sched_postpass;
mod opt_instr_sched_prepass;
mod opt_jump_thread;
mod opt_lop;
mod opt_out;
mod opt_prmt;
mod opt_uniform_instrs;
pub(crate) mod pipeline;
mod reg_tracker;
mod repair_ssa;
mod sm120_instr_latencies;
mod sm20;
mod sm30_instr_latencies;
mod sm32;
mod sm50;
mod sm70;
mod sm70_encode;
mod sm70_instr_latencies;
mod sm75_instr_latencies;
mod sm80_instr_latencies;
mod sph;
mod spill_values;
mod ssa_value;
mod to_cssa;
mod union_find;

pub(crate) mod from_spirv;
