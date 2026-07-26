// SPDX-License-Identifier: AGPL-3.0-or-later

use super::assert_ok_or_not_implemented;
use coral_reef::{CompileOptions, compile_wgsl};

// ---------------------------------------------------------------------------
// Stress tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_stress_large_workgroup_256() {
    let wgsl = "@compute @workgroup_size(256) fn main() { workgroupBarrier(); }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert_ok_or_not_implemented(&result, "workgroup_size(256)");
}

#[test]
fn test_pipeline_stress_large_workgroup_1024() {
    let wgsl = "@compute @workgroup_size(1024) fn main() { workgroupBarrier(); }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert_ok_or_not_implemented(&result, "workgroup_size(1024)");
}

#[test]
fn test_pipeline_stress_many_barriers() {
    let wgsl = "@compute @workgroup_size(64) fn main() {
        workgroupBarrier();
        workgroupBarrier();
        workgroupBarrier();
    }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert_ok_or_not_implemented(&result, "many barriers");
}

#[test]
fn test_pipeline_stress_deep_nesting() {
    let wgsl = "@compute @workgroup_size(1) fn main() {
        if true {
            if true {
                if true {
                    if true { } else { }
                } else { }
            } else { }
        } else { }
    }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert_ok_or_not_implemented(&result, "deep nesting");
}
