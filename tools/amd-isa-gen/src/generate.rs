// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! Rust code generation for AMD ISA tables.

use anyhow::Result;
use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;

use crate::parse::{EncodingInfo, InstrInfo, encoding_to_rust_mod};

/// Maximum lines per generated file (policy: stay under 1000).
pub const MAX_LINES_PER_FILE: usize = 950;
const LINES_PER_INSTR: usize = 8;

/// Output of encoding file generation.
pub struct EncodingOutput {
    pub main_file: String,
    pub table_file: Option<String>,
    pub table_sub_files: Vec<(String, String)>,
}

/// Generate the standard file header for generated code.
pub fn file_header() -> Result<String> {
    let mut out = String::new();
    writeln!(out, "// SPDX-License-Identifier: AGPL-3.0-only")?;
    writeln!(out, "// Copyright © 2026 ecoPrimals")?;
    writeln!(
        out,
        "//! AUTO-GENERATED from AMD RDNA2 ISA XML specification."
    )?;
    writeln!(out, "//!")?;
    writeln!(
        out,
        "//! Source: specs/amd/amdgpu_isa_rdna2.xml (MIT license, AMD GPUOpen)"
    )?;
    writeln!(
        out,
        "//! Generator: tools/amd-isa-gen (pure Rust, sovereign toolchain)"
    )?;
    writeln!(out, "//!")?;
    writeln!(out, "//! DO NOT EDIT BY HAND. Regenerate with:")?;
    writeln!(out, "//!   cargo run -p amd-isa-gen")?;
    writeln!(out)?;
    Ok(out)
}

/// Generate the isa_types.rs file content.
pub fn generate_types_file() -> Result<String> {
    let mut out = file_header()?;
    writeln!(out, "/// Bit field within an encoding format.")?;
    writeln!(out, "#[derive(Debug, Clone, Copy)]")?;
    writeln!(out, "pub struct BitField {{")?;
    writeln!(out, "    /// Bit offset within the instruction word(s).")?;
    writeln!(out, "    pub offset: u32,")?;
    writeln!(out, "    /// Number of bits.")?;
    writeln!(out, "    pub width: u32,")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "/// Instruction entry in the opcode table.")?;
    writeln!(out, "#[derive(Debug, Clone, Copy)]")?;
    writeln!(out, "pub struct InstrEntry {{")?;
    writeln!(out, "    /// Instruction mnemonic.")?;
    writeln!(out, "    pub name: &'static str,")?;
    writeln!(out, "    /// Numeric opcode within the encoding format.")?;
    writeln!(out, "    pub opcode: u16,")?;
    writeln!(out, "    /// Whether this instruction is a branch.")?;
    writeln!(out, "    pub is_branch: bool,")?;
    writeln!(
        out,
        "    /// Whether this instruction terminates the program."
    )?;
    writeln!(out, "    pub is_terminator: bool,")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    Ok(out)
}

/// Sub-categorize VOP3 comparison instructions for table split (float vs int).
fn cmp_sub_category(name: &str) -> &'static str {
    if name.ends_with("_F32")
        || name.ends_with("_F64")
        || name.ends_with("_F16")
        || name.contains("CLASS_F")
    {
        "cmp_f32_f64"
    } else {
        "cmp_int"
    }
}

/// Categorize VOP3 instruction for sub-table split.
pub fn vop3_category(name: &str) -> &'static str {
    if name.starts_with("V_CMP_") || name.starts_with("V_CMPX_") {
        "cmp"
    } else if name.starts_with("V_CVT_")
        || name.starts_with("V_TRUNC_")
        || name.starts_with("V_CEIL_")
        || name.starts_with("V_RNDNE_")
        || name.starts_with("V_FLOOR_")
        || name.starts_with("V_FRACT_")
        || name.starts_with("V_EXP_")
        || name.starts_with("V_LOG_")
        || name.starts_with("V_RCP_")
        || name.starts_with("V_RSQ_")
        || name.starts_with("V_SQRT_")
        || name.starts_with("V_SIN_")
        || name.starts_with("V_COS_")
        || name.starts_with("V_FREXP_")
        || name.starts_with("V_TRIG_PREOP_")
        || name.starts_with("V_LDEXP_")
    {
        "math"
    } else if name.starts_with("V_LSHRREV_")
        || name.starts_with("V_ASHRREV_")
        || name.starts_with("V_LSHLREV_")
        || name == "V_AND_B32"
        || name == "V_OR_B32"
        || name == "V_XOR_B32"
        || name == "V_XNOR_B32"
        || name.starts_with("V_XOR3_")
        || name.starts_with("V_OR3_")
        || name.starts_with("V_AND_OR_")
        || name.starts_with("V_LSHL_OR_")
        || name.starts_with("V_BFE_")
        || name.starts_with("V_BFI_")
        || name.starts_with("V_ALIGNBIT_")
        || name.starts_with("V_ALIGNBYTE_")
        || name == "V_NOT_B32"
        || name.starts_with("V_BFREV_")
        || name.starts_with("V_FFBH_")
        || name.starts_with("V_FFBL_")
        || name.starts_with("V_BCNT_")
        || name.starts_with("V_MBCNT_")
        || name.starts_with("V_BFM_")
        || name.starts_with("V_PERM_")
        || name.starts_with("V_PERMLANE")
    {
        "logic"
    } else {
        "arith"
    }
}

