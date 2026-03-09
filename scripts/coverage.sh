#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
set -euo pipefail

echo "=== coralReef Coverage Report ==="
cargo llvm-cov --all --summary-only

echo ""
echo "=== Per-crate coverage ==="
for crate in coralreef-core coral-reef coral-reef-stubs coral-reef-bitview coral-reef-isa coral-driver coral-gpu nak-ir-proc; do
    echo "--- $crate ---"
    cargo llvm-cov --package "$crate" --summary-only 2>/dev/null || echo "  (no tests)"
done

echo ""
echo "=== HTML report ==="
cargo llvm-cov --all --html --output-dir target/llvm-cov-html
echo "Report written to target/llvm-cov-html/index.html"
