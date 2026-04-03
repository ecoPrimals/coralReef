// SPDX-License-Identifier: AGPL-3.0-only
use super::*;

use coralreef_core::capability::{Capability, SelfDescription, Transport};

static DISCOVERY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn discovery_dir_returns_path() {
    let dir = discovery_dir().unwrap();
    assert!(dir.ends_with(crate::config::ECOSYSTEM_NAMESPACE));
}

#[test]
fn write_and_remove_discovery_file() {
    let _lock = DISCOVERY_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let desc = SelfDescription {
        provides: vec![Capability {
            id: "test.provide".into(),
            version: "1.0".into(),
            metadata: serde_json::Value::Null,
        }],
        requires: vec![],
        transports: vec![
            Transport {
                protocol: "jsonrpc".into(),
                address: "127.0.0.1:12345".into(),
            },
            Transport {
                protocol: "tarpc+tcp".into(),
                address: "127.0.0.1:12346".into(),
            },
        ],
    };

    write_discovery_file(&desc).unwrap();
    let dir = discovery_dir().unwrap();
    let path = dir.join(format!("{}.json", env!("CARGO_PKG_NAME")));
    assert!(path.exists(), "discovery file should exist after write");

    let contents = std::fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(json["primal"], env!("CARGO_PKG_NAME"));
    assert!(json.get("version").is_some(), "Phase 10 requires version");
    assert!(json.get("pid").is_some(), "Phase 10 requires pid");
    assert!(json.get("provides").is_some(), "Phase 10 requires provides");
    assert_eq!(json["transports"]["jsonrpc"]["bind"], "127.0.0.1:12345");
    assert_eq!(json["transports"]["jsonrpc_line"]["bind"], "");
    assert_eq!(json["transports"]["tarpc"]["bind"], "127.0.0.1:12346");

    remove_discovery_file();
    assert!(!path.exists(), "discovery file should be removed");

    let desc_empty = SelfDescription {
        provides: vec![],
        requires: vec![],
        transports: vec![],
    };
    write_discovery_file(&desc_empty).unwrap();
    let contents_empty = std::fs::read_to_string(&path).unwrap();
    let json_empty: serde_json::Value = serde_json::from_str(&contents_empty).unwrap();
    assert_eq!(json_empty["transports"]["jsonrpc"]["bind"], "");
    assert_eq!(json_empty["transports"]["jsonrpc_line"]["bind"], "");
    assert_eq!(json_empty["transports"]["tarpc"]["bind"], "");
    remove_discovery_file();
}

#[test]
fn remove_discovery_file_idempotent() {
    let _lock = DISCOVERY_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    remove_discovery_file();
    remove_discovery_file();
}

#[test]
fn discovery_dir_leaf_is_ecosystem_namespace() {
    let dir = discovery_dir().expect("discovery_dir");
    assert_eq!(
        dir.file_name().and_then(|n| n.to_str()),
        Some(crate::config::ECOSYSTEM_NAMESPACE),
        "expected .../<namespace> with final segment biomeos"
    );
}

#[test]
fn write_discovery_file_includes_jsonrpc_line_bind_when_transport_present() {
    const JSONRPC_LINE_ADDR: &str = "127.0.0.1:54321";
    let _lock = DISCOVERY_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let desc = SelfDescription {
        provides: vec![Capability {
            id: "test.provide".into(),
            version: "1.0".into(),
            metadata: serde_json::Value::Null,
        }],
        requires: vec![],
        transports: vec![
            Transport {
                protocol: "jsonrpc".into(),
                address: "127.0.0.1:11111".into(),
            },
            Transport {
                protocol: "jsonrpc-line".into(),
                address: JSONRPC_LINE_ADDR.into(),
            },
            Transport {
                protocol: "tarpc+tcp".into(),
                address: "127.0.0.1:22222".into(),
            },
        ],
    };

    write_discovery_file(&desc).unwrap();
    let dir = discovery_dir().unwrap();
    let path = dir.join(format!("{}.json", env!("CARGO_PKG_NAME")));
    let contents = std::fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(
        json["transports"]["jsonrpc_line"]["bind"].as_str().unwrap(),
        JSONRPC_LINE_ADDR
    );
    remove_discovery_file();
}
