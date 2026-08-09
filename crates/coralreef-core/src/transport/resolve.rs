// SPDX-License-Identifier: AGPL-3.0-or-later

use super::TransportEndpoint;

/// Create a platform-native link for capability-domain discovery.
///
/// - **Unix**: symbolic link via `std::os::unix::fs::symlink`.
/// - **Windows**: `std::os::windows::fs::symlink_file` (requires
///   Developer Mode or `SeCreateSymbolicLinkPrivilege`). Falls back to
///   `Unsupported` if the privilege is unavailable.
///
/// Capability-domain links enable primals to discover peers by domain
/// (`shader.sock → coralreef-core-default.sock`). On Windows TCP-only
/// deployments, discovery uses the JSON manifest instead.
///
/// # Errors
///
/// Returns an IO error if link creation fails or the platform cannot
/// create links with current privileges.
pub fn create_local_symlink(
    target: &std::ffi::OsStr,
    link: &std::path::Path,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "platform links not available on this target",
        ))
    }
}

/// Resolved bind configuration from transport injection or CLI fallback.
#[derive(Debug, Clone)]
pub enum ResolvedBind {
    /// Bind UDS only (launcher-injected transport).
    UdsOnly {
        /// Path to bind the Unix domain socket.
        #[cfg_attr(
            not(unix),
            allow(
                dead_code,
                reason = "UDS path carried on all platforms; only used on Unix"
            )
        )]
        path: std::path::PathBuf,
    },
    /// Bind TCP only (launcher-injected transport).
    TcpOnly {
        /// Address to bind (e.g. `127.0.0.1:9200`).
        addr: String,
    },
    /// Default: bind both UDS + TCP (standalone/debug mode — Tier 5 fallback).
    Both {
        /// TCP bind address.
        tcp_bind: String,
        /// UDS socket path override (from `--socket` flag), or `None` for default.
        socket_override: Option<std::path::PathBuf>,
    },
}

/// Resolve the bind configuration from environment and CLI args.
///
/// Priority:
/// 1. `$TRANSPORT_ENDPOINT` (launcher injection — highest priority)
/// 2. CLI flags (`--socket`, `--rpc-bind`) — Tier 5 standalone fallback
///
/// # Errors
///
/// Returns an error if `$TRANSPORT_ENDPOINT` is set but contains invalid JSON
/// or specifies an unsupported transport for server binding.
#[allow(
    dead_code,
    reason = "pub API for ecosystem consumers without bind-mode override"
)]
pub fn resolve_bind(
    cli_tcp_bind: &str,
    cli_socket: Option<&std::path::Path>,
) -> Result<ResolvedBind, TransportResolveError> {
    resolve_bind_with_mode(cli_tcp_bind, cli_socket, None)
}

/// Resolve server bind configuration with an explicit bind-mode override.
///
/// When `cli_bind_mode` is `Some`, it takes precedence over the
/// `$PRIMAL_BIND_MODE` environment variable. This supports the
/// standard primal startup envelope (`--bind-mode` CLI flag).
///
/// # Errors
///
/// Returns an error if `$TRANSPORT_ENDPOINT` is set but contains invalid JSON
/// or specifies an unsupported transport for server binding.
pub fn resolve_bind_with_mode(
    cli_tcp_bind: &str,
    cli_socket: Option<&std::path::Path>,
    cli_bind_mode: Option<&str>,
) -> Result<ResolvedBind, TransportResolveError> {
    let bind_mode = cli_bind_mode.map_or_else(bind_mode_from_env, parse_bind_mode);

    if bind_mode == BindModeOverride::TcpOnly {
        tracing::info!("PRIMAL_BIND_MODE=tcp_only — skipping UDS, binding TCP only");
        return Ok(ResolvedBind::TcpOnly {
            addr: cli_tcp_bind.to_owned(),
        });
    }

    if let Some(parsed) = TransportEndpoint::from_env() {
        let endpoint = parsed.map_err(TransportResolveError::InvalidJson)?;
        match endpoint {
            TransportEndpoint::Uds { path } => {
                if bind_mode == BindModeOverride::Fallback {
                    tracing::info!(
                        transport = "uds+tcp_fallback",
                        path = %path,
                        "PRIMAL_BIND_MODE=fallback — upgrading UdsOnly to Both for graceful degradation"
                    );
                    Ok(ResolvedBind::Both {
                        tcp_bind: cli_tcp_bind.to_owned(),
                        socket_override: Some(std::path::PathBuf::from(path)),
                    })
                } else {
                    tracing::info!(
                        transport = "uds",
                        path = %path,
                        "transport injected by launcher"
                    );
                    Ok(ResolvedBind::UdsOnly {
                        path: std::path::PathBuf::from(path),
                    })
                }
            }
            TransportEndpoint::Tcp { host, port } => {
                let addr = format!("{host}:{port}");
                tracing::info!(
                    transport = "tcp",
                    addr = %addr,
                    "transport injected by launcher"
                );
                Ok(ResolvedBind::TcpOnly { addr })
            }
            TransportEndpoint::MeshRelay { .. } => Err(TransportResolveError::UnsupportedForBind(
                "mesh_relay cannot be used for server binding".into(),
            )),
        }
    } else {
        Ok(ResolvedBind::Both {
            tcp_bind: cli_tcp_bind.to_owned(),
            socket_override: cli_socket.map(std::path::Path::to_path_buf),
        })
    }
}

