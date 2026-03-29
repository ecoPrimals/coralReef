// SPDX-License-Identifier: AGPL-3.0-only
use super::*;

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
            port,
            rpc_bind,
            tarpc_bind,
        } => {
            assert!(port.is_none());
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
            port,
            rpc_bind,
            tarpc_bind,
        } => {
            assert!(port.is_none());
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
            port,
            rpc_bind,
            tarpc_bind,
        } => {
            assert!(port.is_none());
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

#[test]
fn parse_cli_compile_unknown_flag_is_error() {
    let result = parse_cli_from(["coralreef", "compile", "x.wgsl", "--unknown-flag"]);
    assert!(result.is_err(), "unknown flags should be rejected");
}

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
fn parse_cli_server_port() {
    let cli = parse_cli_from(["coralreef", "server", "--port", "8765"]).unwrap();
    match &cli.command {
        Commands::Server { port, .. } => {
            assert_eq!(*port, Some(8765));
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

#[test]
fn parse_cli_compile_all_options_explicit() {
    let cli = parse_cli_from([
        "coralreef",
        "compile",
        "in.wgsl",
        "-o",
        "custom.bin",
        "--arch",
        "sm89",
        "--opt-level",
        "3",
        "--fp64-software",
    ])
    .expect("parse full compile");

    match &cli.command {
        Commands::Compile {
            input,
            output,
            arch,
            opt_level,
            fp64_software,
        } => {
            assert_eq!(input.to_string_lossy(), "in.wgsl");
            assert_eq!(output.as_ref().unwrap().to_string_lossy(), "custom.bin");
            assert_eq!(*arch, GpuArch::Sm89);
            assert_eq!(*opt_level, 3);
            assert!(*fp64_software);
        }
        _ => panic!("expected Compile"),
    }
}

#[test]
fn parse_cli_rejects_non_numeric_opt_level() {
    let result = parse_cli_from([
        "coralreef",
        "compile",
        "x.wgsl",
        "--opt-level",
        "not-a-number",
    ]);
    assert!(result.is_err(), "non-numeric opt-level must fail parse");
}

#[test]
fn parse_cli_rejects_negative_style_opt_level() {
    let result = parse_cli_from(["coralreef", "compile", "x.wgsl", "--opt-level", "-1"]);
    assert!(
        result.is_err(),
        "negative opt-level is not a valid u32 for clap"
    );
}
