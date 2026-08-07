// SPDX-License-Identifier: AGPL-3.0-or-later
//! Transport endpoint injection — ecosystem wire-compatible.
//!
//! Primals do not choose their transport — the launcher or Tower Atomic decides.
//! When `$TRANSPORT_ENDPOINT` is set, the primal binds on the injected endpoint
//! instead of self-selecting via `--rpc-bind` / `--socket`.
//!
//! Wire format (JSON, serde-tagged — ecosystem canonical `TransportEndpoint`):
//! ```json
//! {"transport":"uds","path":"/run/user/1000/biomeos/coralreef.sock"}
//! {"transport":"tcp","host":"127.0.0.1","port":9200}
//! ```

use serde::{Deserialize, Serialize};

use crate::env_keys;

/// Structured transport endpoint — wire-compatible with ecosystem canonical standard.
///
/// The launcher sets `$TRANSPORT_ENDPOINT` to a JSON string of this type.
/// Primals parse it and bind/connect accordingly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport")]
#[non_exhaustive]
pub enum TransportEndpoint {
    /// Unix Domain Socket — local primal on same host (fastest path).
    #[serde(rename = "uds")]
    Uds {
        /// Filesystem path to the socket.
        path: String,
    },

    /// TCP — direct network connection (cross-host or container).
    #[serde(rename = "tcp")]
    Tcp {
        /// Host address (IPv4, IPv6, or hostname).
        host: String,
        /// TCP port number.
        port: u16,
    },

    /// Mesh relay — primal reachable via the ecosystem's mesh discovery relay.
    #[serde(rename = "mesh_relay")]
    MeshRelay {
        /// Mesh peer identifier.
        peer_id: String,
        /// Capability being resolved on the remote peer.
        capability: String,
    },
}

impl TransportEndpoint {
    /// Parse from the `$TRANSPORT_ENDPOINT` environment variable.
    ///
    /// Returns `None` if the variable is unset or empty.
    /// Returns `Some(Err(_))` if set but contains invalid JSON.
    #[must_use]
    pub fn from_env() -> Option<Result<Self, serde_json::Error>> {
        let val = std::env::var(env_keys::TRANSPORT_ENDPOINT).ok()?;
        let trimmed = val.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(serde_json::from_str(trimmed))
    }

    /// Whether this is a local (same-host) transport.
    #[must_use]
    #[allow(dead_code, reason = "pub API for ecosystem consumers and tests")]
    pub fn is_local(&self) -> bool {
        match self {
            Self::Uds { .. } => true,
            Self::Tcp { host, .. } => host == "127.0.0.1" || host == "::1" || host == "localhost",
            Self::MeshRelay { .. } => false,
        }
    }

    /// Transport name as it appears in the wire format.
    #[must_use]
    #[allow(dead_code, reason = "pub API for ecosystem consumers and tests")]
    pub const fn transport_name(&self) -> &'static str {
        match self {
            Self::Uds { .. } => "uds",
            Self::Tcp { .. } => "tcp",
            Self::MeshRelay { .. } => "mesh_relay",
        }
    }

    /// URI-style string for logging/diagnostics.
    #[must_use]
    pub fn display_uri(&self) -> String {
        match self {
            Self::Uds { path } => format!("unix://{path}"),
            Self::Tcp { host, port } => {
                if host.contains(':') {
                    format!("tcp://[{host}]:{port}")
                } else {
                    format!("tcp://{host}:{port}")
                }
            }
            Self::MeshRelay {
                peer_id,
                capability,
            } => format!("mesh://{peer_id}/{capability}"),
        }
    }
}

impl TransportEndpoint {
    /// Platform-appropriate default for a given service.
    ///
    /// Unix: UDS socket under the ecosystem namespace directory.
    /// Non-Unix: TCP localhost on `fallback_port`.
    #[must_use]
    #[allow(
        dead_code,
        reason = "G66 API — wired by server lifecycle and BTSP client"
    )]
    pub fn platform_default(socket_path: &str, fallback_port: u16) -> Self {
        if cfg!(unix) {
            Self::Uds {
                path: socket_path.to_owned(),
            }
        } else {
            Self::Tcp {
                host: "127.0.0.1".into(),
                port: fallback_port,
            }
        }
    }

    /// Read from `$TRANSPORT_ENDPOINT` or fall back to [`Self::platform_default`].
    #[must_use]
    #[allow(
        dead_code,
        reason = "G66 API — wired by BTSP client and ecosystem connect"
    )]
    pub fn from_env_or_default(socket_path: &str, fallback_port: u16) -> Self {
        match Self::from_env() {
            Some(Ok(ep)) => ep,
            _ => Self::platform_default(socket_path, fallback_port),
        }
    }
}

