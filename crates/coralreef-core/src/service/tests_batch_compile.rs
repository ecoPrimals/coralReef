// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tests for `shader.compile.multi` — mixed-input batch compilation.

use super::*;
use std::sync::Arc;
use types::{BatchCompileJob, BatchCompileRequest};

fn wgsl_job(arch: &str) -> BatchCompileJob {
    BatchCompileJob {
        input_type: "wgsl".into(),
        source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: arch.into(),
        opt_level: 2,
        fp64_software: false,
        fma_policy: None,
        label: None,
    }
}

fn glsl_job(arch: &str) -> BatchCompileJob {
    BatchCompileJob {
        input_type: "glsl".into(),
        source: Arc::from("#version 450\nlayout(local_size_x = 1) in;\nvoid main() {}"),
        arch: arch.into(),
        opt_level: 2,
        fp64_software: false,
        fma_policy: None,
        label: None,
    }
}

#[test]
fn batch_compile_single_wgsl() {
    let req = BatchCompileRequest {
        jobs: vec![wgsl_job("sm_70")],
    };
    let resp = handle_compile_multi(req).expect("single WGSL job should succeed");
    assert_eq!(resp.total_count, 1);
    assert_eq!(resp.success_count, 1);
    assert_eq!(resp.results.len(), 1);
    assert_eq!(resp.results[0].index, 0);
    assert_eq!(resp.results[0].arch, "sm_70");
    assert_eq!(resp.results[0].input_type, "wgsl");
    assert!(resp.results[0].binary.is_some());
    assert!(resp.results[0].size > 0);
    assert!(resp.results[0].error.is_none());
    assert!(resp.results[0].info.is_some());
    assert!(resp.results[0].compile_time_ms.is_some());
}

#[test]
fn batch_compile_single_glsl() {
    let req = BatchCompileRequest {
        jobs: vec![glsl_job("sm_70")],
    };
    let resp = handle_compile_multi(req).expect("single GLSL job should succeed");
    assert_eq!(resp.total_count, 1);
    assert_eq!(resp.success_count, 1);
    assert!(resp.results[0].binary.is_some());
    assert_eq!(resp.results[0].input_type, "glsl");
}

#[test]
fn batch_compile_mixed_wgsl_glsl() {
    let req = BatchCompileRequest {
        jobs: vec![wgsl_job("sm_70"), glsl_job("sm_86")],
    };
    let resp = handle_compile_multi(req).expect("mixed WGSL+GLSL should succeed");
    assert_eq!(resp.total_count, 2);
    assert_eq!(resp.success_count, 2);
    assert_eq!(resp.results[0].input_type, "wgsl");
    assert_eq!(resp.results[0].arch, "sm_70");
    assert_eq!(resp.results[1].input_type, "glsl");
    assert_eq!(resp.results[1].arch, "sm_86");
}

#[test]
fn batch_compile_cross_vendor() {
    let req = BatchCompileRequest {
        jobs: vec![wgsl_job("sm_80"), wgsl_job("rdna2")],
    };
    let resp = handle_compile_multi(req).expect("cross-vendor should succeed");
    assert_eq!(resp.success_count, 2);
    assert_eq!(resp.results[0].arch, "sm_80");
    assert_eq!(resp.results[1].arch, "rdna2");
}

#[test]
fn batch_compile_partial_failure() {
    let req = BatchCompileRequest {
        jobs: vec![wgsl_job("sm_70"), wgsl_job("sm_99")],
    };
    let resp = handle_compile_multi(req).expect("partial failure is not top-level error");
    assert_eq!(resp.total_count, 2);
    assert_eq!(resp.success_count, 1);
    assert!(resp.results[0].binary.is_some());
    assert!(resp.results[0].error.is_none());
    assert!(resp.results[1].binary.is_none());
    assert!(resp.results[1].error.is_some());
}

#[test]
fn batch_compile_unsupported_input_type() {
    let req = BatchCompileRequest {
        jobs: vec![BatchCompileJob {
            input_type: "hlsl".into(),
            source: Arc::from("void main() {}"),
            arch: "sm_70".into(),
            opt_level: 2,
            fp64_software: false,
            fma_policy: None,
            label: None,
        }],
    };
    let resp = handle_compile_multi(req).expect("bad input_type is per-job error");
    assert_eq!(resp.success_count, 0);
    assert!(resp.results[0].error.is_some());
    let err = resp.results[0].error.as_deref().unwrap_or_default();
    assert!(
        err.contains("unsupported input_type"),
        "error should mention unsupported type, got: {err}"
    );
}

#[test]
fn batch_compile_empty_jobs_rejected() {
    let req = BatchCompileRequest { jobs: vec![] };
    assert!(handle_compile_multi(req).is_err());
}

#[test]
fn batch_compile_labels_echoed() {
    let mut job = wgsl_job("sm_70");
    job.label = Some("kernel-A".into());
    let req = BatchCompileRequest { jobs: vec![job] };
    let resp = handle_compile_multi(req).expect("labeled job should succeed");
    assert_eq!(resp.results[0].label.as_deref(), Some("kernel-A"));
}

#[test]
fn batch_compile_preserves_index_order() {
    let req = BatchCompileRequest {
        jobs: vec![wgsl_job("sm_70"), glsl_job("sm_86"), wgsl_job("rdna2")],
    };
    let resp = handle_compile_multi(req).expect("three jobs should succeed");
    assert_eq!(resp.total_count, 3);
    assert_eq!(resp.success_count, 3);
    for (i, result) in resp.results.iter().enumerate() {
        assert_eq!(result.index, i, "index must match position");
    }
}

#[test]
fn batch_compile_request_serde_roundtrip() {
    let req = BatchCompileRequest {
        jobs: vec![
            BatchCompileJob {
                input_type: "wgsl".into(),
                source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
                arch: "sm_70".into(),
                opt_level: 3,
                fp64_software: true,
                fma_policy: Some("fused".into()),
                label: Some("test-0".into()),
            },
            BatchCompileJob {
                input_type: "glsl".into(),
                source: Arc::from("#version 450\nvoid main(){}"),
                arch: "rdna3".into(),
                opt_level: 1,
                fp64_software: false,
                fma_policy: None,
                label: None,
            },
        ],
    };
    let json = serde_json::to_string(&req).unwrap();
    let roundtrip: BatchCompileRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.jobs.len(), 2);
    assert_eq!(roundtrip.jobs[0].input_type, "wgsl");
    assert_eq!(roundtrip.jobs[0].label.as_deref(), Some("test-0"));
    assert_eq!(roundtrip.jobs[1].input_type, "glsl");
    assert!(roundtrip.jobs[1].label.is_none());
}

#[test]
fn batch_compile_fma_policy_forwarded() {
    let mut job = wgsl_job("sm_70");
    job.fma_policy = Some("fused".into());
    let req = BatchCompileRequest { jobs: vec![job] };
    let resp = handle_compile_multi(req).expect("fma policy should be accepted");
    assert_eq!(resp.success_count, 1);
}

#[test]
fn batch_compile_case_insensitive_input_type() {
    let mut job = wgsl_job("sm_70");
    job.input_type = "WGSL".into();
    let req = BatchCompileRequest { jobs: vec![job] };
    let resp = handle_compile_multi(req).expect("uppercase input_type should work");
    assert_eq!(resp.success_count, 1);
}
