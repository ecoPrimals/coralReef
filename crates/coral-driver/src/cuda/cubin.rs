// SPDX-License-Identifier: AGPL-3.0-or-later
//! Minimal cubin (ELF) assembler for packaging raw SASS binaries into a
//! format that `cuModuleLoadData` / cudarc `Ptx::from_binary` can consume.
//!
//! A cubin is an ELF64-LE object matching the format produced by `nvcc -cubin`:
//! - NVIDIA-specific OS/ABI (0x33) and ABI version (0x07)
//! - `e_flags` encodes SM version as `(sm << 16) | 0x0500 | sm`
//! - `.nv.info` global section (section type `SHT_LOPROC`)
//! - `.nv.info.main_kernel` per-kernel info section
//! - `.text.main_kernel` section (SASS code, 128-byte aligned)
//! - `.nv.shared.main_kernel` section (shared memory, NOBITS)
//! - `.strtab` / `.symtab` / `.shstrtab` for symbol and section naming

/// ELF magic bytes.
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// ELF header size for 64-bit.
const ELF64_EHDR_SIZE: usize = 64;
/// Section header entry size for 64-bit.
const ELF64_SHDR_SIZE: usize = 64;
/// Symbol table entry size for 64-bit.
const ELF64_SYM_SIZE: usize = 24;

/// NVIDIA cubin `e_flags` encoding: `(sm << 16) | 0x0500 | sm`.
/// Matches the format observed in nvcc-generated cubins for SM35–SM120.
const fn sm_to_elf_flags(sm: u32) -> u32 {
    (sm << 16) | 0x0500 | sm
}

/// NVIDIA cubin ELF ident — OS/ABI 0x33, ABI version 0x07.
const ELF_IDENT: [u8; 16] = [
    0x7f, b'E', b'L', b'F', // magic
    2,    // ELFCLASS64
    1,    // ELFDATA2LSB
    1,    // EV_CURRENT
    0x33, // NVIDIA CUDA OS/ABI
    0x07, // ABI version 7
    0, 0, 0, 0, 0, 0, 0, // padding
];

/// NVIDIA cubin `e_version` value (0x7e, matches nvcc output).
const NV_ELF_VERSION: u32 = 0x7e;

/// Section types.
const SHT_NULL: u32 = 0;
const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_NOBITS: u32 = 8;
/// NVIDIA-specific section type for `.nv.info` sections.
const SHT_CUDA_INFO: u32 = 0x7000_0000; // SHT_LOPROC

/// Symbol bindings/types.
const STB_GLOBAL: u8 = 1;
const STT_FUNC: u8 = 2;

