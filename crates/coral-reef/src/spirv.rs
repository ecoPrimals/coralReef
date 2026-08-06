// SPDX-License-Identifier: AGPL-3.0-or-later
//! SPIR-V emission and parsing utilities.
//!
//! Extracts SPIR-V-specific functionality from the top-level API:
//! WGSL→SPIR-V emission, pre-parsed module emission, and WGSL→naga parsing.

use crate::error::CompileError;
use crate::preamble::prepare_wgsl;
use crate::{CompileOptions, SpirVOptions};

/// Emit validated SPIR-V binary from WGSL source.
///
/// Parses the WGSL, validates with `naga::valid::Validator`, and emits
/// standard SPIR-V binary (magic `0x07230203`). This is the sovereign
/// SPIR-V output path — GAP-HS-124.
///
/// Returns SPIR-V words as a `Vec<u8>` (little-endian u32 words packed
/// to bytes, as expected by Vulkan `VkShaderModuleCreateInfo`).
///
/// # Errors
///
/// Returns [`CompileError`] if WGSL parsing or SPIR-V emission fails.
pub fn wgsl_to_spirv(wgsl: &str, options: &CompileOptions) -> Result<Vec<u8>, CompileError> {
    if wgsl.is_empty() {
        return Err(CompileError::InvalidInput("empty WGSL source".into()));
    }
    let prepared = prepare_wgsl(wgsl, options);
    let module = naga::front::wgsl::parse_str(&prepared)
        .map_err(|e| CompileError::InvalidInput(format!("WGSL parse: {e}").into()))?;
    module_to_spirv(&module, options)
}

/// Emit SPIR-V from a pre-parsed `naga::Module`.
///
/// Validates the module, then emits standard SPIR-V binary. Use this when
/// the caller already has a parsed module (e.g. shared between the native
/// codegen and SPIR-V emission paths to avoid double-parsing WGSL).
///
/// # Errors
///
/// Returns [`CompileError`] if validation or SPIR-V emission fails.
pub fn module_to_spirv(
    module: &naga::Module,
    options: &CompileOptions,
) -> Result<Vec<u8>, CompileError> {
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let info = validator
        .validate(module)
        .map_err(|e| CompileError::Validation(format!("{e}").into()))?;

    let spv_opts = build_spirv_backend_options(options);
    let spv_words = naga::back::spv::write_vec(module, &info, &spv_opts, None)
        .map_err(|e| CompileError::Encoding(format!("SPIR-V emit: {e}").into()))?;

    let mut bytes = Vec::with_capacity(spv_words.len() * 4);
    for word in &spv_words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    Ok(bytes)
}

/// Parse and preprocess WGSL source into a `naga::Module`.
///
/// Applies `prepare_wgsl` preprocessing and then parses with the naga
/// frontend. Returns the module for reuse across both the native codegen
/// pipeline and SPIR-V emission (via [`module_to_spirv`]).
///
/// # Errors
///
/// Returns [`CompileError::InvalidInput`] if WGSL parsing fails.
pub fn parse_wgsl_to_naga(
    wgsl: &str,
    options: &CompileOptions,
) -> Result<naga::Module, CompileError> {
    let prepared = prepare_wgsl(wgsl, options);
    naga::front::wgsl::parse_str(&prepared)
        .map_err(|e| CompileError::InvalidInput(format!("WGSL parse: {e}").into()))
}

/// Build naga SPIR-V backend options from [`CompileOptions`].
pub(crate) fn build_spirv_backend_options(
    options: &CompileOptions,
) -> naga::back::spv::Options<'static> {
    let spv = options
        .spirv
        .as_ref()
        .map_or_else(SpirVOptions::default, Clone::clone);
    let zero_init = if spv.zero_init_workgroup_memory {
        naga::back::spv::ZeroInitializeWorkgroupMemoryMode::Polyfill
    } else {
        naga::back::spv::ZeroInitializeWorkgroupMemoryMode::None
    };
    let flags = if options.debug_info {
        naga::back::spv::WriterFlags::DEBUG
            | naga::back::spv::WriterFlags::LABEL_VARYINGS
            | naga::back::spv::WriterFlags::CLAMP_FRAG_DEPTH
    } else {
        naga::back::spv::WriterFlags::LABEL_VARYINGS
            | naga::back::spv::WriterFlags::CLAMP_FRAG_DEPTH
    };
    naga::back::spv::Options {
        lang_version: spv.version,
        flags,
        fake_missing_bindings: true,
        binding_map: naga::back::spv::BindingMap::default(),
        capabilities: None,
        bounds_check_policies: naga::proc::BoundsCheckPolicies::default(),
        zero_initialize_workgroup_memory: zero_init,
        force_loop_bounding: spv.force_loop_bounding,
        ray_query_initialization_tracking: true,
        use_storage_input_output_16: true,
        debug_info: None,
    }
}
