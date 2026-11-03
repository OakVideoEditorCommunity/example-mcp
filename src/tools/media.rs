//! Media import and project-bin tools.

use quercus_engine_rs::edit::EditCommand;
use serde_json::{Value, json};

use super::{Args, ToolDef, ToolReply, resolve_session};
use crate::convert::{self, id_json};
use crate::state::ServerState;

/// The tools in this module.
pub fn tools() -> Vec<ToolDef> {
	vec![
		ToolDef {
			name: "media_import",
			title: "Import media",
			description: "Import a video/audio/image file into the project bin. Returns the footage \
				node id and probed stream metadata (duration, resolution, frame rate, audio).",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"path": {"type": "string", "description": "Absolute path to the media file"}
					},
					"required": ["path"]
				})
			},
			handler: media_import,
		},
		ToolDef {
			name: "media_list",
			title: "List media",
			description: "List all footage imported into the project bin.",
			read_only: true,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"}
					}
				})
			},
			handler: media_list,
		},
	]
}

fn media_import(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let path = args.str("path")?;
	let path = std::fs::canonicalize(&path)
		.map_err(|error| format!("media file is not readable: `{path}`: {error}"))?
		.to_string_lossy()
		.into_owned();
	let entry = state.entry(&session_id)?;
	let result = entry
		.session
		.execute(EditCommand::ImportFootage { path: path.clone() })
		.map_err(|error| format!("import failed: {error}"))?;
	let footage = result
		.primary
		.ok_or_else(|| "engine did not report the imported footage".to_string())?;
	let snapshot = entry.session.snapshot();
	let metadata = snapshot
		.footages
		.iter()
		.find(|item| item.id == footage)
		.map(convert::footage_json)
		.unwrap_or_else(|| json!({ "id": id_json(footage) }));
	Ok(ToolReply::json(json!({
		"session": session_id,
		"footage": metadata,
		"revision": result.revision,
	})))
}

fn media_list(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let entry = state.entry(&session_id)?;
	let snapshot = entry.session.snapshot();
	Ok(ToolReply::json(json!({
		"session": session_id,
		"count": snapshot.footages.len(),
		"footages": snapshot.footages.iter().map(convert::footage_json).collect::<Vec<_>>(),
	})))
}
