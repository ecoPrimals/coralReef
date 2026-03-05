// SPDX-License-Identifier: AGPL-3.0-only
//! NIR instruction printer — replacement for `compiler::nir_instr_printer`.
//!
//! **Legacy**: `from_nir` is disabled; coralNak is evolving to SPIR-V via naga.
//! This module is dead code until removed.
//!
//! Provides debug formatting for NIR instructions. This will be removed
//! entirely when the NIR frontend is replaced by a naga SPIR-V frontend.

use std::fmt;

/// NIR instruction printer for debug output.
///
/// Formats NIR instructions for logging and debugging. In the sovereign
/// pipeline this will be replaced by a coral-nak IR printer.
pub struct NirInstrPrinter {
    indent: usize,
}

impl NirInstrPrinter {
    /// Create a new printer with default indentation.
    #[must_use]
    pub fn new() -> Self {
        Self { indent: 0 }
    }

    /// Create a printer with custom indentation level.
    #[must_use]
    pub fn with_indent(indent: usize) -> Self {
        Self { indent }
    }

    /// Current indentation level.
    #[must_use]
    pub fn indent(&self) -> usize {
        self.indent
    }

    /// Increase indentation by one level.
    pub fn push_indent(&mut self) {
        self.indent += 1;
    }

    /// Decrease indentation by one level.
    pub fn pop_indent(&mut self) {
        self.indent = self.indent.saturating_sub(1);
    }

    /// Format an indentation prefix.
    #[must_use]
    pub fn indent_str(&self) -> String {
        "  ".repeat(self.indent)
    }
}

impl Default for NirInstrPrinter {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for NirInstrPrinter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NirInstrPrinter")
            .field("indent", &self.indent)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_default() {
        let p = NirInstrPrinter::new();
        assert_eq!(p.indent(), 0);
    }

    #[test]
    fn test_with_indent() {
        let p = NirInstrPrinter::with_indent(3);
        assert_eq!(p.indent(), 3);
        assert_eq!(p.indent_str(), "      ");
    }

    #[test]
    fn test_push_pop_indent() {
        let mut p = NirInstrPrinter::new();
        p.push_indent();
        assert_eq!(p.indent(), 1);
        p.push_indent();
        assert_eq!(p.indent(), 2);
        p.pop_indent();
        assert_eq!(p.indent(), 1);
        p.pop_indent();
        p.pop_indent(); // saturating
        assert_eq!(p.indent(), 0);
    }

    #[test]
    fn test_debug() {
        let p = NirInstrPrinter::new();
        let dbg = format!("{p:?}");
        assert!(dbg.contains("NirInstrPrinter"));
    }
}
