// SPDX-License-Identifier: AGPL-3.0-or-later
//! G65 Protocol Negotiation (Phase 3 Cephalization).
//!
//! Enables automatic protocol selection between JSON-RPC and tarpc at
//! connection time on a **single socket**, replacing the C2 dual-socket
//! pattern (`.sock` + `.tarpc.sock`).
//!
//! ## Wire Protocol
//!
//! ```text
//! Client → Server: "PROTOCOLS: tarpc,jsonrpc\n"
//! Server → Client: "PROTOCOL: tarpc\n"
//! [Connection proceeds in selected protocol]
//! ```
//!
//! ## Backward Compatibility
//!
//! If the client does not send a `PROTOCOLS:` line (first byte is not `P`),
//! the server falls back to JSON-RPC. Existing clients work with zero changes.
//!
//! Convergent evolution from squirrel/sourDough G65 reference implementation.
//! See `wateringHole/specs/PROTOCOL_NEGOTIATION_SPEC.md`.

use super::ipc_protocol::IpcProtocol;

/// Maximum length of a `PROTOCOLS:` line before the server rejects it.
const MAX_NEGOTIATION_LINE_LEN: usize = 256;

/// G65 protocol negotiation request from a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolRequest {
    /// Protocols supported by the client (in preference order).
    pub supported: Vec<IpcProtocol>,
}

impl ProtocolRequest {
    /// Serialize to G65 wire format: `"PROTOCOLS: tarpc,jsonrpc\n"`.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "used by client-side G65 negotiation")
    )]
    #[must_use]
    pub fn to_wire(&self) -> String {
        let names: Vec<&str> = self
            .supported
            .iter()
            .copied()
            .map(IpcProtocol::negotiation_name)
            .collect();
        format!("PROTOCOLS: {}\n", names.join(","))
    }

    /// Parse from G65 wire format.
    ///
    /// Returns `None` when the line does not start with `PROTOCOLS: ` or
    /// contains no recognised protocol names.
    #[must_use]
    pub fn from_wire(line: &str) -> Option<Self> {
        let trimmed = line.trim();
        let protocols_str = trimmed.strip_prefix("PROTOCOLS: ")?;

        let supported: Vec<IpcProtocol> = protocols_str
            .split(',')
            .filter_map(|name| IpcProtocol::from_name(name.trim()))
            .collect();

        if supported.is_empty() {
            return None;
        }

        Some(Self { supported })
    }
}

/// G65 protocol negotiation response from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolResponse {
    /// The selected protocol.
    pub selected: IpcProtocol,
}

impl ProtocolResponse {
    /// Create a new protocol response.
    #[must_use]
    pub const fn new(selected: IpcProtocol) -> Self {
        Self { selected }
    }

    /// Serialize to G65 wire format: `"PROTOCOL: tarpc\n"`.
    #[must_use]
    pub fn to_wire(&self) -> String {
        format!("PROTOCOL: {}\n", self.selected.negotiation_name())
    }
}

/// Select the best mutual protocol (client preference order wins).
///
/// Returns the first client-preferred protocol that the server also supports.
/// Falls back to `JsonRpc` if no intersection exists.
#[must_use]
pub fn select_protocol(
    client_supported: &[IpcProtocol],
    server_supported: &[IpcProtocol],
) -> IpcProtocol {
    for proto in client_supported {
        if server_supported.contains(proto) {
            return *proto;
        }
    }
    IpcProtocol::JsonRpc
}

