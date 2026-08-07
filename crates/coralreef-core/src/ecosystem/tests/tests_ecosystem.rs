// SPDX-License-Identifier: AGPL-3.0-or-later
use super::super::*;
use std::io::Write;
use std::path::Path;

#[test]
fn jsonrpc_bind_to_unix_path_accepts_unix_scheme() {
    let p = jsonrpc_bind_to_unix_path("unix:///run/biomeos/registry.sock");
    assert_eq!(p.as_deref(), Some(Path::new("/run/biomeos/registry.sock")));
}

#[test]
fn jsonrpc_bind_to_unix_path_accepts_absolute() {
    let p = jsonrpc_bind_to_unix_path("/tmp/foo.sock");
    assert_eq!(p.as_deref(), Some(Path::new("/tmp/foo.sock")));
}

#[test]
fn jsonrpc_bind_to_unix_path_rejects_tcp_like() {
    assert!(jsonrpc_bind_to_unix_path("127.0.0.1:9000").is_none());
}

#[test]
fn registry_bind_from_json_file_finds_nested_provides() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("registry.json");
    let j = serde_json::json!({
        "provides": [{"id": "capability.register", "version": "1.0.0"}],
        "transports": { "jsonrpc": { "bind": "unix:///run/ecosystem/reg.sock" } }
    });
    let mut f = std::fs::File::create(&path).expect("create");
    write!(f, "{j}").expect("write");
    let bind = registry_bind_from_json_file(&path).expect("bind");
    assert_eq!(bind, "unix:///run/ecosystem/reg.sock");
}

#[test]
fn registry_bind_from_json_file_string_provides() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("reg2.json");
    let j = serde_json::json!({
        "provides": ["capability.register"],
        "endpoint": "unix:///tmp/x.sock"
    });
    let mut f = std::fs::File::create(&path).expect("create");
    write!(f, "{j}").expect("write");
    let bind = registry_bind_from_json_file(&path).expect("bind");
    assert_eq!(bind, "unix:///tmp/x.sock");
}

#[test]
fn registry_bind_from_json_file_ignores_wrong_capability() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("other.json");
    let j = serde_json::json!({
        "provides": ["gpu.dispatch"],
        "transports": { "jsonrpc": { "bind": "unix:///run/x.sock" } }
    });
    let mut f = std::fs::File::create(&path).expect("create");
    write!(f, "{j}").expect("write");
    assert!(registry_bind_from_json_file(&path).is_none());
}

#[test]
fn registry_bind_from_json_file_malformed_returns_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("broken.json");
    std::fs::write(&path, "not-json").expect("write");
    assert!(registry_bind_from_json_file(&path).is_none());
}

#[test]
fn jsonrpc_bind_to_unix_path_strips_whitespace() {
    let p = jsonrpc_bind_to_unix_path("  unix:///run/test.sock  ");
    assert_eq!(p.as_deref(), Some(Path::new("/run/test.sock")));
}

#[test]
fn jsonrpc_bind_to_unix_path_rejects_empty_unix_scheme() {
    assert!(jsonrpc_bind_to_unix_path("unix://").is_none());
}

#[test]
fn jsonrpc_bind_to_unix_path_rejects_relative_path() {
    assert!(jsonrpc_bind_to_unix_path("relative/path.sock").is_none());
}

#[test]
fn registry_bind_from_json_file_prefers_transports_over_endpoint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("both.json");
    let j = serde_json::json!({
        "provides": ["capability.register"],
        "transports": { "jsonrpc": { "bind": "unix:///run/preferred.sock" } },
        "endpoint": "unix:///run/fallback.sock"
    });
    let mut f = std::fs::File::create(&path).expect("create");
    write!(f, "{j}").expect("write");
    let bind = registry_bind_from_json_file(&path).expect("bind");
    assert_eq!(bind, "unix:///run/preferred.sock");
}

#[test]
fn registry_bind_from_json_file_missing_provides() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("no-provides.json");
    let j = serde_json::json!({
        "transports": { "jsonrpc": { "bind": "unix:///run/x.sock" } }
    });
    let mut f = std::fs::File::create(&path).expect("create");
    write!(f, "{j}").expect("write");
    assert!(registry_bind_from_json_file(&path).is_none());
}

