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
    assert!(dir.ends_with(coralreef_core::config::ECOSYSTEM_NAMESPACE));
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
}
