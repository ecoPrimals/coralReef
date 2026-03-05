// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Compiler debug flags, driven by `NAK_DEBUG` environment variable.

use std::env;
use std::sync::OnceLock;

#[repr(u8)]
enum DebugFlag {
    Panic,
    Print,
    Serial,
    Spill,
    Annotate,
    NoUgpr,
    Cycles,
}

pub struct Debug {
    flags: u32,
}

impl Debug {
    fn from_env() -> Self {
        let Ok(debug_str) = env::var("NAK_DEBUG") else {
            return Self { flags: 0 };
        };

        let mut flags = 0u32;
        for flag in debug_str.split(',') {
            match flag.trim() {
                "panic" => flags |= 1 << DebugFlag::Panic as u8,
                "print" => flags |= 1 << DebugFlag::Print as u8,
                "serial" => flags |= 1 << DebugFlag::Serial as u8,
                "spill" => flags |= 1 << DebugFlag::Spill as u8,
                "annotate" => flags |= 1 << DebugFlag::Annotate as u8,
                "nougpr" => flags |= 1 << DebugFlag::NoUgpr as u8,
                "cycles" => flags |= 1 << DebugFlag::Cycles as u8,
                unk => eprintln!("Unknown NAK_DEBUG flag \"{unk}\""),
            }
        }
        Self { flags }
    }
}

pub trait GetDebugFlags {
    fn debug_flags(&self) -> u32;

    fn panic(&self) -> bool {
        self.debug_flags() & (1 << DebugFlag::Panic as u8) != 0
    }

    fn print(&self) -> bool {
        self.debug_flags() & (1 << DebugFlag::Print as u8) != 0
    }

    fn serial(&self) -> bool {
        self.debug_flags() & (1 << DebugFlag::Serial as u8) != 0
    }

    fn spill(&self) -> bool {
        self.debug_flags() & (1 << DebugFlag::Spill as u8) != 0
    }

    fn annotate(&self) -> bool {
        self.debug_flags() & (1 << DebugFlag::Annotate as u8) != 0
    }

    fn no_ugpr(&self) -> bool {
        self.debug_flags() & (1 << DebugFlag::NoUgpr as u8) != 0
    }

    fn cycles(&self) -> bool {
        self.debug_flags() & (1 << DebugFlag::Cycles as u8) != 0
    }
}

pub static DEBUG: OnceLock<Debug> = OnceLock::new();

impl GetDebugFlags for OnceLock<Debug> {
    fn debug_flags(&self) -> u32 {
        self.get_or_init(Debug::from_env).flags
    }
}
