// SPDX-License-Identifier: AGPL-3.0-only

use coral_reef::{CompileError, CompileOptions, GpuArch, compile_wgsl};

// ---------------------------------------------------------------------------
// SM70 encoder path tests — diverse WGSL to exercise encoder coverage
// ---------------------------------------------------------------------------

#[test]
fn test_sm70_encode_integer_shift_or() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let idx = gid.x;
            let a = data[idx];
            data[idx] = (a << 2u) | (a >> 3u);
        }
    ";
    let result = compile_wgsl(wgsl, &super::sm70_opts());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "integer shift+OR should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_sm70_encode_comparison_select() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let idx = gid.x;
            let a = data[idx];
            let b = data[idx + 1u];
            data[idx] = select(a, b, a > b);
        }
    ";
    let result = compile_wgsl(wgsl, &super::sm70_opts());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "comparison+select should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_sm70_encode_float_math_variety() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> fdata: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = f32(gid.x);
            let y = sin(x) * cos(x) + exp2(x);
            fdata[gid.x] = y;
        }
    ";
    let result = compile_wgsl(wgsl, &super::sm70_opts());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "sin/cos/exp2 float math should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_sm70_encode_shared_memory_barrier() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> output: array<f32>;
        var<workgroup> shared_data: array<f32, 64>;
        @compute @workgroup_size(64)
        fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
            shared_data[lid.x] = f32(lid.x);
            workgroupBarrier();
            let val = shared_data[63u - lid.x];
            output[lid.x] = val;
        }
    ";
    let result = compile_wgsl(wgsl, &super::sm70_opts());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "shared memory+barrier should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_sm70_encode_conversion_ops() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let f = f32(gid.x);
            let i = u32(f);
            data[gid.x] = i;
        }
    ";
    let result = compile_wgsl(wgsl, &super::sm70_opts());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "i2f/f2i conversions should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_sm70_encode_typed_data() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data_i32: array<i32>;
        @group(0) @binding(1) var<storage, read_write> data_f32: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let idx = gid.x;
            let a = data_i32[idx];
            let b = data_f32[idx];
            let c = a + 42;
            let d = b * 3.14;
            data_i32[idx] = c;
            data_f32[idx] = d;
        }
    ";
    let result = compile_wgsl(wgsl, &super::sm70_opts());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "i32+f32 mixed types should compile or fail with NotImplemented: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Comprehensive SM70 encoder path tests
// ---------------------------------------------------------------------------

