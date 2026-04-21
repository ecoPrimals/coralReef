// SPDX-License-Identifier: AGPL-3.0-or-later
//! Full sovereign E2E pipeline test: WGSL → coral-reef compile → coral-driver dispatch → readback.
//!
//! Exercises the complete stack on real hardware. Requires an NVIDIA GPU with
//! `nouveau` loaded and the `hw-e2e` feature enabled.
//!
//! Run: `cargo test --test hw_sovereign_e2e --features hw-e2e -- --ignored`

#[cfg(feature = "hw-e2e")]
mod tests {
    use coral_driver::nv::NvDevice;
    use coral_driver::{ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};
    use coral_reef::{CompileOptions, FmaPolicy, GpuTarget, NvArch};

    fn open_nv() -> NvDevice {
        NvDevice::open().expect("NvDevice::open() — is nouveau loaded?")
    }

    fn arch_for_sm(sm: u32) -> NvArch {
        match sm {
            100.. => NvArch::Sm120,
            89.. => NvArch::Sm89,
            86..=88 => NvArch::Sm86,
            80..=85 => NvArch::Sm80,
            75..=79 => NvArch::Sm75,
            _ => NvArch::Sm70,
        }
    }

    fn compile(sm: u32, wgsl: &str) -> coral_reef::backend::CompiledBinary {
        let opts = CompileOptions {
            target: GpuTarget::Nvidia(arch_for_sm(sm)),
            opt_level: 2,
            debug_info: false,
            fp64_software: false,
            fma_policy: FmaPolicy::Fused,
            ..CompileOptions::default()
        };
        coral_reef::compile_wgsl_full(wgsl, &opts)
            .unwrap_or_else(|e| panic!("SM{sm} compilation failed: {e}"))
    }

    fn dispatch_and_readback(
        dev: &mut NvDevice,
        compiled: &coral_reef::backend::CompiledBinary,
        bufs: &[coral_driver::BufferHandle],
        grid: DispatchDims,
    ) {
        let info = ShaderInfo {
            gpr_count: compiled.info.gpr_count,
            shared_mem_bytes: compiled.info.shared_mem_bytes,
            barrier_count: compiled.info.barrier_count,
            workgroup: compiled.info.local_size,
            wave_size: 32,
            local_mem_bytes: None,
        };
        dev.dispatch(&compiled.binary, bufs, grid, &info)
            .expect("dispatch");
        dev.sync().expect("sync");
    }

    // -- Single-binding: write 42 --

    const WRITE_42: &str = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = 42u;
}
";

    #[test]
    #[ignore = "requires nouveau hardware"]
    fn e2e_write_42() {
        let mut dev = open_nv();
        let sm = dev.sm_version();
        let compiled = compile(sm, WRITE_42);

        let n = 64u64;
        let buf = dev.alloc(n * 4, MemoryDomain::Gtt).expect("alloc");
        dev.upload(buf, 0, &vec![0u8; (n * 4) as usize])
            .expect("zero");

        dispatch_and_readback(&mut dev, &compiled, &[buf], DispatchDims::linear(1));

        let data = dev.readback(buf, 0, (n * 4) as usize).expect("readback");
        for i in 0..n as usize {
            let val = u32::from_le_bytes(data[i * 4..(i + 1) * 4].try_into().unwrap());
            assert_eq!(val, 42, "element {i}: expected 42, got {val}");
        }
        dev.free(buf).expect("free");
    }

    // -- Multi-binding: copy A → B --

    const COPY_AB: &str = r"
@group(0) @binding(0)
var<storage, read> src: array<u32>;

@group(0) @binding(1)
var<storage, read_write> dst: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    dst[gid.x] = src[gid.x];
}
";

    #[test]
    #[ignore = "requires nouveau hardware"]
    fn e2e_multi_binding_copy() {
        let mut dev = open_nv();
        let sm = dev.sm_version();
        let compiled = compile(sm, COPY_AB);

        let n = 64u64;
        let byte_size = n * 4;

        let src_data: Vec<u8> = (0..n as u32)
            .flat_map(|v| v.to_le_bytes())
            .collect();

        let src = dev.alloc(byte_size, MemoryDomain::Gtt).expect("alloc src");
        let dst = dev.alloc(byte_size, MemoryDomain::Gtt).expect("alloc dst");
        dev.upload(src, 0, &src_data).expect("upload src");
        dev.upload(dst, 0, &vec![0u8; byte_size as usize]).expect("zero dst");

        dispatch_and_readback(&mut dev, &compiled, &[src, dst], DispatchDims::linear(1));

        let readback = dev.readback(dst, 0, byte_size as usize).expect("readback");
        for i in 0..n as usize {
            let got = u32::from_le_bytes(readback[i * 4..(i + 1) * 4].try_into().unwrap());
            assert_eq!(got, i as u32, "element {i}: expected {i}, got {got}");
        }
        dev.free(src).expect("free src");
        dev.free(dst).expect("free dst");
    }

    // -- arrayLength: runtime buffer size query --

    const ARRAY_LENGTH_SHADER: &str = r"
