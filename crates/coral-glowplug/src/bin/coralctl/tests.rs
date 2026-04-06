// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unit tests for coralctl CLI parsing and helpers.

use super::{
    Cli, Command, ExperimentAction, MmioAction, OracleAction, SnapshotAction, parse_hex_or_dec,
    resolve_glowplug_socket_path,
};
use crate::deploy::try_load_config;
use clap::Parser;

#[test]
fn resolve_glowplug_socket_path_matches_env_set_semantics() {
    const CUSTOM_SOCKET: &str = "/tmp/coralctl-test-glowplug.sock";
    assert_eq!(
        resolve_glowplug_socket_path(Some(CUSTOM_SOCKET)),
        CUSTOM_SOCKET
    );
}

#[test]
fn resolve_glowplug_socket_path_matches_env_unset_semantics() {
    const FALLBACK_SOCKET: &str = "/run/coralreef/glowplug.sock";
    assert_eq!(resolve_glowplug_socket_path(None), FALLBACK_SOCKET);
}

#[test]
fn cli_parses_custom_socket_for_status() {
    let cli =
        Cli::try_parse_from(["coralctl", "--socket", "/custom/glowplug.sock", "status"]).unwrap();
    assert_eq!(cli.socket, "/custom/glowplug.sock");
}

#[test]
fn try_load_config_nonexistent_path_errors() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("definitely-missing-glowplug.toml");
    let err = try_load_config(Some(missing.to_string_lossy().into_owned())).unwrap_err();
    assert!(
        err.paths.iter().any(|p| p.contains("definitely-missing")),
        "expected missing path in error list: {err:?}"
    );
}

#[test]
fn try_load_config_reads_minimal_toml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("glowplug.toml");
    std::fs::write(
        &path,
        r#"
[[device]]
bdf = "0000:01:00.0"
"#,
    )
    .expect("write");
    let cfg = try_load_config(Some(path.to_string_lossy().into_owned())).expect("load");
    assert_eq!(cfg.device.len(), 1);
    assert_eq!(cfg.device[0].bdf, "0000:01:00.0");
}

#[test]
fn deploy_udev_cli_accepts_config_and_dry_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("gp.toml");
    std::fs::write(
        &path,
        r#"
[[device]]
bdf = "0000:ff:00.0"
"#,
    )
    .expect("write");
    let path_str = path.to_str().expect("utf8 path");
    let cli = Cli::try_parse_from([
        "coralctl",
        "--socket",
        "/tmp/x.sock",
        "deploy-udev",
        "--config",
        path_str,
        "--dry-run",
        "--group",
        "coralreef",
    ])
    .expect("parse");
    let Command::DeployUdev {
        config,
        dry_run,
        group,
        ..
    } = cli.command
    else {
        panic!("expected DeployUdev");
    };
    assert!(dry_run);
    assert_eq!(group, "coralreef");
    assert_eq!(config.as_deref(), Some(path_str));
}

#[test]
fn cli_parses_probe_subcommand() {
    let cli = Cli::try_parse_from(["coralctl", "probe", "0000:03:00.0"]).expect("parse probe");
    let Command::Probe { bdf } = cli.command else {
        panic!("expected Probe");
    };
    assert_eq!(bdf, "0000:03:00.0");
}

#[test]
fn cli_parses_vram_probe_subcommand() {
    let cli =
        Cli::try_parse_from(["coralctl", "vram-probe", "0000:4b:00.0"]).expect("parse vram-probe");
    let Command::VramProbe { bdf } = cli.command else {
        panic!("expected VramProbe");
    };
    assert_eq!(bdf, "0000:4b:00.0");
}

#[test]
fn cli_parses_mmio_read_subcommand() {
    let cli = Cli::try_parse_from(["coralctl", "mmio", "read", "0000:03:00.0", "0x200"])
        .expect("parse mmio read");
    let Command::Mmio {
        action: MmioAction::Read { bdf, offset },
    } = cli.command
    else {
        panic!("expected Mmio Read");
    };
    assert_eq!(bdf, "0000:03:00.0");
    assert_eq!(offset, "0x200");
}

#[test]
fn cli_parses_mmio_write_with_dangerous_flag() {
    let cli = Cli::try_parse_from([
        "coralctl",
        "mmio",
        "write",
        "0000:03:00.0",
        "0x200",
        "0xFFFFFFFF",
        "--allow-dangerous",
    ])
    .expect("parse mmio write");
    let Command::Mmio {
        action:
            MmioAction::Write {
                bdf,
                offset,
                value,
                allow_dangerous,
            },
    } = cli.command
    else {
        panic!("expected Mmio Write");
    };
    assert_eq!(bdf, "0000:03:00.0");
    assert_eq!(offset, "0x200");
    assert_eq!(value, "0xFFFFFFFF");
    assert!(allow_dangerous);
}

#[test]
fn cli_parses_snapshot_save_subcommand() {
    let cli = Cli::try_parse_from(["coralctl", "snapshot", "save", "0000:03:00.0", "out.json"])
        .expect("parse snapshot save");
    let Command::Snapshot {
        action: SnapshotAction::Save { bdf, file },
    } = cli.command
    else {
        panic!("expected Snapshot Save");
    };
    assert_eq!(bdf, "0000:03:00.0");
    assert_eq!(file.as_deref(), Some("out.json"));
}

