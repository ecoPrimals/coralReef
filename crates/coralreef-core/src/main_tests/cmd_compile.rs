// SPDX-License-Identifier: AGPL-3.0-only
use super::*;

use tempfile::tempdir;

#[test]
fn cmd_compile_success_with_temp_file() {
    let tmp = std::env::temp_dir().join("coralreef_test_compile.wgsl");
    std::fs::write(&tmp, "@compute @workgroup_size(1)\nfn main() {}").unwrap();
    let out_path = tmp.with_extension("bin");
    let result = cmd_compile(&tmp, Some(out_path.as_path()), GpuArch::Sm70, 2, true);
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(&out_path);
    assert!(matches!(result, UniBinExit::Success));
}

#[test]
fn cmd_compile_config_error_nonexistent_file() {
    let result = cmd_compile(
        std::path::Path::new("/nonexistent/path/shader.wgsl"),
        None,
        GpuArch::Sm70,
        2,
        true,
    );
    assert!(matches!(result, UniBinExit::ConfigError));
}

#[test]
fn cmd_compile_write_failure_output_is_directory() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("input.wgsl");
    std::fs::write(&input, "@compute @workgroup_size(1)\nfn main() {}").unwrap();
    let out_dir = dir.path().join("output_dir");
    std::fs::create_dir_all(&out_dir).unwrap();

    let result = cmd_compile(&input, Some(out_dir.as_path()), GpuArch::Sm70, 2, true);

    assert!(
        matches!(result, UniBinExit::GeneralError),
        "writing to directory path should fail with GeneralError"
    );
}

#[test]
fn cmd_compile_general_error_corrupt_spirv() {
    let tmp = std::env::temp_dir().join("coralreef_test_corrupt.spv");
    let corrupt_words: Vec<u32> = vec![0xDEAD_BEEF, 0x0001_0000, 0, 0, 0];
    let bytes: Vec<u8> = corrupt_words.iter().flat_map(|w| w.to_le_bytes()).collect();
    std::fs::write(&tmp, &bytes).unwrap();

    let result = cmd_compile(&tmp, None, GpuArch::Sm70, 2, true);

    let _ = std::fs::remove_file(&tmp);
    assert!(
        matches!(result, UniBinExit::GeneralError | UniBinExit::ConfigError),
        "corrupt SPIR-V should produce error"
    );
}

#[test]
fn cmd_compile_all_archs() {
    let tmp = std::env::temp_dir().join("coralreef_test_all_archs.wgsl");
    std::fs::write(&tmp, "@compute @workgroup_size(1)\nfn main() {}").unwrap();
    for arch in [
        GpuArch::Sm70,
        GpuArch::Sm75,
        GpuArch::Sm80,
        GpuArch::Sm86,
        GpuArch::Sm89,
    ] {
        let out_path = tmp.with_extension(format!("{arch}.bin"));
        let result = cmd_compile(&tmp, Some(out_path.as_path()), arch, 2, true);
        let _ = std::fs::remove_file(&out_path);
        assert!(
            matches!(result, UniBinExit::Success),
            "compile should succeed for {arch:?}"
        );
    }
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn cmd_compile_default_output_path() {
    let tmp = std::env::temp_dir().join("coralreef_test_default_output.wgsl");
    std::fs::write(&tmp, "@compute @workgroup_size(1)\nfn main() {}").unwrap();
    let result = cmd_compile(&tmp, None, GpuArch::Sm70, 2, true);
    let expected_out = tmp.with_extension("bin");
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(&expected_out);
    assert!(matches!(result, UniBinExit::Success));
}

#[test]
fn cmd_compile_opt_levels() {
    let tmp = std::env::temp_dir().join("coralreef_test_opt_levels.wgsl");
    std::fs::write(&tmp, "@compute @workgroup_size(1)\nfn main() {}").unwrap();
    for opt in [0, 1, 2, 3] {
        let out_path = tmp.with_extension(format!("opt{opt}.bin"));
        let result = cmd_compile(&tmp, Some(out_path.as_path()), GpuArch::Sm70, opt, true);
        let _ = std::fs::remove_file(&out_path);
        assert!(
            matches!(result, UniBinExit::Success),
            "compile should succeed at opt level {opt}"
        );
    }
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn cmd_compile_fp64_software_false() {
    let tmp = std::env::temp_dir().join("coralreef_test_fp64_false.wgsl");
    std::fs::write(&tmp, "@compute @workgroup_size(1)\nfn main() {}").unwrap();
    let result = cmd_compile(&tmp, None, GpuArch::Sm70, 2, false);
    let _ = std::fs::remove_file(&tmp);
    assert!(matches!(result, UniBinExit::Success));
}

#[test]
fn cmd_compile_read_error_directory_as_input() {
    // Passing a directory path causes read to fail with IsADirectory (GeneralError)
    let tmp_dir = std::env::temp_dir().join("coralreef_test_input_dir");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let result = cmd_compile(tmp_dir.as_path(), None, GpuArch::Sm70, 2, true);
    let _ = std::fs::remove_dir(&tmp_dir);
    assert!(
        matches!(result, UniBinExit::GeneralError),
        "reading directory as input should produce GeneralError"
    );
}

#[test]
fn cmd_compile_success_with_tempfile_wgsl() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("shader.wgsl");
    std::fs::write(&input, "@compute @workgroup_size(1)\nfn main() {}").expect("write wgsl");
    let output = dir.path().join("out.bin");

    let result = cmd_compile(&input, Some(output.as_path()), GpuArch::Sm75, 1, true);

    assert!(matches!(result, UniBinExit::Success));
    assert!(output.exists(), "output written");
}

#[test]
fn cmd_compile_nonexistent_input_is_config_error() {
    let dir = tempdir().expect("tempdir");
    let missing = dir.path().join("nope.wgsl");
    let result = cmd_compile(&missing, None, GpuArch::Sm70, 2, true);
    assert!(
        matches!(result, UniBinExit::ConfigError),
        "missing file should map to ConfigError (not GeneralError)"
    );
}

#[test]
fn cmd_compile_opt_levels_above_documented_range() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("opt.wgsl");
    std::fs::write(&input, "@compute @workgroup_size(1)\nfn main() {}").expect("write wgsl");

    for opt in [4_u32, 10, u32::MAX] {
        let out = dir.path().join(format!("out_{opt}.bin"));
        let result = cmd_compile(&input, Some(out.as_path()), GpuArch::Sm70, opt, true);
        assert!(
            matches!(result, UniBinExit::Success | UniBinExit::GeneralError),
            "opt_level {opt} should not panic; exit code {}",
            result as i32
        );
    }
}
