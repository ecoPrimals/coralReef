// SPDX-License-Identifier: AGPL-3.0-or-later

use coral_reef::{CompileError, CompileOptions, compile_wgsl};

// ---------------------------------------------------------------------------
// Stress tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_stress_large_workgroup_256() {
    let wgsl = "@compute @workgroup_size(256) fn main() { workgroupBarrier(); }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "workgroup_size(256) should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_stress_large_workgroup_1024() {
    let wgsl = "@compute @workgroup_size(1024) fn main() { workgroupBarrier(); }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "workgroup_size(1024) should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_stress_many_barriers() {
    let wgsl = "@compute @workgroup_size(64) fn main() {
        workgroupBarrier();
        workgroupBarrier();
        workgroupBarrier();
    }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "many barriers should compile or fail with NotImplemented: {result:?}"
    );
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
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "deep nesting should compile or fail with NotImplemented: {result:?}"
    );
}