#[test]
fn registry_bind_from_json_file_provides_not_array() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("provides-string.json");
    let j = serde_json::json!({
        "provides": "capability.register",
        "endpoint": "unix:///run/x.sock"
    });
    let mut f = std::fs::File::create(&path).expect("create");
    write!(f, "{j}").expect("write");
    assert!(registry_bind_from_json_file(&path).is_none());
}

#[test]
fn registry_bind_from_json_file_nonexistent_path() {
    assert!(registry_bind_from_json_file(Path::new("/nonexistent/file.json")).is_none());
}

#[test]
fn discover_ecosystem_scans_directory_with_no_matching_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let j = serde_json::json!({
        "provides": ["gpu.dispatch"],
        "endpoint": "unix:///tmp/irrelevant.sock"
    });
    std::fs::write(dir.path().join("other.json"), j.to_string()).expect("write");
}

#[cfg(unix)]
#[test]
fn socket_is_alive_returns_false_for_nonexistent() {
    assert!(!socket_is_alive(Path::new("/nonexistent/socket.sock")));
}

#[cfg(unix)]
#[test]
fn socket_is_alive_returns_false_for_regular_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("not-a-socket");
    std::fs::write(&path, "data").expect("write");
    assert!(!socket_is_alive(&path));
}

#[test]
fn ecosystem_error_display_transport() {
    let err = EcosystemError::Transport("connection refused".into());
    assert!(err.to_string().contains("connection refused"));
}

#[test]
fn ecosystem_error_display_encode() {
    let bad_json: Result<serde_json::Value, _> = serde_json::from_str("}{");
    let err = EcosystemError::Encode(bad_json.unwrap_err());
    assert!(err.to_string().contains("JSON encode"));
}

#[test]
fn resolve_own_socket_path_ends_with_sock() {
    let path = resolve_own_socket_path();
    assert!(
        path.extension().is_some_and(|e| e == "sock"),
        "socket path should end in .sock: {}",
        path.display()
    );
}

#[test]
fn register_params_serializes_correctly() {
    let params = RegisterParams {
        name: "test-primal",
        version: "0.1.0",
        provides: &[],
        requires: &[],
        transports: &[],
    };
    let json = serde_json::to_value(&params).expect("serialize");
    assert_eq!(json["name"], "test-primal");
    assert_eq!(json["version"], "0.1.0");
    assert!(json["provides"].as_array().expect("array").is_empty());
}

#[test]
fn primal_announce_payload_has_required_fields() {
    let socket_path = resolve_own_socket_path();
    let params = serde_json::json!({
        "primal": config::PRIMAL_NAME,
        "version": config::PRIMAL_VERSION,
        "pid": std::process::id(),
        "socket": socket_path.to_string_lossy(),
        "capabilities": ["compile", "shader_compile", "gpu"],
        "methods": config::SERVED_METHODS,
        "signal_tiers": ["node"],
        "cost_hints": {
            "compile": 60.0,
            "shader_compile": 80.0,
            "gpu": 100.0
        },
        "latency_estimates": {
            "compile": 500,
            "shader_compile": 800,
            "gpu": 50
        }
    });

    assert!(
        params.get("name").is_none(),
        "payload must use 'primal' not 'name' (biomeOS rejects 'name')"
    );
    assert_eq!(
        params["primal"].as_str().expect("primal field"),
        config::PRIMAL_NAME
    );

    let methods = params["methods"].as_array().expect("methods array");
    assert!(
        !methods.is_empty(),
        "methods must be non-empty for Neural API routing"
    );
    assert!(
        methods.len() >= 16,
        "expected at least 16 announced methods"
    );
    assert!(methods.contains(&serde_json::json!("shader.compile.wgsl")));
    assert!(methods.contains(&serde_json::json!("shader.compile.spirv")));

    let caps = params["capabilities"]
        .as_array()
        .expect("capabilities array");
    assert_eq!(caps.len(), 3);
    assert_eq!(caps[0], "compile");
    assert_eq!(caps[1], "shader_compile");
    assert_eq!(caps[2], "gpu");

    let tiers = params["signal_tiers"]
        .as_array()
        .expect("signal_tiers array");
    assert_eq!(tiers.len(), 1);
    assert_eq!(tiers[0], "node");

    let costs = params["cost_hints"].as_object().expect("cost_hints object");
    assert_eq!(costs.len(), 3);
    assert_eq!(costs["compile"], 60.0);
    assert_eq!(costs["shader_compile"], 80.0);
    assert_eq!(costs["gpu"], 100.0);

    let latency = params["latency_estimates"]
        .as_object()
        .expect("latency_estimates object");
    assert_eq!(latency.len(), 3);
    assert_eq!(latency["compile"], 500);
    assert_eq!(latency["shader_compile"], 800);
    assert_eq!(latency["gpu"], 50);

    assert!(
        Path::new(params["socket"].as_str().expect("socket string"))
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("sock"))
    );
    assert!(params["pid"].as_u64().is_some(), "pid must be present");
}

