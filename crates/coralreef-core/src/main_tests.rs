// SPDX-License-Identifier: AGPL-3.0-only
use super::*;
use coralreef_core::capability::{Capability, SelfDescription, Transport};

#[test]
fn parse_cli_doctor() {
    let cli = parse_cli_from(["coralreef", "doctor"]).unwrap();
    assert!(matches!(cli.command, Commands::Doctor));
}

#[test]
fn parse_cli_server_defaults() {
    let cli = parse_cli_from(["coralreef", "server"]).unwrap();
    match &cli.command {
        Commands::Server {
            rpc_bind,
            tarpc_bind,
        } => {
            assert!(rpc_bind.contains("127.0.0.1"));
            assert!(tarpc_bind.is_none());
        }
        _ => panic!("expected Server command"),
    }
}

#[test]
fn parse_cli_compile_minimal() {
    let cli = parse_cli_from(["coralreef", "compile", "input.wgsl"]).unwrap();
    match &cli.command {
        Commands::Compile { input, output, .. } => {
            assert_eq!(input.to_string_lossy(), "input.wgsl");
            assert!(output.is_none());
        }
        _ => panic!("expected Compile command"),
    }
}

#[test]
fn parse_cli_compile_with_options() {
    let cli = parse_cli_from([
        "coralreef",
        "compile",
        "shader.wgsl",
        "--output",
        "out.bin",
        "--arch",
        "sm70",
        "--opt-level",
        "3",
    ])
    .unwrap();
    match &cli.command {
        Commands::Compile {
            input,
            output,
            arch,
            opt_level,
            ..
        } => {
            assert_eq!(input.to_string_lossy(), "shader.wgsl");
            assert_eq!(output.as_ref().unwrap().to_string_lossy(), "out.bin");
            assert_eq!(*arch, GpuArch::Sm70);
            assert_eq!(*opt_level, 3);
        }
        _ => panic!("expected Compile command"),
    }
}

#[test]
fn parse_cli_rejects_missing_subcommand() {
    assert!(parse_cli_from(["coralreef"]).is_err());
}

#[test]
fn parse_cli_rejects_unknown_subcommand() {
    let err = parse_cli_from(["coralreef", "nonexistent"]).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("subcommand"));
}

#[test]
fn parse_cli_rejects_compile_without_input() {
    assert!(parse_cli_from(["coralreef", "compile"]).is_err());
}

#[test]
fn install_panic_hook_sets_hook() {
    let prev = std::panic::take_hook();
    install_panic_hook();
    std::panic::set_hook(prev);
}

#[test]
fn unibin_exit_to_exit_code_success() {
    let ec: ExitCode = UniBinExit::Success.into();
    assert_eq!(ec, ExitCode::SUCCESS);
}

#[test]
fn unibin_exit_to_exit_code_general_error() {
    let ec: ExitCode = UniBinExit::GeneralError.into();
    assert_eq!(ec, ExitCode::from(1u8));
}

#[test]
fn unibin_exit_to_exit_code_config_error() {
    let ec: ExitCode = UniBinExit::ConfigError.into();
    assert_eq!(ec, ExitCode::from(2u8));
}

#[test]
fn unibin_exit_to_exit_code_internal_error() {
    let ec: ExitCode = UniBinExit::InternalError.into();
    assert_eq!(ec, ExitCode::from(3u8));
}

#[test]
fn unibin_exit_to_exit_code_signal() {
    let ec: ExitCode = UniBinExit::Signal.into();
    assert_eq!(ec, ExitCode::from(130u8));
}

#[test]
fn discovery_dir_returns_path() {
    let dir = discovery_dir().unwrap();
    assert!(dir.ends_with(crate::config::ECOSYSTEM_NAMESPACE));
}

#[test]
fn parse_cli_server_custom_bind_addresses() {
    let cli = parse_cli_from([
        "coralreef",
        "server",
        "--rpc-bind",
        "127.0.0.1:9999",
        "--tarpc-bind",
        "unix:///tmp/coralreef-test.sock",
    ])
    .unwrap();
    match &cli.command {
        Commands::Server {
            rpc_bind,
            tarpc_bind,
        } => {
            assert_eq!(rpc_bind, "127.0.0.1:9999");
            assert_eq!(
                tarpc_bind.as_deref(),
                Some("unix:///tmp/coralreef-test.sock")
            );
        }
        _ => panic!("expected Server command"),
    }
}

