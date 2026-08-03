// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2025-2026 ecoPrimals Collective
//! Environment variable names used by the coral-reef compiler crate.

/// When set, emit intermediate representation during compilation.
pub const CORAL_DEBUG_IR: &str = "CORAL_DEBUG_IR";

/// Output path for instruction dependency graphviz dumps.
pub const CORAL_DEP_GRAPH_PATH: &str = "CORAL_DEP_GRAPH_PATH";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_key_names_have_coral_prefix() {
        assert!(CORAL_DEBUG_IR.starts_with("CORAL_"));
        assert!(CORAL_DEP_GRAPH_PATH.starts_with("CORAL_"));
    }

    #[test]
    fn env_key_names_are_screaming_snake_case() {
        for key in [CORAL_DEBUG_IR, CORAL_DEP_GRAPH_PATH] {
            assert_eq!(
                key,
                key.to_ascii_uppercase(),
                "env key {key} must be SCREAMING_SNAKE_CASE"
            );
        }
    }
}
