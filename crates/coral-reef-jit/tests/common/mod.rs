// SPDX-License-Identifier: AGPL-3.0-only
//! Shared helpers for JIT integration tests.

use bytes::Bytes;
use coral_reef_cpu::types::{BindingData, BindingUsage, ExecuteCpuRequest};
use coral_reef_jit::execute_jit;

pub const TOLERANCE: f64 = 1e-5;

pub fn f32_bytes(values: &[f32]) -> Bytes {
    Bytes::from(
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

pub fn u32_bytes(values: &[u32]) -> Bytes {
    Bytes::from(
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

pub fn read_f32s(data: &[u8]) -> Vec<f32> {
    data.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

pub fn read_u32s(data: &[u8]) -> Vec<u32> {
    data.chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

pub fn assert_f32_close(got: f32, expected: f32, label: &str) {
    let abs = (got - expected).abs();
    let rel = if expected.abs() > f32::EPSILON {
        abs / expected.abs()
    } else {
        abs
    };
    assert!(
        abs < TOLERANCE as f32 || rel < TOLERANCE as f32,
        "{label}: got {got}, expected {expected} (abs={abs}, rel={rel})"
    );
}

pub fn make_request(
    wgsl: &str,
    workgroups: [u32; 3],
    bindings: Vec<BindingData>,
) -> ExecuteCpuRequest {
    ExecuteCpuRequest {
        wgsl_source: wgsl.into(),
        entry_point: None,
        workgroups,
        bindings,
        uniforms: vec![],
        strategy: coral_reef_cpu::types::ExecutionStrategy::Jit,
    }
}

pub fn rw_f32_binding(group: u32, binding: u32, data: &[f32]) -> BindingData {
    BindingData {
        group,
        binding,
        data: f32_bytes(data),
        usage: BindingUsage::ReadWrite,
    }
}

pub fn ro_f32_binding(group: u32, binding: u32, data: &[f32]) -> BindingData {
    BindingData {
        group,
        binding,
        data: f32_bytes(data),
        usage: BindingUsage::ReadOnly,
    }
}

pub fn rw_u32_binding(group: u32, binding: u32, data: &[u32]) -> BindingData {
    BindingData {
        group,
        binding,
        data: u32_bytes(data),
        usage: BindingUsage::ReadWrite,
    }
}

pub fn rw_zero_bytes(group: u32, binding: u32, len: usize) -> BindingData {
    BindingData {
        group,
        binding,
        data: Bytes::from(vec![0u8; len]),
        usage: BindingUsage::ReadWrite,
    }
}

pub fn assert_triple_path_f32(
    request: &ExecuteCpuRequest,
    binding_idx: usize,
    expected: &[f32],
    label: &str,
) {
    let jit_resp = execute_jit(request).unwrap_or_else(|e| panic!("{label}: JIT failed: {e}"));
    let jit_vals = read_f32s(&jit_resp.bindings[binding_idx].data);
    for (i, &exp) in expected.iter().enumerate() {
        assert_f32_close(jit_vals[i], exp, &format!("{label} JIT[{i}]"));
    }

    let coral_resp = coral_reef_cpu::execute_coral_ir(request)
        .unwrap_or_else(|e| panic!("{label}: CoralIR interp failed: {e}"));
    let coral_vals = read_f32s(&coral_resp.bindings[binding_idx].data);
    for (i, (&jit, &coral)) in jit_vals.iter().zip(coral_vals.iter()).enumerate() {
        assert_f32_close(jit, coral, &format!("{label} JIT↔CoralIR[{i}]"));
    }

    if let Ok(naga_resp) = coral_reef_cpu::execute_cpu(request) {
        if naga_resp.bindings.len() > binding_idx {
            let naga_vals = read_f32s(&naga_resp.bindings[binding_idx].data);
            let mut naga_ok = true;
            for (&jit, &naga) in jit_vals.iter().zip(naga_vals.iter()) {
                let abs = (jit - naga).abs();
                let rel = if naga.abs() > f32::EPSILON {
                    abs / naga.abs()
                } else {
                    abs
                };
                if abs >= TOLERANCE as f32 && rel >= TOLERANCE as f32 {
                    naga_ok = false;
                    break;
                }
            }
            if !naga_ok {
                eprintln!("{label}: Naga best-effort mismatch (non-fatal)");
            }
        }
    }
}

pub fn assert_triple_path_u32(
    request: &ExecuteCpuRequest,
    binding_idx: usize,
    expected: &[u32],
    label: &str,
) {
    let jit_resp = execute_jit(request).unwrap_or_else(|e| panic!("{label}: JIT failed: {e}"));
    let jit_vals = read_u32s(&jit_resp.bindings[binding_idx].data);
    assert_eq!(jit_vals, expected, "{label} JIT output");

    let coral_resp = coral_reef_cpu::execute_coral_ir(request)
        .unwrap_or_else(|e| panic!("{label}: CoralIR interp failed: {e}"));
    let coral_vals = read_u32s(&coral_resp.bindings[binding_idx].data);
    assert_eq!(jit_vals, coral_vals, "{label} JIT↔CoralIR");

    if let Ok(naga_resp) = coral_reef_cpu::execute_cpu(request) {
        if naga_resp.bindings.len() > binding_idx {
            let naga_vals = read_u32s(&naga_resp.bindings[binding_idx].data);
            if jit_vals != naga_vals {
                eprintln!("{label}: Naga best-effort mismatch (non-fatal)");
            }
        }
    }
}