#[test]
fn discover_ecosystem_scans_tempdir_with_registry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let registry = serde_json::json!({
        "provides": ["capability.register"],
        "transports": { "jsonrpc": { "bind": "unix:///tmp/test-registry.sock" } }
    });
    std::fs::write(dir.path().join("registry.json"), registry.to_string()).expect("write");
    let non_registry = serde_json::json!({
        "provides": ["gpu.dispatch"],
        "endpoint": "unix:///tmp/gpu.sock"
    });
    std::fs::write(
        dir.path().join("gpu-provider.json"),
        non_registry.to_string(),
    )
    .expect("write");

    let bind = registry_bind_from_json_file(&dir.path().join("registry.json"));
    assert!(bind.is_some(), "should find registry bind");
    assert_eq!(bind.unwrap(), "unix:///tmp/test-registry.sock");

    let bind2 = registry_bind_from_json_file(&dir.path().join("gpu-provider.json"));
    assert!(bind2.is_none(), "should not find gpu provider as registry");
}

#[test]
fn cost_and_latency_constants_are_positive() {
    let cost_compile = COST_COMPILE;
    let cost_shader_compile = COST_SHADER_COMPILE;
    let cost_gpu_dispatch = COST_GPU_DISPATCH;
    let latency_compile_ms = LATENCY_COMPILE_MS;
    let latency_shader_compile_ms = LATENCY_SHADER_COMPILE_MS;
    let latency_gpu_dispatch_ms = LATENCY_GPU_DISPATCH_MS;
    assert!(cost_compile > 0.0);
    assert!(cost_shader_compile > 0.0);
    assert!(cost_gpu_dispatch > 0.0);
    assert!(latency_compile_ms > 0);
    assert!(latency_shader_compile_ms > 0);
    assert!(latency_gpu_dispatch_ms > 0);
}

#[test]
fn cost_ordering_reflects_complexity() {
    let cost_compile = COST_COMPILE;
    let cost_shader_compile = COST_SHADER_COMPILE;
    let cost_gpu_dispatch = COST_GPU_DISPATCH;
    assert!(
        cost_compile < cost_shader_compile,
        "shader compile should cost more than basic compile"
    );
    assert!(
        cost_shader_compile < cost_gpu_dispatch,
        "GPU dispatch should cost the most"
    );
}

#[cfg(unix)]
#[test]
fn socket_is_alive_returns_true_for_live_listener() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("live.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&sock).expect("bind test listener");
    assert!(socket_is_alive(&sock));
}

#[test]
fn registry_bind_provides_non_string_element_returns_false() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("numeric.json");
    let j = serde_json::json!({
        "provides": [42, true, null, "capability.register"],
        "transports": { "jsonrpc": { "bind": "unix:///tmp/reg.sock" } }
    });
    std::fs::write(&path, j.to_string()).expect("write");
    let bind = registry_bind_from_json_file(&path);
    assert!(
        bind.is_some(),
        "should still find registry even with non-string elements"
    );
}

#[test]
fn registry_bind_provides_object_with_matching_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("obj-provides.json");
    let j = serde_json::json!({
        "provides": [{"id": "capability.register", "version": "1.0"}],
        "endpoint": "unix:///tmp/obj-reg.sock"
    });
    std::fs::write(&path, j.to_string()).expect("write");
    let bind = registry_bind_from_json_file(&path);
    assert_eq!(bind.as_deref(), Some("unix:///tmp/obj-reg.sock"));
}

