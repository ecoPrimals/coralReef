// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn parse_u32_pairs_basic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.bin");
    // Two pairs: (0x100, 0x42) and (0x200, 0xFF)
    let mut data = Vec::new();
    data.extend_from_slice(&0x100u32.to_le_bytes());
    data.extend_from_slice(&0x42u32.to_le_bytes());
    data.extend_from_slice(&0x200u32.to_le_bytes());
    data.extend_from_slice(&0xFFu32.to_le_bytes());
    std::fs::write(&path, &data).unwrap();

    let pairs = parse_u32_pairs(&path).unwrap();
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0], (0x100, 0x42));
    assert_eq!(pairs[1], (0x200, 0xFF));
}

#[test]
fn legacy_parse_retains_ctx_data() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    std::fs::write(base.join("sw_bundle_init.bin"), []).unwrap();
    std::fs::write(base.join("sw_method_init.bin"), []).unwrap();
    let ctx_content = vec![0xAA_u8; 256];
    std::fs::write(base.join("sw_ctx.bin"), &ctx_content).unwrap();
    let nonctx_content = vec![0xBB_u8; 128];
    std::fs::write(base.join("sw_nonctx.bin"), &nonctx_content).unwrap();

    let blobs = GrFirmwareBlobs::parse_from(base, "test").unwrap();
    assert_eq!(blobs.ctx_data.len(), 256);
    assert_eq!(blobs.ctx_data, ctx_content);
    assert_eq!(blobs.nonctx_data.len(), 128);
    assert_eq!(blobs.nonctx_data, nonctx_content);
    assert!(blobs.has_ctx_template());
    assert_eq!(blobs.ctx_size(), 256);
    assert_eq!(blobs.nonctx_size(), 128);
}

#[test]
fn missing_ctx_produces_empty() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    std::fs::write(base.join("sw_bundle_init.bin"), []).unwrap();
    std::fs::write(base.join("sw_method_init.bin"), []).unwrap();

    let blobs = GrFirmwareBlobs::parse_from(base, "test").unwrap();
    assert!(blobs.ctx_data.is_empty());
    assert!(!blobs.has_ctx_template());
}

#[test]
fn parse_real_gv100_firmware() {
    match GrFirmwareBlobs::parse("gv100") {
        Ok(blobs) => {
            assert_eq!(blobs.chip, "gv100");
            assert_eq!(blobs.format, FirmwareFormat::Legacy);
            assert!(blobs.bundle_count() > 0);
            assert!(blobs.method_count() > 0);
            tracing::debug!(
                bundle_writes = blobs.bundle_count(),
                method_inits = blobs.method_count(),
                ctx_bytes = blobs.ctx_data.len(),
                nonctx_bytes = blobs.nonctx_data.len(),
                "GV100 (legacy) firmware parse"
            );
            let addrs = blobs.unique_bundle_addrs();
            tracing::debug!(unique_registers = addrs.len(), "GV100 bundle addrs");
        }
        Err(e) => {
            tracing::debug!(error = %e, "GV100 firmware not present (expected in CI)");
        }
    }
}

#[test]
fn parse_real_ga102_net_img() {
    match GrFirmwareBlobs::parse("ga102") {
        Ok(blobs) => {
            assert_eq!(blobs.chip, "ga102");
            assert_eq!(blobs.format, FirmwareFormat::NetImg);
            assert!(
                blobs.bundle_count() > 0,
                "GA102 NET_img should have register init data"
            );
            let addrs = blobs.unique_bundle_addrs();
            tracing::debug!(
                bundle_writes = blobs.bundle_count(),
                unique_regs = addrs.len(),
                method_inits = blobs.method_count(),
                ctx_bytes = blobs.ctx_data.len(),
                nonctx_bytes = blobs.nonctx_data.len(),
                "GA102 (NET_img) firmware parse"
            );
        }
        Err(e) => {
            tracing::debug!(error = %e, "GA102 firmware not present");
        }
    }
}

#[test]
fn parse_net_img_bytes_without_filesystem() {
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    let blobs = GrFirmwareBlobs::parse_net_img_bytes(&data, "test").expect("parse");
    assert_eq!(blobs.format, FirmwareFormat::NetImg);
    assert!(blobs.bundle_init.is_empty());
}

#[test]
fn from_legacy_bytes_smoke() {
    let mut bundle = Vec::new();
    bundle.extend_from_slice(&0x0040_1000u32.to_le_bytes());
    bundle.extend_from_slice(&1u32.to_le_bytes());
    let blobs = GrFirmwareBlobs::from_legacy_bytes(&bundle, &[], &[], &[], "chip");
    assert_eq!(blobs.bundle_init.len(), 1);
    assert_eq!(blobs.bundle_init[0].addr, 0x0040_1000);
}