fn write_table_part(out: &mut String, instrs: &[&InstrInfo]) -> Result<()> {
    writeln!(out, "pub const TABLE: &[InstrEntry] = &[")?;
    for instr in instrs {
        writeln!(
            out,
            "    InstrEntry {{ name: \"{}\", opcode: {}, is_branch: {}, is_terminator: {} }},",
            instr.name, instr.opcode, instr.is_branch, instr.is_terminator
        )?;
    }
    writeln!(out, "];")?;
    writeln!(out)?;
    writeln!(out, "#[must_use]")?;
    writeln!(
        out,
        "pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {{"
    )?;
    writeln!(out, "    TABLE.iter().find(|e| e.opcode == opcode)")?;
    writeln!(out, "}}")?;
    Ok(())
}

fn write_table_and_lookup(out: &mut String, enc_name: &str, instrs: &[InstrInfo]) -> Result<()> {
    writeln!(out, "/// All {enc_name} instructions.")?;
    writeln!(out, "pub const TABLE: &[InstrEntry] = &[")?;
    for instr in instrs {
        writeln!(
            out,
            "    InstrEntry {{ name: \"{}\", opcode: {}, is_branch: {}, is_terminator: {} }},",
            instr.name, instr.opcode, instr.is_branch, instr.is_terminator
        )?;
    }
    writeln!(out, "];")?;
    writeln!(out)?;
    writeln!(out, "/// Look up an instruction by opcode.")?;
    writeln!(out, "#[must_use]")?;
    writeln!(
        out,
        "pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {{"
    )?;
    writeln!(out, "    TABLE.iter().find(|e| e.opcode == opcode)")?;
    writeln!(out, "}}")?;
    Ok(())
}

