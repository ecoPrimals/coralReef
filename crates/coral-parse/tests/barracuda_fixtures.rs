// SPDX-License-Identifier: AGPL-3.0-only
//! Integration tests: parse + lower all barracuda WGSL fixtures through the
//! sovereign `CoralFrontend`, validating that every compute shader in the
//! corpus produces a valid `Shader` with at least one function.

use coral_parse::CoralFrontend;
use coral_reef::codegen::nv::sm70::ShaderModel70;
use coral_reef::Frontend;

const FIXTURES_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../coral-reef-jit/tests/fixtures/barracuda"
);

fn compile_fixture(name: &str) {
    let path = format!("{FIXTURES_DIR}/{name}");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    let sm = ShaderModel70::new(70);
    let frontend = CoralFrontend;
    let shader = frontend
        .compile_wgsl(&source, &sm)
        .unwrap_or_else(|e| panic!("compile_wgsl failed for {name}: {e}"));
    assert!(
        !shader.functions.is_empty(),
        "{name}: expected at least one function in compiled shader"
    );
}

#[test]
fn elementwise_add() {
    compile_fixture("elementwise_add.wgsl");
}

#[test]
fn elementwise_sub() {
    compile_fixture("elementwise_sub.wgsl");
}

#[test]
fn elementwise_mul() {
    compile_fixture("elementwise_mul.wgsl");
}

#[test]
fn elementwise_fma() {
    compile_fixture("elementwise_fma.wgsl");
}

#[test]
fn relu() {
    compile_fixture("relu.wgsl");
}

#[test]
fn leaky_relu() {
    compile_fixture("leaky_relu.wgsl");
}

#[test]
fn elu() {
    compile_fixture("elu.wgsl");
}

#[test]
fn silu() {
    compile_fixture("silu.wgsl");
}

#[test]
fn sigmoid() {
    compile_fixture("sigmoid.wgsl");
}

#[test]
fn hardsigmoid() {
    compile_fixture("hardsigmoid.wgsl");
}

#[test]
fn hardtanh() {
    compile_fixture("hardtanh.wgsl");
}

#[test]
fn abs() {
    compile_fixture("abs.wgsl");
}

#[test]
fn sign() {
    compile_fixture("sign.wgsl");
}

#[test]
fn sqrt() {
    compile_fixture("sqrt.wgsl");
}

#[test]
fn scalar_sum_reduce() {
    compile_fixture("scalar_sum_reduce.wgsl");
}

#[test]
fn scalar_mean() {
    compile_fixture("scalar_mean.wgsl");
}

#[test]
fn scalar_variance() {
    compile_fixture("scalar_variance.wgsl");
}

#[test]
fn scalar_dot_product() {
    compile_fixture("scalar_dot_product.wgsl");
}

#[test]
fn sum_reduce_workgroup() {
    compile_fixture("sum_reduce_workgroup.wgsl");
}

#[test]
fn max_reduce_workgroup() {
    compile_fixture("max_reduce_workgroup.wgsl");
}

#[test]
fn layer_norm() {
    compile_fixture("layer_norm.wgsl");
}

#[test]
fn tiled_matmul_2x2() {
    compile_fixture("tiled_matmul_2x2.wgsl");
}
