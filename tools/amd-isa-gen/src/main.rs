// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![allow(
    clippy::doc_markdown,
    reason = "ISA mnemonics in docs are not Rust identifiers"
)]
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
#[path = "main_tests.rs"]
mod tests;
