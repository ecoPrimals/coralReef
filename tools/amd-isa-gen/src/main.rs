// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! Pure Rust AMD ISA table generator — replaces `gen_rdna2_opcodes.py`.
//!
//! Parses AMD's machine-readable ISA XML specification and generates
//! Rust source code with encoding field layouts and opcode tables.
//!
//! # Usage
//!
//! ```bash
//! cargo run -p amd-isa-gen
//! ```
//!
//! This is the sovereign Rust replacement for the Python scaffold.
//! The Rust compiler is our DNA synthase — no non-Rust tools remain.

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::PathBuf;

const COMPUTE_ENCODINGS: &[&str] = &[
    "ENC_SOP1",
    "ENC_SOP2",
    "ENC_SOPC",
    "ENC_SOPK",
    "ENC_SOPP",
    "ENC_SMEM",
    "ENC_VOP1",
    "ENC_VOP2",
    "ENC_VOP3",
    "ENC_VOP3P",
    "ENC_VOPC",
    "ENC_DS",
    "ENC_FLAT",
    "ENC_FLAT_GLBL",
    "ENC_FLAT_SCRATCH",
    "ENC_MUBUF",
    "ENC_MTBUF",
    "ENC_MIMG",
];

#[derive(Debug, Clone)]
struct BitField {
    name: String,
    offset: u32,
    width: u32,
}

#[derive(Debug, Clone)]
struct EncodingInfo {
    bits: u32,
    fields: Vec<BitField>,
}

#[derive(Debug, Clone)]
struct InstrInfo {
    name: String,
    opcode: u16,
    desc: String,
    is_branch: bool,
    is_terminator: bool,
}

fn encoding_to_rust_mod(enc: &str) -> String {
    enc.replace("ENC_", "").to_lowercase()
}

fn repo_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map_or_else(|| PathBuf::from("."), std::path::Path::to_path_buf)
}

