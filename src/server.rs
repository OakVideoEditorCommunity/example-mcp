//! MCP protocol handling: lifecycle, tools, resources, prompts, logging.

use serde_json::{Value, json};

use crate::convert;
use crate::rpc::{self, Request, RpcError};
use crate::state::ServerState;
use crate::tools;

/// Protocol revisions this server implements, newest first.
const SUPPORTED_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "\
This server edits video through the Quercus nonlinear editing engine. \
Times are seconds; node ids are opaque decimal strings that come from tool \
results (never invent one). A typical session: project_new -> media_import -> \
clip_place -> timeline_get -> preview_frame -> export_start -> task_wait. \
The engine keeps undo history per process; use the undo/redo tools. \
Preview frames are returned as PNG images, so check your edit visually before \
exporting. Export runs asynchronously: keep the task id and poll it.";

/// Handles one decoded JSON-RPC message (or a batch) and returns the response,
/// or `None` for notifications.
pub fn handle(state: &mut ServerState, message: Value) -> Option<Value> {
	if let Some(batch) = message.as_array() {
		if batch.is_empty() {
			return Some(rpc::error_response(
				Value::Null,
				&RpcError::invalid_request("empty JSON-RPC batch"),
			));
		}
		let responses: Vec<Value> = batch
			.iter()
			.filter_map(|item| handle_single(state, item.clone()))
			.collect();
		if responses.is_empty() {
			None
		} else {
			Some(Value::Array(responses))
		}
	} else {
		handle_single(state, message)
	}
}

fn handle_single(state: &mut ServerState, message: Value) -> Option<Value> {
	let id = message.get("id").cloned().unwrap_or(Value::Null);
	let request = match Request::from_value(&message) {
		Ok(request) => request,
		Err(error) => return Some(rpc::error_response(id, &error)),
	};
	let is_notification = request.id.is_none();
	let outcome = dispatch(state, &request);
	if is_notification {
		return None;
	}
	let id = request.id.clone().unwrap_or(Value::Null);
	match outcome {
		Ok(Some(result)) => Some(rpc::response(id, result)),
		Ok(None) => None,
		Err(error) => Some(rpc::error_response(id, &error)),
	}
}

fn dispatch(state: &mut ServerState, request: &Request) -> Result<Option<Value>, RpcError> {
	match request.method.as_str() {
		"initialize" => initialize(state, request).map(Some),
		"ping" => Ok(Some(json!({}))),
		"tools/list" => Ok(Some(json!({
			"tools": tools::all().iter().map(|tool| tool.descriptor()).collect::<Vec<_>>(),
		}))),
		"tools/call" => call_tool(state, request).map(Some),
		"resources/list" => Ok(Some(list_resources(state))),
		"resources/read" => read_resource(state, request).map(Some),
		"resources/templates/list" => Ok(Some(json!({
			"resourceTemplates": [
				{
					"uriTemplate": "quercus://session/{session}/timeline",
					"name": "Session timeline",
					"description": "Sequences, tracks, and clips of one session",
					"mimeType": "application/json"
				},
				{
					"uriTemplate": "quercus://session/{session}/project",
					"name": "Session project",
					"description": "The full project snapshot of one session",
					"mimeType": "application/json"
				}
			]
		}))),
		"prompts/list" => Ok(Some(list_prompts())),
		"prompts/get" => get_prompt(request).map(Some),
		"logging/setLevel" => set_log_level(state, request).map(Some),
		"notifications/initialized"
		| "initialized"
		| "notifications/cancelled"
		| "notifications/roots/list_changed"
		| "notifications/progress" => Ok(None),
		other => Err(RpcError::method_not_found(other)),
	}
}

