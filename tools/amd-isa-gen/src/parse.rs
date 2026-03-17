// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! XML parsing for AMD RDNA2 ISA specification.

use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;

/// Compute-relevant encoding names from the AMD ISA XML.
pub const COMPUTE_ENCODINGS: &[&str] = &[
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

/// Bit field within an encoding format.
#[derive(Debug, Clone)]
pub struct BitField {
    pub name: String,
    pub offset: u32,
    pub width: u32,
}

/// Encoding format metadata.
#[derive(Debug, Clone)]
pub struct EncodingInfo {
    pub bits: u32,
    pub fields: Vec<BitField>,
}

/// Instruction metadata from the XML.
#[derive(Debug, Clone)]
pub struct InstrInfo {
    pub name: String,
    pub opcode: u16,
    pub desc: String,
    pub is_branch: bool,
    pub is_terminator: bool,
}

/// Convert encoding name to Rust module name (e.g. ENC_SOP1 → sop1).
pub fn encoding_to_rust_mod(enc: &str) -> String {
    enc.replace("ENC_", "").to_lowercase()
}

/// Parse the AMD ISA XML into encoding fields and instruction tables.
pub fn parse_xml(
    xml_path: &std::path::Path,
) -> Result<(
    BTreeMap<String, EncodingInfo>,
    BTreeMap<String, Vec<InstrInfo>>,
)> {
    let xml_content = fs::read_to_string(xml_path).with_context(|| {
        format!(
            "Cannot read {} (download from: https://gpuopen.com/download/machine-readable-isa/latest/)",
            xml_path.display()
        )
    })?;

    let compute_set: BTreeSet<&str> = COMPUTE_ENCODINGS.iter().copied().collect();
    let mut encoding_fields: BTreeMap<String, EncodingInfo> = BTreeMap::new();
    let mut instructions: BTreeMap<String, Vec<InstrInfo>> = BTreeMap::new();
    let mut instr_global_info: HashMap<String, InstrInfo> = HashMap::new();

    let mut reader = Reader::from_str(&xml_content);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut path: Vec<String> = Vec::new();

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
                    "BitMap" if in_encoding => in_bitmap = true,
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
                    | "EncodingCondition" => current_text_target = tag,
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
                    "BitMap" => in_bitmap = false,
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
                let text = e.decode().unwrap_or_default().to_string();
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
                    "InstructionName" => current_instr_name = text,
                    "Description" => current_instr_desc = text,
                    "IsBranch" => current_instr_is_branch = text == "TRUE",
                    "IsProgramTerminator" => current_instr_is_term = text == "TRUE",
                    "Opcode" => {
                        if let Ok(v) = text.parse::<u16>() {
                            current_ie_opcode = v;
                        }
                    }
                    "EncodingCondition" => current_ie_condition = text,
                    _ => {}
                }
                current_text_target.clear();
            }
            Err(e) => return Err(anyhow::anyhow!("XML parse error: {e}")),
            _ => {}
        }
        buf.clear();
    }

    for instrs in instructions.values_mut() {
        instrs.sort_by_key(|i| i.opcode);
    }

    Ok((encoding_fields, instructions))
}