/// SHF_EXECINSTR | SHF_ALLOC.
const SHF_ALLOC_EXEC: u64 = 0x2 | 0x4;
/// SHF_WRITE | SHF_ALLOC.
const SHF_ALLOC_WRITE: u64 = 0x1 | 0x2;
/// SHF_INFO_LINK — .nv.info.main_kernel links to .text via sh_info.
const SHF_INFO_LINK: u64 = 0x40;

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
/// Matches the format produced by `nvcc -cubin` including NVIDIA-specific
/// OS/ABI, `e_flags` encoding, and section types.
#[must_use]
pub fn assemble_cubin(sass_bytes: &[u8], info: &CubinKernelInfo) -> Vec<u8> {
    let kernel_name = b"main_kernel\0";
    let text_name = b".text.main_kernel\0";
    let nv_info_global_name = b".nv.info\0";
    let nv_info_name = b".nv.info.main_kernel\0";
    let nv_shared_name = b".nv.shared.main_kernel\0";
    let symtab_name = b".symtab\0";
    let strtab_name = b".strtab\0";
    let shstrtab_name = b".shstrtab\0";

    // ---- Section ordering (matches nvcc) ----
    // [0] NULL
    // [1] .shstrtab
    // [2] .strtab
    // [3] .symtab
    // [4] .nv.info              (global, per-module)
    // [5] .nv.info.main_kernel  (per-kernel, SHF_INFO_LINK → .text idx)
    // [6] .nv.shared.main_kernel
    // [7] .text.main_kernel
    let num_sections: u16 = 8;
    let shstrtab_idx: u16 = 1;
    let strtab_idx: u16 = 2;
    let symtab_idx: u16 = 3;
    let text_idx: u16 = 7;

    // Build .shstrtab
    let mut shstrtab = vec![0u8]; // index 0 = null
    let shstrtab_shstrtab = shstrtab.len();
    shstrtab.extend_from_slice(shstrtab_name);
    let shstrtab_strtab = shstrtab.len();
    shstrtab.extend_from_slice(strtab_name);
    let shstrtab_symtab = shstrtab.len();
    shstrtab.extend_from_slice(symtab_name);
    let shstrtab_nv_info_global = shstrtab.len();
    shstrtab.extend_from_slice(nv_info_global_name);
    let shstrtab_nvinfo = shstrtab.len();
    shstrtab.extend_from_slice(nv_info_name);
    let shstrtab_nvshared = shstrtab.len();
    shstrtab.extend_from_slice(nv_shared_name);
    let shstrtab_text = shstrtab.len();
    shstrtab.extend_from_slice(text_name);

    // Build .strtab
    let mut strtab = vec![0u8]; // index 0 = null
    let strtab_kernel = strtab.len();
    strtab.extend_from_slice(kernel_name);

    // Build .nv.info (global): SM version attribute.
    // EIATTR_MIN_STACK_SIZE (0x12): EIFMT_SVAL(0x04) | attr → tag 0x1204
    let mut nv_info_global_data = Vec::new();
    nv_info_global_data.extend_from_slice(&0x1204_u32.to_le_bytes());
    nv_info_global_data.extend_from_slice(&4_u32.to_le_bytes());
    nv_info_global_data.extend_from_slice(&0_u32.to_le_bytes());

    // Build .nv.info.main_kernel: per-kernel attributes.
    let mut nv_info_data = Vec::new();

    // EIATTR_REGCOUNT (0x2a): EIFMT_SVAL(0x04) → tag 0x2a04
    nv_info_data.extend_from_slice(&0x2a04_u32.to_le_bytes());
    nv_info_data.extend_from_slice(&4_u32.to_le_bytes());
    nv_info_data.extend_from_slice(&info.gpr_count.to_le_bytes());

    // EIATTR_MAX_STACK_SIZE (0x23): tag 0x2304
    nv_info_data.extend_from_slice(&0x2304_u32.to_le_bytes());
    nv_info_data.extend_from_slice(&4_u32.to_le_bytes());
    nv_info_data.extend_from_slice(&0_u32.to_le_bytes());

    // EIATTR_NUM_BARRIERS (0x0f): tag 0x0f04
    nv_info_data.extend_from_slice(&0x0f04_u32.to_le_bytes());
    nv_info_data.extend_from_slice(&4_u32.to_le_bytes());
    nv_info_data.extend_from_slice(&info.barrier_count.to_le_bytes());

    // Build .symtab: null + section symbols + main_kernel FUNC.
    let symtab_data = build_symtab(strtab_kernel, sass_bytes.len(), text_idx);

    // ---- Compute file layout ----
    // Data order: shstrtab, strtab, symtab, nv_info_global, nv_info,
    //             pad-to-128, .text (128-byte aligned for SASS)

    let shstrtab_offset = ELF64_EHDR_SIZE;
    let shstrtab_size = shstrtab.len();

    let strtab_offset = shstrtab_offset + shstrtab_size;
    let strtab_size = strtab.len();

    let symtab_offset = align_up(strtab_offset + strtab_size, 8);
    let symtab_size = symtab_data.len();

    let nv_info_global_offset = symtab_offset + symtab_size;
    let nv_info_global_size = nv_info_global_data.len();

    let nvinfo_offset = nv_info_global_offset + nv_info_global_size;
    let nvinfo_size = nv_info_data.len();

    // .nv.shared is NOBITS — occupies no file space.
    let nvshared_offset = nvinfo_offset + nvinfo_size;

    // .text.main_kernel at 128-byte alignment (matches nvcc)
    let text_offset = align_up(nvshared_offset, 128);
    let text_size = sass_bytes.len();

    let shdr_offset = align_up(text_offset + text_size, 8);

    let total_size = shdr_offset + num_sections as usize * ELF64_SHDR_SIZE;
    let mut elf = Vec::with_capacity(total_size);

    // ---- ELF header ----
    elf.extend_from_slice(&ELF_IDENT);
    elf.extend_from_slice(&2_u16.to_le_bytes()); // e_type: ET_EXEC
    elf.extend_from_slice(&0xBE_u16.to_le_bytes()); // e_machine: EM_CUDA
    elf.extend_from_slice(&NV_ELF_VERSION.to_le_bytes()); // e_version
    elf.extend_from_slice(&0_u64.to_le_bytes()); // e_entry
    elf.extend_from_slice(&0_u64.to_le_bytes()); // e_phoff (no program headers)
    elf.extend_from_slice(&(shdr_offset as u64).to_le_bytes()); // e_shoff
    elf.extend_from_slice(&sm_to_elf_flags(info.sm).to_le_bytes()); // e_flags
    elf.extend_from_slice(&(ELF64_EHDR_SIZE as u16).to_le_bytes()); // e_ehsize
    elf.extend_from_slice(&0_u16.to_le_bytes()); // e_phentsize
    elf.extend_from_slice(&0_u16.to_le_bytes()); // e_phnum
    elf.extend_from_slice(&(ELF64_SHDR_SIZE as u16).to_le_bytes()); // e_shentsize
    elf.extend_from_slice(&num_sections.to_le_bytes()); // e_shnum
    elf.extend_from_slice(&shstrtab_idx.to_le_bytes()); // e_shstrndx
    debug_assert_eq!(elf.len(), ELF64_EHDR_SIZE);

    // ---- Section data ----
    elf.extend_from_slice(&shstrtab);
    elf.extend_from_slice(&strtab);
    // Pad for symtab alignment
    while elf.len() < symtab_offset {
        elf.push(0);
    }
    elf.extend_from_slice(&symtab_data);
    elf.extend_from_slice(&nv_info_global_data);
    elf.extend_from_slice(&nv_info_data);
    // Pad to 128-byte alignment for .text
    while elf.len() < text_offset {
        elf.push(0);
    }
    elf.extend_from_slice(sass_bytes);
    // Pad to section header alignment
    while elf.len() < shdr_offset {
        elf.push(0);
    }

    // ---- Section headers ----
    // [0] NULL
    write_shdr(&mut elf, 0, SHT_NULL, 0, 0, 0, 0, 0, 0, 0);
    // [1] .shstrtab
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
    // [2] .strtab
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
    // [3] .symtab
    write_shdr(
        &mut elf,
        shstrtab_symtab as u32,
        SHT_SYMTAB,
        0,
        strtab_idx as u32,
        symtab_offset as u64,
        symtab_size as u64,
        1, // sh_info: first global symbol index
        0,
        8,
    );
    // [4] .nv.info (global, SHT_CUDA_INFO, sh_link → .symtab)
    write_shdr(
        &mut elf,
        shstrtab_nv_info_global as u32,
        SHT_CUDA_INFO,
        0,
        symtab_idx as u32,
        nv_info_global_offset as u64,
        nv_info_global_size as u64,
        0,
        0,
        4,
    );
    // [5] .nv.info.main_kernel (SHT_CUDA_INFO, SHF_INFO_LINK, sh_link → .symtab,
    //     sh_info → .text section index)
    write_shdr(
        &mut elf,
        shstrtab_nvinfo as u32,
        SHT_CUDA_INFO,
        SHF_INFO_LINK,
        symtab_idx as u32,
        nvinfo_offset as u64,
        nvinfo_size as u64,
        text_idx as u32,
        0,
        4,
    );
    // [6] .nv.shared.main_kernel (NOBITS)
    write_shdr(
        &mut elf,
        shstrtab_nvshared as u32,
        SHT_NOBITS,
        SHF_ALLOC_WRITE,
        0,
        nvshared_offset as u64,
        info.shared_mem_bytes as u64,
        0,
        0,
        4,
    );
    // [7] .text.main_kernel (PROGBITS, SHF_ALLOC|SHF_EXECINSTR)
    // sh_info encodes NVIDIA kernel metadata flags (0x08000006 observed in nvcc
    // cubins — high bit marks it as a kernel entry, low bits = SM config).
    write_shdr(
        &mut elf,
        shstrtab_text as u32,
        SHT_PROGBITS,
        SHF_ALLOC_EXEC,
        symtab_idx as u32,
        text_offset as u64,
        text_size as u64,
        0x0800_0006, // NVIDIA kernel entry flags
        0,
        128, // 128-byte alignment (matches nvcc)
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
    // st_other = 0x10 flags the symbol as a kernel entry point (matches nvcc).
    data.extend_from_slice(&(name_offset as u32).to_le_bytes()); // st_name
    data.push((STB_GLOBAL << 4) | STT_FUNC); // st_info
    data.push(0x10); // st_other: NVIDIA kernel marker
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
        assert_eq!(cubin[7], 0x33, "NVIDIA OS/ABI");
        assert_eq!(cubin[8], 0x07, "ABI version 7");
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
        let flags = u32::from_le_bytes([cubin[48], cubin[49], cubin[50], cubin[51]]);
        assert_eq!(flags, sm_to_elf_flags(86));
        assert_eq!(flags, (86 << 16) | 0x0500 | 86);
    }

    #[test]
    fn cubin_sm120_flags() {
        let sass = vec![0u8; 32];
        let info = CubinKernelInfo {
            sm: 120,
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
        };
        let cubin = assemble_cubin(&sass, &info);
        let flags = u32::from_le_bytes([cubin[48], cubin[49], cubin[50], cubin[51]]);
        assert_eq!(flags, (120 << 16) | 0x0500 | 120);
    }

    #[test]
    fn cubin_e_version_matches_nvcc() {
        let sass = vec![0u8; 8];
        let info = CubinKernelInfo {
            sm: 89,
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
        };
        let cubin = assemble_cubin(&sass, &info);
        let version = u32::from_le_bytes([cubin[20], cubin[21], cubin[22], cubin[23]]);
        assert_eq!(version, 0x7e);
    }

    #[test]
    fn cubin_contains_sass_data() {
        let sass: Vec<u8> = (0..64).collect();
        let info = CubinKernelInfo {
            sm: 70,
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
        };
        let cubin = assemble_cubin(&sass, &info);
        // Find .text by searching for the SASS content.
        let pos = cubin
            .windows(sass.len())
            .position(|w| w == &sass[..])
            .expect("SASS not found in cubin");
        // .text should be 128-byte aligned
        assert_eq!(pos % 128, 0, "SASS at offset {pos} is not 128-byte aligned");
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
        let shnum = u16::from_le_bytes([cubin[60], cubin[61]]);
        assert_eq!(shnum, 8);
    }

    #[test]
    fn cubin_shstrtab_at_index_1() {
        let sass = vec![0u8; 8];
        let info = CubinKernelInfo {
            sm: 70,
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
        };
        let cubin = assemble_cubin(&sass, &info);
        let shstrndx = u16::from_le_bytes([cubin[62], cubin[63]]);
        assert_eq!(shstrndx, 1);
    }
}
