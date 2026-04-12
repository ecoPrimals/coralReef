use coral_reef::{CompileOptions, GpuTarget, NvArch};
fn main() {
    let wgsl = r#"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = 42.0;
}
"#;
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        opt_level: 2,
        debug_info: false,
        ..CompileOptions::default()
    };
    let compiled = coral_reef::compile_wgsl_full(wgsl, &opts).unwrap();
    eprintln!("binary len: {} bytes", compiled.binary.len());
    eprintln!("gpr_count: {}", compiled.info.gpr_count);
    eprintln!("instr_count: {}", compiled.info.instr_count);
    // Dump as hex words
    let words: &[u32] = bytemuck::cast_slice(&compiled.binary);
    for (i, chunk) in words.chunks(4).enumerate() {
        let off = i * 16;
        print!("{off:04x}: ");
        for w in chunk {
            print!("{w:08x} ");
        }
        println!();
    }
}