#[test]
fn cli_parses_snapshot_diff_subcommand() {
    let cli = Cli::try_parse_from(["coralctl", "snapshot", "diff", "0000:03:00.0", "saved.json"])
        .expect("parse snapshot diff");
    let Command::Snapshot {
        action: SnapshotAction::Diff { bdf, file },
    } = cli.command
    else {
        panic!("expected Snapshot Diff");
    };
    assert_eq!(bdf, "0000:03:00.0");
    assert_eq!(file, "saved.json");
}

#[test]
fn parse_hex_or_dec_works() {
    assert_eq!(parse_hex_or_dec("0x200").unwrap(), 0x200);
    assert_eq!(parse_hex_or_dec("0XDEAD").unwrap(), 0xDEAD);
    assert_eq!(parse_hex_or_dec("512").unwrap(), 512);
    assert!(parse_hex_or_dec("garbage").is_err());
}

#[test]
fn snapshot_save_default_file_omits_optional() {
    let cli = Cli::try_parse_from(["coralctl", "snapshot", "save", "0000:03:00.0"])
        .expect("parse snapshot save no file");
    let Command::Snapshot {
        action: SnapshotAction::Save { bdf, file },
    } = cli.command
    else {
        panic!("expected Snapshot Save");
    };
    assert_eq!(bdf, "0000:03:00.0");
    assert!(file.is_none());
}

#[test]
fn cli_parses_reset_subcommand() {
    let cli = Cli::try_parse_from(["coralctl", "reset", "0000:4a:00.0"]).expect("parse reset");
    let Command::Reset { bdf, method } = cli.command else {
        panic!("expected Reset");
    };
    assert_eq!(bdf, "0000:4a:00.0");
    assert_eq!(method, "auto");
}

#[test]
fn cli_parses_reset_with_method_flag() {
    let cli = Cli::try_parse_from(["coralctl", "reset", "0000:4a:00.0", "--method", "sbr"])
        .expect("parse reset --method sbr");
    let Command::Reset { bdf, method } = cli.command else {
        panic!("expected Reset");
    };
    assert_eq!(bdf, "0000:4a:00.0");
    assert_eq!(method, "sbr");
}

#[test]
fn cli_parses_oracle_capture_subcommand() {
    let cli = Cli::try_parse_from([
        "coralctl",
        "oracle",
        "capture",
        "0000:03:00.0",
        "--output",
        "nvidia.json",
    ])
    .expect("parse oracle capture");
    let Command::Oracle {
        action:
            OracleAction::Capture {
                bdf,
                output,
                max_channels,
                local,
            },
    } = cli.command
    else {
        panic!("expected Oracle Capture");
    };
    assert_eq!(bdf, "0000:03:00.0");
    assert_eq!(output.as_deref(), Some("nvidia.json"));
    assert!(!local);
    assert_eq!(max_channels, 0);
}

#[test]
fn cli_parses_oracle_diff_subcommand() {
    let cli = Cli::try_parse_from(["coralctl", "oracle", "diff", "left.json", "right.json"])
        .expect("parse oracle diff");
    let Command::Oracle {
        action: OracleAction::Diff { left, right },
    } = cli.command
    else {
        panic!("expected Oracle Diff");
    };
    assert_eq!(left, "left.json");
    assert_eq!(right, "right.json");
}

#[test]
fn cli_parses_experiment_sweep_defaults() {
    let cli = Cli::try_parse_from(["coralctl", "experiment", "sweep", "0000:03:00.0"])
        .expect("parse experiment sweep");
    let Command::Experiment {
        action:
            ExperimentAction::Sweep {
                bdf,
                personalities,
                return_to,
                trace,
                repeat,
            },
    } = cli.command
    else {
        panic!("expected Experiment Sweep");
    };
    assert_eq!(bdf, "0000:03:00.0");
    assert!(personalities.is_none());
    assert_eq!(return_to, "vfio");
    assert!(trace);
    assert_eq!(repeat, 1);
}

#[test]
fn cli_parses_experiment_sweep_with_repeat_and_multi_bdf() {
    let cli = Cli::try_parse_from([
        "coralctl",
        "experiment",
        "sweep",
        "0000:03:00.0,0000:4a:00.0",
        "--personalities",
        "nouveau,nvidia-open",
        "--repeat",
        "5",
        "--return-to",
        "vfio",
        "--trace",
    ])
    .expect("parse experiment sweep with repeat");
    let Command::Experiment {
        action:
            ExperimentAction::Sweep {
                bdf,
                personalities,
                return_to,
                trace,
                repeat,
            },
    } = cli.command
    else {
        panic!("expected Experiment Sweep");
    };
    assert_eq!(bdf, "0000:03:00.0,0000:4a:00.0");
    assert_eq!(personalities.as_deref(), Some("nouveau,nvidia-open"));
    assert_eq!(return_to, "vfio");
    assert!(trace);
    assert_eq!(repeat, 5);
}
