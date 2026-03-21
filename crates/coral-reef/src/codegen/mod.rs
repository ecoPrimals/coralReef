// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023) — upstream NAK.

// Codegen module — derived from upstream compiler, evolving to idiomatic Rust.
// ISA domain types intentionally use naming conventions that mirror hardware docs
// (e.g. OpFAdd, SrcType, UGPR). dead_code covers builder traits and ISA variants
// reserved for future use.
#![expect(
    clippy::wildcard_imports,
    clippy::enum_glob_use,
    reason = "Ported NAK codegen relies on `use crate::codegen::ir::*` and `use SomeEnum::*` across many modules; explicit imports would be large churn without readability gain."
)]
#![expect(
    non_snake_case,
    dead_code,
    missing_docs,
    reason = "ISA domain types mirror hardware docs; codegen has intentionally unused items"
)]
// Domain-required expects (ISA encoding, GPU register naming, compiler pass structure).
#![expect(
    // ISA / encoding domain
    clippy::match_same_arms,                    // ISA encoding tables: explicit arms for clarity
    clippy::upper_case_acronyms,               // SSA, GPR, UGPR etc. are domain terms
    clippy::similar_names,                      // conventional GPU register names
    clippy::many_single_char_names,             // texture/encoding: x,y,z,a,b,c,d,o
    clippy::verbose_bit_mask,                   // encoding bit masks
    clippy::float_cmp,                          // constant folding: exact float cmp
    clippy::doc_markdown,                       // SSA, GPR etc. are domain terms
    // GPU register field reinterpretation
    clippy::cast_possible_wrap,                 // intentional u32↔i32
    clippy::cast_possible_truncation,           // known-width encoding fields
    clippy::cast_sign_loss,                     // intentional bit-pattern casts
    clippy::cast_lossless,                      // u32/u64 conversions
    clippy::cast_precision_loss,                // loop depth as f32
    // Compiler pass structure
    clippy::trivially_copy_pass_by_ref,         // trait compat: &self on small types
    clippy::needless_pass_by_value,             // pass functions take ownership by design
    clippy::too_many_arguments,                 // compiler passes have many parameters
    clippy::struct_excessive_bools,             // option structs
    clippy::wrong_self_convention,              // to_cssa mutates in place by design
    clippy::module_name_repetitions,            // module naming
    clippy::unused_self,                        // trait implementations
    clippy::missing_panics_doc,                 // internal functions
    // Ported code patterns (narrow further as code matures)
    clippy::explicit_deref_methods,             // SSAValueArray Deref usage
    clippy::bool_to_int_with_if,                // carry/overflow handling
    clippy::items_after_statements,             // use statements in match arms
    clippy::write_with_newline,                 // program formatting
    clippy::needless_range_loop,                // index for multi-array access
    clippy::stable_sort_primitive,              // sort semantics
    clippy::used_underscore_binding,            // debug assertions
    clippy::if_not_else,                        // dominance/CFG logic
    clippy::range_plus_one,                     // loop patterns
    clippy::absurd_extreme_comparisons,         // use_count comparison
    clippy::unnecessary_wraps,                  // pipeline returns Result for API
    // Style: fixable but need manual review (100 instances across 129 files)
    clippy::redundant_closure_for_method_calls,
    clippy::elidable_lifetime_names,
    clippy::explicit_into_iter_loop,
    clippy::manual_let_else,
    clippy::len_zero,
    clippy::from_iter_instead_of_collect,
    clippy::collapsible_else_if,
    clippy::collapsible_if,
    clippy::single_match,
    clippy::redundant_else,
    clippy::needless_return,
    clippy::map_unwrap_or,
    clippy::redundant_closure,
    clippy::useless_conversion,
    clippy::partialeq_to_none,
    clippy::question_mark,
    clippy::borrow_deref_ref,
    // Pedantic/nursery: fix incrementally
    clippy::missing_const_for_fn,
    clippy::option_if_let_else,
    clippy::derive_partial_eq_without_eq,
    reason = "domain-required for ISA encoding, GPU register naming, compiler pass structure"
)]

/// Internal Compiler Error — use instead of bare `panic!()` in codegen.
///
/// Provides source location and a consistent "this is a bug" message,
/// making ICE reports actionable. Accepts `format_args!` syntax.
macro_rules! ice {
    ($($arg:tt)*) => {
        panic!(
            "internal compiler error at {}:{}: {}\n\
             This is a bug in coralReef. Please report it.",
            file!(), line!(), format_args!($($arg)*)
        )
    };
}
pub(crate) use ice;

/// Wrap an encoder call in `catch_unwind`, converting ICE panics to
/// `CompileError::Internal`. Encoder panics are internal invariant
/// violations — this prevents process death and returns a structured error.
pub fn catch_ice<F>(f: F) -> Result<Vec<u32>, crate::CompileError>
where
    F: FnOnce() -> Vec<u32>,
{
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|payload| {
        let msg = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown ICE".to_string());
        crate::CompileError::Internal(msg.into())
    })
}

/// Default capacity for SSA-pass `BitSet<Phi>` allocations.
///
/// Sized for typical shader programs. Individual passes may exceed this
/// (the `BitSet` grows dynamically), but this avoids frequent reallocations
/// for the common case.
const PHI_BITSET_CAPACITY: usize = 4096;

pub mod amd;
mod api;
mod assign_regs;
mod builder;
mod calc_instr_deps;
mod const_tracker;
pub mod debug;
pub mod ir;
mod legalize;
mod liveness;
mod lower_copy_swap;
mod lower_f64;
mod lower_fma;
mod lower_par_copies;
pub mod nv;
pub mod ops;
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
pub mod pipeline;
mod reg_tracker;
mod repair_ssa;
mod spill_values;
mod ssa_value;
mod to_cssa;
mod union_find;

pub mod naga_translate;
