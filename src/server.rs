//! MCP server built on the official `rmcp` SDK.
//!
//! The MCP transport, lifecycle and JSON-RPC plumbing come from `rmcp`; this
//! module only maps the engine-facing tools/resources/prompts onto its
//! `ServerHandler` trait. Tools are declared once in [`crate::tools`] with a
//! JSON Schema and a handler, so adding a tool never touches protocol code.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use rmcp::model::{
	CallToolRequestParams, CallToolResponse, GetPromptRequestParams, GetPromptResponse,
	GetPromptResult, Implementation, JsonObject, ListPromptsResult, ListResourcesResult,
	ListToolsResult, PaginatedRequestParams, Prompt, PromptArgument, PromptMessage,
	ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
	ResourceContents, Role, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{Value, json};

use crate::convert;
use crate::state::ServerState;
use crate::tools;

/// Protocol revisions this server implements (newest first).
const SUPPORTED_VERSIONS: [rmcp::model::ProtocolVersion; 3] = [
	rmcp::model::ProtocolVersion::V_2025_06_18,
	rmcp::model::ProtocolVersion::V_2025_03_26,
	rmcp::model::ProtocolVersion::V_2024_11_05,
];

const INSTRUCTIONS: &str = "\
This server edits video through the Quercus nonlinear editing engine. \
Times are seconds; node ids are opaque decimal strings that come from tool \
results (never invent one). A typical session: project_new -> media_import -> \
clip_place -> timeline_get -> preview_frame -> export_start -> task_wait. \
Preview frames are returned as PNG images, so check your edit visually before \
exporting. Export runs asynchronously: keep the task id and poll it.";

/// The MCP server state shared by all handlers.
pub struct QuercusServer {
	state: Arc<Mutex<ServerState>>,
}

impl QuercusServer {
	/// Wraps the server state shared with the tool implementations.
	pub fn new(state: ServerState) -> Self {
		Self {
			state: Arc::new(Mutex::new(state)),
		}
	}

	fn lock(&self) -> std::sync::MutexGuard<'_, ServerState> {
		self.state.lock().unwrap_or_else(|error| error.into_inner())
	}
}

impl ServerHandler for QuercusServer {
	fn get_info(&self) -> ServerConfig {
		let mut implementation = Implementation::new("oak-mcp", env!("CARGO_PKG_VERSION"));
		implementation.title = Some("Oak MCP (Quercus engine)".into());
		implementation.description = Some("MCP server for the Quercus video-editing engine".into());
		ServerConfig::new(
			ServerCapabilities::builder()
				.enable_tools()
				.enable_resources()
				.enable_prompts()
				.build(),
		)
		.with_server_info(implementation)
		.with_instructions(INSTRUCTIONS)
	}

