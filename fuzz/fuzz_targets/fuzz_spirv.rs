// SPDX-License-Identifier: AGPL-3.0-or-later
#![no_main]

use std::panic::{AssertUnwindSafe, catch_unwind};

use coral_reef::{AmdArch, CompileOptions, GpuTarget, NvArch, compile};
use libfuzzer_sys::fuzz_target;

fn words_from_bytes(data: &[u8]) -> Vec<u32> {
    let len = data.len() & !3;
    let mut words = Vec::with_capacity(len / 4);
    for chunk in data[..len].chunks_exact(4) {
        let mut arr = [0u8; 4];
        arr.copy_from_slice(chunk);
        words.push(u32::from_le_bytes(arr));
    }
    words
}

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
    let words = words_from_bytes(data);
    if words.is_empty() {
        return;
    }

    let run = || {
        let _ = compile(words.as_slice(), &opts_sm70());
        let _ = compile(words.as_slice(), &opts_rdna2());
    };

    let _ = catch_unwind(AssertUnwindSafe(run));
});