#[test]
fn registry_bind_provides_object_wrong_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("wrong-id.json");
    let j = serde_json::json!({
        "provides": [{"id": "gpu.dispatch"}],
        "endpoint": "unix:///tmp/wrong.sock"
    });
    std::fs::write(&path, j.to_string()).expect("write");
    assert!(registry_bind_from_json_file(&path).is_none());
}

#[test]
fn registry_bind_has_provides_but_no_bind_or_endpoint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("no-bind.json");
    let j = serde_json::json!({
        "provides": ["capability.register"],
        "other_field": "value"
    });
    std::fs::write(&path, j.to_string()).expect("write");
    assert!(
        registry_bind_from_json_file(&path).is_none(),
        "no transports.jsonrpc.bind and no endpoint => None"
    );
}

#[test]
fn registry_bind_endpoint_fallback_when_no_transports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("endpoint-only.json");
    let j = serde_json::json!({
        "provides": ["capability.register"],
        "endpoint": "unix:///tmp/ep.sock"
    });
    std::fs::write(&path, j.to_string()).expect("write");
    let bind = registry_bind_from_json_file(&path);
    assert_eq!(bind.as_deref(), Some("unix:///tmp/ep.sock"));
}

#[test]
fn discover_ecosystem_scans_dir_skips_non_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let registry = serde_json::json!({
        "provides": ["capability.register"],
        "endpoint": "unix:///tmp/valid-reg.sock"
    });
    std::fs::write(dir.path().join("registry.json"), registry.to_string()).expect("write");
    std::fs::write(dir.path().join("notes.txt"), "not json").expect("write");
    std::fs::write(dir.path().join("data.yaml"), "key: val").expect("write");

    let mut found = false;
    let entries = std::fs::read_dir(dir.path()).expect("readdir");
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            if let Some(bind) = registry_bind_from_json_file(&path) {
                assert_eq!(bind, "unix:///tmp/valid-reg.sock");
                found = true;
            }
        }
    }
    assert!(found, "should find registry from .json, not .txt or .yaml");
}

#[test]
fn parse_bind_to_endpoint_unix_scheme() {
    let ep = parse_bind_to_endpoint("unix:///run/biomeos/registry.sock");
    assert!(matches!(
        ep,
        Some(crate::transport::TransportEndpoint::Uds { ref path }) if path == "/run/biomeos/registry.sock"
    ));
}

#[test]
fn parse_bind_to_endpoint_absolute_path() {
    let ep = parse_bind_to_endpoint("/tmp/foo.sock");
    assert!(matches!(
        ep,
        Some(crate::transport::TransportEndpoint::Uds { ref path }) if path == "/tmp/foo.sock"
    ));
}

#[test]
fn parse_bind_to_endpoint_tcp_scheme() {
    let ep = parse_bind_to_endpoint("tcp://127.0.0.1:9100");
    assert!(matches!(
        ep,
        Some(crate::transport::TransportEndpoint::Tcp { ref host, port }) if host == "127.0.0.1" && port == 9100
    ));
}

#[test]
fn parse_bind_to_endpoint_host_port() {
    let ep = parse_bind_to_endpoint("192.168.1.5:8080");
    assert!(matches!(
        ep,
        Some(crate::transport::TransportEndpoint::Tcp { ref host, port }) if host == "192.168.1.5" && port == 8080
    ));
}

#[test]
fn parse_bind_to_endpoint_empty() {
    assert!(parse_bind_to_endpoint("").is_none());
    assert!(parse_bind_to_endpoint("   ").is_none());
}

#[test]
fn parse_bind_to_endpoint_empty_unix_scheme() {
    assert!(parse_bind_to_endpoint("unix://").is_none());
}

#[test]
fn parse_bind_to_endpoint_strips_whitespace() {
    let ep = parse_bind_to_endpoint("  unix:///run/test.sock  ");
    assert!(matches!(
        ep,
        Some(crate::transport::TransportEndpoint::Uds { ref path }) if path == "/run/test.sock"
    ));
}

#[test]
fn parse_bind_to_endpoint_rejects_relative_path() {
    let ep = parse_bind_to_endpoint("relative/path.sock");
    assert!(ep.is_none(), "relative paths are not valid bind strings");
}