fn parse_xml(
    xml_path: &std::path::Path,
) -> (
    BTreeMap<String, EncodingInfo>,
    BTreeMap<String, Vec<InstrInfo>>,
) {
    let xml_content = fs::read_to_string(xml_path).unwrap_or_else(|e| {
        eprintln!("ERROR: Cannot read {}: {e}", xml_path.display());
        eprintln!("Download from: https://gpuopen.com/download/machine-readable-isa/latest/");
        std::process::exit(1);
    });

    let compute_set: BTreeSet<&str> = COMPUTE_ENCODINGS.iter().copied().collect();
    let mut encoding_fields: BTreeMap<String, EncodingInfo> = BTreeMap::new();
    let mut instructions: BTreeMap<String, Vec<InstrInfo>> = BTreeMap::new();
    let mut instr_global_info: HashMap<String, InstrInfo> = HashMap::new();

    let mut reader = Reader::from_str(&xml_content);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut path: Vec<String> = Vec::new();

    // State machine for parsing
    let mut in_encoding = false;
    let mut current_encoding_name = String::new();
    let mut current_encoding_bits: u32 = 0;
    let mut current_fields: Vec<BitField> = Vec::new();
    let mut in_bitmap = false;
    let mut in_field = false;
    let mut current_field_name = String::new();
    let mut current_field_offset: u32 = 0;
    let mut current_field_width: u32 = 0;

    let mut in_instruction = false;
    let mut current_instr_name = String::new();
    let mut current_instr_desc = String::new();
    let mut current_instr_is_branch = false;
    let mut current_instr_is_term = false;
    let mut in_instr_encoding = false;
    let mut current_ie_enc_name = String::new();
    let mut current_ie_opcode: u16 = 0;
    let mut current_ie_condition = String::new();
    let mut current_text_target = String::new();
    let mut ie_defaults: Vec<(String, u16)> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                path.push(tag.clone());

                match tag.as_str() {
                    "Encoding" => {
                        in_encoding = true;
                        current_encoding_name.clear();
                        current_encoding_bits = 0;
                        current_fields.clear();
                    }
                    "BitMap" if in_encoding => {
                        in_bitmap = true;
                    }
                    "Field" if in_bitmap => {
                        in_field = true;
                        current_field_name.clear();
                        current_field_offset = 0;
                        current_field_width = 0;
                    }
                    "Instruction" => {
                        in_instruction = true;
                        current_instr_name.clear();
                        current_instr_desc.clear();
                        current_instr_is_branch = false;
                        current_instr_is_term = false;
                        ie_defaults.clear();
                    }
                    "InstructionEncoding" if in_instruction => {
                        in_instr_encoding = true;
                        current_ie_enc_name.clear();
                        current_ie_opcode = 0;
                        current_ie_condition.clear();
                    }
                    "EncodingName"
                    | "BitCount"
                    | "BitOffset"
                    | "FieldName"
                    | "InstructionName"
                    | "Description"
                    | "IsBranch"
                    | "IsProgramTerminator"
                    | "Opcode"
                    | "EncodingCondition" => {
                        current_text_target = tag;
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();

                match tag.as_str() {
                    "Encoding" => {
                        if in_encoding
                            && compute_set.contains(current_encoding_name.as_str())
                            && !current_fields.is_empty()
                        {
                            encoding_fields.insert(
                                current_encoding_name.clone(),
                                EncodingInfo {
                                    bits: current_encoding_bits,
                                    fields: current_fields.clone(),
                                },
                            );
                        }
                        in_encoding = false;
                    }
                    "BitMap" => {
                        in_bitmap = false;
                    }
                    "Field" if in_field => {
                        if !current_field_name.is_empty() {
                            current_fields.push(BitField {
                                name: current_field_name.clone(),
                                offset: current_field_offset,
                                width: current_field_width,
                            });
                        }
                        in_field = false;
                    }
                    "InstructionEncoding" => {
                        if in_instr_encoding
                            && current_ie_condition == "default"
                            && compute_set.contains(current_ie_enc_name.as_str())
                        {
                            ie_defaults.push((current_ie_enc_name.clone(), current_ie_opcode));
                        }
                        in_instr_encoding = false;
                    }
                    "Instruction" => {
                        if in_instruction && !ie_defaults.is_empty() {
                            let mut seen_enc: BTreeSet<String> = BTreeSet::new();
                            for (enc_name, opcode) in &ie_defaults {
                                if seen_enc.contains(enc_name) {
                                    continue;
                                }
                                seen_enc.insert(enc_name.clone());

                                let desc = current_instr_desc
                                    .replace('\\', "\\\\")
                                    .replace('"', "\\\"");
                                let desc = if desc.len() > 120 {
                                    format!("{}...", &desc[..117])
                                } else {
                                    desc
                                };

                                let info = InstrInfo {
                                    name: current_instr_name.clone(),
                                    opcode: *opcode,
                                    desc: desc.clone(),
                                    is_branch: current_instr_is_branch,
                                    is_terminator: current_instr_is_term,
                                };

                                instructions
                                    .entry(enc_name.clone())
                                    .or_default()
                                    .push(info.clone());

                                instr_global_info
                                    .entry(current_instr_name.clone())
                                    .or_insert(info);
                            }
                        }
                        in_instruction = false;
                    }
                    _ => {}
                }

                if path.last().is_some_and(|last| *last == tag) {
                    path.pop();
                }
                current_text_target.clear();
            }
            Ok(Event::Text(e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                match current_text_target.as_str() {
                    "EncodingName" => {
                        if in_instr_encoding {
                            current_ie_enc_name = text;
                        } else if in_encoding {
                            current_encoding_name = text;
                        }
                    }
                    "BitCount" => {
                        if let Ok(v) = text.parse::<u32>() {
                            if in_field {
                                current_field_width = v;
                            } else if in_encoding {
                                current_encoding_bits = v;
                            }
                        }
                    }
                    "BitOffset" => {
                        if let Ok(v) = text.parse::<u32>()
                            && in_field
                        {
                            current_field_offset = v;
                        }
                    }
                    "FieldName" => {
                        if in_field {
                            current_field_name = text;
                        }
                    }
                    "InstructionName" => {
                        current_instr_name = text;
                    }
                    "Description" => {
                        current_instr_desc = text;
                    }
                    "IsBranch" => {
                        current_instr_is_branch = text == "TRUE";
                    }
                    "IsProgramTerminator" => {
                        current_instr_is_term = text == "TRUE";
                    }
                    "Opcode" => {
                        if let Ok(v) = text.parse::<u16>() {
                            current_ie_opcode = v;
                        }
                    }
                    "EncodingCondition" => {
                        current_ie_condition = text;
                    }
                    _ => {}
                }
                current_text_target.clear();
            }
            Err(e) => {
                eprintln!("XML parse error: {e}");
                std::process::exit(1);
            }
            _ => {}
        }
        buf.clear();
    }

    // Sort instructions by opcode within each encoding
    for instrs in instructions.values_mut() {
        instrs.sort_by_key(|i| i.opcode);
    }

    (encoding_fields, instructions)
}

