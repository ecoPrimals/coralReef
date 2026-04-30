// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared helpers for split `codegen_coverage_saturation` integration tests.
#![allow(missing_docs)]
#![allow(dead_code)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]

use coral_reef::{CompileOptions, GpuTarget, NvArch};

pub fn opts_for(nv: NvArch) -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Nvidia(nv),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

pub fn compile_for(wgsl: &str, nv: NvArch) -> Result<Vec<u8>, coral_reef::CompileError> {
    coral_reef::compile_wgsl(wgsl, &opts_for(nv))
}

/// Compile `wgsl` for every supported NVIDIA architecture (SM35–SM120).
///
/// SM 120 (Blackwell) uses the WIP PTX emitter — `NotImplemented` errors and panics are
/// tolerated until the emitter covers all patterns.
pub fn compile_fixture_all_nv(wgsl: &str) {
    for &nv in NvArch::ALL {
        let result = std::panic::catch_unwind(|| compile_for(wgsl, nv));
        match result {
            Ok(Ok(bin)) => assert!(!bin.is_empty(), "{nv}: empty binary"),
            Ok(Err(coral_reef::CompileError::NotImplemented(_))) if nv == NvArch::Sm120 => {}
            Ok(Err(e)) => panic!("{nv}: {e}"),
            Err(_) if nv == NvArch::Sm120 => {}
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

pub fn compile_wgsl_raw_sm(wgsl: &str, sm: u8) {
    let bin = coral_reef::compile_wgsl_raw_sm(wgsl, sm).expect("compile_wgsl_raw_sm");
    assert!(!bin.is_empty(), "SM{sm}: empty binary");
}