#[test]
fn parse_bind_to_endpoint_localhost() {
    let ep = parse_bind_to_endpoint("localhost:9200");
    assert!(matches!(
        ep,
        Some(crate::transport::TransportEndpoint::Tcp { ref host, port }) if host == "localhost" && port == 9200
    ));
}

#[test]
fn parse_bind_to_endpoint_invalid_port() {
    assert!(
        parse_bind_to_endpoint("127.0.0.1:notaport").is_none(),
        "non-numeric port should fail"
    );
}

#[tokio::test]
async fn send_jsonrpc_line_tcp_connect_failure() {
    let ep = crate::transport::TransportEndpoint::Tcp {
        host: "127.0.0.1".into(),
        port: 1,
    };
    let result = send_jsonrpc_line(&ep, "test.method", serde_json::json!({}), 1).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, EcosystemError::Transport(_)),
        "TCP connect failure should produce Transport error: {err}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn send_jsonrpc_line_connect_failure() {
    let ep = crate::transport::TransportEndpoint::Uds {
        path: "/nonexistent/socket.sock".into(),
    };
    let result = send_jsonrpc_line(&ep, "test.method", serde_json::json!({}), 1).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, EcosystemError::Transport(_)),
        "connect failure should produce Transport error: {err}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn send_jsonrpc_line_happy_path_with_mock_listener() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("mock-registry.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind mock registry");

    let ep = crate::transport::TransportEndpoint::Uds {
        path: sock.to_string_lossy().into_owned(),
    };
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read request");

        let parsed: serde_json::Value =
            serde_json::from_str(&line).expect("request should be valid JSON");
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "capability.register");

        let response = serde_json::json!({"jsonrpc": "2.0", "result": "ok", "id": 1});
        let writer = reader.into_inner();
        let (_, mut write_half) = tokio::io::split(writer);
        write_half
            .write_all(format!("{response}\n").as_bytes())
            .await
            .expect("write response");
    });

    let result = send_jsonrpc_line(
        &ep,
        "capability.register",
        serde_json::json!({"name": "test"}),
        1,
    )
    .await;
    assert!(
        result.is_ok(),
        "should succeed with mock listener: {result:?}"
    );

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn send_capability_register_with_mock() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("cap-reg.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

    let ep = crate::transport::TransportEndpoint::Uds {
        path: sock.to_string_lossy().into_owned(),
    };
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read");
        let writer = reader.into_inner();
        let (_, mut w) = tokio::io::split(writer);
        let resp = serde_json::json!({"jsonrpc":"2.0","result":"ok","id":1});
        w.write_all(format!("{resp}\n").as_bytes())
            .await
            .expect("write");
    });

    let desc = crate::capability::self_description();
    let result = send_capability_register(&ep, &desc).await;
    assert!(
        result.is_ok(),
        "capability register should succeed: {result:?}"
    );

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn send_primal_announce_with_mock() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("announce.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

    let ep = crate::transport::TransportEndpoint::Uds {
        path: sock.to_string_lossy().into_owned(),
    };
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(parsed["method"], "primal.announce");
        let writer = reader.into_inner();
        let (_, mut w) = tokio::io::split(writer);
        let resp = serde_json::json!({"jsonrpc":"2.0","result":"ok","id":3});
        w.write_all(format!("{resp}\n").as_bytes())
            .await
            .expect("write");
    });

    let result = send_primal_announce(&ep).await;
    assert!(result.is_ok(), "primal announce should succeed: {result:?}");

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
}

#[cfg(unix)]
#[tokio::test]
async fn send_ipc_heartbeat_with_mock() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("heartbeat.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind");

    let ep = crate::transport::TransportEndpoint::Uds {
        path: sock.to_string_lossy().into_owned(),
    };
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(parsed["method"], "ipc.heartbeat");
        let writer = reader.into_inner();
        let (_, mut w) = tokio::io::split(writer);
        let resp = serde_json::json!({"jsonrpc":"2.0","result":"ok","id":2});
        w.write_all(format!("{resp}\n").as_bytes())
            .await
            .expect("write");
    });

    let result = send_ipc_heartbeat(&ep).await;
    assert!(result.is_ok(), "heartbeat should succeed: {result:?}");

    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), server).await;
}
