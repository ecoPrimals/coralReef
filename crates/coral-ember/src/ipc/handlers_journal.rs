// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON-RPC handlers for journal query, stats, and append.

use std::io::Write;
use std::sync::Arc;

use crate::error::EmberIpcError;
use crate::journal::{Journal, JournalEntry, JournalFilter};

use super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};

pub(crate) fn query(
    stream: &mut impl Write,
    id: serde_json::Value,
    params: &serde_json::Value,
    journal: Option<&Arc<Journal>>,
) -> Result<(), EmberIpcError> {
    if let Some(j) = journal {
        let filter: JournalFilter = serde_json::from_value(params.clone()).unwrap_or_default();
        match j.query(&filter) {
            Ok(entries) => write_jsonrpc_ok(stream, id, serde_json::json!({"entries": entries}))
                .map_err(EmberIpcError::from),
            Err(e) => write_jsonrpc_error(stream, id, -32000, &format!("journal query: {e}"))
                .map_err(EmberIpcError::from),
        }
    } else {
        write_jsonrpc_error(stream, id, -32000, "journal not available")
            .map_err(EmberIpcError::from)
    }
}

pub(crate) fn stats(
    stream: &mut impl Write,
    id: serde_json::Value,
    params: &serde_json::Value,
    journal: Option<&Arc<Journal>>,
) -> Result<(), EmberIpcError> {
    if let Some(j) = journal {
        let bdf = params.get("bdf").and_then(|v| v.as_str());
        match j.stats(bdf) {
            Ok(stats) => {
                let stats_json = serde_json::to_value(&stats).unwrap_or_default();
                write_jsonrpc_ok(stream, id, stats_json).map_err(EmberIpcError::from)
            }
            Err(e) => write_jsonrpc_error(stream, id, -32000, &format!("journal stats: {e}"))
                .map_err(EmberIpcError::from),
        }
    } else {
        write_jsonrpc_error(stream, id, -32000, "journal not available")
            .map_err(EmberIpcError::from)
    }
}

pub(crate) fn append(
    stream: &mut impl Write,
    id: serde_json::Value,
    params: &serde_json::Value,
    journal: Option<&Arc<Journal>>,
) -> Result<(), EmberIpcError> {
    if let Some(j) = journal {
        match serde_json::from_value::<JournalEntry>(params.clone()) {
            Ok(entry) => match j.append(&entry) {
                Ok(()) => write_jsonrpc_ok(stream, id, serde_json::json!({"ok": true}))
                    .map_err(EmberIpcError::from),
                Err(e) => write_jsonrpc_error(stream, id, -32000, &format!("journal append: {e}"))
                    .map_err(EmberIpcError::from),
            },
            Err(e) => {
                write_jsonrpc_error(stream, id, -32602, &format!("invalid journal entry: {e}"))
                    .map_err(EmberIpcError::from)
            }
        }
    } else {
        write_jsonrpc_error(stream, id, -32000, "journal not available")
            .map_err(EmberIpcError::from)
    }
}
