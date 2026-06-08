// SPDX-License-Identifier: AGPL-3.0-or-later
//! Transport endpoint injection — sourDough wire-compatible.
//!
//! Primals do not choose their transport — the launcher or Tower Atomic decides.
//! When `$TRANSPORT_ENDPOINT` is set, the primal binds on the injected endpoint
//! instead of self-selecting via `--rpc-bind` / `--socket`.
//!
//! Wire format (JSON, serde-tagged — compatible with `sourdough_core::TransportEndpoint`):
//! ```json
//! {"transport":"uds","path":"/run/user/1000/biomeos/coralreef.sock"}
//! {"transport":"tcp","host":"127.0.0.1","port":9200}
//! ```

use serde::{Deserialize, Serialize};

use crate::env_keys;

/// Structured transport endpoint — wire-compatible with sourDough canonical standard.
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

    /// Mesh relay — primal reachable via Songbird's mesh network.
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

impl std::fmt::Display for TransportEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display_uri())
    }
}

/// Resolved bind configuration from transport injection or CLI fallback.
#[derive(Debug, Clone)]
pub enum ResolvedBind {
    /// Bind UDS only (launcher-injected transport).
    UdsOnly {
        /// Path to bind the Unix domain socket.
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
pub fn resolve_bind(
    cli_tcp_bind: &str,
    cli_socket: Option<&std::path::Path>,
) -> Result<ResolvedBind, TransportResolveError> {
    if let Some(parsed) = TransportEndpoint::from_env() {
        let endpoint = parsed.map_err(TransportResolveError::InvalidJson)?;
        match endpoint {
            TransportEndpoint::Uds { path } => {
                tracing::info!(
                    transport = "uds",
                    path = %path,
                    "transport injected by launcher"
                );
                Ok(ResolvedBind::UdsOnly {
                    path: std::path::PathBuf::from(path),
                })
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
    fn wire_compat_with_sourdough_format() {
        let cases = [
            r#"{"transport":"uds","path":"/run/membrane/beardog.sock"}"#,
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
}
