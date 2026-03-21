// SPDX-License-Identifier: AGPL-3.0-only
#![no_main]

use std::panic::{AssertUnwindSafe, catch_unwind};

use coral_reef::{AmdArch, CompileOptions, GpuTarget, NvArch, compile_wgsl};
use libfuzzer_sys::fuzz_target;

fn opts_sm70() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    }
}

fn opts_rdna2() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        ..CompileOptions::default()
    }
}

fuzz_target!(|data: &[u8]| {
    let wgsl = String::from_utf8_lossy(data);
    if wgsl.is_empty() {
        return;
    }

    let run = || {
        let _ = compile_wgsl(wgsl.as_ref(), &opts_sm70());
        let _ = compile_wgsl(wgsl.as_ref(), &opts_rdna2());
    };

    let _ = catch_unwind(AssertUnwindSafe(run));
});