impl std::fmt::Display for TransportEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display_uri())
    }
}

// ---------------------------------------------------------------------------
// TransportStream — platform-abstracted async byte pipe (G66)
// ---------------------------------------------------------------------------

/// Connected async byte pipe — the G66 transport abstraction.
///
/// `#[cfg(unix)]` lives here, not in business logic. Protocol negotiation,
/// JSON-RPC dispatch, and tarpc framing all operate on `TransportStream`
/// without knowing the underlying transport.
#[allow(dead_code, reason = "G66 API — wired by unix_jsonrpc accept loop")]
pub enum TransportStream {
    /// Unix domain socket connection (fastest local path, `SO_PEERCRED` available).
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),

    /// TCP connection (cross-host, container, or non-Unix fallback).
    Tcp(tokio::net::TcpStream),
}

impl std::fmt::Debug for TransportStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(unix)]
            Self::Unix(_) => f.write_str("TransportStream::Unix(..)"),
            Self::Tcp(_) => f.write_str("TransportStream::Tcp(..)"),
        }
    }
}

impl TransportStream {
    /// Whether this stream runs over a local (same-host) transport.
    #[must_use]
    #[allow(
        dead_code,
        reason = "G66 API — wired by BTSP local-trust and auth decisions"
    )]
    pub const fn is_local(&self) -> bool {
        match self {
            #[cfg(unix)]
            Self::Unix(_) => true,
            Self::Tcp(_) => false,
        }
    }
}