/// Read a single newline-terminated line **byte-by-byte** from the stream.
///
/// Avoids `BufReader` read-ahead so no bytes beyond the line are consumed.
/// The caller must have already consumed the first byte (`P`) — this
/// function reads the remainder and returns the complete line including the
/// prepended `P`.
///
/// # Errors
///
/// Returns `None` on I/O failure or if the line exceeds
/// [`MAX_NEGOTIATION_LINE_LEN`].
pub async fn read_negotiation_line_after_p<R>(stream: &mut R) -> Option<String>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    let mut buf = Vec::with_capacity(64);
    buf.push(b'P');
    let mut byte = [0u8; 1];

    loop {
        match stream.read(&mut byte).await {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }
        buf.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
        if buf.len() > MAX_NEGOTIATION_LINE_LEN {
            tracing::warn!("G65 negotiation line exceeds {MAX_NEGOTIATION_LINE_LEN} bytes");
            return None;
        }
    }

    String::from_utf8(buf).ok()
}

/// Server-side G65 negotiation on a stream where the first byte (`P`) has
/// already been consumed.
///
/// Reads the rest of the `PROTOCOLS:` line byte-by-byte, selects the best
/// protocol, writes the `PROTOCOL:` response, and returns the selected
/// protocol.
///
/// Returns `None` if the line is malformed or I/O fails. On parse failure
/// the server responds with `PROTOCOL: jsonrpc\n` for robustness.
pub async fn negotiate_server_after_p<S>(
    stream: &mut S,
    server_supported: &[IpcProtocol],
) -> Option<IpcProtocol>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    let line = read_negotiation_line_after_p(stream).await?;

    let Some(request) = ProtocolRequest::from_wire(&line) else {
        tracing::warn!("Invalid G65 protocol request: {line:?}");
        let _ = stream.write_all(b"PROTOCOL: jsonrpc\n").await;
        let _ = stream.flush().await;
        return Some(IpcProtocol::JsonRpc);
    };

    let selected = select_protocol(&request.supported, server_supported);
    let response = ProtocolResponse::new(selected);

    if stream
        .write_all(response.to_wire().as_bytes())
        .await
        .is_err()
    {
        tracing::warn!("G65 response write failed");
        return None;
    }
    let _ = stream.flush().await;

    tracing::info!("G65 protocol negotiated: {selected}");
    Some(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_format_single() {
        let req = ProtocolRequest {
            supported: vec![IpcProtocol::JsonRpc],
        };
        assert_eq!(req.to_wire(), "PROTOCOLS: jsonrpc\n");
    }

    #[test]
    fn request_wire_format_multi() {
        let req = ProtocolRequest {
            supported: vec![IpcProtocol::Tarpc, IpcProtocol::JsonRpc],
        };
        assert_eq!(req.to_wire(), "PROTOCOLS: tarpc,jsonrpc\n");
    }

    #[test]
    fn request_parse_single() {
        let req = ProtocolRequest::from_wire("PROTOCOLS: jsonrpc\n").expect("parse");
        assert_eq!(req.supported, vec![IpcProtocol::JsonRpc]);
    }

    #[test]
    fn request_parse_multi() {
        let req = ProtocolRequest::from_wire("PROTOCOLS: tarpc,jsonrpc\n").expect("parse");
        assert_eq!(
            req.supported,
            vec![IpcProtocol::Tarpc, IpcProtocol::JsonRpc]
        );
    }

    #[test]
    fn response_wire_roundtrip() {
        let resp = ProtocolResponse::new(IpcProtocol::Tarpc);
        assert_eq!(resp.to_wire(), "PROTOCOL: tarpc\n");
    }

    #[test]
    fn select_protocol_client_preference_wins() {
        let client = vec![IpcProtocol::Tarpc, IpcProtocol::JsonRpc];
        let server = vec![IpcProtocol::Tarpc, IpcProtocol::JsonRpc];
        assert_eq!(select_protocol(&client, &server), IpcProtocol::Tarpc);
    }

    #[test]
    fn select_protocol_server_only_jsonrpc() {
        let client = vec![IpcProtocol::Tarpc, IpcProtocol::JsonRpc];
        let server = vec![IpcProtocol::JsonRpc];
        assert_eq!(select_protocol(&client, &server), IpcProtocol::JsonRpc);
    }

    #[test]
    fn select_protocol_no_match_falls_back() {
        let client = vec![IpcProtocol::Tarpc];
        let server = vec![IpcProtocol::JsonRpc];
        assert_eq!(select_protocol(&client, &server), IpcProtocol::JsonRpc);
    }

    #[test]
    fn request_invalid_prefix_returns_none() {
        assert!(ProtocolRequest::from_wire("NOT_PROTOCOLS: jsonrpc\n").is_none());
    }

    #[test]
    fn request_no_valid_protocols_returns_none() {
        assert!(ProtocolRequest::from_wire("PROTOCOLS: unknown\n").is_none());
    }

    #[tokio::test]
    async fn read_negotiation_line_after_p_exact() {
        let rest = b"ROTOCOLS: tarpc,jsonrpc\ngarbage";
        let mut cursor = &rest[..];
        let line = read_negotiation_line_after_p(&mut cursor)
            .await
            .expect("read");
        assert_eq!(line, "PROTOCOLS: tarpc,jsonrpc\n");
    }

    #[tokio::test]
    async fn read_negotiation_line_after_p_rejects_overlong() {
        let rest = vec![b'A'; MAX_NEGOTIATION_LINE_LEN + 2];
        let mut cursor = &rest[..];
        assert!(read_negotiation_line_after_p(&mut cursor).await.is_none());
    }

    #[tokio::test]
    async fn negotiate_server_after_p_tarpc() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (mut client, mut server) = tokio::io::duplex(4096);
        let server_supported = IpcProtocol::supported();

        let client_task = tokio::spawn(async move {
            client
                .write_all(b"ROTOCOLS: tarpc,jsonrpc\n")
                .await
                .unwrap();
            client.flush().await.unwrap();

            let mut response = String::new();
            let mut buf = [0u8; 1];
            loop {
                let n = client.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                response.push(buf[0] as char);
                if buf[0] == b'\n' {
                    break;
                }
            }
            response
        });

        let selected = negotiate_server_after_p(&mut server, &server_supported)
            .await
            .expect("negotiate");
        assert_eq!(selected, IpcProtocol::Tarpc);

        let response = client_task.await.expect("join");
        assert_eq!(response, "PROTOCOL: tarpc\n");
    }

    #[tokio::test]
    async fn negotiate_server_after_p_jsonrpc_only() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (mut client, mut server) = tokio::io::duplex(4096);
        let server_supported = vec![IpcProtocol::JsonRpc];

        let client_task = tokio::spawn(async move {
            client
                .write_all(b"ROTOCOLS: tarpc,jsonrpc\n")
                .await
                .unwrap();
            client.flush().await.unwrap();

            let mut response = String::new();
            let mut buf = [0u8; 1];
            loop {
                let n = client.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                response.push(buf[0] as char);
                if buf[0] == b'\n' {
                    break;
                }
            }
            response
        });

        let selected = negotiate_server_after_p(&mut server, &server_supported)
            .await
            .expect("negotiate");
        assert_eq!(selected, IpcProtocol::JsonRpc);

        let response = client_task.await.expect("join");
        assert_eq!(response, "PROTOCOL: jsonrpc\n");
    }

    #[tokio::test]
    async fn negotiate_server_after_p_malformed_falls_back() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (mut client, mut server) = tokio::io::duplex(4096);
        let server_supported = IpcProtocol::supported();

        let client_task = tokio::spawn(async move {
            client.write_all(b"ROTOCOLS: garbage\n").await.unwrap();
            client.flush().await.unwrap();

            let mut response = String::new();
            let mut buf = [0u8; 1];
            loop {
                let n = client.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                response.push(buf[0] as char);
                if buf[0] == b'\n' {
                    break;
                }
            }
            response
        });

        let selected = negotiate_server_after_p(&mut server, &server_supported)
            .await
            .expect("negotiate");
        assert_eq!(selected, IpcProtocol::JsonRpc);

        let response = client_task.await.expect("join");
        assert_eq!(response, "PROTOCOL: jsonrpc\n");
    }
}