	fn supported_protocol_versions(&self) -> Cow<'static, [rmcp::model::ProtocolVersion]> {
		Cow::Borrowed(&SUPPORTED_VERSIONS)
	}

	async fn list_tools(
		&self,
		_request: Option<PaginatedRequestParams>,
		_context: RequestContext<RoleServer>,
	) -> Result<ListToolsResult, McpError> {
		let tools = tools::all()
			.iter()
			.map(|definition| definition.tool())
			.collect();
		Ok(ListToolsResult::with_all_items(tools))
	}

	async fn call_tool(
		&self,
		request: CallToolRequestParams,
		_context: RequestContext<RoleServer>,
	) -> Result<CallToolResponse, McpError> {
		let arguments = request
			.arguments
			.map(Value::Object)
			.unwrap_or_else(|| Value::Object(JsonObject::new()));
		let mut state = self.lock();
		let reply = tools::call(&mut state, request.name.as_ref(), arguments);
		state.prune_finished_tasks();
		match reply {
			Some(reply) => Ok(reply.into_call_tool_result().into()),
			None => Err(McpError::invalid_params(
				format!("unknown tool `{}`", request.name),
				None,
			)),
		}
	}

	async fn list_resources(
		&self,
		_request: Option<PaginatedRequestParams>,
		_context: RequestContext<RoleServer>,
	) -> Result<ListResourcesResult, McpError> {
		let state = self.lock();
		let mut resources = vec![
			Resource::new("quercus://server/info", "Server info")
				.with_description("Server version, protocol, and open sessions")
				.with_mime_type("application/json"),
		];
		for entry in state.session_entries() {
			resources.push(
				Resource::new(
					format!("quercus://session/{}/timeline", entry.id),
					format!("{} timeline", entry.label),
				)
				.with_description("Sequences, tracks, and clips")
				.with_mime_type("application/json"),
			);
			resources.push(
				Resource::new(
					format!("quercus://session/{}/project", entry.id),
					format!("{} project", entry.label),
				)
				.with_description("Full project snapshot")
				.with_mime_type("application/json"),
			);
		}
		Ok(ListResourcesResult::with_all_items(resources))
	}

	async fn read_resource(
		&self,
		request: ReadResourceRequestParams,
		_context: RequestContext<RoleServer>,
	) -> Result<ReadResourceResponse, McpError> {
		let state = self.lock();
		let body = resource_body(&state, &request.uri)?;
		let text = serde_json::to_string(&body)
			.map_err(|error| McpError::internal_error(error.to_string(), None))?;
		let contents = ResourceContents::text(text, request.uri).with_mime_type("application/json");
		Ok(ReadResourceResult::new(vec![contents]).into())
	}

	async fn list_prompts(
		&self,
		_request: Option<PaginatedRequestParams>,
		_context: RequestContext<RoleServer>,
	) -> Result<ListPromptsResult, McpError> {
		Ok(ListPromptsResult::with_all_items(prompts()))
	}

	async fn get_prompt(
		&self,
		request: GetPromptRequestParams,
		_context: RequestContext<RoleServer>,
	) -> Result<GetPromptResponse, McpError> {
		let text = prompt_text(&request)?;
		let message = PromptMessage::new_text(Role::User, text);
		Ok(GetPromptResult::new(vec![message])
			.with_description(format!("Prompt `{}`", request.name))
			.into())
	}
}

fn resource_body(state: &ServerState, uri: &str) -> Result<Value, McpError> {
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
		.ok_or_else(|| McpError::invalid_params(format!("resource not found: {uri}"), None))?;
	let (session_id, kind) = rest
		.split_once('/')
		.ok_or_else(|| McpError::invalid_params(format!("resource not found: {uri}"), None))?;
	let entry = state
		.entry(session_id)
		.map_err(|error| McpError::invalid_params(error, None))?;
	let snapshot = entry.session.snapshot();
	match kind {
		"timeline" => Ok(json!({
			"session": session_id,
			"revision": snapshot.revision,
			"current_sequence": snapshot.current_sequence.map(convert::id_json),
			"sequences": snapshot.sequences.iter().map(convert::sequence_json).collect::<Vec<_>>(),
		})),
		"project" => Ok(convert::project_json(&snapshot)),
		_ => Err(McpError::invalid_params(
			format!("resource not found: {uri}"),
			None,
		)),
	}
}

fn prompts() -> Vec<Prompt> {
	vec![
		Prompt::new(
			"rough_cut",
			Some("Plan and assemble a rough cut from one media file"),
			Some(vec![
				{
					let mut argument = PromptArgument::new("media");
					argument.description = Some("Path to the source media file".into());
					argument.required = Some(true);
					argument
				},
				{
					let mut argument = PromptArgument::new("brief");
					argument.description = Some("Editing brief or target duration".into());
					argument.required = Some(false);
					argument
				},
			]),
		),
		Prompt::new(
			"deliver_export",
			Some("Export the current timeline and verify the result"),
			Some(vec![{
				let mut argument = PromptArgument::new("filename");
				argument.description = Some("Output file path".into());
				argument.required = Some(true);
				argument
			}]),
		),
	]
}

fn prompt_text(request: &GetPromptRequestParams) -> Result<String, McpError> {
	let arguments = request.arguments.clone().unwrap_or_default();
	let text = match request.name.as_ref() {
		"rough_cut" => {
			let media = arguments
				.get("media")
				.and_then(Value::as_str)
				.ok_or_else(|| McpError::invalid_params("rough_cut requires `media`", None))?;
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
				.ok_or_else(|| {
					McpError::invalid_params("deliver_export requires `filename`", None)
				})?;
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
			return Err(McpError::invalid_params(
				format!("unknown prompt `{other}`"),
				None,
			));
		}
	};
	Ok(text)
}
