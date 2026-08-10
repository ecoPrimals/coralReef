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

mod resolve;
mod stream;
mod sync_stream;

pub use resolve::{
    ResolvedBind, TransportResolveError, create_local_symlink, resolve_bind, resolve_bind_with_mode,
};
pub use stream::{TransportListener, TransportStream, bind_transport, connect_transport};
pub use sync_stream::{SyncTransportStream, connect_transport_sync};

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

impl TransportEndpoint {
    /// Parse a Phase-10 / ecosystem bind string into a transport endpoint.
    ///
    /// **G68**: handles all bind formats the ecosystem uses:
    /// - `unix:///path/to/socket.sock` → [`TransportEndpoint::Uds`]
    /// - `/absolute/path.sock` → [`TransportEndpoint::Uds`]
    /// - `tcp://host:port` → [`TransportEndpoint::Tcp`]
    /// - `host:port` → [`TransportEndpoint::Tcp`]
    ///
    /// Returns `None` if the string is empty or unrecognised.
    #[must_use]
    pub fn from_bind_string(bind: &str) -> Option<Self> {
        let b = bind.trim();
        if b.is_empty() {
            return None;
        }

        if let Some(rest) = b.strip_prefix("unix://") {
            return if rest.is_empty() {
                None
            } else {
                Some(Self::Uds {
                    path: rest.to_owned(),
                })
            };
        }

        if let Some(rest) = b.strip_prefix("tcp://") {
            return Self::parse_host_port(rest);
        }

        if b.starts_with('/') {
            return Some(Self::Uds { path: b.to_owned() });
        }

        Self::parse_host_port(b)
    }

    fn parse_host_port(s: &str) -> Option<Self> {
        let (host, port_str) = s.rsplit_once(':')?;
        let port: u16 = port_str.parse().ok()?;
        if host.is_empty() {
            return None;
        }
        Some(Self::Tcp {
            host: host.to_owned(),
            port,
        })
    }
}

impl std::fmt::Display for TransportEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display_uri())
    }
}

// ---------------------------------------------------------------------------
// wait_for_shutdown — cross-platform signal handling (G66)
// ---------------------------------------------------------------------------

/// Wait for a process termination signal. Returns which signal was received.
///
/// Unix: SIGTERM or SIGINT.
/// Non-Unix: Ctrl+C.
///
/// Confines `tokio::signal::unix::SignalKind` to the transport layer.
///
/// # Panics
///
/// Panics if signal registration fails.
pub async fn wait_for_shutdown() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
        let mut sigint =
            signal(SignalKind::interrupt()).expect("failed to register SIGINT handler");
        tokio::select! {
            _ = sigterm.recv() => "SIGTERM",
            _ = sigint.recv() => "SIGINT",
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to register Ctrl+C handler");
        "SIGINT"
    }
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
}