fn file_header() -> String {
    let mut out = String::new();
    writeln!(out, "// SPDX-License-Identifier: AGPL-3.0-only").unwrap();
    writeln!(out, "// Copyright © 2026 ecoPrimals").unwrap();
    writeln!(
        out,
        "//! AUTO-GENERATED from AMD RDNA2 ISA XML specification."
    )
    .unwrap();
    writeln!(out, "//!").unwrap();
    writeln!(
        out,
        "//! Source: specs/amd/amdgpu_isa_rdna2.xml (MIT license, AMD GPUOpen)"
    )
    .unwrap();
    writeln!(
        out,
        "//! Generator: tools/amd-isa-gen (pure Rust, sovereign toolchain)"
    )
    .unwrap();
    writeln!(out, "//!").unwrap();
    writeln!(out, "//! DO NOT EDIT BY HAND. Regenerate with:").unwrap();
    writeln!(out, "//!   cargo run -p amd-isa-gen").unwrap();
    writeln!(out).unwrap();
    out
}

fn generate_types_file() -> String {
    let mut out = file_header();
    writeln!(out, "/// Bit field within an encoding format.").unwrap();
    writeln!(out, "#[derive(Debug, Clone, Copy)]").unwrap();
    writeln!(out, "pub struct BitField {{").unwrap();
    writeln!(out, "    /// Bit offset within the instruction word(s).").unwrap();
    writeln!(out, "    pub offset: u32,").unwrap();
    writeln!(out, "    /// Number of bits.").unwrap();
    writeln!(out, "    pub width: u32,").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "/// Instruction entry in the opcode table.").unwrap();
    writeln!(out, "#[derive(Debug, Clone, Copy)]").unwrap();
    writeln!(out, "pub struct InstrEntry {{").unwrap();
    writeln!(out, "    /// Instruction mnemonic.").unwrap();
    writeln!(out, "    pub name: &'static str,").unwrap();
    writeln!(out, "    /// Numeric opcode within the encoding format.").unwrap();
    writeln!(out, "    pub opcode: u16,").unwrap();
    writeln!(out, "    /// Whether this instruction is a branch.").unwrap();
    writeln!(out, "    pub is_branch: bool,").unwrap();
    writeln!(
        out,
        "    /// Whether this instruction terminates the program."
    )
    .unwrap();
    writeln!(out, "    pub is_terminator: bool,").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    out
}

const MAX_LINES_PER_FILE: usize = 950;
const LINES_PER_INSTR: usize = 8;

/// Categorize VOP3 instruction for sub-table split (cmp, math, arith, logic).
fn vop3_category(name: &str) -> &'static str {
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

struct EncodingOutput {
    main_file: String,
    table_file: Option<String>,
    table_sub_files: Vec<(String, String)>,
}