#[test]
fn parse_cli_compile_with_target_and_opt_level() {
    let cli = parse_cli_from([
        "coralreef",
        "compile",
        "shader.wgsl",
        "--arch",
        "sm80",
        "--opt-level",
        "3",
    ])
    .unwrap();
    match &cli.command {
        Commands::Compile {
            input,
            arch,
            opt_level,
            ..
        } => {
            assert_eq!(input.to_string_lossy(), "shader.wgsl");
            assert_eq!(*arch, GpuArch::Sm80);
            assert_eq!(*opt_level, 3);
        }
        _ => panic!("expected Compile command"),
    }
}

#[test]
fn parse_cli_log_level_global() {
    let cli = parse_cli_from(["coralreef", "--log-level", "debug", "doctor"]).unwrap();
    assert_eq!(cli.log_level, "debug");

    let cli = parse_cli_from(["coralreef", "--log-level", "trace", "compile", "x.wgsl"]).unwrap();
    assert_eq!(cli.log_level, "trace");
}

#[test]
fn parse_cli_version_flag() {
    let result = parse_cli_from(["coralreef", "--version"]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains(env!("CARGO_PKG_VERSION")),
        "version error should contain package version: {err_str}"
    );
}

#[test]
fn parse_cli_rejects_invalid_arch() {
    let result = parse_cli_from([
        "coralreef",
        "compile",
        "input.wgsl",
        "--arch",
        "invalid_arch",
    ]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("arch")
            || err.to_string().to_lowercase().contains("invalid"),
        "invalid arch should produce parse error"
    );
}

#[tokio::test]
async fn cmd_doctor_output_formatting() {
    let result = cmd_doctor().await;
    assert!(matches!(result, UniBinExit::Success));
    let report = commands::run_doctor().await.unwrap();
    assert!(report.contains("doctor"));
    assert!(report.contains("[OK]"));
    assert!(report.contains("Capabilities"));
    assert!(report.contains("Supported architectures"));
    assert!(report.contains("Diagnostic complete"));
}

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
fn parse_cli_compile_help() {
    let result = parse_cli_from(["coralreef", "compile", "--help"]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("input") || err_str.contains("Input") || err_str.contains("help"),
        "compile --help should produce help or parse error: {err_str}"
    );
}

#[test]
fn parse_cli_doctor_help() {
    let result = parse_cli_from(["coralreef", "doctor", "--help"]);
    assert!(result.is_err());
}

