// SPDX-License-Identifier: AGPL-3.0-or-later
//! NVIDIA DRM device tests — probe, alloc, dispatch via UVM.
//!
//! Run: `cargo test --test hw_nv_buffers --features nvidia-drm -- --ignored`

#[cfg(feature = "nvidia-drm")]
mod tests {
    use coral_driver::ComputeDevice;
    use coral_driver::nv::NvDrmDevice;

    fn open_nv() -> NvDrmDevice {
        NvDrmDevice::open().expect("NvDrmDevice::open() failed — is nvidia-drm loaded?")
    }

    /// Compile PTX source to SASS binary via ptxas, returning the raw
    /// machine code from the cubin ELF's .text section.
    fn ptx_to_sass(ptx: &[u8], sm: u32) -> Vec<u8> {
        use std::io::Write;
        let dir = std::env::temp_dir().join("coral_ptxas");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let ptx_path = dir.join("kernel.ptx");
        let cubin_path = dir.join("kernel.cubin");
        std::fs::File::create(&ptx_path)
            .and_then(|mut f| f.write_all(ptx))
            .expect("write ptx");
        let ptxas = std::env::var("PTXAS")
            .unwrap_or_else(|_| "/usr/local/cuda/bin/ptxas".to_string());
        let output = std::process::Command::new(&ptxas)
            .args([
                &format!("--gpu-name=sm_{sm}"),
                "-o",
                cubin_path.to_str().unwrap(),
                ptx_path.to_str().unwrap(),
            ])
            .output()
            .unwrap_or_else(|e| panic!("ptxas launch failed: {e}"));
        if !output.status.success() {
            panic!(
                "ptxas failed (exit {}):\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let cubin = std::fs::read(&cubin_path).expect("read cubin");
        extract_sass_from_cubin(&cubin)
    }

    /// Extract the kernel's SASS code from a cubin ELF.
    /// Returns the content of the first `.text.*` or `.text` section.
    fn extract_sass_from_cubin(cubin: &[u8]) -> Vec<u8> {
        assert!(cubin.len() > 64, "cubin too small for ELF");
        assert_eq!(&cubin[..4], b"\x7fELF", "not an ELF file");
        let is_64 = cubin[4] == 2;
        assert!(is_64, "expected 64-bit ELF");

        let e_shoff = u64::from_le_bytes(cubin[40..48].try_into().unwrap()) as usize;
        let e_shentsize = u16::from_le_bytes(cubin[58..60].try_into().unwrap()) as usize;
        let e_shnum = u16::from_le_bytes(cubin[60..62].try_into().unwrap()) as usize;
        let e_shstrndx = u16::from_le_bytes(cubin[62..64].try_into().unwrap()) as usize;

        let shstrtab_off = e_shoff + e_shstrndx * e_shentsize;
        let shstrtab_offset =
            u64::from_le_bytes(cubin[shstrtab_off + 24..shstrtab_off + 32].try_into().unwrap())
                as usize;
        let shstrtab_size =
            u64::from_le_bytes(cubin[shstrtab_off + 32..shstrtab_off + 40].try_into().unwrap())
                as usize;
        let shstrtab = &cubin[shstrtab_offset..shstrtab_offset + shstrtab_size];

        for i in 0..e_shnum {
            let sh = e_shoff + i * e_shentsize;
            let sh_name_idx =
                u32::from_le_bytes(cubin[sh..sh + 4].try_into().unwrap()) as usize;
            let sh_type = u32::from_le_bytes(cubin[sh + 4..sh + 8].try_into().unwrap());
            let sh_offset =
                u64::from_le_bytes(cubin[sh + 24..sh + 32].try_into().unwrap()) as usize;
            let sh_size =
                u64::from_le_bytes(cubin[sh + 32..sh + 40].try_into().unwrap()) as usize;

            if sh_type != 1 {
                continue; // SHT_PROGBITS = 1
            }

            let name_end = shstrtab[sh_name_idx..]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(0);
            let name = std::str::from_utf8(&shstrtab[sh_name_idx..sh_name_idx + name_end])
                .unwrap_or("");

            if name.starts_with(".text") {
                eprintln!(
                    "ptxas: extracted section '{name}' ({sh_size} bytes) from cubin"
                );
                return cubin[sh_offset..sh_offset + sh_size].to_vec();
            }
        }
        panic!("no .text section found in cubin ELF");
    }

    #[test]
    #[ignore = "requires nvidia-drm hardware"]
    fn device_opens_successfully() {
        let dev = open_nv();
        assert!(dev.path().contains("renderD"));
        let name = dev.driver_name().expect("driver_name");
        assert_eq!(name, "nvidia-drm");
    }

    #[test]
    #[ignore = "requires nvidia-drm hardware"]
    fn alloc_and_free() {
        let mut dev = open_nv();
        let handle = dev
            .alloc(4096, coral_driver::MemoryDomain::Gtt)
            .expect("alloc should succeed via UVM");
        dev.free(handle).expect("free should succeed");
    }

    #[test]
    #[ignore = "requires nvidia-drm hardware"]
    fn sync_succeeds() {
        let mut dev = open_nv();
        dev.sync().expect("sync should succeed");
    }

    #[test]
    #[ignore = "requires nvidia-drm hardware"]
    fn dispatch_write_42() {
        use coral_driver::{DispatchDims, MemoryDomain, ShaderInfo};

        let mut dev = open_nv();
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    out[0] = 42u;
}
";
        let arch = coral_reef::NvArch::Sm120;
        let opts = coral_reef::CompileOptions {
            target: coral_reef::GpuTarget::Nvidia(arch),
            opt_level: 2,
            ..Default::default()
        };
        let compiled = coral_reef::compile_wgsl_full(wgsl, &opts)
            .expect("WGSL compilation should succeed");
        eprintln!(
            "Compiled: {} bytes, {} GPRs, format={:?}",
            compiled.binary.len(),
            compiled.info.gpr_count,
            compiled.format,
        );

        let shader_binary = if compiled.format == coral_reef::BinaryFormat::Ptx {
            eprintln!("PTX detected — compiling to SASS via ptxas...");
            ptx_to_sass(&compiled.binary, 120)
        } else {
            compiled.binary.clone()
        };
        eprintln!("SASS binary: {} bytes", shader_binary.len());

        let buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");
        dev.upload(buf, 0, &0xDEAD_BEEFu32.to_le_bytes()).expect("sentinel");

        let pre = dev.readback(buf, 0, 4).expect("pre-readback");
        let pre_val = u32::from_le_bytes(pre[..4].try_into().unwrap());
        eprintln!("pre-dispatch readback: 0x{pre_val:08X} (expect 0xDEADBEEF)");
        assert_eq!(pre_val, 0xDEAD_BEEF, "buffer mapping broken: upload/readback mismatch");

        dev.upload(buf, 0, &0xBAAD_F00Du32.to_le_bytes()).expect("sentinel2");

        eprintln!("SASS binary ({} bytes):", shader_binary.len());
        for (i, chunk) in shader_binary.chunks(16).enumerate().take(6) {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            eprintln!("  +{:03x}: {}", i * 16, hex.join(" "));
        }
        if shader_binary.len() > 96 {
            eprintln!("  ... ({} more bytes)", shader_binary.len() - 96);
        }

        let info = ShaderInfo {
            gpr_count: compiled.info.gpr_count.max(8),
            shared_mem_bytes: compiled.info.shared_mem_bytes,
            barrier_count: compiled.info.barrier_count,
            workgroup: compiled.info.local_size,
            wave_size: 32,
            local_mem_bytes: None,
        };
        dev.dispatch(&shader_binary, &[buf], DispatchDims::linear(1), &info)
            .expect("dispatch should succeed");
        dev.sync().expect("sync should succeed");

        std::thread::sleep(std::time::Duration::from_millis(50));

        let data = dev.readback(buf, 0, 4).expect("readback");
        let val = u32::from_le_bytes(data[..4].try_into().unwrap());
        eprintln!("post-dispatch readback: 0x{val:08X} (expect 0x0000002A = 42)");
        assert_eq!(val, 42, "expected 42, got {val}");
        eprintln!("dispatch_write_42: GPU wrote 42 — sovereign compute verified!");
        dev.free(buf).expect("free");
    }

    /// Dispatch test using a handcrafted minimal PTX kernel to isolate
    /// whether the dispatch infrastructure works independently of coral-reef.
    ///
    /// The PTX kernel: load buffer pointer from param, store 42, return.
    /// Uses the standard CUDA parameter convention (c[0][0x160]).
    #[test]
    #[ignore = "requires nvidia-drm hardware"]
    fn dispatch_handcrafted_ptx() {
        use coral_driver::{DispatchDims, MemoryDomain, ShaderInfo};

        let mut dev = open_nv();

        let ptx = b"\
.version 8.7\n\
.target sm_120\n\
.address_size 64\n\
\n\
.visible .entry main_kernel(\n\
    .param .u64 _buf0_ptr,\n\
    .param .u64 _buf0_size\n\
)\n\
{\n\
    .reg .b64 %rd<2>;\n\
    .reg .b32 %r<1>;\n\
    ld.param.u64 %rd0, [_buf0_ptr];\n\
    ld.param.u64 %rd1, [_buf0_size];\n\
    mov.u32 %r0, 42;\n\
    st.global.u32 [%rd0], %r0;\n\
    ret;\n\
}\n";

        let sass = ptx_to_sass(ptx, 120);
        eprintln!("Handcrafted SASS: {} bytes", sass.len());
        for (i, chunk) in sass.chunks(16).enumerate().take(6) {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            eprintln!("  +{:03x}: {}", i * 16, hex.join(" "));
        }

        let buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");
        dev.upload(buf, 0, &0xBAAD_F00Du32.to_le_bytes())
            .expect("sentinel");

        let pre = dev.readback(buf, 0, 4).expect("pre-readback");
        let pre_val = u32::from_le_bytes(pre[..4].try_into().unwrap());
        eprintln!("pre-dispatch readback: 0x{pre_val:08X}");

        let info = ShaderInfo {
            gpr_count: 8,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [1, 1, 1],
            wave_size: 32,
            local_mem_bytes: None,
        };
        dev.dispatch(&sass, &[buf], DispatchDims::linear(1), &info)
            .expect("dispatch should succeed");
        dev.sync().expect("sync should succeed");

        std::thread::sleep(std::time::Duration::from_millis(50));

        let data = dev.readback(buf, 0, 4).expect("readback");
        let val = u32::from_le_bytes(data[..4].try_into().unwrap());
        eprintln!("post-dispatch readback: 0x{val:08X} (expect 42 = 0x2A)");
        assert_eq!(
            val, 42,
            "handcrafted PTX: expected 42, got {val} (0x{val:08X})"
        );
        eprintln!("dispatch_handcrafted_ptx: PASSED — dispatch infra verified!");
        dev.free(buf).expect("free");
    }

    /// Verify that SM86 shader compilation succeeds independently of
    /// the driver dispatch path. The compiled SASS is identical whether
    /// the target dispatches via nouveau or nvidia-drm.
    #[test]
    #[ignore = "requires nvidia-drm hardware"]
    fn sm86_compilation_independent_of_driver() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    out[0] = 42u;
}
";
        let opts = coral_reef::CompileOptions {
            target: coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm86),
            opt_level: 2,
            ..Default::default()
        };
        let compiled =
            coral_reef::compile_wgsl_full(wgsl, &opts).expect("SM86 compilation should succeed");
        assert!(!compiled.binary.is_empty());
        eprintln!(
            "SM86 compiled: {} bytes, {} GPRs, {} instrs",
            compiled.binary.len(),
            compiled.info.gpr_count,
            compiled.info.instr_count
        );
    }
}