#[test]
fn parse_net_img_synthetic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("NET_img.bin");

    // Build synthetic NET_img: header [pad, num_sections=2], then 2 sections
    // Section 0: type 0x05 (register init), size 8, offset 32
    // Section 1: type 0x07 (method init), size 8, offset 40
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes()); // pad
    data.extend_from_slice(&2u32.to_le_bytes()); // num_sections

    // Section 0: type 0x05, size 8, offset 32
    data.extend_from_slice(&0x05u32.to_le_bytes());
    data.extend_from_slice(&8u32.to_le_bytes());
    data.extend_from_slice(&32u32.to_le_bytes());

    // Section 1: type 0x07, size 8, offset 40
    data.extend_from_slice(&0x07u32.to_le_bytes());
    data.extend_from_slice(&8u32.to_le_bytes());
    data.extend_from_slice(&40u32.to_le_bytes());

    // Pad to offset 32, then section 0 data: one register pair in GPU range
    let reg_addr = 0x0040_1000u32; // in 0x0040_0000..0x0080_0000
    let reg_val = 0xDEAD_BEEFu32;
    data.resize(32, 0);
    data.extend_from_slice(&reg_addr.to_le_bytes());
    data.extend_from_slice(&reg_val.to_le_bytes());

    // Section 1 data: method pair at offset 40
    let method_addr = 0x0100u32;
    let method_val = 0x42u32;
    data.extend_from_slice(&method_addr.to_le_bytes());
    data.extend_from_slice(&method_val.to_le_bytes());

    std::fs::write(&path, &data).unwrap();

    let blobs = GrFirmwareBlobs::parse_from(dir.path(), "ga102").unwrap();
    assert_eq!(blobs.chip, "ga102");
    assert_eq!(blobs.format, FirmwareFormat::NetImg);
    assert_eq!(blobs.bundle_init.len(), 1);
    assert_eq!(blobs.bundle_init[0].addr, reg_addr);
    assert_eq!(blobs.bundle_init[0].value, reg_val);
    assert_eq!(blobs.method_init.len(), 1);
    assert_eq!(blobs.method_init[0].addr, method_addr);
    assert_eq!(blobs.method_init[0].value, method_val);
}

#[test]
fn parse_net_img_too_small() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("NET_img.bin");
    std::fs::write(&path, [0u8; 4]).unwrap();
    let result = GrFirmwareBlobs::parse_from(dir.path(), "ga102");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("too small"));
}

#[test]
fn parse_net_img_header_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("NET_img.bin");
    // Header: pad(4) + num_sections=10(4) = 8 bytes, but we claim 10 sections
    // so we need 8 + 10*12 = 128 bytes. We only provide 20 bytes.
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&10u32.to_le_bytes());
    data.extend_from_slice(&[0u8; 12]); // only 1 section entry
    std::fs::write(&path, &data).unwrap();
    let result = GrFirmwareBlobs::parse_from(dir.path(), "ga102");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("truncated"));
}

#[test]
fn parse_net_img_empty_header() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("NET_img.bin");
    // Valid minimal header: 0 sections
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    std::fs::write(&path, &data).unwrap();
    let blobs = GrFirmwareBlobs::parse_from(dir.path(), "ga102").unwrap();
    assert_eq!(blobs.chip, "ga102");
    assert_eq!(blobs.format, FirmwareFormat::NetImg);
    assert!(blobs.bundle_init.is_empty());
    assert!(blobs.method_init.is_empty());
    assert!(blobs.ctx_data.is_empty());
    assert!(blobs.nonctx_data.is_empty());
}

#[test]
fn parse_net_img_register_section_out_of_range_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("NET_img.bin");
    // Section with offset+size beyond data length - should be skipped
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&0x05u32.to_le_bytes()); // register type
    data.extend_from_slice(&8u32.to_le_bytes()); // size
    data.extend_from_slice(&1000u32.to_le_bytes()); // offset beyond our 20-byte file
    std::fs::write(&path, &data).unwrap();
    let blobs = GrFirmwareBlobs::parse_from(dir.path(), "ga102").unwrap();
    assert!(blobs.bundle_init.is_empty());
}