/// Generate a single encoding module.
pub fn generate_encoding_file(
    enc_name: &str,
    info: &EncodingInfo,
    instrs: Option<&Vec<InstrInfo>>,
) -> Result<EncodingOutput> {
    let mod_name = encoding_to_rust_mod(enc_name);
    let estimated_lines = instrs.map_or(0, |v| v.len() * LINES_PER_INSTR + 30);
    let needs_split = estimated_lines > MAX_LINES_PER_FILE;

    let mut out = file_header()?;
    writeln!(out, "use super::isa_types::{{BitField, InstrEntry}};")?;

    let mut table_file = None;
    let mut table_sub_files = Vec::new();

    let vop3_sub_split = enc_name == "ENC_VOP3" && instrs.is_some();
    let vopc_split = enc_name == "ENC_VOPC" && instrs.is_some();

    if vop3_sub_split {
        let instrs = instrs.unwrap();
        let mut by_cat: std::collections::BTreeMap<&str, Vec<&InstrInfo>> =
            std::collections::BTreeMap::new();
        for i in instrs {
            let cat = vop3_category(&i.name);
            by_cat.entry(cat).or_default().push(i);
        }
        let vop3_cats: Vec<&str> = ["cmp", "math", "arith", "logic"]
            .into_iter()
            .filter(|c| by_cat.contains_key(*c))
            .collect();
        // Effective table names: cmp splits into cmp_f32_f64 and cmp_int
        let effective_tables: Vec<String> = vop3_cats
            .iter()
            .flat_map(|cat| {
                if *cat == "cmp" {
                    vec!["cmp_f32_f64".to_string(), "cmp_int".to_string()]
                } else {
                    vec![(*cat).to_string()]
                }
            })
            .collect();
        for cat in &vop3_cats {
            let cat_instrs = by_cat
                .get(*cat)
                .ok_or_else(|| anyhow::anyhow!("VOP3 category {cat} missing from by_cat"))?;
            if *cat == "cmp" {
                let mut cmp_float: Vec<&InstrInfo> = Vec::new();
                let mut cmp_int: Vec<&InstrInfo> = Vec::new();
                for i in cat_instrs {
                    if cmp_sub_category(&i.name) == "cmp_f32_f64" {
                        cmp_float.push(i);
                    } else {
                        cmp_int.push(i);
                    }
                }
                let mut tbl_f = file_header()?;
                writeln!(tbl_f, "use super::super::isa_types::InstrEntry;")?;
                writeln!(tbl_f)?;
                write_table_part(&mut tbl_f, &cmp_float)?;
                table_sub_files.push(("table_cmp_f32_f64.rs".to_string(), tbl_f));
                let mut tbl_i = file_header()?;
                writeln!(tbl_i, "use super::super::isa_types::InstrEntry;")?;
                writeln!(tbl_i)?;
                write_table_part(&mut tbl_i, &cmp_int)?;
                table_sub_files.push(("table_cmp_int.rs".to_string(), tbl_i));
            } else {
                let mut tbl = file_header()?;
                writeln!(tbl, "use super::super::isa_types::InstrEntry;")?;
                writeln!(tbl)?;
                write_table_part(&mut tbl, cat_instrs)?;
                table_sub_files.push((format!("table_{cat}.rs"), tbl));
            }
        }
        for t in &effective_tables {
            writeln!(out, "mod table_{t};")?;
        }
        writeln!(out)?;
        writeln!(out, "use std::sync::OnceLock;")?;
        writeln!(out)?;
        writeln!(
            out,
            "static TABLE_CACHE: OnceLock<Vec<InstrEntry>> = OnceLock::new();"
        )?;
        writeln!(out)?;
        writeln!(
            out,
            "/// All {enc_name} instructions (combined from sub-tables)."
        )?;
        writeln!(out, "#[must_use]")?;
        writeln!(out, "pub fn table() -> &'static [InstrEntry] {{")?;
        writeln!(out, "    TABLE_CACHE.get_or_init(|| {{")?;
        write!(out, "        [")?;
        for (i, t) in effective_tables.iter().enumerate() {
            if i > 0 {
                write!(out, ", ")?;
            }
            write!(out, "table_{t}::TABLE")?;
        }
        writeln!(out, "].concat()")?;
        writeln!(out, "    }}).as_slice()")?;
        writeln!(out, "}}")?;
        writeln!(out)?;
        writeln!(out, "/// Look up an instruction by opcode.")?;
        writeln!(out, "#[must_use]")?;
        writeln!(
            out,
            "pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {{"
        )?;
        for (i, t) in effective_tables.iter().enumerate() {
            if i == 0 {
                write!(out, "    table_{t}::lookup(opcode)")?;
            } else {
                write!(out, "\n        .or_else(|| table_{t}::lookup(opcode))")?;
            }
        }
        writeln!(out)?;
        writeln!(out, "}}")?;
    } else if vopc_split {
        let instrs = instrs.ok_or_else(|| anyhow::anyhow!("instrs required for VOPC split"))?;
        let a: Vec<_> = instrs.iter().filter(|i| i.opcode < 64).cloned().collect();
        let b: Vec<_> = instrs.iter().filter(|i| i.opcode >= 64).cloned().collect();
        let mut tbl_a = file_header()?;
        writeln!(tbl_a, "use super::super::isa_types::InstrEntry;")?;
        writeln!(tbl_a)?;
        write_table_and_lookup(&mut tbl_a, "ENC_VOPC (F32/F64)", &a)?;
        let mut tbl_b = file_header()?;
        writeln!(tbl_b, "use super::super::isa_types::InstrEntry;")?;
        writeln!(tbl_b)?;
        write_table_and_lookup(&mut tbl_b, "ENC_VOPC (I/U/F16)", &b)?;
        table_sub_files.push(("table_a.rs".to_string(), tbl_a));
        table_sub_files.push(("table_b.rs".to_string(), tbl_b));
        writeln!(out, "mod table_a;")?;
        writeln!(out, "mod table_b;")?;
        writeln!(out)?;
        writeln!(out, "use std::sync::OnceLock;")?;
        writeln!(out)?;
        writeln!(
            out,
            "static TABLE_CACHE: OnceLock<Vec<InstrEntry>> = OnceLock::new();"
        )?;
        writeln!(out)?;
        writeln!(
            out,
            "/// All {enc_name} instructions (combined from sub-tables)."
        )?;
        writeln!(out, "#[must_use]")?;
        writeln!(out, "pub fn table() -> &'static [InstrEntry] {{")?;
        writeln!(out, "    TABLE_CACHE.get_or_init(|| {{")?;
        writeln!(out, "        [table_a::TABLE, table_b::TABLE].concat()")?;
        writeln!(out, "    }}).as_slice()")?;
        writeln!(out, "}}")?;
        writeln!(out)?;
        writeln!(out, "/// Look up an instruction by opcode.")?;
        writeln!(out, "#[must_use]")?;
        writeln!(
            out,
            "pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {{"
        )?;
        writeln!(
            out,
            "    table_a::lookup(opcode).or_else(|| table_b::lookup(opcode))"
        )?;
        writeln!(out, "}}")?;
    } else if needs_split {
        writeln!(out, "mod table;")?;
        writeln!(out, "pub use table::{{TABLE, lookup}};")?;
    }
    writeln!(out)?;

    writeln!(out, "/// {enc_name} encoding fields ({} bits).", info.bits)?;
    writeln!(out, "pub mod fields {{")?;
    writeln!(out, "    use super::BitField;")?;
    let mut sorted_fields = info.fields.clone();
    sorted_fields.sort_by_key(|f| f.offset);
    for field in &sorted_fields {
        let const_name = field.name.to_uppercase();
        writeln!(
            out,
            "    pub const {const_name}: BitField = BitField {{ offset: {}, width: {} }};",
            field.offset, field.width
        )?;
    }
    writeln!(out, "}}")?;
    writeln!(out)?;

    if let Some(instrs) = instrs {
        for instr in instrs {
            let const_name = instr.name.to_uppercase();
            if !instr.desc.is_empty() {
                writeln!(out, "/// {}", instr.desc)?;
            }
            writeln!(out, "pub const {const_name}: u16 = {};", instr.opcode)?;
        }

        if needs_split && !vop3_sub_split && !vopc_split {
            let mut tbl = file_header()?;
            writeln!(tbl, "use super::super::isa_types::InstrEntry;")?;
            writeln!(tbl)?;
            write_table_and_lookup(&mut tbl, enc_name, instrs)?;
            table_file = Some(tbl);
        } else if !vop3_sub_split && !vopc_split {
            writeln!(out)?;
            write_table_and_lookup(&mut out, enc_name, instrs)?;
        }
    }
    writeln!(out)?;

    let _ = mod_name;
    Ok(EncodingOutput {
        main_file: out,
        table_file,
        table_sub_files,
    })
}

