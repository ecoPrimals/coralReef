// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compile-fail UI tests for `nak_ir_proc` (expected macro expansions that error).

#[test]
fn compile_fail_cases() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