@group(0) @binding(0)
var<storage, read_write> buf: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let len = arrayLength(&buf);
    buf[0] = len;
}
";

    #[test]
    #[ignore = "requires nouveau hardware"]
    fn e2e_array_length() {
        let mut dev = open_nv();
        let sm = dev.sm_version();
        let compiled = compile(sm, ARRAY_LENGTH_SHADER);

        let n_elements = 256u64;
        let byte_size = n_elements * 4;
        let buf = dev.alloc(byte_size, MemoryDomain::Gtt).expect("alloc");
        dev.upload(buf, 0, &vec![0u8; byte_size as usize]).expect("zero");

        dispatch_and_readback(&mut dev, &compiled, &[buf], DispatchDims::linear(1));

        let readback = dev.readback(buf, 0, 4).expect("readback");
        let len = u32::from_le_bytes(readback[..4].try_into().unwrap());
        assert_eq!(
            len, n_elements as u32,
            "arrayLength: expected {n_elements}, got {len}"
        );
        dev.free(buf).expect("free");
    }

    // -- Multi-binding arrayLength: the test case for the 16-byte stride fix --

    const MULTI_ARRAY_LENGTH: &str = r"
@group(0) @binding(0)
var<storage, read> a: array<u32>;

@group(0) @binding(1)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let len_a = arrayLength(&a);
    let len_out = arrayLength(&out);
    out[0] = len_a;
    out[1] = len_out;
}
";

    #[test]
    #[ignore = "requires nouveau hardware"]
    fn e2e_multi_binding_array_length() {
        let mut dev = open_nv();
        let sm = dev.sm_version();
        let compiled = compile(sm, MULTI_ARRAY_LENGTH);

        let a_elems = 128u64;
        let out_elems = 64u64;
        let a = dev.alloc(a_elems * 4, MemoryDomain::Gtt).expect("alloc a");
        let out = dev.alloc(out_elems * 4, MemoryDomain::Gtt).expect("alloc out");
        dev.upload(a, 0, &vec![0u8; (a_elems * 4) as usize]).expect("zero a");
        dev.upload(out, 0, &vec![0u8; (out_elems * 4) as usize]).expect("zero out");

        dispatch_and_readback(&mut dev, &compiled, &[a, out], DispatchDims::linear(1));

        let readback = dev.readback(out, 0, 8).expect("readback");
        let len_a = u32::from_le_bytes(readback[..4].try_into().unwrap());
        let len_out = u32::from_le_bytes(readback[4..8].try_into().unwrap());
        assert_eq!(
            len_a, a_elems as u32,
            "arrayLength(&a): expected {a_elems}, got {len_a}"
        );
        assert_eq!(
            len_out, out_elems as u32,
            "arrayLength(&out): expected {out_elems}, got {len_out}"
        );
        dev.free(a).expect("free a");
        dev.free(out).expect("free out");
    }

    // -- num_workgroups builtin --

    const NUM_WORKGROUPS_SHADER: &str = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main(@builtin(num_workgroups) nwg: vec3<u32>) {
    out[0] = nwg.x;
    out[1] = nwg.y;
    out[2] = nwg.z;
}
";

    #[test]
    #[ignore = "requires nouveau hardware"]
    fn e2e_num_workgroups() {
        let mut dev = open_nv();
        let sm = dev.sm_version();
        let compiled = compile(sm, NUM_WORKGROUPS_SHADER);

        let buf = dev.alloc(12, MemoryDomain::Gtt).expect("alloc");
        dev.upload(buf, 0, &[0u8; 12]).expect("zero");

        let grid = DispatchDims { x: 7, y: 3, z: 2 };
        dispatch_and_readback(&mut dev, &compiled, &[buf], grid);

        let readback = dev.readback(buf, 0, 12).expect("readback");
        let x = u32::from_le_bytes(readback[0..4].try_into().unwrap());
        let y = u32::from_le_bytes(readback[4..8].try_into().unwrap());
        let z = u32::from_le_bytes(readback[8..12].try_into().unwrap());
        assert_eq!((x, y, z), (7, 3, 2), "num_workgroups: expected (7,3,2), got ({x},{y},{z})");
        dev.free(buf).expect("free");
    }
}
