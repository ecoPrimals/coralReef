// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
#![forbid(unsafe_code)]
#![warn(missing_docs)]
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

mod generate;
mod parse;

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map_or_else(|| PathBuf::from("."), std::path::Path::to_path_buf)
}

fn main() -> Result<()> {
    let root = repo_root();
    let xml_path = root.join("specs").join("amd").join("amdgpu_isa_rdna2.xml");
    let output_dir = root
        .join("crates")
        .join("coral-reef")
        .join("src")
        .join("codegen")
        .join("amd")
        .join("isa_generated");

    let (encoding_fields, instructions) = parse::parse_xml(&xml_path)?;

    fs::create_dir_all(&output_dir)
        .with_context(|| format!("Cannot create output directory {}", output_dir.display()))?;

    let write_file = |name: &str, content: &str| -> Result<()> {
        let path = output_dir.join(name);
        fs::write(&path, content).with_context(|| format!("Cannot write {}", path.display()))?;
        let lines = content.lines().count();
        println!("  {name}: {lines} lines");
        Ok(())
    };

    write_file(
        "mod.rs",
        &generate::generate_mod_file(&encoding_fields, &instructions)?,
    )?;
    write_file("isa_types.rs", &generate::generate_types_file()?)?;

    for (enc_name, info) in &encoding_fields {
        let mod_name = parse::encoding_to_rust_mod(enc_name);
        let instrs = instructions.get(enc_name);
        let output = generate::generate_encoding_file(enc_name, info, instrs)?;

        if output.table_file.is_some() || !output.table_sub_files.is_empty() {
            let enc_dir = output_dir.join(&mod_name);
            fs::create_dir_all(&enc_dir).with_context(|| {
                format!("Cannot create encoding directory {}", enc_dir.display())
            })?;
            let write_enc = |name: &str, content: &str| -> Result<()> {
                let path = enc_dir.join(name);
                fs::write(&path, content)
                    .with_context(|| format!("Cannot write {}", path.display()))?;
                let lines = content.lines().count();
                println!("  {mod_name}/{name}: {lines} lines");
                Ok(())
            };
            write_enc("mod.rs", &output.main_file)?;
            if let Some(tbl) = &output.table_file {
                write_enc("table.rs", tbl)?;
            }
            for (name, content) in &output.table_sub_files {
                write_enc(name, content)?;
            }
            if !output.table_sub_files.is_empty() {
                let old_table = enc_dir.join("table.rs");
                let _ = fs::remove_file(&old_table);
            }
        } else {
            let filename = format!("{mod_name}.rs");
            write_file(&filename, &output.main_file)?;
        }
    }

    let total: usize = instructions.values().map(std::vec::Vec::len).sum();
    println!("Generated {}", output_dir.display());
    println!("  Encodings: {}", encoding_fields.len());
    println!("  Instructions: {total}");
    for (enc_name, instrs) in &instructions {
        println!("    {enc_name}: {}", instrs.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::Write;

    fn write_xml_temp(xml: &str) -> std::path::PathBuf {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(xml.as_bytes()).unwrap();
        f.flush().unwrap();
        f.into_temp_path().keep().unwrap()
    }

    #[test]
    fn parse_xml_basic_encoding_and_instruction() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <Encoding>
    <EncodingName>ENC_SOP1</EncodingName>
    <BitCount>32</BitCount>
    <BitMap>
      <Field>
        <FieldName>OP</FieldName>
        <BitOffset>0</BitOffset>
        <BitCount>8</BitCount>
      </Field>
    </BitMap>
  </Encoding>
  <Instruction>
    <InstructionName>S_MOV_B32</InstructionName>
    <Description>Move value</Description>
    <IsBranch>FALSE</IsBranch>
    <IsProgramTerminator>FALSE</IsProgramTerminator>
    <InstructionEncoding>
      <EncodingName>ENC_SOP1</EncodingName>
      <Opcode>3</Opcode>
      <EncodingCondition>default</EncodingCondition>
    </InstructionEncoding>
  </Instruction>
</root>"#;
        let path = write_xml_temp(xml);
        let (encodings, instructions) = parse::parse_xml(&path).unwrap();
        assert_eq!(encodings.len(), 1);
        let info = encodings.get("ENC_SOP1").unwrap();
        assert_eq!(info.bits, 32);
        assert_eq!(info.fields.len(), 1);
        assert_eq!(info.fields[0].name, "OP");
        assert_eq!(info.fields[0].offset, 0);
        assert_eq!(info.fields[0].width, 8);
        let instrs = instructions.get("ENC_SOP1").unwrap();
        assert_eq!(instrs.len(), 1);
        assert_eq!(instrs[0].name, "S_MOV_B32");
        assert_eq!(instrs[0].opcode, 3);
        assert_eq!(instrs[0].desc, "Move value");
        assert!(!instrs[0].is_branch);
        assert!(!instrs[0].is_terminator);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_xml_multiple_encodings() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <Encoding>
    <EncodingName>ENC_SOP1</EncodingName>
    <BitCount>32</BitCount>
    <BitMap>
      <Field><FieldName>OP</FieldName><BitOffset>0</BitOffset><BitCount>8</BitCount></Field>
    </BitMap>
  </Encoding>
  <Encoding>
    <EncodingName>ENC_VOP1</EncodingName>
    <BitCount>64</BitCount>
    <BitMap>
      <Field><FieldName>OP</FieldName><BitOffset>18</BitOffset><BitCount>9</BitCount></Field>
    </BitMap>
  </Encoding>
</root>"#;
        let path = write_xml_temp(xml);
        let (encodings, _) = parse::parse_xml(&path).unwrap();
        assert_eq!(encodings.len(), 2);
        assert_eq!(encodings.get("ENC_SOP1").unwrap().bits, 32);
        assert_eq!(encodings.get("ENC_VOP1").unwrap().bits, 64);
        assert_eq!(encodings.get("ENC_VOP1").unwrap().fields[0].offset, 18);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_xml_branch_and_terminator_flags() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <Encoding>
    <EncodingName>ENC_SOPP</EncodingName>
    <BitCount>32</BitCount>
    <BitMap>
      <Field><FieldName>OP</FieldName><BitOffset>0</BitOffset><BitCount>8</BitCount></Field>
    </BitMap>
  </Encoding>
  <Instruction>
    <InstructionName>S_BRANCH</InstructionName>
    <Description>Branch</Description>
    <IsBranch>TRUE</IsBranch>
    <IsProgramTerminator>FALSE</IsProgramTerminator>
    <InstructionEncoding>
      <EncodingName>ENC_SOPP</EncodingName>
      <Opcode>1</Opcode>
      <EncodingCondition>default</EncodingCondition>
    </InstructionEncoding>
  </Instruction>
  <Instruction>
    <InstructionName>S_ENDPGM</InstructionName>
    <Description>End program</Description>
    <IsBranch>FALSE</IsBranch>
    <IsProgramTerminator>TRUE</IsProgramTerminator>
    <InstructionEncoding>
      <EncodingName>ENC_SOPP</EncodingName>
      <Opcode>2</Opcode>
      <EncodingCondition>default</EncodingCondition>
    </InstructionEncoding>
  </Instruction>
</root>"#;
        let path = write_xml_temp(xml);
        let (_, instructions) = parse::parse_xml(&path).unwrap();
        let instrs = instructions.get("ENC_SOPP").unwrap();
        let br = instrs.iter().find(|i| i.name == "S_BRANCH").unwrap();
        assert!(br.is_branch);
        assert!(!br.is_terminator);
        let term = instrs.iter().find(|i| i.name == "S_ENDPGM").unwrap();
        assert!(!term.is_branch);
        assert!(term.is_terminator);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_xml_non_compute_encoding_excluded() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <Encoding>
    <EncodingName>ENC_GRAPHICS</EncodingName>
    <BitCount>32</BitCount>
    <BitMap>
      <Field><FieldName>OP</FieldName><BitOffset>0</BitOffset><BitCount>8</BitCount></Field>
    </BitMap>
  </Encoding>
</root>"#;
        let path = write_xml_temp(xml);
        let (encodings, instructions) = parse::parse_xml(&path).unwrap();
        assert!(encodings.is_empty());
        assert!(instructions.is_empty());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_xml_description_truncation() {
        let long_desc = "a".repeat(150);
        let xml = format!(
            r#"<?xml version="1.0"?>
<root>
  <Encoding>
    <EncodingName>ENC_SOP1</EncodingName>
    <BitCount>32</BitCount>
    <BitMap>
      <Field><FieldName>OP</FieldName><BitOffset>0</BitOffset><BitCount>8</BitCount></Field>
    </BitMap>
  </Encoding>
  <Instruction>
    <InstructionName>S_NOP</InstructionName>
    <Description>{}</Description>
    <IsBranch>FALSE</IsBranch>
    <IsProgramTerminator>FALSE</IsProgramTerminator>
    <InstructionEncoding>
      <EncodingName>ENC_SOP1</EncodingName>
      <Opcode>0</Opcode>
      <EncodingCondition>default</EncodingCondition>
    </InstructionEncoding>
  </Instruction>
</root>"#,
            long_desc
        );
        let path = write_xml_temp(&xml);
        let (_, instructions) = parse::parse_xml(&path).unwrap();
        let desc = &instructions.get("ENC_SOP1").unwrap()[0].desc;
        assert_eq!(desc.len(), 120);
        assert!(desc.ends_with("..."));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_xml_duplicate_encoding_dedup() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <Encoding>
    <EncodingName>ENC_SOP1</EncodingName>
    <BitCount>32</BitCount>
    <BitMap>
      <Field><FieldName>OP</FieldName><BitOffset>0</BitOffset><BitCount>8</BitCount></Field>
    </BitMap>
  </Encoding>
  <Instruction>
    <InstructionName>S_MOV_B32</InstructionName>
    <Description>Move</Description>
    <IsBranch>FALSE</IsBranch>
    <IsProgramTerminator>FALSE</IsProgramTerminator>
    <InstructionEncoding>
      <EncodingName>ENC_SOP1</EncodingName>
      <Opcode>3</Opcode>
      <EncodingCondition>default</EncodingCondition>
    </InstructionEncoding>
    <InstructionEncoding>
      <EncodingName>ENC_SOP1</EncodingName>
      <Opcode>3</Opcode>
      <EncodingCondition>default</EncodingCondition>
    </InstructionEncoding>
  </Instruction>
</root>"#;
        let path = write_xml_temp(xml);
        let (_, instructions) = parse::parse_xml(&path).unwrap();
        let instrs = instructions.get("ENC_SOP1").unwrap();
        assert_eq!(instrs.len(), 1);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_xml_instructions_sorted_by_opcode() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <Encoding>
    <EncodingName>ENC_SOP1</EncodingName>
    <BitCount>32</BitCount>
    <BitMap>
      <Field><FieldName>OP</FieldName><BitOffset>0</BitOffset><BitCount>8</BitCount></Field>
    </BitMap>
  </Encoding>
  <Instruction>
    <InstructionName>INSTR_Z</InstructionName>
    <Description>Z</Description>
    <IsBranch>FALSE</IsBranch>
    <IsProgramTerminator>FALSE</IsProgramTerminator>
    <InstructionEncoding>
      <EncodingName>ENC_SOP1</EncodingName>
      <Opcode>10</Opcode>
      <EncodingCondition>default</EncodingCondition>
    </InstructionEncoding>
  </Instruction>
  <Instruction>
    <InstructionName>INSTR_A</InstructionName>
    <Description>A</Description>
    <IsBranch>FALSE</IsBranch>
    <IsProgramTerminator>FALSE</IsProgramTerminator>
    <InstructionEncoding>
      <EncodingName>ENC_SOP1</EncodingName>
      <Opcode>1</Opcode>
      <EncodingCondition>default</EncodingCondition>
    </InstructionEncoding>
  </Instruction>
</root>"#;
        let path = write_xml_temp(xml);
        let (_, instructions) = parse::parse_xml(&path).unwrap();
        let instrs = instructions.get("ENC_SOP1").unwrap();
        assert_eq!(instrs[0].opcode, 1);
        assert_eq!(instrs[0].name, "INSTR_A");
        assert_eq!(instrs[1].opcode, 10);
        assert_eq!(instrs[1].name, "INSTR_Z");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_xml_empty_no_encodings() {
        let xml = r#"<?xml version="1.0"?><root></root>"#;
        let path = write_xml_temp(xml);
        let (encodings, instructions) = parse::parse_xml(&path).unwrap();
        assert!(encodings.is_empty());
        assert!(instructions.is_empty());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_xml_encoding_with_no_instructions() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <Encoding>
    <EncodingName>ENC_SOP1</EncodingName>
    <BitCount>32</BitCount>
    <BitMap>
      <Field><FieldName>OP</FieldName><BitOffset>0</BitOffset><BitCount>8</BitCount></Field>
    </BitMap>
  </Encoding>
</root>"#;
        let path = write_xml_temp(xml);
        let (encodings, instructions) = parse::parse_xml(&path).unwrap();
        assert_eq!(encodings.len(), 1);
        assert!(instructions.get("ENC_SOP1").is_none_or(|v| v.is_empty()));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_xml_non_default_condition_excluded() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <Encoding>
    <EncodingName>ENC_SOP1</EncodingName>
    <BitCount>32</BitCount>
    <BitMap>
      <Field><FieldName>OP</FieldName><BitOffset>0</BitOffset><BitCount>8</BitCount></Field>
    </BitMap>
  </Encoding>
  <Instruction>
    <InstructionName>S_MOV_B32</InstructionName>
    <Description>Move</Description>
    <IsBranch>FALSE</IsBranch>
    <IsProgramTerminator>FALSE</IsProgramTerminator>
    <InstructionEncoding>
      <EncodingName>ENC_SOP1</EncodingName>
      <Opcode>3</Opcode>
      <EncodingCondition>wave32</EncodingCondition>
    </InstructionEncoding>
  </Instruction>
</root>"#;
        let path = write_xml_temp(xml);
        let (encodings, instructions) = parse::parse_xml(&path).unwrap();
        assert_eq!(encodings.len(), 1);
        assert!(instructions.get("ENC_SOP1").is_none_or(|v| v.is_empty()));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_xml_description_escape() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <Encoding>
    <EncodingName>ENC_SOP1</EncodingName>
    <BitCount>32</BitCount>
    <BitMap>
      <Field><FieldName>OP</FieldName><BitOffset>0</BitOffset><BitCount>8</BitCount></Field>
    </BitMap>
  </Encoding>
  <Instruction>
    <InstructionName>S_MOV_B32</InstructionName>
    <Description>Backslash \ and "quote"</Description>
    <IsBranch>FALSE</IsBranch>
    <IsProgramTerminator>FALSE</IsProgramTerminator>
    <InstructionEncoding>
      <EncodingName>ENC_SOP1</EncodingName>
      <Opcode>3</Opcode>
      <EncodingCondition>default</EncodingCondition>
    </InstructionEncoding>
  </Instruction>
</root>"#;
        let path = write_xml_temp(xml);
        let (_, instructions) = parse::parse_xml(&path).unwrap();
        let desc = &instructions.get("ENC_SOP1").unwrap()[0].desc;
        assert!(desc.contains("\\\\"));
        assert!(desc.contains("\\\""));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_xml_invalid_xml_error() {
        let path = write_xml_temp("<root></mismatched>");
        let result = parse::parse_xml(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("XML parse error"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn parse_xml_encoding_no_fields_excluded() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <Encoding>
    <EncodingName>ENC_SOP1</EncodingName>
    <BitCount>32</BitCount>
    <BitMap></BitMap>
  </Encoding>
</root>"#;
        let path = write_xml_temp(xml);
        let (encodings, _) = parse::parse_xml(&path).unwrap();
        assert!(encodings.is_empty());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn encoding_to_rust_mod_strips_prefix() {
        assert_eq!(parse::encoding_to_rust_mod("ENC_SOP1"), "sop1");
        assert_eq!(parse::encoding_to_rust_mod("ENC_VOP3P"), "vop3p");
        assert_eq!(parse::encoding_to_rust_mod("ENC_FLAT_GLBL"), "flat_glbl");
    }

    #[test]
    fn file_header_contains_spdx() {
        let header = generate::file_header().unwrap();
        assert!(header.contains("SPDX-License-Identifier: AGPL-3.0-or-later"));
        assert!(header.contains("AUTO-GENERATED"));
        assert!(header.contains("DO NOT EDIT BY HAND"));
    }

    #[test]
    fn generate_types_has_bitfield_and_instrentry() {
        let types = generate::generate_types_file().unwrap();
        assert!(types.contains("pub struct BitField"));
        assert!(types.contains("pub struct InstrEntry"));
        assert!(types.contains("pub offset: u32"));
        assert!(types.contains("pub opcode: u16"));
    }

    #[test]
    fn vop3_category_classifies_correctly() {
        assert_eq!(generate::vop3_category("V_CMP_F32_E64"), "cmp");
        assert_eq!(generate::vop3_category("V_CMPX_LT_U32"), "cmp");
        assert_eq!(generate::vop3_category("V_ADD_F32_E64"), "arith");
        assert_eq!(generate::vop3_category("V_SIN_F32"), "math");
        assert_eq!(generate::vop3_category("V_AND_B32"), "logic");
        assert_eq!(generate::vop3_category("V_MUL_F32_E64"), "arith");
        assert_eq!(generate::vop3_category("V_UNKNOWN_OP"), "arith");
    }

    #[test]
    fn max_lines_per_file_is_under_1000() {
        const { assert!(generate::MAX_LINES_PER_FILE < 1000) };
    }

    #[test]
    fn compute_encodings_contains_expected() {
        assert!(parse::COMPUTE_ENCODINGS.contains(&"ENC_SOP1"));
        assert!(parse::COMPUTE_ENCODINGS.contains(&"ENC_VOP3"));
        assert!(parse::COMPUTE_ENCODINGS.contains(&"ENC_DS"));
        assert!(!parse::COMPUTE_ENCODINGS.contains(&"ENC_GRAPHICS"));
    }

    #[test]
    fn repo_root_returns_valid_path() {
        let root = repo_root();
        assert!(root.components().count() >= 1);
    }

    #[test]
    fn encoding_to_rust_mod_no_prefix() {
        assert_eq!(parse::encoding_to_rust_mod("NOPREFIX"), "noprefix");
    }

    #[test]
    fn generate_encoding_produces_valid_rust() {
        let info = parse::EncodingInfo {
            bits: 64,
            fields: vec![parse::BitField {
                name: "OP".to_string(),
                offset: 0,
                width: 8,
            }],
        };
        let instrs = vec![parse::InstrInfo {
            name: "V_ADD_F32".to_string(),
            opcode: 3,
            desc: "Add float".to_string(),
            is_branch: false,
            is_terminator: false,
        }];
        let out = generate::generate_encoding_file("ENC_VOP1", &info, Some(&instrs)).unwrap();
        assert!(out.main_file.contains("V_ADD_F32"));
        assert!(out.main_file.contains("opcode: 3"));
    }

    #[test]
    fn generate_encoding_vop3_sub_split() {
        let info = parse::EncodingInfo {
            bits: 64,
            fields: vec![parse::BitField {
                name: "OP".to_string(),
                offset: 0,
                width: 8,
            }],
        };
        let instrs = vec![
            parse::InstrInfo {
                name: "V_CMP_F32_E64".to_string(),
                opcode: 0,
                desc: "Cmp".to_string(),
                is_branch: false,
                is_terminator: false,
            },
            parse::InstrInfo {
                name: "V_ADD_F32_E64".to_string(),
                opcode: 1,
                desc: "Add".to_string(),
                is_branch: false,
                is_terminator: false,
            },
            parse::InstrInfo {
                name: "V_SIN_F32".to_string(),
                opcode: 2,
                desc: "Sin".to_string(),
                is_branch: false,
                is_terminator: false,
            },
            parse::InstrInfo {
                name: "V_AND_B32".to_string(),
                opcode: 3,
                desc: "And".to_string(),
                is_branch: false,
                is_terminator: false,
            },
        ];
        let out = generate::generate_encoding_file("ENC_VOP3", &info, Some(&instrs)).unwrap();
        assert!(out.main_file.contains("mod table_cmp_f32_f64"));
        assert!(out.main_file.contains("mod table_cmp_int"));
        assert!(out.main_file.contains("mod table_arith"));
        assert!(out.main_file.contains("mod table_math"));
        assert!(out.main_file.contains("mod table_logic"));
        assert_eq!(out.table_sub_files.len(), 5);
        assert!(out.table_file.is_none());
    }

    #[test]
    fn generate_encoding_vopc_split() {
        let info = parse::EncodingInfo {
            bits: 64,
            fields: vec![parse::BitField {
                name: "OP".to_string(),
                offset: 0,
                width: 8,
            }],
        };
        let instrs = vec![
            parse::InstrInfo {
                name: "V_CMP_F32".to_string(),
                opcode: 10,
                desc: "".to_string(),
                is_branch: false,
                is_terminator: false,
            },
            parse::InstrInfo {
                name: "V_CMP_F16".to_string(),
                opcode: 80,
                desc: "".to_string(),
                is_branch: false,
                is_terminator: false,
            },
        ];
        let out = generate::generate_encoding_file("ENC_VOPC", &info, Some(&instrs)).unwrap();
        assert!(out.main_file.contains("mod table_a"));
        assert!(out.main_file.contains("mod table_b"));
        assert_eq!(out.table_sub_files.len(), 2);
        assert!(out.table_sub_files[0].0 == "table_a.rs");
        assert!(out.table_sub_files[1].0 == "table_b.rs");
    }

    #[test]
    fn generate_encoding_needs_split() {
        let info = parse::EncodingInfo {
            bits: 64,
            fields: vec![parse::BitField {
                name: "OP".to_string(),
                offset: 0,
                width: 8,
            }],
        };
        let instrs: Vec<parse::InstrInfo> = (0..120)
            .map(|i| parse::InstrInfo {
                name: format!("INSTR_{i}"),
                opcode: i as u16,
                desc: "".to_string(),
                is_branch: false,
                is_terminator: false,
            })
            .collect();
        let out = generate::generate_encoding_file("ENC_SOP1", &info, Some(&instrs)).unwrap();
        assert!(out.main_file.contains("mod table"));
        assert!(out.main_file.contains("pub use table::"));
        assert!(out.table_file.is_some());
    }

    #[test]
    fn generate_encoding_no_instructions() {
        let info = parse::EncodingInfo {
            bits: 32,
            fields: vec![parse::BitField {
                name: "OP".to_string(),
                offset: 0,
                width: 8,
            }],
        };
        let out = generate::generate_encoding_file("ENC_SOP1", &info, None).unwrap();
        assert!(out.main_file.contains("fields"));
        assert!(out.table_file.is_none());
        assert!(out.table_sub_files.is_empty());
    }

    #[test]
    fn vop3_category_cmp_int_vs_cmp_float() {
        assert_eq!(generate::vop3_category("V_CMP_F32_E64"), "cmp");
        assert_eq!(generate::vop3_category("V_CMP_LT_U32"), "cmp");
        assert_eq!(generate::vop3_category("V_CMPX_GT_I32"), "cmp");
    }

    #[test]
    fn vop3_category_math_ops() {
        assert_eq!(generate::vop3_category("V_SQRT_F32"), "math");
        assert_eq!(generate::vop3_category("V_RCP_F64"), "math");
        assert_eq!(generate::vop3_category("V_LOG_F32"), "math");
        assert_eq!(generate::vop3_category("V_EXP_F16"), "math");
    }

    #[test]
    fn generate_types_file_contains_required_fields() {
        let types = generate::generate_types_file().unwrap();
        assert!(types.contains("is_branch"));
        assert!(types.contains("is_terminator"));
        assert!(types.contains("BitField"));
        assert!(types.contains("InstrEntry"));
    }

    #[test]
    fn file_header_contains_generator_info() {
        let header = generate::file_header().unwrap();
        assert!(header.contains("amd-isa-gen"));
        assert!(header.contains("cargo run -p amd-isa-gen"));
    }

    #[test]
    fn generate_mod_file() {
        let mut encoding_fields = BTreeMap::new();
        encoding_fields.insert(
            "ENC_SOP1".to_string(),
            parse::EncodingInfo {
                bits: 32,
                fields: vec![parse::BitField {
                    name: "OP".to_string(),
                    offset: 0,
                    width: 8,
                }],
            },
        );
        let mut instructions = BTreeMap::new();
        instructions.insert(
            "ENC_SOP1".to_string(),
            vec![parse::InstrInfo {
                name: "S_MOV_B32".to_string(),
                opcode: 3,
                desc: "Move".to_string(),
                is_branch: false,
                is_terminator: false,
            }],
        );
        let out = generate::generate_mod_file(&encoding_fields, &instructions).unwrap();
        assert!(out.contains("pub mod sop1"));
        assert!(out.contains("TOTAL_INSTRUCTIONS: usize = 1"));
        assert!(out.contains("\"ENC_SOP1\" => Some(32)"));
    }
}
