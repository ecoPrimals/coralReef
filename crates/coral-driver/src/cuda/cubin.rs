// SPDX-License-Identifier: AGPL-3.0-or-later
//! Minimal cubin (ELF) assembler for packaging raw SASS binaries into a
//! format that `cuModuleLoadData` / cudarc `Ptx::from_binary` can consume.
//!
//! A cubin is an ELF64-LE object with:
//! - `.text.main_kernel` section (SASS code bytes)
//! - `.nv.info.main_kernel` section (register count, SM version, shared memory)
//! - `.nv.shared.main_kernel` section (shared memory size)
//! - `.strtab` / `.symtab` / `.shstrtab` for symbol and section naming
//!
//! This is sufficient for CUDA to load and launch the kernel.

/// ELF magic bytes.
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// ELF header size for 64-bit.
const ELF64_EHDR_SIZE: usize = 64;
/// Section header entry size for 64-bit.
const ELF64_SHDR_SIZE: usize = 64;
/// Symbol table entry size for 64-bit.
const ELF64_SYM_SIZE: usize = 24;

/// NVIDIA-specific ELF flags encoding SM version.
/// Format: `SM_xx` encoded as `0x00xx` in `e_flags`.
const fn sm_to_elf_flags(sm: u32) -> u32 {
    sm
}

/// `ELFCLASS64`, `ELFDATA2LSB`, ELF version 1, OS/ABI NONE.
const ELF_IDENT: [u8; 16] = [
    0x7f, b'E', b'L', b'F', // magic
    2,    // ELFCLASS64
    1,    // ELFDATA2LSB
    1,    // EV_CURRENT
    0, 0, 0, 0, 0, 0, 0, 0, 0, // padding
];

/// Section types.
const SHT_NULL: u32 = 0;
const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;

/// Symbol bindings/types.
const STB_GLOBAL: u8 = 1;
const STT_FUNC: u8 = 2;

/// SHF_EXECINSTR | SHF_ALLOC.
const SHF_ALLOC_EXEC: u64 = 0x2 | 0x4;
/// SHF_WRITE | SHF_ALLOC.
const SHF_ALLOC_WRITE: u64 = 0x1 | 0x2;

/// Metadata for kernel compilation, needed to produce `.nv.info`.
pub struct CubinKernelInfo {
    /// SM version (e.g. 70, 86, 120).
    pub sm: u32,
    /// GPR count from register allocation.
    pub gpr_count: u32,
    /// Shared memory size in bytes.
    pub shared_mem_bytes: u32,
    /// Number of barriers.
    pub barrier_count: u32,
}

impl CubinKernelInfo {
    /// Build from a [`ShaderInfo`] and an SM version.
    ///
    /// Convenient bridge: the driver holds `ShaderInfo` from the compiler and
    /// knows the SM version from device open — this combines both into the
    /// metadata the cubin assembler needs.
    #[must_use]
    pub fn from_shader_info(info: &crate::ShaderInfo, sm: u32) -> Self {
        Self {
            sm,
            gpr_count: info.gpr_count,
            shared_mem_bytes: info.shared_mem_bytes,
            barrier_count: info.barrier_count,
        }
    }
}

