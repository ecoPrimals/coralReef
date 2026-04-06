// SPDX-License-Identifier: AGPL-3.0-or-later
#![no_main]

use std::panic::{AssertUnwindSafe, catch_unwind};

use coralreef_core::ipc::dispatch;
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

const METHODS: &[&str] = &[
    "shader.compile.status",
    "shader.compile.capabilities",
    "shader.compile.wgsl",
    "shader.compile.spirv",
    "shader.compile.wgsl.multi",
    "health.check",
    "health.liveness",
    "health.readiness",
    "method.not.registered",
];

fn dispatch_from_value(value: &Value, seed: usize) {
    if let Some(obj) = value.as_object() {
        if let Some(Value::String(method)) = obj.get("method") {
            let params = obj
                .get("params")
                .cloned()
                .unwrap_or(Value::Null);
            let run = || {
                let _ = dispatch(method.as_str(), params);
            };
            let _ = catch_unwind(AssertUnwindSafe(run));
            return;
        }
    }

    let method = METHODS[seed % METHODS.len()];
    let run = || {
        let _ = dispatch(method, value.clone());
    };
    let _ = catch_unwind(AssertUnwindSafe(run));
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let seed = data.len();

    match serde_json::from_str::<Value>(text.as_ref()) {
        Ok(value) => dispatch_from_value(&value, seed),
        Err(_) => {
            let run = || {
                let _ = dispatch("shader.compile.status", Value::Null);
            };
            let _ = catch_unwind(AssertUnwindSafe(run));
        }
    }
});
