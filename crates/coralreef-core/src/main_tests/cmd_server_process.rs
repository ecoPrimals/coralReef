// SPDX-License-Identifier: AGPL-3.0-only
use std::fs;
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::tempdir;

use crate::UniBinExit;

/// `std::env::current_exe()` points at the test harness (`coralreef_core-*`), not the `coralreef`
/// `UniBin` — use the workspace `target/debug/coralreef` artifact for subprocess server tests.
fn coralreef_unibin_exe() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_coralreef") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/coralreef")
}

/// Wall-clock budget for the child to write its discovery file.
const DISCOVERY_POLL_TIMEOUT: Duration = Duration::from_secs(20);
/// Sleep between polls (bounded backoff for fast machines).
const DISCOVERY_POLL_INTERVAL: Duration = Duration::from_millis(25);
/// Budget for graceful exit after SIGTERM.
const CHILD_EXIT_TIMEOUT: Duration = Duration::from_secs(45);

fn wait_until<F: FnMut() -> bool>(mut pred: F, deadline: Instant) -> bool {
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        thread::sleep(DISCOVERY_POLL_INTERVAL);
    }
    pred()
}

#[test]
fn cmd_server_subprocess_tcp_unix_listen_and_sigterm() {
    let tmp = tempdir().unwrap();
    let family_id = format!("subproc-{}", std::process::id());

    let exe = coralreef_unibin_exe();
    assert!(
        exe.exists(),
        "coralreef binary missing at {} — run `cargo build -p coralreef-core --bin coralreef`",
        exe.display()
    );

    let mut child = Command::new(&exe)
        .args([
            "server",
            "--port",
            "0",
            "--rpc-bind",
            "127.0.0.1:0",
            "--tarpc-bind",
            "127.0.0.1:0",
        ])
        .env("XDG_RUNTIME_DIR", tmp.path())
        .env("BIOMEOS_FAMILY_ID", &family_id)
        .env_remove("CORALREEF_TEST_SHUTDOWN_JOIN_TIMEOUT_MS")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn coralreef server subprocess");

    let discovery_path = tmp
        .path()
        .join(crate::config::ECOSYSTEM_NAMESPACE)
        .join(format!("{}.json", env!("CARGO_PKG_NAME")));

    let deadline = Instant::now() + DISCOVERY_POLL_TIMEOUT;
    assert!(
        wait_until(|| discovery_path.exists(), deadline),
        "discovery file should appear under XDG_RUNTIME_DIR"
    );

    let contents = fs::read_to_string(&discovery_path).expect("read discovery");
    let json: serde_json::Value = serde_json::from_str(&contents).expect("parse discovery json");
    let jsonrpc_bind = json["transports"]["jsonrpc"]["bind"]
        .as_str()
        .expect("jsonrpc bind");
    let addr = jsonrpc_bind
        .parse::<std::net::SocketAddr>()
        .expect("parse addr");
    TcpStream::connect(addr).expect("TCP JSON-RPC should accept connections");

    let unix_path = tmp
        .path()
        .join(crate::config::ECOSYSTEM_NAMESPACE)
        .join(format!("{}-{}.sock", env!("CARGO_PKG_NAME"), family_id));
    let unix_deadline = Instant::now() + DISCOVERY_POLL_TIMEOUT;
    assert!(
        wait_until(|| unix_path.exists(), unix_deadline),
        "Unix JSON-RPC socket should exist at {unix_path:?}"
    );
    UnixStream::connect(&unix_path).expect("Unix JSON-RPC should accept connections");

    let pid = child.id();
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .expect("send SIGTERM to server child");
    assert!(status.success(), "kill -TERM should succeed");

    let exit_deadline = Instant::now() + CHILD_EXIT_TIMEOUT;
    let code = loop {
        match child.try_wait().expect("try_wait") {
            Some(st) => break st.code(),
            None if Instant::now() >= exit_deadline => {
                let _ = child.kill();
                panic!("child did not exit after SIGTERM within {CHILD_EXIT_TIMEOUT:?}");
            }
            None => thread::sleep(DISCOVERY_POLL_INTERVAL),
        }
    };

    assert_eq!(
        code,
        Some(UniBinExit::Signal as i32),
        "graceful shutdown should map to signal exit code"
    );
    assert!(
        !discovery_path.exists(),
        "discovery file should be removed on shutdown"
    );
}

/// Exercise `cmd_server` graceful shutdown with a near-zero join timeout so the
/// `shutdown_result.is_err()` branch (warning log) is reachable in the binary.
#[test]
fn cmd_server_subprocess_shutdown_join_timeout_still_exits_on_sigterm() {
    let tmp = tempdir().unwrap();
    let family_id = format!("shutdown-timeout-{}", std::process::id());

    let exe = coralreef_unibin_exe();
    assert!(
        exe.exists(),
        "coralreef binary missing at {} — run `cargo build -p coralreef-core --bin coralreef`",
        exe.display()
    );

    let mut child = Command::new(&exe)
        .args([
            "server",
            "--rpc-bind",
            "127.0.0.1:0",
            "--tarpc-bind",
            "127.0.0.1:0",
        ])
        .env("XDG_RUNTIME_DIR", tmp.path())
        .env("BIOMEOS_FAMILY_ID", &family_id)
        .env("CORALREEF_TEST_SHUTDOWN_JOIN_TIMEOUT_MS", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn coralreef server subprocess");

    let deadline = Instant::now() + DISCOVERY_POLL_TIMEOUT;
    assert!(
        wait_until(
            || tmp
                .path()
                .join(crate::config::ECOSYSTEM_NAMESPACE)
                .join(format!("{}.json", env!("CARGO_PKG_NAME")))
                .exists(),
            deadline
        ),
        "discovery file should appear (server ready)"
    );

    let pid = child.id();
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .expect("send SIGTERM to server child");
    assert!(status.success(), "kill -TERM should succeed");

    let exit_deadline = Instant::now() + CHILD_EXIT_TIMEOUT;
    let code = loop {
        match child.try_wait().expect("try_wait") {
            Some(st) => break st.code(),
            None if Instant::now() >= exit_deadline => {
                let _ = child.kill();
                panic!("child did not exit after SIGTERM within {CHILD_EXIT_TIMEOUT:?}");
            }
            None => thread::sleep(DISCOVERY_POLL_INTERVAL),
        }
    };

    assert_eq!(
        code,
        Some(UniBinExit::Signal as i32),
        "graceful shutdown should map to signal exit code even with short join timeout"
    );
}
