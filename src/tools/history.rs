// SPDX-License-Identifier: GPL-3.0-or-later
//! Undo/redo tools.
//!
//! The engine's undo stack is global to the process (by design), so undo/redo
//! apply to the most recent edit regardless of which session made it.

use quercus_engine_rs::node;
use serde_json::{Value, json};

use super::{ToolDef, ToolReply};
use crate::state::ServerState;

/// The tools in this module.
pub fn tools() -> Vec<ToolDef> {
	vec![
		ToolDef {
			name: "undo",
			title: "Undo",
			description: "Undo the most recent undoable edit.",
			read_only: false,
			destructive: false,
			input_schema: || json!({"type": "object", "properties": {}}),
			handler: undo,
		},
		ToolDef {
			name: "redo",
			title: "Redo",
			description: "Redo the most recently undone edit.",
			read_only: false,
			destructive: false,
			input_schema: || json!({"type": "object", "properties": {}}),
			handler: redo,
		},
		ToolDef {
			name: "history",
			title: "Undo history",
			description: "Report whether undo/redo are available and the stack position.",
			read_only: true,
			destructive: false,
			input_schema: || json!({"type": "object", "properties": {}}),
			handler: history,
		},
	]
}

fn undo(_state: &mut ServerState, _arguments: Value) -> Result<ToolReply, String> {
	let undone = node::undo();
	Ok(ToolReply::json(json!({
		"undone": undone,
		"can_undo": node::undo_can_undo(),
		"can_redo": node::undo_can_redo(),
	})))
}

fn redo(_state: &mut ServerState, _arguments: Value) -> Result<ToolReply, String> {
	let redone = node::redo();
	Ok(ToolReply::json(json!({
		"redone": redone,
		"can_undo": node::undo_can_undo(),
		"can_redo": node::undo_can_redo(),
	})))
}

fn history(_state: &mut ServerState, _arguments: Value) -> Result<ToolReply, String> {
	Ok(ToolReply::json(json!({
		"can_undo": node::undo_can_undo(),
		"can_redo": node::undo_can_redo(),
		"count": node::undo_count(),
		"index": node::undo_index(),
	})))
}
