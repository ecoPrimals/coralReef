// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unix JSON-RPC (newline-delimited over Unix socket) tests.

#[cfg(unix)]
use super::newline_jsonrpc::dispatch;
use super::unix_jsonrpc::make_response;
#[cfg(unix)]
use super::*;

// --- Unit tests for dispatch and make_response ---

#[cfg(unix)]
#[test]
fn test_dispatch_valid_method_status() {
    let result = dispatch("shader.compile.status", serde_json::json!({}));
    let val = result.expect("status should succeed");
    assert!(val.get("name").and_then(|v| v.as_str()).is_some());
    assert!(
        val.get("supported_archs")
            .and_then(|v| v.as_array())
            .is_some()
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_health_check() {
    let result = dispatch("health.check", serde_json::json!({}));
    let val = result.expect("health.check should succeed");
    assert_eq!(
        val.get("healthy").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(
        val.get("name")
            .and_then(serde_json::Value::as_str)
            .is_some()
    );
    assert!(
        val.get("version")
            .and_then(serde_json::Value::as_str)
            .is_some()
    );
    assert!(
        val.get("family_id")
            .and_then(serde_json::Value::as_str)
            .is_some()
    );
    assert!(
        val.get("supported_archs")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|a| !a.is_empty())
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_health_liveness() {
    let result = dispatch("health.liveness", serde_json::json!({}));
    let val = result.expect("health.liveness should succeed");
    assert_eq!(
        val.get("status").and_then(serde_json::Value::as_str),
        Some("alive")
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_health_readiness() {
    crate::service::mark_startup();
    let result = dispatch("health.readiness", serde_json::json!({}));
    let val = result.expect("health.readiness should succeed");
    assert_eq!(
        val.get("ready").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert!(
        val.get("name")
            .and_then(serde_json::Value::as_str)
            .is_some()
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_valid_method_capabilities() {
    let result = dispatch("shader.compile.capabilities", serde_json::json!({}));
    let val = result.expect("capabilities should succeed");
    let obj = val.as_object().expect("capabilities returns object");
    let archs = obj["targets"]
        .as_array()
        .expect("targets is array (Gate 1 wire contract)");
    assert!(!archs.is_empty());
    assert_eq!(obj["f64_transcendentals"]["composite_lowering"], true);
}

#[cfg(unix)]
#[test]
fn test_dispatch_valid_method_wgsl() {
    let params = serde_json::json!({
        "wgsl_source": "@compute @workgroup_size(1) fn main() {}",
        "arch": "sm_70",
        "opt_level": 2,
        "fp64_software": true
    });
    let result = dispatch("shader.compile.wgsl", params);
    let val = result.expect("wgsl compile should succeed");
    assert!(
        val.get("size")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_unknown_method() {
    let result = dispatch("nonexistent.method", serde_json::json!({}));
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(msg.contains("not found"));
    assert!(msg.contains("nonexistent.method"));
}

#[cfg(unix)]
#[test]
fn test_dispatch_empty_params_array() {
    let result = dispatch("shader.compile.wgsl", serde_json::json!([]));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("missing")
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_null_params() {
    let result = dispatch("shader.compile.wgsl", serde_json::Value::Null);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("must be array or object")
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_invalid_params_type() {
    let result = dispatch("shader.compile.wgsl", serde_json::json!("invalid"));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("must be array or object")
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_wgsl_array_params() {
    let params = serde_json::json!([{
        "wgsl_source": "@compute @workgroup_size(1) fn main() {}",
        "arch": "sm_70",
        "opt_level": 2,
        "fp64_software": true
    }]);
    let result = dispatch("shader.compile.wgsl", params);
    let val = result.expect("wgsl compile with array params should succeed");
    assert!(
        val.get("size")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
    );
}

#[cfg(unix)]
#[test]
fn test_make_response_success() {
    let id = serde_json::json!(42);
    let result = Ok(serde_json::json!({"foo": "bar"}));
    let resp = make_response(id, result);
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["id"], 42);
    assert_eq!(parsed["result"]["foo"], "bar");
    assert!(parsed.get("error").is_none());
}

#[cfg(unix)]
#[test]
fn test_make_response_error() {
    use super::error::IpcServiceError;
    let id = serde_json::json!("req-1");
    let result = Err(IpcServiceError::handler("something went wrong"));
    let resp = make_response(id, result);
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["id"], "req-1");
    assert_eq!(parsed["error"]["code"], -32000);
    assert!(
        parsed["error"]["message"]
            .as_str()
            .unwrap()
            .contains("something went wrong")
    );
    assert!(parsed.get("result").is_none());
}

#[cfg(unix)]
#[test]
fn test_make_response_dispatch_error() {
    use super::error::IpcServiceError;
    let id = serde_json::json!(42);
    let result = Err(IpcServiceError::dispatch("method not found: foo.bar"));
    let resp = make_response(id, result);
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["error"]["code"], -32601);
    assert!(
        parsed["error"]["message"]
            .as_str()
            .unwrap()
            .contains("method not found")
    );
}

#[cfg(unix)]
#[test]
fn test_make_response_internal_error() {
    use super::error::IpcServiceError;
    let id = serde_json::json!(1);
    let result = Err(IpcServiceError::internal("serialization failed"));
    let resp = make_response(id, result);
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["error"]["code"], -32603);
    assert!(
        parsed["error"]["message"]
            .as_str()
            .unwrap()
            .contains("serialization failed")
    );
}

#[cfg(unix)]
#[test]
fn test_make_response_null_id() {
    let result = Ok(serde_json::json!(true));
    let resp = make_response(serde_json::Value::Null, result);
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert!(parsed["id"].is_null());
    assert_eq!(parsed["result"], true);
}

#[cfg(unix)]
#[test]
fn test_dispatch_spirv_empty_array_params() {
    let result = dispatch("shader.compile.spirv", serde_json::json!([]));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("missing")
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_wgsl_multi_empty_array_params() {
    let result = dispatch("shader.compile.wgsl.multi", serde_json::json!([]));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("missing")
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_spirv_object_params() {
    let spirv = test_helpers::valid_spirv_minimal_compute();
    let params = serde_json::json!({
        "spirv_words": spirv,
        "arch": "sm_70",
        "opt_level": 2,
        "fp64_software": true
    });
    let result = dispatch("shader.compile.spirv", params);
    match &result {
        Ok(val) => assert!(
            val.get("size")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
        ),
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            assert!(msg.contains("implemented") || msg.contains("not"));
        }
    }
}

#[cfg(unix)]
#[test]
fn test_dispatch_wgsl_multi_array_params() {
    let params = serde_json::json!([{
        "wgsl_source": "@compute @workgroup_size(1) fn main() {}",
        "targets": [{ "card_index": 0, "arch": "sm_70" }],
        "opt_level": 2
    }]);
    let result = dispatch("shader.compile.wgsl.multi", params);
    let val = result.expect("wgsl.multi with array params should succeed");
    assert!(
        val.get("success_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            >= 1
    );
    assert!(
        !val.get("results")
            .and_then(serde_json::Value::as_array)
            .unwrap()
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn test_unix_socket_path_with_xdg() {
    let path = unix_socket_path_for_base(Some("/run/user/1234".into()));
    assert!(path.to_string_lossy().contains("/run/user/1234"));
    assert!(
        path.to_string_lossy()
            .contains(crate::config::ECOSYSTEM_NAMESPACE)
    );
    assert!(
        path.to_string_lossy().contains("coralreef") && path.to_string_lossy().contains(".sock"),
        "path should contain primal name and .sock: {}",
        path.to_string_lossy()
    );
}

#[cfg(unix)]
#[test]
fn test_unix_socket_path_fallback() {
    let path = unix_socket_path_for_base(None);
    assert!(
        path.to_string_lossy()
            .contains(crate::config::ECOSYSTEM_NAMESPACE)
    );
    assert!(
        path.to_string_lossy().contains("coralreef") && path.to_string_lossy().contains(".sock"),
        "path should contain primal name and .sock: {}",
        path.to_string_lossy()
    );
}

#[cfg(unix)]
#[test]
fn test_default_unix_socket_path_with_xdg() {
    let temp = tempfile::tempdir().unwrap();
    let xdg = temp.path().to_path_buf();
    // Test unix_socket_path_for_base directly to avoid unsafe env mutation
    let path = unix_socket_path_for_base(Some(xdg.clone()));
    assert!(
        path.to_string_lossy()
            .contains(xdg.to_string_lossy().as_ref()),
        "path should contain XDG_RUNTIME_DIR"
    );
    assert!(
        path.to_string_lossy().contains("coralreef") && path.to_string_lossy().contains(".sock"),
        "path should contain primal name and .sock: {}",
        path.to_string_lossy()
    );
}

#[cfg(unix)]
#[test]
fn test_default_unix_socket_path_structure() {
    let path = default_unix_socket_path();
    assert!(
        path.to_string_lossy()
            .contains(crate::config::ECOSYSTEM_NAMESPACE)
    );
    assert!(
        path.to_string_lossy()
            .ends_with(&crate::config::primal_socket_name()),
        "path should end with primal socket name: {}",
        path.to_string_lossy()
    );
}

// --- Coverage expansion: handler error paths ---

#[cfg(unix)]
#[test]
fn test_dispatch_wgsl_invalid_source_returns_handler_error() {
    let params = serde_json::json!({
        "wgsl_source": "THIS IS NOT VALID WGSL AT ALL!!!\n{{{",
        "arch": "sm_70"
    });
    let result = dispatch("shader.compile.wgsl", params);
    assert!(result.is_err(), "invalid WGSL should produce handler error");
    let err = result.unwrap_err();
    assert!(
        err.to_string().len() > 5,
        "error message should describe the failure: {err}"
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_spirv_invalid_params_returns_dispatch_error() {
    let params = serde_json::json!({"wrong": "shape"});
    let result = dispatch("shader.compile.spirv", params);
    assert!(
        result.is_err(),
        "invalid spirv params should produce dispatch error"
    );
    let err_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err_msg.contains("invalid") || err_msg.contains("missing"),
        "should describe params issue: {err_msg}"
    );
}

#[cfg(unix)]
#[test]
fn test_dispatch_wgsl_multi_invalid_params_returns_error() {
    let params = serde_json::json!({"completely": "wrong"});
    let result = dispatch("shader.compile.wgsl.multi", params);
    assert!(result.is_err(), "invalid multi params should produce error");
}

#[cfg(unix)]
#[test]
fn test_make_response_transport_error() {
    use super::error::IpcServiceError;
    let id = serde_json::json!(99);
    let result = Err(IpcServiceError::transport("connection reset"));
    let resp = make_response(id, result);
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(parsed["jsonrpc"], "2.0");
    assert_eq!(parsed["id"], 99);
    assert!(parsed["error"]["code"].is_number());
    assert!(
        parsed["error"]["message"]
            .as_str()
            .unwrap()
            .contains("connection reset")
    );
}