fn initialize(state: &mut ServerState, request: &Request) -> Result<Value, RpcError> {
	let params = request.params_or_empty();
	let requested = params
		.get("protocolVersion")
		.and_then(Value::as_str)
		.unwrap_or(SUPPORTED_VERSIONS[0]);
	let negotiated = if SUPPORTED_VERSIONS.contains(&requested) {
		requested.to_string()
	} else {
		SUPPORTED_VERSIONS[0].to_string()
	};
	state.protocol_version = negotiated.clone();
	Ok(json!({
		"protocolVersion": negotiated,
		"capabilities": {
			"tools": { "listChanged": false },
			"resources": { "subscribe": false, "listChanged": false },
			"prompts": { "listChanged": false },
			"logging": {},
		},
		"serverInfo": {
			"name": "oak-mcp",
			"title": "Oak MCP (Quercus engine)",
			"version": env!("CARGO_PKG_VERSION"),
		},
		"instructions": INSTRUCTIONS,
	}))
}

fn call_tool(state: &mut ServerState, request: &Request) -> Result<Value, RpcError> {
	let params = request
		.params
		.as_ref()
		.ok_or_else(|| RpcError::invalid_params("tools/call requires params"))?;
	let name = params
		.get("name")
		.and_then(Value::as_str)
		.ok_or_else(|| RpcError::invalid_params("missing tool `name`"))?;
	let arguments = params
		.get("arguments")
		.cloned()
		.unwrap_or_else(|| json!({}));
	if !arguments.is_object() {
		return Err(RpcError::invalid_params("`arguments` must be an object"));
	}
	let reply = tools::call(state, name, arguments)
		.ok_or_else(|| RpcError::invalid_params(format!("unknown tool `{name}`")))?;
	state.prune_finished_tasks();
	Ok(reply.into_value())
}

fn list_resources(state: &ServerState) -> Value {
	let mut resources = vec![json!({
		"uri": "quercus://server/info",
		"name": "Server info",
		"description": "Server version, negotiated protocol, and open sessions",
		"mimeType": "application/json",
	})];
	for entry in state.session_entries() {
		resources.push(json!({
			"uri": format!("quercus://session/{}/timeline", entry.id),
			"name": format!("{} timeline", entry.label),
			"description": "Sequences, tracks, and clips",
			"mimeType": "application/json",
		}));
		resources.push(json!({
			"uri": format!("quercus://session/{}/project", entry.id),
			"name": format!("{} project", entry.label),
			"description": "Full project snapshot",
			"mimeType": "application/json",
		}));
	}
	json!({ "resources": resources })
}

fn read_resource(state: &mut ServerState, request: &Request) -> Result<Value, RpcError> {
	let params = request
		.params
		.as_ref()
		.ok_or_else(|| RpcError::invalid_params("resources/read requires params"))?;
	let uri = params
		.get("uri")
		.and_then(Value::as_str)
		.ok_or_else(|| RpcError::invalid_params("missing `uri`"))?;
	let body = resource_body(state, uri)?;
	Ok(json!({
		"contents": [{
			"uri": uri,
			"mimeType": "application/json",
			"text": serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string()),
		}]
	}))
}

fn resource_body(state: &ServerState, uri: &str) -> Result<Value, RpcError> {
	if uri == "quercus://server/info" {
		let sessions: Vec<Value> = state
			.session_entries()
			.map(|entry| {
				json!({
					"session": entry.id,
					"label": entry.label,
					"path": entry.path,
				})
			})
			.collect();
		return Ok(json!({
			"name": "oak-mcp",
			"version": env!("CARGO_PKG_VERSION"),
			"protocol": state.protocol_version,
			"active_session": state.active_session_id(),
			"uptime_seconds": state.started.elapsed().as_secs(),
			"sessions": sessions,
			"tools": tools::all().len(),
		}));
	}
	let rest = uri
		.strip_prefix("quercus://session/")
		.ok_or_else(|| RpcError::new(-32002, format!("resource not found: {uri}")))?;
	let (session_id, kind) = rest
		.split_once('/')
		.ok_or_else(|| RpcError::new(-32002, format!("resource not found: {uri}")))?;
	let entry = state
		.entry(session_id)
		.map_err(|error| RpcError::new(-32002, error))?;
	let snapshot = entry.session.snapshot();
	match kind {
		"timeline" => Ok(json!({
			"session": session_id,
			"revision": snapshot.revision,
			"current_sequence": snapshot.current_sequence.map(convert::id_json),
			"sequences": snapshot.sequences.iter().map(convert::sequence_json).collect::<Vec<_>>(),
		})),
		"project" => Ok(convert::project_json(&snapshot)),
		_ => Err(RpcError::new(-32002, format!("resource not found: {uri}"))),
	}
}

