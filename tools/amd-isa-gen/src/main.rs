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

fn generate_rust(
    encoding_fields: &BTreeMap<String, EncodingInfo>,
    instructions: &BTreeMap<String, Vec<InstrInfo>>,
) -> String {
    let mut out = String::with_capacity(256 * 1024);

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
    writeln!(out, "#![allow(dead_code, missing_docs)]").unwrap();
    writeln!(out).unwrap();

    // Bit field struct
    writeln!(out, "/// Bit field within an encoding format.").unwrap();
    writeln!(out, "#[derive(Debug, Clone, Copy)]").unwrap();
    writeln!(out, "pub struct BitField {{").unwrap();
    writeln!(out, "    /// Bit offset within the instruction word(s).").unwrap();
    writeln!(out, "    pub offset: u32,").unwrap();
    writeln!(out, "    /// Number of bits.").unwrap();
    writeln!(out, "    pub width: u32,").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Encoding field layouts
    for (enc_name, info) in encoding_fields {
        let mod_name = encoding_to_rust_mod(enc_name);
        writeln!(out, "/// {enc_name} encoding fields ({} bits).", info.bits).unwrap();
        writeln!(out, "pub mod {mod_name}_fields {{").unwrap();
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
    }

    // Instruction entry struct
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

    // Opcode modules
    let mut total_instrs = 0usize;
    for (enc_name, instrs) in instructions {
        let mod_name = encoding_to_rust_mod(enc_name);
        total_instrs += instrs.len();

        writeln!(
            out,
            "/// {enc_name} opcodes ({} instructions).",
            instrs.len()
        )
        .unwrap();
        writeln!(out, "pub mod {mod_name} {{").unwrap();

        for instr in instrs {
            let const_name = instr.name.to_uppercase();
            if !instr.desc.is_empty() {
                writeln!(out, "    /// {}", instr.desc).unwrap();
            }
            writeln!(out, "    pub const {const_name}: u16 = {};", instr.opcode).unwrap();
        }

        writeln!(out).unwrap();
        writeln!(out, "    /// All {enc_name} instructions.").unwrap();
        writeln!(out, "    pub const TABLE: &[super::InstrEntry] = &[").unwrap();
        for instr in instrs {
            writeln!(
                out,
                "        super::InstrEntry {{ name: \"{}\", opcode: {}, is_branch: {}, is_terminator: {} }},",
                instr.name, instr.opcode, instr.is_branch, instr.is_terminator
            )
            .unwrap();
        }
        writeln!(out, "    ];").unwrap();

        writeln!(out).unwrap();
        writeln!(out, "    /// Look up an instruction by opcode.").unwrap();
        writeln!(
            out,
            "    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {{"
        )
        .unwrap();
        writeln!(out, "        TABLE.iter().find(|e| e.opcode == opcode)").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }

    writeln!(
        out,
        "/// Total instruction count across all compute-relevant encodings: {total_instrs}"
    )
    .unwrap();
    writeln!(out, "pub const TOTAL_INSTRUCTIONS: usize = {total_instrs};").unwrap();
    writeln!(out).unwrap();

    // Encoding bits lookup
    writeln!(out, "/// Look up encoding field info by name.").unwrap();
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
    let output_path = root
        .join("crates")
        .join("coral-reef")
        .join("src")
        .join("codegen")
        .join("amd")
        .join("isa_generated.rs");

    let (encoding_fields, instructions) = parse_xml(&xml_path);
    let rust_code = generate_rust(&encoding_fields, &instructions);

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&output_path, &rust_code).unwrap_or_else(|e| {
        eprintln!("ERROR: Cannot write {}: {e}", output_path.display());
        std::process::exit(1);
    });

    let total: usize = instructions.values().map(std::vec::Vec::len).sum();
    println!("Generated {}", output_path.display());
    println!("  Encodings: {}", encoding_fields.len());
    println!("  Instructions: {total}");
    for (enc_name, instrs) in &instructions {
        println!("    {enc_name}: {}", instrs.len());
    }
}