#[test]
fn parse_net_img_section_size_too_small_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("NET_img.bin");
    // Section with size < 8 (can't hold a u32 pair) - skipped
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&0x05u32.to_le_bytes());
    data.extend_from_slice(&4u32.to_le_bytes()); // size too small
    data.extend_from_slice(&20u32.to_le_bytes());
    data.resize(24, 0);
    std::fs::write(&path, &data).unwrap();
    let blobs = GrFirmwareBlobs::parse_from(dir.path(), "ga102").unwrap();
    assert!(blobs.bundle_init.is_empty());
}

#[test]
fn parse_net_img_ctx_and_nonctx_sections() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("NET_img.bin");

    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0x04u32.to_le_bytes()); // 4 sections

    // Section 0: type 0x01 (ctx), size 64, offset 56
    for v in &[0x01u32, 64u32, 56u32] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    // Section 1: type 0x03 (nonctx), size 32, offset 120
    for v in &[0x03u32, 32u32, 120u32] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    // Section 2: type 0x05 (register), size 8, offset 152
    for v in &[0x05u32, 8u32, 152u32] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    // Section 3: type 0x07 (method), size 0, offset 0
    for v in &[0x07u32, 0u32, 0u32] {
        data.extend_from_slice(&v.to_le_bytes());
    }

    data.resize(56, 0);
    data.extend_from_slice(&[0xAAu8; 64]); // ctx data
    data.extend_from_slice(&[0xBBu8; 32]); // nonctx data
    data.extend_from_slice(&0x0040_2000u32.to_le_bytes());
    data.extend_from_slice(&0x1234u32.to_le_bytes());

    std::fs::write(&path, &data).unwrap();

    let blobs = GrFirmwareBlobs::parse_from(dir.path(), "ga102").unwrap();
    assert_eq!(blobs.ctx_data.len(), 64);
    assert_eq!(blobs.ctx_data[0], 0xAA);
    assert_eq!(blobs.nonctx_data.len(), 32);
    assert_eq!(blobs.nonctx_data[0], 0xBB);
    assert!(blobs.has_ctx_template());
}

#[test]
fn parse_u32_pairs_odd_length_ignores_trailing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.bin");
    // 8 bytes (one pair) + 4 bytes trailing
    let mut data = Vec::new();
    data.extend_from_slice(&0x100u32.to_le_bytes());
    data.extend_from_slice(&0x42u32.to_le_bytes());
    data.extend_from_slice(&0xFFu32.to_le_bytes());
    std::fs::write(&path, &data).unwrap();

    let pairs = parse_u32_pairs(&path).unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0], (0x100, 0x42));
}

#[test]
fn bundle_writes_to_and_unique_addrs() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    let mut bundle_data = Vec::new();
    for &(addr, val) in &[
        (0x0040_1000u32, 1u32),
        (0x0040_2000u32, 2u32),
        (0x0040_1000u32, 3u32),
    ] {
        bundle_data.extend_from_slice(&addr.to_le_bytes());
        bundle_data.extend_from_slice(&val.to_le_bytes());
    }
    std::fs::write(base.join("sw_bundle_init.bin"), &bundle_data).unwrap();
    std::fs::write(base.join("sw_method_init.bin"), []).unwrap();

    let blobs = GrFirmwareBlobs::parse_from(base, "test").unwrap();
    let to_1000 = blobs.bundle_writes_to(0x0040_1000);
    assert_eq!(to_1000.len(), 2);
    assert_eq!(to_1000[0].value, 1);
    assert_eq!(to_1000[1].value, 3);

    let addrs = blobs.unique_bundle_addrs();
    assert_eq!(addrs.len(), 2);
    assert_eq!(addrs[0], 0x0040_1000);
    assert_eq!(addrs[1], 0x0040_2000);
}

#[test]
fn read_u32_le_roundtrip_offsets() {
    let data: Vec<u8> = (0u32..=3).flat_map(|w| w.to_le_bytes()).collect();
    assert_eq!(read_u32_le(&data, 0), 0);
    assert_eq!(read_u32_le(&data, 4), 1);
    assert_eq!(read_u32_le(&data, 8), 2);
    assert_eq!(read_u32_le(&data, 12), 3);
}

#[test]
fn parse_u32_pairs_from_bytes_empty_and_exact_multiple() {
    assert!(parse_u32_pairs_from_bytes(&[]).is_empty());
    let mut eight = Vec::new();
    eight.extend_from_slice(&0xABCD_u32.to_le_bytes());
    eight.extend_from_slice(&0x1234_u32.to_le_bytes());
    let pairs = parse_u32_pairs_from_bytes(&eight);
    assert_eq!(pairs, vec![(0xABCD, 0x1234)]);
}