fn list_prompts() -> Value {
	json!({
		"prompts": [
			{
				"name": "rough_cut",
				"title": "Rough cut",
				"description": "Plan and assemble a rough cut from one media file",
				"arguments": [
					{"name": "media", "description": "Path to the source media file", "required": true},
					{"name": "brief", "description": "Editing brief or target duration", "required": false}
				]
			},
			{
				"name": "deliver_export",
				"title": "Deliver an export",
				"description": "Export the current timeline and verify the result",
				"arguments": [
					{"name": "filename", "description": "Output file path", "required": true}
				]
			}
		]
	})
}

fn get_prompt(request: &Request) -> Result<Value, RpcError> {
	let params = request
		.params
		.as_ref()
		.ok_or_else(|| RpcError::invalid_params("prompts/get requires params"))?;
	let name = params
		.get("name")
		.and_then(Value::as_str)
		.ok_or_else(|| RpcError::invalid_params("missing prompt `name`"))?;
	let arguments = params
		.get("arguments")
		.cloned()
		.unwrap_or_else(|| json!({}));
	let text = match name {
		"rough_cut" => {
			let media = arguments
				.get("media")
				.and_then(Value::as_str)
				.ok_or_else(|| RpcError::invalid_params("rough_cut requires `media`"))?;
			let brief = arguments
				.get("brief")
				.and_then(Value::as_str)
				.unwrap_or("about 30 seconds");
			format!(
				"Assemble a rough cut from {media} ({brief}).\n\n\
				1. Call project_new (1920x1080, 30 fps).\n\
				2. Call media_import with the absolute path to {media}, then read the returned \
				   duration and frame rate.\n\
				3. Choose 3-6 segments totalling the target length. Prefer visually distinct \
				   moments; keep each segment a few seconds.\n\
				4. For each segment call clip_place with start/end on the timeline and media_in for \
				   the source in-point. Snapping to whole seconds is fine for a rough cut.\n\
				5. Call timeline_get to confirm there are no gaps or overlaps you did not intend.\n\
				6. Render one preview_frame at each segment boundary and check the results.\n\
				7. Report the edit: clip ranges, total duration, and any remaining work."
			)
		}
		"deliver_export" => {
			let filename = arguments
				.get("filename")
				.and_then(Value::as_str)
				.ok_or_else(|| RpcError::invalid_params("deliver_export requires `filename`"))?;
			format!(
				"Export the current timeline to {filename}.\n\n\
				1. Call timeline_get and confirm the sequence is not empty.\n\
				2. Call export_start with filename {filename} and format mp4.\n\
				3. Call task_wait with the returned task id and a generous timeout.\n\
				4. If the task failed, read the error, fix the cause, and start again.\n\
				5. Report the output path, resolution, duration, and task state."
			)
		}
		other => {
			return Err(RpcError::invalid_params(format!(
				"unknown prompt `{other}`"
			)));
		}
	};
	Ok(json!({
		"description": format!("Prompt `{name}`"),
		"messages": [{
			"role": "user",
			"content": { "type": "text", "text": text },
		}]
	}))
}

fn set_log_level(state: &mut ServerState, request: &Request) -> Result<Value, RpcError> {
	let params = request
		.params
		.as_ref()
		.ok_or_else(|| RpcError::invalid_params("logging/setLevel requires params"))?;
	let level = params
		.get("level")
		.and_then(Value::as_str)
		.ok_or_else(|| RpcError::invalid_params("missing `level`"))?;
	state.log_level = level.to_string();
	Ok(json!({}))
}