fn generate_encoding_file(
    enc_name: &str,
    info: &EncodingInfo,
    instrs: Option<&Vec<InstrInfo>>,
) -> EncodingOutput {
    let mod_name = encoding_to_rust_mod(enc_name);
    let estimated_lines = instrs.map_or(0, |v| v.len() * LINES_PER_INSTR + 30);
    let needs_split = estimated_lines > MAX_LINES_PER_FILE;

    let mut out = file_header();
    writeln!(out, "use super::isa_types::{{BitField, InstrEntry}};").unwrap();

    let mut table_file = None;
    let mut table_sub_files = Vec::new();

    // VOP3: split into cmp, math, arith, logic sub-tables
    let vop3_sub_split = enc_name == "ENC_VOP3" && instrs.is_some();
    // VOPC: split into two halves (opcodes 0-63 vs 128-255)
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
        for cat in &vop3_cats {
            let cat_instrs = by_cat.get(*cat).unwrap();
            let mut tbl = file_header();
            writeln!(tbl, "use super::super::isa_types::InstrEntry;").unwrap();
            writeln!(tbl).unwrap();
            write_table_part(&mut tbl, cat_instrs);
            table_sub_files.push((format!("table_{cat}.rs"), tbl));
        }
        for cat in &vop3_cats {
            writeln!(out, "mod table_{cat};").unwrap();
        }
        writeln!(out).unwrap();
        writeln!(out, "use std::sync::OnceLock;").unwrap();
        writeln!(out).unwrap();
        writeln!(
            out,
            "static TABLE_CACHE: OnceLock<Vec<InstrEntry>> = OnceLock::new();"
        )
        .unwrap();
        writeln!(out).unwrap();
        writeln!(
            out,
            "/// All {enc_name} instructions (combined from sub-tables)."
        )
        .unwrap();
        writeln!(out, "#[must_use]").unwrap();
        writeln!(out, "pub fn table() -> &'static [InstrEntry] {{").unwrap();
        writeln!(out, "    TABLE_CACHE.get_or_init(|| {{").unwrap();
        write!(out, "        [").unwrap();
        for (i, cat) in vop3_cats.iter().enumerate() {
            if i > 0 {
                write!(out, ", ").unwrap();
            }
            write!(out, "table_{cat}::TABLE").unwrap();
        }
        writeln!(out, "].concat()").unwrap();
        writeln!(out, "    }}).as_slice()").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "/// Look up an instruction by opcode.").unwrap();
        writeln!(out, "#[must_use]").unwrap();
        writeln!(
            out,
            "pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {{"
        )
        .unwrap();
        for (i, cat) in vop3_cats.iter().enumerate() {
            if i == 0 {
                write!(out, "    table_{cat}::lookup(opcode)").unwrap();
            } else {
                write!(out, "\n        .or_else(|| table_{cat}::lookup(opcode))").unwrap();
            }
        }
        writeln!(out).unwrap();
        writeln!(out, "}}").unwrap();
    } else if vopc_split {
        let instrs = instrs.unwrap();
        let a: Vec<_> = instrs.iter().filter(|i| i.opcode < 64).cloned().collect();
        let b: Vec<_> = instrs.iter().filter(|i| i.opcode >= 64).cloned().collect();
        let mut tbl_a = file_header();
        writeln!(tbl_a, "use super::super::isa_types::InstrEntry;").unwrap();
        writeln!(tbl_a).unwrap();
        write_table_and_lookup(&mut tbl_a, "ENC_VOPC (F32/F64)", &a);
        let mut tbl_b = file_header();
        writeln!(tbl_b, "use super::super::isa_types::InstrEntry;").unwrap();
        writeln!(tbl_b).unwrap();
        write_table_and_lookup(&mut tbl_b, "ENC_VOPC (I/U/F16)", &b);
        table_sub_files.push(("table_a.rs".to_string(), tbl_a));
        table_sub_files.push(("table_b.rs".to_string(), tbl_b));
        writeln!(out, "mod table_a;").unwrap();
        writeln!(out, "mod table_b;").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "use std::sync::OnceLock;").unwrap();
        writeln!(out).unwrap();
        writeln!(
            out,
            "static TABLE_CACHE: OnceLock<Vec<InstrEntry>> = OnceLock::new();"
        )
        .unwrap();
        writeln!(out).unwrap();
        writeln!(
            out,
            "/// All {enc_name} instructions (combined from sub-tables)."
        )
        .unwrap();
        writeln!(out, "#[must_use]").unwrap();
        writeln!(out, "pub fn table() -> &'static [InstrEntry] {{").unwrap();
        writeln!(out, "    TABLE_CACHE.get_or_init(|| {{").unwrap();
        writeln!(out, "        [table_a::TABLE, table_b::TABLE].concat()").unwrap();
        writeln!(out, "    }}).as_slice()").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "/// Look up an instruction by opcode.").unwrap();
        writeln!(out, "#[must_use]").unwrap();
        writeln!(
            out,
            "pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {{"
        )
        .unwrap();
        writeln!(
            out,
            "    table_a::lookup(opcode).or_else(|| table_b::lookup(opcode))"
        )
        .unwrap();
        writeln!(out, "}}").unwrap();
    } else if needs_split {
        writeln!(out, "mod table;").unwrap();
        writeln!(out, "pub use table::{{TABLE, lookup}};").unwrap();
    }
    writeln!(out).unwrap();

    // Fields
    writeln!(out, "/// {enc_name} encoding fields ({} bits).", info.bits).unwrap();
    writeln!(out, "pub mod fields {{").unwrap();
    writeln!(out, "    use super::BitField;").unwrap();
    let mut sorted_fields = info.fields.clone();
    sorted_fields.sort_by_key(|f| f.offset);
    for field in &sorted_fields {
        let const_name = field.name.to_uppercase();
        writeln!(
            out,
            "    pub const {const_name}: BitField = BitField {{ offset: {}, width: {} }};",
            field.offset, field.width
        )
        .unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    if let Some(instrs) = instrs {
        // Opcode constants always go in the main file
        for instr in instrs {
            let const_name = instr.name.to_uppercase();
            if !instr.desc.is_empty() {
                writeln!(out, "/// {}", instr.desc).unwrap();
            }
            writeln!(out, "pub const {const_name}: u16 = {};", instr.opcode).unwrap();
        }

        if needs_split && !vop3_sub_split && !vopc_split {
            // TABLE + lookup go in a sub-file
            let mut tbl = file_header();
            writeln!(tbl, "use super::super::isa_types::InstrEntry;").unwrap();
            writeln!(tbl).unwrap();
            write_table_and_lookup(&mut tbl, enc_name, instrs);
            table_file = Some(tbl);
        } else if !vop3_sub_split && !vopc_split {
            writeln!(out).unwrap();
            write_table_and_lookup(&mut out, enc_name, instrs);
        }
    }
    writeln!(out).unwrap();

    let _ = mod_name;
    EncodingOutput {
        main_file: out,
        table_file,
        table_sub_files,
    }
}