#[test]
fn cmd_compile_write_failure_output_is_directory() {
    let tmp = std::env::temp_dir().join("coralreef_test_compile_input.wgsl");
    std::fs::write(&tmp, "@compute @workgroup_size(1)\nfn main() {}").unwrap();
    let out_dir = std::env::temp_dir().join("coralreef_test_output_dir");
    let _ = std::fs::create_dir_all(&out_dir);

    let result = cmd_compile(&tmp, Some(out_dir.as_path()), GpuArch::Sm70, 2, true);

    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_dir(&out_dir);
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
fn write_and_remove_discovery_file() {
    let desc = SelfDescription {
        provides: vec![Capability {
            id: "test.provide".into(),
            version: "1.0".into(),
            metadata: serde_json::Value::Null,
        }],
        requires: vec![],
        transports: vec![
            Transport {
                protocol: "jsonrpc".into(),
                address: "127.0.0.1:12345".into(),
            },
            Transport {
                protocol: "tarpc+tcp".into(),
                address: "127.0.0.1:12346".into(),
            },
        ],
    };

    write_discovery_file(&desc).unwrap();
    let dir = discovery_dir().unwrap();
    let path = dir.join(format!("{}.json", env!("CARGO_PKG_NAME")));
    assert!(path.exists(), "discovery file should exist after write");

    let contents = std::fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(json["primal"], env!("CARGO_PKG_NAME"));
    assert!(json.get("version").is_some(), "Phase 10 requires version");
    assert!(json.get("pid").is_some(), "Phase 10 requires pid");
    assert!(json.get("provides").is_some(), "Phase 10 requires provides");
    assert_eq!(json["transports"]["jsonrpc"]["bind"], "127.0.0.1:12345");
    assert_eq!(json["transports"]["tarpc"]["bind"], "127.0.0.1:12346");

    remove_discovery_file();
    assert!(!path.exists(), "discovery file should be removed");

    let desc_empty = SelfDescription {
        provides: vec![],
        requires: vec![],
        transports: vec![],
    };
    write_discovery_file(&desc_empty).unwrap();
    let contents_empty = std::fs::read_to_string(&path).unwrap();
    let json_empty: serde_json::Value = serde_json::from_str(&contents_empty).unwrap();
    assert_eq!(json_empty["transports"]["jsonrpc"]["bind"], "");
    assert_eq!(json_empty["transports"]["tarpc"]["bind"], "");
    remove_discovery_file();
}

#[test]
fn remove_discovery_file_idempotent() {
    remove_discovery_file();
    remove_discovery_file();
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

// --- Command parsing edge cases ---

#[test]
fn parse_cli_compile_default_fp64_software() {
    let cli = parse_cli_from(["coralreef", "compile", "x.wgsl"]).unwrap();
    match &cli.command {
        Commands::Compile { fp64_software, .. } => assert!(*fp64_software, "default is true"),
        _ => panic!("expected Compile command"),
    }
}

#[test]
fn parse_cli_compile_default_opt_level() {
    let cli = parse_cli_from(["coralreef", "compile", "x.wgsl"]).unwrap();
    match &cli.command {
        Commands::Compile { opt_level, .. } => assert_eq!(*opt_level, 2, "default opt_level is 2"),
        _ => panic!("expected Compile command"),
    }
}

#[test]
fn parse_cli_server_rpc_bind_only() {
    let cli = parse_cli_from(["coralreef", "server", "--rpc-bind", "127.0.0.1:8888"]).unwrap();
    match &cli.command {
        Commands::Server {
            rpc_bind,
            tarpc_bind,
        } => {
            assert_eq!(rpc_bind, "127.0.0.1:8888");
            assert!(tarpc_bind.is_none());
        }
        _ => panic!("expected Server command"),
    }
}

#[test]
fn parse_cli_help_flag() {
    let result = parse_cli_from(["coralreef", "--help"]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("usage")
            || err.to_string().to_lowercase().contains("coralreef"),
        "help should show usage"
    );
}

#[test]
fn parse_cli_server_help() {
    let result = parse_cli_from(["coralreef", "server", "--help"]);
    assert!(result.is_err());
}

#[test]
fn parse_cli_version_long_form() {
    let result = parse_cli_from(["coralreef", "-V"]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains(env!("CARGO_PKG_VERSION")),
        "version output should contain package version"
    );
}

#[test]
fn unibin_exit_code_values() {
    assert_eq!(UniBinExit::Success as i32, 0);
    assert_eq!(UniBinExit::GeneralError as i32, 1);
    assert_eq!(UniBinExit::ConfigError as i32, 2);
    assert_eq!(UniBinExit::InternalError as i32, 3);
    assert_eq!(UniBinExit::Signal as i32, 130);
}

#[test]
fn unibin_exit_to_exit_code() {
    let _: ExitCode = UniBinExit::Success.into();
    let _: ExitCode = UniBinExit::GeneralError.into();
    let _: ExitCode = UniBinExit::ConfigError.into();
    let _: ExitCode = UniBinExit::Signal.into();
}

#[test]
fn parse_cli_invalid_subcommand() {
    let result = parse_cli_from(["coralreef", "nonexistent"]);
    assert!(result.is_err());
}

#[test]
fn parse_cli_compile_nv_archs() {
    for arch_str in ["sm70", "sm75", "sm80", "sm86", "sm89"] {
        let result = parse_cli_from(["coralreef", "compile", "x.wgsl", "--arch", arch_str]);
        assert!(result.is_ok(), "arch {arch_str} should be valid");
    }
}

#[test]
fn parse_cli_compile_invalid_arch() {
    let result = parse_cli_from(["coralreef", "compile", "x.wgsl", "--arch", "invalid"]);
    assert!(result.is_err(), "invalid arch should fail CLI parse");
}

#[test]
fn parse_cli_global_log_level() {
    let cli = parse_cli_from(["coralreef", "--log-level", "debug", "doctor"]).unwrap();
    assert_eq!(cli.log_level, "debug");
}

#[test]
fn parse_cli_default_log_level() {
    let cli = parse_cli_from(["coralreef", "doctor"]).unwrap();
    assert_eq!(cli.log_level, "info");
}

// --- Config/error path coverage ---

#[test]
fn parse_cli_compile_unknown_flag_is_error() {
    let result = parse_cli_from(["coralreef", "compile", "x.wgsl", "--unknown-flag"]);
    assert!(result.is_err(), "unknown flags should be rejected");
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

// --- cmd_server error path coverage ---

#[tokio::test]
async fn cmd_server_jsonrpc_invalid_bind_returns_general_error() {
    let result = cmd_server("not-a-valid-address", "127.0.0.1:0").await;
    assert!(
        matches!(result, UniBinExit::GeneralError),
        "invalid JSON-RPC bind address should produce GeneralError"
    );
}

#[tokio::test]
async fn cmd_server_tarpc_invalid_bind_returns_general_error() {
    // JSON-RPC binds successfully; tarpc fails with invalid address
    let result = cmd_server("127.0.0.1:0", "garbage:not-valid").await;
    assert!(
        matches!(result, UniBinExit::GeneralError),
        "invalid tarpc bind address should produce GeneralError"
    );
}

// --- parse_cli edge cases for coverage ---

#[test]
fn parse_cli_long_about() {
    let result = parse_cli_from(["coralreef", "doctor", "--help"]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("coralreef") || err.to_string().contains("doctor"),
        "help output should mention command"
    );
}

// --- main() error path coverage via parse_cli ---

#[test]
fn parse_cli_error_returns_clap_error() {
    let result = parse_cli_from(["coralreef"]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("subcommand")
            || err.to_string().to_lowercase().contains("required"),
        "missing subcommand should produce parse error"
    );
}

#[test]
fn parse_cli_compile_invalid_opt_level() {
    let result = parse_cli_from(["coralreef", "compile", "x.wgsl", "--opt-level", "99"]);
    assert!(
        result.is_ok(),
        "opt-level 99 is valid (clamped by compiler)"
    );
}

#[test]
fn parse_cli_compile_with_opt_level_zero() {
    let cli = parse_cli_from(["coralreef", "compile", "x.wgsl", "--opt-level", "0"]).unwrap();
    match &cli.command {
        Commands::Compile { opt_level, .. } => assert_eq!(*opt_level, 0),
        _ => panic!("expected Compile command"),
    }
}

// --- Additional main.rs coverage: CLI edge cases ---

#[test]
fn parse_cli_empty_args_fails() {
    let result = parse_cli_from::<Vec<&str>, &str>(vec![]);
    assert!(result.is_err());
}

#[test]
fn parse_cli_single_arg_fails() {
    let result = parse_cli_from(["coralreef"]);
    assert!(result.is_err());
}

#[test]
fn parse_cli_server_tarpc_bind_tcp() {
    let cli = parse_cli_from(["coralreef", "server", "--tarpc-bind", "127.0.0.1:0"]).unwrap();
    match &cli.command {
        Commands::Server { tarpc_bind, .. } => {
            assert_eq!(tarpc_bind.as_deref(), Some("127.0.0.1:0"));
        }
        _ => panic!("expected Server command"),
    }
}

#[test]
fn parse_cli_compile_output_explicit() {
    let cli = parse_cli_from(["coralreef", "compile", "a.wgsl", "-o", "out.bin"]).unwrap();
    match &cli.command {
        Commands::Compile { output, .. } => {
            assert_eq!(output.as_ref().unwrap().to_string_lossy(), "out.bin");
        }
        _ => panic!("expected Compile command"),
    }
}

#[test]
fn parse_cli_version_help_both_flags() {
    let v_result = parse_cli_from(["coralreef", "--version"]);
    assert!(v_result.is_err());
    assert!(
        v_result
            .unwrap_err()
            .to_string()
            .contains(env!("CARGO_PKG_VERSION"))
    );

    let h_result = parse_cli_from(["coralreef", "-h"]);
    assert!(h_result.is_err());
}
