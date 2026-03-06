// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022) — upstream NAK.
//! Compiler debug flags, driven by `CORAL_REEF_DEBUG` environment variable.
//!
//! Falls back to `NAK_DEBUG` for backward compatibility.

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
        let debug_str = env::var("CORAL_REEF_DEBUG")
            .or_else(|_| env::var("NAK_DEBUG"))
            .unwrap_or_default();

        if debug_str.is_empty() {
            return Self { flags: 0 };
        }

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
                unk => eprintln!("Unknown CORAL_REEF_DEBUG flag \"{unk}\""),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Test struct that implements GetDebugFlags with configurable flags
    struct TestDebugFlags {
        flags: u32,
    }

    impl GetDebugFlags for TestDebugFlags {
        fn debug_flags(&self) -> u32 {
            self.flags
        }
    }

    #[test]
    fn test_get_debug_flags_panic() {
        let d = TestDebugFlags {
            flags: 1 << 0, // Panic
        };
        assert!(d.panic());
    }

    #[test]
    fn test_get_debug_flags_no_panic() {
        let d = TestDebugFlags { flags: 0 };
        assert!(!d.panic());
    }

    #[test]
    fn test_get_debug_flags_annotate() {
        let d = TestDebugFlags {
            flags: 1 << 4, // Annotate
        };
        assert!(d.annotate());
    }

    #[test]
    fn test_get_debug_flags_serial() {
        let d = TestDebugFlags {
            flags: 1 << 2, // Serial
        };
        assert!(d.serial());
    }

    #[test]
    fn test_debug_initializes() {
        let _ = DEBUG.debug_flags();
        assert!(DEBUG.get().is_some());
    }
}
