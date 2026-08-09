// SPDX-License-Identifier: AGPL-3.0-or-later
//! Vertebrate self-audit: verify that the actual JSON-RPC dispatch surface
//! matches what `capability_registry.toml` declares.
//!
//! This test parses the registry at compile time and exercises every declared
//! method against the live `dispatch_jsonrpc` function, ensuring:
//!
//! 1. **No phantom methods** — every registry method is handled (not "method not found").
//! 2. **No silent health fallback** — methods return domain-specific responses,
//!    not a generic health stub (the bearDog P0-A pattern).
//! 3. **No undeclared methods** — every dispatch arm appears in the registry.

use std::collections::BTreeSet;

const REGISTRY_TOML: &str = include_str!("../../../../config/capability_registry.toml");

/// Parse fully-qualified method names from the registry's `[domains.*]` tables.
fn registry_methods() -> BTreeSet<String> {
    let doc: toml::Table =
        toml::from_str(REGISTRY_TOML).expect("capability_registry.toml is valid TOML");

    let domains = doc
        .get("domains")
        .and_then(toml::Value::as_table)
        .expect("registry has [domains] table");

    let mut methods = BTreeSet::new();
    for (domain, table) in domains {
        let domain_table = table.as_table().expect("domain entry is a table");
        let method_arr = domain_table
            .get("methods")
            .and_then(toml::Value::as_array)
            .expect("domain has methods array");
        for m in method_arr {
            let name = m.as_str().expect("method name is a string");
            methods.insert(format!("{domain}.{name}"));
        }

        if let Some(aliases) = domain_table.get("aliases").and_then(toml::Value::as_table) {
            for alias in aliases.keys() {
                methods.insert(alias.clone());
            }
        }
    }

    methods
}

#[test]
fn every_registry_method_is_dispatched() {
    let declared = registry_methods();
    assert!(
        declared.len() >= 18,
        "registry should declare at least 18 methods, found {}",
        declared.len()
    );

    let mut missing = Vec::new();
    for method in &declared {
        let result = super::newline_jsonrpc::dispatch_jsonrpc(method, serde_json::json!({}));
        match &result {
            Err(e) if e.to_string().contains("method not found") => {
                missing.push(method.clone());
            }
            _ => {}
        }
    }

    assert!(
        missing.is_empty(),
        "registry declares methods not handled by dispatch: {missing:?}"
    );
}

#[test]
fn every_dispatched_method_is_in_registry() {
    let declared = registry_methods();
    let served: BTreeSet<String> = crate::config::SERVED_METHODS
        .iter()
        .map(|&s| s.to_owned())
        .collect();

    let undeclared: Vec<_> = served.difference(&declared).collect();
    assert!(
        undeclared.is_empty(),
        "dispatch serves methods not declared in registry: {undeclared:?}"
    );
}

#[test]
fn no_silent_health_fallback_on_unknown_method() {
    let result =
        super::newline_jsonrpc::dispatch_jsonrpc("nonexistent.phantom.xyz", serde_json::json!({}));
    assert!(
        result.is_err(),
        "unknown method must return Err, not a silent health response"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("method not found"),
        "error should say 'method not found': {msg}"
    );
}

#[test]
fn dispatch_never_returns_health_stub_for_declared_methods() {
    let declared = registry_methods();
    let health_stub_keys = ["primal", "status", "alive", "version"];

    for method in &declared {
        let result = super::newline_jsonrpc::dispatch_jsonrpc(method, serde_json::json!({}));
        match result {
            Ok(val) => {
                let is_stub =
                    val.is_object() && health_stub_keys.iter().all(|k| val.get(k).is_some());
                assert!(
                    !is_stub,
                    "method {method} returned what looks like a health stub: {val}"
                );
            }
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    !msg.contains("method not found"),
                    "declared method {method} returned 'method not found'"
                );
            }
        }
    }
}

#[test]
fn registry_version_matches_crate() {
    let doc: toml::Table =
        toml::from_str(REGISTRY_TOML).expect("capability_registry.toml is valid TOML");

    let registry_version = doc
        .get("primal")
        .and_then(toml::Value::as_table)
        .and_then(|t| t.get("version"))
        .and_then(toml::Value::as_str)
        .expect("registry has primal.version");

    assert_eq!(
        registry_version,
        crate::config::PRIMAL_VERSION,
        "registry version must match crate version"
    );
}