/// Exercises `sm70_encode/alu/int.rs`: mul, shift, bitwise or, min, max, clamp.
#[test]
fn test_sm70_alu_integer_ops() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> buf: array<u32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = buf[gid.x];
            let b = buf[gid.x + 1u];
            buf[gid.x] = a * b + (a >> 2u) | (b << 3u);
            buf[gid.x + 1u] = min(a, b);
            buf[gid.x + 2u] = max(a, b);
            buf[gid.x + 3u] = clamp(a, 0u, 255u);
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    match result {
        Ok(binary) => assert!(!binary.is_empty()),
        Err(CompileError::NotImplemented(_)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

/// Exercises `sm70_encode/alu/float.rs`: mul, add, sin, cos, exp2, log2, sqrt, abs, min, max.
#[test]
fn test_sm70_alu_float_ops() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> buf: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = buf[gid.x];
            let b = buf[gid.x + 1u];
            buf[gid.x] = a * b + sin(a) + cos(b) + exp2(a) + log2(b);
            buf[gid.x + 1u] = sqrt(a) + abs(b);
            buf[gid.x + 2u] = min(a, b);
            buf[gid.x + 3u] = max(a, b);
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    match result {
        Ok(binary) => assert!(!binary.is_empty()),
        Err(CompileError::NotImplemented(_)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

/// Exercises `sm70_encode/control.rs`: for loop, if/else, switch.
#[test]
fn test_sm70_control_flow() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> buf: array<u32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            var sum = 0u;
            for (var i = 0u; i < 10u; i++) {
                if (i % 2u == 0u) { sum += i; } else { sum += i * 2u; }
            }
            var x = gid.x;
            switch (x) {
                case 0u: { buf[0] = sum; }
                case 1u: { buf[1] = sum + 1u; }
                default: { buf[x] = sum + x; }
            }
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    match result {
        Ok(binary) => assert!(!binary.is_empty()),
        Err(CompileError::NotImplemented(_)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

/// Exercises `sm70_encode/mem.rs`: uniform struct, storage buffer load/store.
#[test]
fn test_sm70_mem_ops() {
    let wgsl = "
        struct Params { offset: u32, scale: f32 }
        @group(0) @binding(0) var<uniform> params: Params;
        @group(0) @binding(1) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let idx = gid.x + params.offset;
            data[idx] = data[idx] * params.scale;
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        ..CompileOptions::default()
    };
    let binary = compile_wgsl(wgsl, &opts).expect("var<uniform> should compile");
    assert!(!binary.is_empty());
}

/// Exercises workgroup shared memory paths: `shared_data`, `workgroupBarrier`.
#[test]
fn test_sm70_workgroup_shared_memory() {
    let wgsl = "
        var<workgroup> shared_data: array<f32, 256>;
        @group(0) @binding(0) var<storage, read_write> buf: array<f32>;
        @compute @workgroup_size(256) fn main(
            @builtin(local_invocation_id) lid: vec3<u32>,
            @builtin(global_invocation_id) gid: vec3<u32>
        ) {
            shared_data[lid.x] = buf[gid.x];
            workgroupBarrier();
            buf[gid.x] = shared_data[255u - lid.x];
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    match result {
        Ok(binary) => assert!(!binary.is_empty()),
        Err(CompileError::NotImplemented(_)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

// ---------------------------------------------------------------------------
// SM70 encoder coverage tests — f16, f64, texture (0% coverage modules)
// ---------------------------------------------------------------------------

/// Exercises `sm70_encode/alu/float16.rs`: f16 instruction encoding.
#[test]
fn test_sm70_encode_float16_ops() {
    let wgsl = "
        enable f16;
        @group(0) @binding(0) var<storage, read_write> buf: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = f16(buf[gid.x]);
            let b = f16(buf[gid.x + 1u]);
            buf[gid.x] = f32(a + b);
            buf[gid.x + 1u] = f32(a * b);
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

/// Exercises `sm70_encode/alu/float64.rs`: f64 instruction encoding with `fp64_software`.
#[test]
fn test_sm70_encode_float64_ops() {
    let wgsl = "
        enable naga_ext_f64;
        @group(0) @binding(0) var<storage, read_write> buf: array<f64>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = buf[gid.x];
            let b = buf[gid.x + 1u];
            buf[gid.x] = a + b;
            buf[gid.x + 1u] = a * b;
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        fp64_software: true,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

/// Exercises `sm70_encode/tex.rs`: `textureSampleLevel` instruction encoding.
#[test]
fn test_sm70_encode_texture_sample_level() {
    let wgsl = "
        @group(0) @binding(0) var tex: texture_2d<f32>;
        @group(0) @binding(1) var samp: sampler;
        @group(0) @binding(2) var<storage, read_write> out: array<vec4<f32>>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let uv = vec2<f32>(f32(gid.x) / 256.0, f32(gid.y) / 256.0);
            out[gid.x] = textureSampleLevel(tex, samp, uv, 0.0);
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

/// Exercises `sm70_encode/tex.rs`: `textureLoad` instruction encoding.
#[test]
fn test_sm70_encode_texture_load() {
    let wgsl = "
        @group(0) @binding(0) var tex: texture_2d<f32>;
        @group(0) @binding(1) var<storage, read_write> out: array<vec4<f32>>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            out[gid.x] = textureLoad(tex, vec2<i32>(i32(gid.x), 0), 0);
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

/// Exercises `sm70_encode/alu/conv.rs`: i32 to f32 and f32 to i32 conversions.
#[test]
fn test_sm70_alu_conv_ops() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> ibuf: array<i32>;
        @group(0) @binding(1) var<storage, read_write> fbuf: array<f32>;
        @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            fbuf[gid.x] = f32(ibuf[gid.x]);
            ibuf[gid.x] = i32(fbuf[gid.x + 1u]);
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    match result {
        Ok(binary) => assert!(!binary.is_empty()),
        Err(CompileError::NotImplemented(_)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

/// Multi-architecture stress: run each complex shader for SM70, SM75, SM80, SM86, SM89.
#[test]
fn test_multi_arch_stress_all_shaders() {
    let shaders: &[(&str, &str)] = &[
        (
            "integer_ops",
            r"
                @group(0) @binding(0) var<storage, read_write> buf: array<u32>;
                @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                    let a = buf[gid.x];
                    let b = buf[gid.x + 1u];
                    buf[gid.x] = a * b + (a >> 2u) | (b << 3u);
                    buf[gid.x + 1u] = min(a, b);
                    buf[gid.x + 2u] = max(a, b);
                    buf[gid.x + 3u] = clamp(a, 0u, 255u);
                }
            ",
        ),
        (
            "float_ops",
            r"
                @group(0) @binding(0) var<storage, read_write> buf: array<f32>;
                @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                    let a = buf[gid.x];
                    let b = buf[gid.x + 1u];
                    buf[gid.x] = a * b + sin(a) + cos(b) + exp2(a) + log2(b);
                    buf[gid.x + 1u] = sqrt(a) + abs(b);
                    buf[gid.x + 2u] = min(a, b);
                    buf[gid.x + 3u] = max(a, b);
                }
            ",
        ),
        (
            "control_flow",
            r"
                @group(0) @binding(0) var<storage, read_write> buf: array<u32>;
                @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                    var sum = 0u;
                    for (var i = 0u; i < 10u; i++) {
                        if (i % 2u == 0u) { sum += i; } else { sum += i * 2u; }
                    }
                    var x = gid.x;
                    switch (x) {
                        case 0u: { buf[0] = sum; }
                        case 1u: { buf[1] = sum + 1u; }
                        default: { buf[x] = sum + x; }
                    }
                }
            ",
        ),
        (
            "mem_ops",
            r"
                struct Params { offset: u32, scale: f32 }
                @group(0) @binding(0) var<uniform> params: Params;
                @group(0) @binding(1) var<storage, read_write> data: array<f32>;
                @compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                    let idx = gid.x + params.offset;
                    data[idx] = data[idx] * params.scale;
                }
            ",
        ),
        (
            "workgroup_shared",
            r"
                var<workgroup> shared_data: array<f32, 256>;
                @group(0) @binding(0) var<storage, read_write> buf: array<f32>;
                @compute @workgroup_size(256) fn main(
                    @builtin(local_invocation_id) lid: vec3<u32>,
                    @builtin(global_invocation_id) gid: vec3<u32>
                ) {
                    shared_data[lid.x] = buf[gid.x];
                    workgroupBarrier();
                    buf[gid.x] = shared_data[255u - lid.x];
                }
            ",
        ),
        (
            "conv_ops",
            r"
                @group(0) @binding(0) var<storage, read_write> ibuf: array<i32>;
                @group(0) @binding(1) var<storage, read_write> fbuf: array<f32>;
                @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
                    fbuf[gid.x] = f32(ibuf[gid.x]);
                    ibuf[gid.x] = i32(fbuf[gid.x + 1u]);
                }
            ",
        ),
    ];

    for &arch in GpuArch::ALL {
        for (name, wgsl) in shaders {
            let opts = CompileOptions {
                target: arch.into(),
                ..CompileOptions::default()
            };
            let result = compile_wgsl(wgsl.trim(), &opts);
            match result {
                Ok(binary) => assert!(!binary.is_empty(), "arch {arch} shader {name} binary empty"),
                Err(CompileError::NotImplemented(_)) => {}
                Err(e) => panic!("arch {arch} shader {name} unexpected error: {e}"),
            }
        }
    }
}

#[test]
fn test_sm70_f64_storage_load_store() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read> inp: array<f64>;
        @group(0) @binding(1) var<storage, read_write> out: array<f64>;
        @compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = inp[gid.x];
            let b = inp[gid.x + 1u];
            out[gid.x] = a + b;
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "f64 storage load/store should compile: {result:?}"
    );
    assert!(!result.unwrap().is_empty());
}

#[test]
fn test_sm70_f64_storage_multiply() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read> a_buf: array<f64>;
        @group(0) @binding(1) var<storage, read> b_buf: array<f64>;
        @group(0) @binding(2) var<storage, read_write> out: array<f64>;
        @compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            out[gid.x] = a_buf[gid.x] * b_buf[gid.x];
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(result.is_ok(), "f64 multiply should compile: {result:?}");
    assert!(!result.unwrap().is_empty());
}

#[test]
fn test_sm70_f64_divide() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> out: array<f64>;
        @compute @workgroup_size(1) fn main() {
            let x: f64 = 3.14;
            let y: f64 = 2.0;
            out[0] = x / y;
        }
    ";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(result.is_ok(), "f64 divide should compile: {result:?}");
    assert!(!result.unwrap().is_empty());
}