impl tokio::io::AsyncRead for TransportStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            Self::Tcp(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for TransportStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            Self::Tcp(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => std::pin::Pin::new(s).poll_flush(cx),
            Self::Tcp(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            #[cfg(unix)]
            Self::Unix(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            Self::Tcp(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

// ---------------------------------------------------------------------------
// TransportListener — platform-abstracted server accept loop (G66)
// ---------------------------------------------------------------------------

/// Server-side listener abstraction — accepts incoming `TransportStream` connections.
///
/// The accept loop uses this instead of platform-specific listener types.
/// G65 protocol negotiation composes on top: negotiate protocol on any transport.
#[allow(dead_code, reason = "G66 API — wired by unix_jsonrpc accept loop")]
pub enum TransportListener {
    /// Unix domain socket listener.
    #[cfg(unix)]
    Unix(tokio::net::UnixListener),

    /// TCP listener.
    Tcp(tokio::net::TcpListener),
}

impl std::fmt::Debug for TransportListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(unix)]
            Self::Unix(_) => f.write_str("TransportListener::Unix(..)"),
            Self::Tcp(_) => f.write_str("TransportListener::Tcp(..)"),
        }
    }
}

impl TransportListener {
    /// Accept an incoming connection, returning a platform-abstracted stream.
    ///
    /// # Errors
    ///
    /// Returns the underlying OS accept error.
    #[allow(dead_code, reason = "G66 API — wired by accept loop")]
    pub async fn accept(&self) -> std::io::Result<TransportStream> {
        match self {
            #[cfg(unix)]
            Self::Unix(l) => {
                let (stream, _addr) = l.accept().await?;
                Ok(TransportStream::Unix(stream))
            }
            Self::Tcp(l) => {
                let (stream, _addr) = l.accept().await?;
                Ok(TransportStream::Tcp(stream))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// connect_transport — the bridge (G66)
// ---------------------------------------------------------------------------

/// Connect to a `TransportEndpoint`, returning a platform-abstracted stream.
///
/// All `#[cfg(unix)]` conditionals for connection live here — callers
/// operate on `TransportStream` without knowing the transport underneath.
///
/// # Errors
///
/// Returns IO errors from the underlying connect, or `Unsupported` when
/// the endpoint requires a transport unavailable on this platform.
#[allow(
    dead_code,
    reason = "G66 API — wired by BTSP client and ecosystem connect"
)]
pub async fn connect_transport(endpoint: &TransportEndpoint) -> std::io::Result<TransportStream> {
    match endpoint {
        #[cfg(unix)]
        TransportEndpoint::Uds { path } => {
            let stream = tokio::net::UnixStream::connect(path).await?;
            Ok(TransportStream::Unix(stream))
        }
        #[cfg(not(unix))]
        TransportEndpoint::Uds { path } => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("UDS not available on this platform for {path}"),
        )),
        TransportEndpoint::Tcp { host, port } => {
            let stream = tokio::net::TcpStream::connect(format!("{host}:{port}")).await?;
            Ok(TransportStream::Tcp(stream))
        }
        TransportEndpoint::MeshRelay { .. } => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "mesh relay transport requires songBird routing",
        )),
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

    #[test]
    fn uds_roundtrip() {
        let json = r#"{"transport":"uds","path":"/run/biomeos/coralreef.sock"}"#;
        let ep: TransportEndpoint = serde_json::from_str(json).expect("parse uds");
        assert_eq!(
            ep,
            TransportEndpoint::Uds {
                path: "/run/biomeos/coralreef.sock".into()
            }
        );
        let re = serde_json::to_string(&ep).expect("serialize");
        let de: TransportEndpoint = serde_json::from_str(&re).expect("roundtrip");
        assert_eq!(ep, de);
    }

    #[test]
    fn tcp_roundtrip() {
        let json = r#"{"transport":"tcp","host":"127.0.0.1","port":9200}"#;
        let ep: TransportEndpoint = serde_json::from_str(json).expect("parse tcp");
        assert_eq!(
            ep,
            TransportEndpoint::Tcp {
                host: "127.0.0.1".into(),
                port: 9200
            }
        );
        let re = serde_json::to_string(&ep).expect("serialize");
        let de: TransportEndpoint = serde_json::from_str(&re).expect("roundtrip");
        assert_eq!(ep, de);
    }

    #[test]
    fn mesh_relay_roundtrip() {
        let json =
            r#"{"transport":"mesh_relay","peer_id":"strand-gate","capability":"shader.compile"}"#;
        let ep: TransportEndpoint = serde_json::from_str(json).expect("parse relay");
        assert_eq!(
            ep,
            TransportEndpoint::MeshRelay {
                peer_id: "strand-gate".into(),
                capability: "shader.compile".into()
            }
        );
    }

    #[test]
    fn is_local_classification() {
        assert!(
            TransportEndpoint::Uds {
                path: "/tmp/x.sock".into()
            }
            .is_local()
        );
        assert!(
            TransportEndpoint::Tcp {
                host: "127.0.0.1".into(),
                port: 80
            }
            .is_local()
        );
        assert!(
            TransportEndpoint::Tcp {
                host: "::1".into(),
                port: 80
            }
            .is_local()
        );
        assert!(
            !TransportEndpoint::Tcp {
                host: "192.168.1.5".into(),
                port: 7700
            }
            .is_local()
        );
        assert!(
            !TransportEndpoint::MeshRelay {
                peer_id: "p".into(),
                capability: "c".into()
            }
            .is_local()
        );
    }

    #[test]
    fn display_uri_formats() {
        let uds = TransportEndpoint::Uds {
            path: "/run/test.sock".into(),
        };
        assert_eq!(uds.display_uri(), "unix:///run/test.sock");

        let tcp = TransportEndpoint::Tcp {
            host: "10.0.0.1".into(),
            port: 7700,
        };
        assert_eq!(tcp.display_uri(), "tcp://10.0.0.1:7700");

        let ipv6 = TransportEndpoint::Tcp {
            host: "::1".into(),
            port: 8080,
        };
        assert_eq!(ipv6.display_uri(), "tcp://[::1]:8080");
    }

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
    fn transport_name_variants() {
        assert_eq!(
            TransportEndpoint::Uds { path: "/x".into() }.transport_name(),
            "uds"
        );
        assert_eq!(
            TransportEndpoint::Tcp {
                host: "h".into(),
                port: 1
            }
            .transport_name(),
            "tcp"
        );
        assert_eq!(
            TransportEndpoint::MeshRelay {
                peer_id: "p".into(),
                capability: "c".into()
            }
            .transport_name(),
            "mesh_relay"
        );
    }

    #[test]
    fn transport_endpoint_display_impl() {
        let uds = TransportEndpoint::Uds {
            path: "/run/test.sock".into(),
        };
        assert_eq!(format!("{uds}"), "unix:///run/test.sock");

        let tcp = TransportEndpoint::Tcp {
            host: "10.0.0.1".into(),
            port: 7700,
        };
        assert_eq!(format!("{tcp}"), "tcp://10.0.0.1:7700");

        let mesh = TransportEndpoint::MeshRelay {
            peer_id: "east".into(),
            capability: "shader.compile".into(),
        };
        assert_eq!(format!("{mesh}"), "mesh://east/shader.compile");
    }

    #[test]
    fn is_local_localhost_string() {
        assert!(
            TransportEndpoint::Tcp {
                host: "localhost".into(),
                port: 8080
            }
            .is_local()
        );
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

    #[test]
    fn display_uri_mesh_relay_format() {
        let mesh = TransportEndpoint::MeshRelay {
            peer_id: "strand-gate".into(),
            capability: "security.verify".into(),
        };
        assert_eq!(mesh.display_uri(), "mesh://strand-gate/security.verify");
    }

    #[test]
    fn wire_compat_with_ecosystem_format() {
        let cases = [
            r#"{"transport":"uds","path":"/run/membrane/security-provider.sock"}"#,
            r#"{"transport":"tcp","host":"192.168.1.144","port":7700}"#,
            r#"{"transport":"mesh_relay","peer_id":"strand-gate","capability":"security"}"#,
        ];
        for json in cases {
            let ep: TransportEndpoint = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("wire compat failed for {json}: {e}"));
            let re = serde_json::to_string(&ep).expect("serialize");
            let de: TransportEndpoint = serde_json::from_str(&re).expect("roundtrip");
            assert_eq!(ep, de);
        }
    }

    #[test]
    fn platform_default_returns_uds_on_unix() {
        let ep = TransportEndpoint::platform_default("/run/test.sock", 9200);
        if cfg!(unix) {
            assert!(matches!(ep, TransportEndpoint::Uds { .. }));
        } else {
            assert!(matches!(ep, TransportEndpoint::Tcp { .. }));
        }
    }

    #[test]
    fn from_env_or_default_falls_back_when_unset() {
        if std::env::var("TRANSPORT_ENDPOINT").is_err() {
            let ep = TransportEndpoint::from_env_or_default("/run/test.sock", 9200);
            if cfg!(unix) {
                assert!(matches!(ep, TransportEndpoint::Uds { .. }));
            } else {
                assert!(matches!(ep, TransportEndpoint::Tcp { .. }));
            }
        }
    }

    #[tokio::test]
    async fn connect_transport_rejects_mesh_relay() {
        let ep = TransportEndpoint::MeshRelay {
            peer_id: "east".into(),
            capability: "shader.compile".into(),
        };
        let err = connect_transport(&ep).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Unsupported);
    }

    #[tokio::test]
    async fn connect_transport_tcp_nonexistent_fails() {
        let ep = TransportEndpoint::Tcp {
            host: "127.0.0.1".into(),
            port: 1,
        };
        assert!(connect_transport(&ep).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn connect_transport_uds_nonexistent_fails() {
        let ep = TransportEndpoint::Uds {
            path: "/nonexistent/test.sock".into(),
        };
        assert!(connect_transport(&ep).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn transport_stream_read_write_uds_roundtrip() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = tempfile::tempdir().expect("tempdir");
        let sock = dir.path().join("g66-stream-test.sock");
        let listener = tokio::net::UnixListener::bind(&sock).expect("bind");
        let tl = TransportListener::Unix(listener);

        let connect_path = sock.to_str().unwrap().to_owned();
        let client = tokio::spawn(async move {
            let ep = TransportEndpoint::Uds { path: connect_path };
            let mut stream = connect_transport(&ep).await.unwrap();
            assert!(stream.is_local());
            stream.write_all(b"hello").await.unwrap();
            stream.shutdown().await.unwrap();
        });

        let mut server_stream = tl.accept().await.unwrap();
        assert!(server_stream.is_local());
        let mut buf = Vec::new();
        server_stream.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello");

        client.await.unwrap();
    }

    #[tokio::test]
    async fn transport_stream_read_write_tcp_roundtrip() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let tcp_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = tcp_listener.local_addr().unwrap();
        let tl = TransportListener::Tcp(tcp_listener);

        let client = tokio::spawn(async move {
            let ep = TransportEndpoint::Tcp {
                host: addr.ip().to_string(),
                port: addr.port(),
            };
            let mut stream = connect_transport(&ep).await.unwrap();
            assert!(!stream.is_local());
            stream.write_all(b"g66").await.unwrap();
            stream.shutdown().await.unwrap();
        });

        let mut server_stream = tl.accept().await.unwrap();
        assert!(!server_stream.is_local());
        let mut buf = Vec::new();
        server_stream.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"g66");

        client.await.unwrap();
    }

    #[tokio::test]
    async fn transport_listener_debug_format() {
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tl = TransportListener::Tcp(tcp);
        let s = format!("{tl:?}");
        assert!(s.contains("Tcp"), "debug output should mention Tcp: {s}");
    }
}