/// Assemble a cubin ELF from raw SASS code bytes.
///
/// The SASS bytes should be the encoded instruction stream (SPH header
/// prepended if the hardware requires it — caller's responsibility).
///
/// Returns a complete ELF binary that `cuModuleLoadData` can load.
#[must_use]
pub fn assemble_cubin(sass_bytes: &[u8], info: &CubinKernelInfo) -> Vec<u8> {
    let kernel_name = b"main_kernel\0";
    let text_name = b".text.main_kernel\0";
    let nv_info_name = b".nv.info.main_kernel\0";
    let nv_shared_name = b".nv.shared.main_kernel\0";
    let symtab_name = b".symtab\0";
    let strtab_name = b".strtab\0";
    let shstrtab_name = b".shstrtab\0";

    // Build .shstrtab: all section names concatenated.
    let mut shstrtab = vec![0u8]; // index 0 = null string
    let shstrtab_text = shstrtab.len();
    shstrtab.extend_from_slice(text_name);
    let shstrtab_nvinfo = shstrtab.len();
    shstrtab.extend_from_slice(nv_info_name);
    let shstrtab_nvshared = shstrtab.len();
    shstrtab.extend_from_slice(nv_shared_name);
    let shstrtab_symtab = shstrtab.len();
    shstrtab.extend_from_slice(symtab_name);
    let shstrtab_strtab = shstrtab.len();
    shstrtab.extend_from_slice(strtab_name);
    let shstrtab_shstrtab = shstrtab.len();
    shstrtab.extend_from_slice(shstrtab_name);

    // Build .strtab: symbol names.
    let mut strtab = vec![0u8]; // index 0 = null
    let strtab_kernel = strtab.len();
    strtab.extend_from_slice(kernel_name);

    // Build .nv.info: NVIDIA ELF attribute entries.
    //
    // Format per entry: [u16 format | u16 attr_id], [u32 size], [payload...]
    // Format 0x04 = EIFMT_SVAL (scalar value).
    //
    // Attribute IDs from NVIDIA ELF spec:
    //   0x2a = EIATTR_REGCOUNT
    //   0x23 = EIATTR_MAX_STACK_SIZE
    //   0x0f = EIATTR_NUM_BARRIERS
    let mut nv_info_data = Vec::new();

    // EIATTR_REGCOUNT (0x2a): EIFMT_SVAL(0x04) | attr(0x2a) → tag 0x2a04
    nv_info_data.extend_from_slice(&0x2a04_u32.to_le_bytes());
    nv_info_data.extend_from_slice(&4_u32.to_le_bytes());
    nv_info_data.extend_from_slice(&info.gpr_count.to_le_bytes());

    // EIATTR_MAX_STACK_SIZE (0x23): tag 0x2304, payload = 0
    nv_info_data.extend_from_slice(&0x2304_u32.to_le_bytes());
    nv_info_data.extend_from_slice(&4_u32.to_le_bytes());
    nv_info_data.extend_from_slice(&0_u32.to_le_bytes());

    // EIATTR_NUM_BARRIERS (0x0f): tag 0x0f04
    nv_info_data.extend_from_slice(&0x0f04_u32.to_le_bytes());
    nv_info_data.extend_from_slice(&4_u32.to_le_bytes());
    nv_info_data.extend_from_slice(&info.barrier_count.to_le_bytes());

    // Build .nv.shared: just the size encoding (empty section, size = shared_mem).
    let nv_shared_data: Vec<u8> = Vec::new();

    // Sections: [0]=NULL, [1]=.text, [2]=.nv.info, [3]=.nv.shared,
    //           [4]=.symtab, [5]=.strtab, [6]=.shstrtab
    let num_sections: u16 = 7;
    let shstrtab_idx: u16 = 6;

    // Layout: ELF header | .text | .nv.info | .nv.shared | .symtab | .strtab | .shstrtab | section headers
    let text_offset = ELF64_EHDR_SIZE;
    let text_size = sass_bytes.len();

    let nvinfo_offset = text_offset + text_size;
    let nvinfo_size = nv_info_data.len();

    let nvshared_offset = nvinfo_offset + nvinfo_size;
    let nvshared_size = nv_shared_data.len();

    // .symtab: null entry + one FUNC symbol.
    let symtab_offset = nvshared_offset + nvshared_size;
    let symtab_data = build_symtab(strtab_kernel, text_size, 1);
    let symtab_size = symtab_data.len();

    let strtab_offset = symtab_offset + symtab_size;
    let strtab_size = strtab.len();

    let shstrtab_offset = strtab_offset + strtab_size;
    let shstrtab_size = shstrtab.len();

    let shdr_offset = align_up(shstrtab_offset + shstrtab_size, 8);

    let mut elf = Vec::with_capacity(shdr_offset + num_sections as usize * ELF64_SHDR_SIZE);

    // ELF header.
    elf.extend_from_slice(&ELF_IDENT);
    elf.extend_from_slice(&2_u16.to_le_bytes()); // e_type: ET_EXEC
    elf.extend_from_slice(&0xBE_u16.to_le_bytes()); // e_machine: EM_CUDA
    elf.extend_from_slice(&1_u32.to_le_bytes()); // e_version
    elf.extend_from_slice(&0_u64.to_le_bytes()); // e_entry
    elf.extend_from_slice(&0_u64.to_le_bytes()); // e_phoff
    elf.extend_from_slice(&(shdr_offset as u64).to_le_bytes()); // e_shoff
    elf.extend_from_slice(&sm_to_elf_flags(info.sm).to_le_bytes()); // e_flags
    elf.extend_from_slice(&(ELF64_EHDR_SIZE as u16).to_le_bytes()); // e_ehsize
    elf.extend_from_slice(&0_u16.to_le_bytes()); // e_phentsize
    elf.extend_from_slice(&0_u16.to_le_bytes()); // e_phnum
    elf.extend_from_slice(&(ELF64_SHDR_SIZE as u16).to_le_bytes()); // e_shentsize
    elf.extend_from_slice(&num_sections.to_le_bytes()); // e_shnum
    elf.extend_from_slice(&shstrtab_idx.to_le_bytes()); // e_shstrndx
    debug_assert_eq!(elf.len(), ELF64_EHDR_SIZE);

    // Section data.
    elf.extend_from_slice(sass_bytes);
    elf.extend_from_slice(&nv_info_data);
    elf.extend_from_slice(&nv_shared_data);
    elf.extend_from_slice(&symtab_data);
    elf.extend_from_slice(&strtab);
    elf.extend_from_slice(&shstrtab);

    // Pad to alignment.
    while elf.len() < shdr_offset {
        elf.push(0);
    }

    // Section headers.
    // [0] NULL
    write_shdr(&mut elf, 0, SHT_NULL, 0, 0, 0, 0, 0, 0, 0);
    // [1] .text.main_kernel
    write_shdr(
        &mut elf,
        shstrtab_text as u32,
        SHT_PROGBITS,
        SHF_ALLOC_EXEC,
        0,
        text_offset as u64,
        text_size as u64,
        0,
        0,
        32, // alignment
    );
    // [2] .nv.info.main_kernel
    write_shdr(
        &mut elf,
        shstrtab_nvinfo as u32,
        SHT_PROGBITS,
        0,
        0,
        nvinfo_offset as u64,
        nvinfo_size as u64,
        0,
        0,
        4,
    );
    // [3] .nv.shared.main_kernel
    write_shdr(
        &mut elf,
        shstrtab_nvshared as u32,
        0x08, // SHT_NOBITS
        SHF_ALLOC_WRITE,
        0,
        nvshared_offset as u64,
        info.shared_mem_bytes as u64,
        0,
        0,
        4,
    );
    // [4] .symtab
    write_shdr(
        &mut elf,
        shstrtab_symtab as u32,
        SHT_SYMTAB,
        0,
        5, // sh_link → .strtab index
        symtab_offset as u64,
        symtab_size as u64,
        1, // sh_info: one local + first global at index 1
        0,
        8,
    );
    // [5] .strtab
    write_shdr(
        &mut elf,
        shstrtab_strtab as u32,
        SHT_STRTAB,
        0,
        0,
        strtab_offset as u64,
        strtab_size as u64,
        0,
        0,
        1,
    );
    // [6] .shstrtab
    write_shdr(
        &mut elf,
        shstrtab_shstrtab as u32,
        SHT_STRTAB,
        0,
        0,
        shstrtab_offset as u64,
        shstrtab_size as u64,
        0,
        0,
        1,
    );

    elf
}