/// `PRIMAL_BIND_MODE` override for server-side transport selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindModeOverride {
    /// Default — no override, use normal resolution.
    Normal,
    /// Skip UDS entirely, bind TCP only.
    TcpOnly,
    /// Try UDS, fall back to TCP on permission error (grapheneGate/Android).
    Fallback,
}

fn bind_mode_from_env() -> BindModeOverride {
    match std::env::var("PRIMAL_BIND_MODE")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "tcp_only" | "tcp" => BindModeOverride::TcpOnly,
        "fallback" | "auto" => BindModeOverride::Fallback,
        _ => BindModeOverride::Normal,
    }
}

fn parse_bind_mode(mode: &str) -> BindModeOverride {
    match mode.to_lowercase().as_str() {
        "tcp_only" | "tcp" => BindModeOverride::TcpOnly,
        "fallback" | "auto" => BindModeOverride::Fallback,
        _ => BindModeOverride::Normal,
    }
}

/// Errors from transport endpoint resolution.
#[derive(Debug, thiserror::Error)]
pub enum TransportResolveError {
    /// `$TRANSPORT_ENDPOINT` contained invalid JSON.
    #[error("invalid TRANSPORT_ENDPOINT JSON: {0}")]
    InvalidJson(#[source] serde_json::Error),

    /// Transport type not usable for server binding.
    #[error("unsupported transport for bind: {0}")]
    UnsupportedForBind(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper: resolve bind from an explicit endpoint value (bypasses env).
    fn resolve_bind_from_value(
        json: Option<&str>,
        cli_tcp: &str,
        cli_socket: Option<&std::path::Path>,
    ) -> Result<ResolvedBind, TransportResolveError> {
        match json {
            None => Ok(ResolvedBind::Both {
                tcp_bind: cli_tcp.to_owned(),
                socket_override: cli_socket.map(std::path::Path::to_path_buf),
            }),
            Some(s) if s.trim().is_empty() => Ok(ResolvedBind::Both {
                tcp_bind: cli_tcp.to_owned(),
                socket_override: cli_socket.map(std::path::Path::to_path_buf),
            }),
            Some(s) => {
                let endpoint: TransportEndpoint =
                    serde_json::from_str(s).map_err(TransportResolveError::InvalidJson)?;
                match endpoint {
                    TransportEndpoint::Uds { path } => Ok(ResolvedBind::UdsOnly {
                        path: std::path::PathBuf::from(path),
                    }),
                    TransportEndpoint::Tcp { host, port } => Ok(ResolvedBind::TcpOnly {
                        addr: format!("{host}:{port}"),
                    }),
                    TransportEndpoint::MeshRelay { .. } => {
                        Err(TransportResolveError::UnsupportedForBind(
                            "mesh_relay cannot be used for server binding".into(),
                        ))
                    }
                }
            }
        }
    }

    #[test]
    fn resolve_bind_defaults_to_both_when_no_env() {
        let resolved = resolve_bind_from_value(None, "127.0.0.1:0", None).expect("resolve");
        assert!(matches!(resolved, ResolvedBind::Both { .. }));
    }

    #[test]
    fn resolve_bind_uds_injection() {
        let json = r#"{"transport":"uds","path":"/tmp/injected.sock"}"#;
        let resolved = resolve_bind_from_value(Some(json), "127.0.0.1:0", None).expect("resolve");
        match resolved {
            ResolvedBind::UdsOnly { path } => {
                assert_eq!(path, std::path::PathBuf::from("/tmp/injected.sock"));
            }
            other => panic!("expected UdsOnly, got {other:?}"),
        }
    }

    #[test]
    fn resolve_bind_tcp_injection() {
        let json = r#"{"transport":"tcp","host":"0.0.0.0","port":9300}"#;
        let resolved = resolve_bind_from_value(Some(json), "127.0.0.1:0", None).expect("resolve");
        match resolved {
            ResolvedBind::TcpOnly { addr } => {
                assert_eq!(addr, "0.0.0.0:9300");
            }
            other => panic!("expected TcpOnly, got {other:?}"),
        }
    }

    #[test]
    fn resolve_bind_rejects_mesh_relay() {
        let json = r#"{"transport":"mesh_relay","peer_id":"east","capability":"shader.compile"}"#;
        let result = resolve_bind_from_value(Some(json), "127.0.0.1:0", None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("mesh_relay"));
    }

    #[test]
    fn resolve_bind_rejects_invalid_json() {
        let result = resolve_bind_from_value(Some("not json"), "127.0.0.1:0", None);
        assert!(result.is_err());
    }

    #[test]
    fn resolve_bind_empty_env_treated_as_unset() {
        let resolved = resolve_bind_from_value(Some(""), "127.0.0.1:0", None).expect("resolve");
        assert!(matches!(resolved, ResolvedBind::Both { .. }));
    }

    #[test]
    fn parse_bind_mode_variants() {
        assert_eq!(parse_bind_mode("tcp_only"), BindModeOverride::TcpOnly);
        assert_eq!(parse_bind_mode("tcp"), BindModeOverride::TcpOnly);
        assert_eq!(parse_bind_mode("TCP_ONLY"), BindModeOverride::TcpOnly);
        assert_eq!(parse_bind_mode("fallback"), BindModeOverride::Fallback);
        assert_eq!(parse_bind_mode("auto"), BindModeOverride::Fallback);
        assert_eq!(parse_bind_mode("AUTO"), BindModeOverride::Fallback);
        assert_eq!(parse_bind_mode(""), BindModeOverride::Normal);
        assert_eq!(parse_bind_mode("something"), BindModeOverride::Normal);
    }

    #[test]
    fn resolve_bind_with_mode_tcp_only_skips_env() {
        let resolved =
            resolve_bind_with_mode("127.0.0.1:0", None, Some("tcp_only")).expect("resolve");
        match resolved {
            ResolvedBind::TcpOnly { addr } => assert_eq!(addr, "127.0.0.1:0"),
            other => panic!("expected TcpOnly, got {other:?}"),
        }
    }

    #[test]
    fn resolve_bind_with_mode_normal_no_env_returns_both() {
        if std::env::var("TRANSPORT_ENDPOINT").is_err() {
            let resolved =
                resolve_bind_with_mode("127.0.0.1:0", None, Some("normal")).expect("resolve");
            assert!(matches!(resolved, ResolvedBind::Both { .. }));
        }
    }

    #[test]
    fn resolve_bind_with_mode_socket_override() {
        if std::env::var("TRANSPORT_ENDPOINT").is_err() {
            let sock = std::path::Path::new("/tmp/test-override.sock");
            let resolved =
                resolve_bind_with_mode("127.0.0.1:0", Some(sock), None).expect("resolve");
            match resolved {
                ResolvedBind::Both {
                    socket_override, ..
                } => {
                    assert_eq!(
                        socket_override.as_deref(),
                        Some(std::path::Path::new("/tmp/test-override.sock"))
                    );
                }
                other => panic!("expected Both, got {other:?}"),
            }
        }
    }

    #[test]
    fn resolve_bind_no_mode_returns_both() {
        if std::env::var("TRANSPORT_ENDPOINT").is_err() {
            let resolved = resolve_bind("127.0.0.1:0", None).expect("resolve");
            assert!(matches!(resolved, ResolvedBind::Both { .. }));
        }
    }

    #[test]
    fn transport_resolve_error_display() {
        let err = TransportResolveError::UnsupportedForBind("mesh_relay".into());
        assert!(err.to_string().contains("mesh_relay"));
    }
}