#[test]
fn parse_u32_pairs_from_bytes_trailing_partial_word_ignored() {
    let mut data = Vec::new();
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&3u32.to_le_bytes());
    assert_eq!(parse_u32_pairs_from_bytes(&data), vec![(1, 2)]);
}

#[test]
fn from_legacy_bytes_keeps_addresses_outside_gpu_register_window() {
    let mut bundle = Vec::new();
    bundle.extend_from_slice(&0x100u32.to_le_bytes());
    bundle.extend_from_slice(&0x42u32.to_le_bytes());
    let blobs = GrFirmwareBlobs::from_legacy_bytes(&bundle, &[], &[], &[], "chip");
    assert_eq!(blobs.bundle_init.len(), 1);
    assert_eq!(blobs.bundle_init[0].addr, 0x100);
}

#[test]
fn parse_net_img_register_pair_outside_bar_window_excluded() {
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&0x05u32.to_le_bytes());
    data.extend_from_slice(&8u32.to_le_bytes());
    data.extend_from_slice(&20u32.to_le_bytes());
    data.resize(20, 0);
    data.extend_from_slice(&0x100u32.to_le_bytes());
    data.extend_from_slice(&0x42u32.to_le_bytes());
    let blobs = GrFirmwareBlobs::parse_net_img_bytes(&data, "t").unwrap();
    assert!(
        blobs.bundle_init.is_empty(),
        "parse_register_pairs filters non-GPU-range addresses"
    );
}

#[test]
fn parse_net_img_unknown_section_type_is_ignored() {
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    data.extend_from_slice(&8u32.to_le_bytes());
    data.extend_from_slice(&20u32.to_le_bytes());
    data.resize(20, 0);
    data.extend_from_slice(&0x0040_3000u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    let blobs = GrFirmwareBlobs::parse_net_img_bytes(&data, "t").unwrap();
    assert!(blobs.bundle_init.is_empty());
}

#[test]
fn parse_net_img_alternate_register_section_type_0x30() {
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&0x30u32.to_le_bytes());
    data.extend_from_slice(&8u32.to_le_bytes());
    data.extend_from_slice(&20u32.to_le_bytes());
    data.resize(20, 0);
    data.extend_from_slice(&0x0040_4000u32.to_le_bytes());
    data.extend_from_slice(&0x99u32.to_le_bytes());
    let blobs = GrFirmwareBlobs::parse_net_img_bytes(&data, "t").unwrap();
    assert_eq!(blobs.bundle_init.len(), 1);
    assert_eq!(blobs.bundle_init[0].addr, 0x0040_4000);
}

#[test]
fn parse_net_img_method_section_drops_trailing_partial_pairs() {
    let mut data = Vec::new();
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&0x07u32.to_le_bytes());
    data.extend_from_slice(&9u32.to_le_bytes());
    data.extend_from_slice(&20u32.to_le_bytes());
    data.resize(20, 0);
    data.extend_from_slice(&0x10u32.to_le_bytes());
    data.extend_from_slice(&0x20u32.to_le_bytes());
    data.push(0xFF);
    let blobs = GrFirmwareBlobs::parse_net_img_bytes(&data, "t").unwrap();
    assert_eq!(blobs.method_init.len(), 1);
    assert_eq!(blobs.method_init[0].addr, 0x10);
    assert_eq!(blobs.method_init[0].value, 0x20);
}

#[test]
fn bundle_writes_to_empty_when_no_match() {
    let blobs = GrFirmwareBlobs::from_legacy_bytes(&[], &[], &[], &[], "empty");
    assert!(blobs.bundle_writes_to(0x0040_0000).is_empty());
}

#[test]
fn parse_net_img_bytes_too_small_error_message() {
    let err = GrFirmwareBlobs::parse_net_img_bytes(&[0, 1, 2, 3], "x").unwrap_err();
    assert!(err.to_string().contains("too small"));
}

#[test]
fn parse_all_available_firmware() {
    let root = nvidia_firmware_root();
    let base = std::path::Path::new(&root);
    let Ok(entries) = std::fs::read_dir(base) else {
        tracing::debug!("No NVIDIA firmware directory");
        return;
    };
    let mut chips: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().join("gr").is_dir() {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    chips.sort();

    for chip in &chips {
        match GrFirmwareBlobs::parse(chip) {
            Ok(blobs) => {
                tracing::debug!(
                    chip,
                    format = ?blobs.format,
                    bundle = blobs.bundle_count(),
                    method = blobs.method_count(),
                    unique_regs = blobs.unique_bundle_addrs().len(),
                    "firmware chip parse"
                );
            }
            Err(e) => tracing::debug!(chip, error = %e, "firmware parse error"),
        }
    }
}