/// Check whether a byte slice begins with ELF magic (`\x7fELF`).
#[must_use]
pub fn is_cubin(data: &[u8]) -> bool {
    data.len() >= 4 && data[..4] == ELF_MAGIC
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

#[expect(clippy::cast_possible_truncation)]
fn build_symtab(name_offset: usize, text_size: usize, text_section_idx: u16) -> Vec<u8> {
    let mut data = Vec::with_capacity(ELF64_SYM_SIZE * 2);

    // [0] Null symbol.
    data.extend_from_slice(&[0u8; ELF64_SYM_SIZE]);

    // [1] main_kernel: STB_GLOBAL | STT_FUNC, section .text.
    data.extend_from_slice(&(name_offset as u32).to_le_bytes()); // st_name
    data.push((STB_GLOBAL << 4) | STT_FUNC); // st_info
    data.push(0); // st_other
    data.extend_from_slice(&text_section_idx.to_le_bytes()); // st_shndx
    data.extend_from_slice(&0_u64.to_le_bytes()); // st_value
    data.extend_from_slice(&(text_size as u64).to_le_bytes()); // st_size

    data
}

#[expect(clippy::too_many_arguments)]
fn write_shdr(
    elf: &mut Vec<u8>,
    sh_name: u32,
    sh_type: u32,
    sh_flags: u64,
    sh_link: u32,
    sh_offset: u64,
    sh_size: u64,
    sh_info: u32,
    sh_addr: u64,
    sh_addralign: u64,
) {
    elf.extend_from_slice(&sh_name.to_le_bytes());
    elf.extend_from_slice(&sh_type.to_le_bytes());
    elf.extend_from_slice(&sh_flags.to_le_bytes());
    elf.extend_from_slice(&sh_addr.to_le_bytes());
    elf.extend_from_slice(&sh_offset.to_le_bytes());
    elf.extend_from_slice(&sh_size.to_le_bytes());
    elf.extend_from_slice(&sh_link.to_le_bytes());
    elf.extend_from_slice(&sh_info.to_le_bytes());
    elf.extend_from_slice(&sh_addralign.to_le_bytes());
    elf.extend_from_slice(&0_u64.to_le_bytes()); // sh_entsize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cubin_starts_with_elf_magic() {
        let sass = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00, 0x00, 0x00];
        let info = CubinKernelInfo {
            sm: 86,
            gpr_count: 32,
            shared_mem_bytes: 0,
            barrier_count: 0,
        };
        let cubin = assemble_cubin(&sass, &info);
        assert!(is_cubin(&cubin));
        assert_eq!(&cubin[..4], &ELF_MAGIC);
    }

    #[test]
    fn cubin_elf_class_and_machine() {
        let sass = vec![0u8; 16];
        let info = CubinKernelInfo {
            sm: 70,
            gpr_count: 16,
            shared_mem_bytes: 4096,
            barrier_count: 1,
        };
        let cubin = assemble_cubin(&sass, &info);
        assert_eq!(cubin[4], 2, "ELFCLASS64");
        assert_eq!(cubin[5], 1, "ELFDATA2LSB");
        // e_machine at offset 18 (LE u16) = 0xBE
        let machine = u16::from_le_bytes([cubin[18], cubin[19]]);
        assert_eq!(machine, 0xBE, "EM_CUDA");
    }

    #[test]
    fn cubin_sm_in_flags() {
        let sass = vec![0u8; 32];
        let info = CubinKernelInfo {
            sm: 86,
            gpr_count: 48,
            shared_mem_bytes: 0,
            barrier_count: 0,
        };
        let cubin = assemble_cubin(&sass, &info);
        // e_flags at offset 48 (4 bytes LE)
        let flags = u32::from_le_bytes([cubin[48], cubin[49], cubin[50], cubin[51]]);
        assert_eq!(flags, 86);
    }

    #[test]
    fn cubin_contains_sass_data() {
        let sass = vec![0x42u8; 64];
        let info = CubinKernelInfo {
            sm: 70,
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
        };
        let cubin = assemble_cubin(&sass, &info);
        // .text starts at ELF header end (64 bytes)
        assert_eq!(&cubin[64..128], &sass[..]);
    }

    #[test]
    fn is_cubin_detects_elf() {
        assert!(is_cubin(b"\x7fELF\x02\x01"));
        assert!(!is_cubin(b"PTX source text"));
        assert!(!is_cubin(b"\x7f"));
        assert!(!is_cubin(b""));
    }

    #[test]
    fn cubin_section_header_count() {
        let sass = vec![0u8; 8];
        let info = CubinKernelInfo {
            sm: 86,
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
        };
        let cubin = assemble_cubin(&sass, &info);
        // e_shnum at offset 60 (u16 LE) = 7
        let shnum = u16::from_le_bytes([cubin[60], cubin[61]]);
        assert_eq!(shnum, 7);
    }
}