fn write_table_part(out: &mut String, instrs: &[&InstrInfo]) {
    writeln!(out, "pub const TABLE: &[InstrEntry] = &[").unwrap();
    for instr in instrs {
        writeln!(
            out,
            "    InstrEntry {{ name: \"{}\", opcode: {}, is_branch: {}, is_terminator: {} }},",
            instr.name, instr.opcode, instr.is_branch, instr.is_terminator
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "#[must_use]").unwrap();
    writeln!(
        out,
        "pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {{"
    )
    .unwrap();
    writeln!(out, "    TABLE.iter().find(|e| e.opcode == opcode)").unwrap();
    writeln!(out, "}}").unwrap();
}

fn write_table_and_lookup(out: &mut String, enc_name: &str, instrs: &[InstrInfo]) {
    writeln!(out, "/// All {enc_name} instructions.").unwrap();
    writeln!(out, "pub const TABLE: &[InstrEntry] = &[").unwrap();
    for instr in instrs {
        writeln!(
            out,
            "    InstrEntry {{ name: \"{}\", opcode: {}, is_branch: {}, is_terminator: {} }},",
            instr.name, instr.opcode, instr.is_branch, instr.is_terminator
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "/// Look up an instruction by opcode.").unwrap();
    writeln!(out, "#[must_use]").unwrap();
    writeln!(
        out,
        "pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {{"
    )
    .unwrap();
    writeln!(out, "    TABLE.iter().find(|e| e.opcode == opcode)").unwrap();
    writeln!(out, "}}").unwrap();
}

fn generate_mod_file(
    encoding_fields: &BTreeMap<String, EncodingInfo>,
    instructions: &BTreeMap<String, Vec<InstrInfo>>,
) -> String {
    let mut out = file_header();

    writeln!(
        out,
        "#[allow(dead_code, missing_docs, reason = \"generated ISA tables from amd-isa-gen\")]"
    )
    .unwrap();
    writeln!(out, "pub mod isa_types;").unwrap();
    writeln!(out).unwrap();

    for enc_name in encoding_fields.keys() {
        let mod_name = encoding_to_rust_mod(enc_name);
        writeln!(out, "#[allow(dead_code, missing_docs, unused_imports, reason = \"generated ISA tables from amd-isa-gen\")]").unwrap();
        writeln!(out, "pub mod {mod_name};").unwrap();
    }
    writeln!(out).unwrap();

    let total: usize = instructions.values().map(Vec::len).sum();
    writeln!(
        out,
        "/// Total instruction count across all compute-relevant encodings: {total}"
    )
    .unwrap();
    writeln!(out, "pub const TOTAL_INSTRUCTIONS: usize = {total};").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "/// Look up encoding width in bits by name.").unwrap();
    writeln!(out, "#[must_use]").unwrap();
    writeln!(out, "pub fn encoding_bits(name: &str) -> Option<u32> {{").unwrap();
    writeln!(out, "    match name {{").unwrap();
    for (enc_name, info) in encoding_fields {
        writeln!(out, "        \"{enc_name}\" => Some({}),", info.bits).unwrap();
    }
    writeln!(out, "        _ => None,").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    out
}

fn main() {
    let root = repo_root();
    let xml_path = root.join("specs").join("amd").join("amdgpu_isa_rdna2.xml");
    let output_dir = root
        .join("crates")
        .join("coral-reef")
        .join("src")
        .join("codegen")
        .join("amd")
        .join("isa_generated");

    let (encoding_fields, instructions) = parse_xml(&xml_path);

    fs::create_dir_all(&output_dir).unwrap_or_else(|e| {
        eprintln!("ERROR: Cannot create {}: {e}", output_dir.display());
        std::process::exit(1);
    });

    let write_file = |name: &str, content: &str| {
        let path = output_dir.join(name);
        fs::write(&path, content).unwrap_or_else(|e| {
            eprintln!("ERROR: Cannot write {}: {e}", path.display());
            std::process::exit(1);
        });
        let lines = content.lines().count();
        println!("  {name}: {lines} lines");
    };

    write_file(
        "mod.rs",
        &generate_mod_file(&encoding_fields, &instructions),
    );
    write_file("isa_types.rs", &generate_types_file());

    for (enc_name, info) in &encoding_fields {
        let mod_name = encoding_to_rust_mod(enc_name);
        let instrs = instructions.get(enc_name);
        let output = generate_encoding_file(enc_name, info, instrs);

        if output.table_file.is_some() || !output.table_sub_files.is_empty() {
            let enc_dir = output_dir.join(&mod_name);
            fs::create_dir_all(&enc_dir).unwrap_or_else(|e| {
                eprintln!("ERROR: Cannot create {}: {e}", enc_dir.display());
                std::process::exit(1);
            });
            let write_enc = |name: &str, content: &str| {
                let path = enc_dir.join(name);
                fs::write(&path, content).unwrap_or_else(|e| {
                    eprintln!("ERROR: Cannot write {}: {e}", path.display());
                    std::process::exit(1);
                });
                let lines = content.lines().count();
                println!("  {mod_name}/{name}: {lines} lines");
            };
            write_enc("mod.rs", &output.main_file);
            if let Some(tbl) = &output.table_file {
                write_enc("table.rs", tbl);
            }
            for (name, content) in &output.table_sub_files {
                write_enc(name, content);
            }
            // Remove old table.rs when we use sub-tables (vop3, vopc)
            if !output.table_sub_files.is_empty() {
                let old_table = enc_dir.join("table.rs");
                let _ = fs::remove_file(&old_table);
            }
        } else {
            let filename = format!("{mod_name}.rs");
            write_file(&filename, &output.main_file);
        }
    }

    let total: usize = instructions.values().map(std::vec::Vec::len).sum();
    println!("Generated {}", output_dir.display());
    println!("  Encodings: {}", encoding_fields.len());
    println!("  Instructions: {total}");
    for (enc_name, instrs) in &instructions {
        println!("    {enc_name}: {}", instrs.len());
    }
}