/// Generate the mod.rs for the isa_generated crate.
pub fn generate_mod_file(
    encoding_fields: &BTreeMap<String, EncodingInfo>,
    instructions: &BTreeMap<String, Vec<InstrInfo>>,
) -> Result<String> {
    let mut out = file_header()?;

    writeln!(
        out,
        "#[expect(dead_code, missing_docs, reason = \"generated ISA tables from amd-isa-gen\")]"
    )?;
    writeln!(out, "pub mod isa_types;")?;
    writeln!(out)?;

    for enc_name in encoding_fields.keys() {
        let mod_name = encoding_to_rust_mod(enc_name);
        writeln!(
            out,
            "#[expect(dead_code, missing_docs, unused_imports, reason = \"generated ISA tables from amd-isa-gen\")]"
        )?;
        writeln!(out, "pub mod {mod_name};")?;
    }
    writeln!(out)?;

    let total: usize = instructions.values().map(Vec::len).sum();
    writeln!(
        out,
        "/// Total instruction count across all compute-relevant encodings: {total}"
    )?;
    writeln!(out, "pub const TOTAL_INSTRUCTIONS: usize = {total};")?;
    writeln!(out)?;

    writeln!(out, "/// Look up encoding width in bits by name.")?;
    writeln!(out, "#[must_use]")?;
    writeln!(out, "pub fn encoding_bits(name: &str) -> Option<u32> {{")?;
    writeln!(out, "    match name {{")?;
    for (enc_name, info) in encoding_fields {
        writeln!(out, "        \"{enc_name}\" => Some({}),", info.bits)?;
    }
    writeln!(out, "        _ => None,")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;

    Ok(out)
}
