//! MCP tool registry, argument parsing, and reply shaping.
//!
//! Tools are plain functions grouped by domain. Each tool declares its JSON
//! schema and a handler; the server turns a handler result into an MCP
//! `tools/call` result. Tool failures become `isError: true` content (per the
//! MCP spec) instead of JSON-RPC errors, so the model can read and recover.

pub mod clips;
pub mod effects;
pub mod history;
pub mod media;
pub mod project;
pub mod render;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, OnceLock};

use quercus_engine_rs::node::NodeId;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject, Tool, ToolAnnotations};
use serde_json::Value;

use crate::convert;
use crate::state::ServerState;

/// A tool handler: typed JSON arguments in, MCP content out.
pub type Handler = fn(&mut ServerState, Value) -> Result<ToolReply, String>;

/// One registered tool.
pub struct ToolDef {
	/// MCP tool name.
	pub name: &'static str,
	/// Human-readable title.
	pub title: &'static str,
	/// Description shown to the model.
	pub description: &'static str,
	/// Whether the tool only reads state.
	pub read_only: bool,
	/// Whether the tool may delete or overwrite user data.
	pub destructive: bool,
	/// Lazily built JSON Schema for the arguments.
	pub input_schema: fn() -> Value,
	/// The implementation.
	pub handler: Handler,
}

impl ToolDef {
	/// The `rmcp` tool descriptor for this tool.
	pub fn tool(&self) -> Tool {
		let schema_value = (self.input_schema)();
		let schema: JsonObject = schema_value.as_object().cloned().unwrap_or_default();
		let annotations = ToolAnnotations::new()
			.read_only(self.read_only)
			.destructive(self.destructive)
			.idempotent(self.read_only)
			.open_world(false);
		Tool::new(self.name, self.description, Arc::new(schema))
			.with_title(self.title)
			.with_annotations(annotations)
	}
}

/// Every tool the server exposes, in a stable order.
pub fn all() -> &'static [ToolDef] {
	static TOOLS: OnceLock<Vec<ToolDef>> = OnceLock::new();
	TOOLS.get_or_init(|| {
		let mut tools = Vec::new();
		tools.extend(project::tools());
		tools.extend(media::tools());
		tools.extend(clips::tools());
		tools.extend(effects::tools());
		tools.extend(render::tools());
		tools.extend(history::tools());
		tools
	})
}

/// Finds a tool by name.
pub fn find(name: &str) -> Option<&'static ToolDef> {
	all().iter().find(|tool| tool.name == name)
}

/// Runs a tool, converting handler errors and panics into `isError` content.
///
/// Returns `None` when the tool name is unknown; the protocol layer then sends
/// a JSON-RPC `-32602` error as the MCP spec requires.
pub fn call(state: &mut ServerState, name: &str, arguments: Value) -> Option<ToolReply> {
	let tool = find(name)?;
	let outcome = catch_unwind(AssertUnwindSafe(|| (tool.handler)(state, arguments)));
	Some(match outcome {
		Ok(Ok(reply)) => reply,
		Ok(Err(message)) => ToolReply::error(message),
		Err(payload) => {
			let detail = payload
				.downcast_ref::<&str>()
				.map(|text| (*text).to_string())
				.or_else(|| payload.downcast_ref::<String>().cloned())
				.unwrap_or_else(|| "unknown panic payload".to_string());
			ToolReply::error(format!("internal error: tool `{name}` panicked: {detail}"))
		}
	})
}

/// A tool result: MCP content blocks plus optional structured content.
pub struct ToolReply {
	/// MCP content blocks (text and/or images).
	pub content: Vec<ContentBlock>,
	/// Machine-readable result, when the tool returns structured data.
	pub structured: Option<Value>,
	/// Whether this is a tool execution error.
	pub is_error: bool,
}

impl ToolReply {
	/// A JSON result: compact JSON as text plus `structuredContent`.
	pub fn json(value: Value) -> Self {
		let text = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
		Self {
			content: vec![ContentBlock::text(text)],
			structured: Some(value),
			is_error: false,
		}
	}

	/// A tool error.
	pub fn error(message: impl Into<String>) -> Self {
		Self {
			content: vec![ContentBlock::text(message.into())],
			structured: None,
			is_error: true,
		}
	}

	/// A PNG image plus a JSON metadata block.
	pub fn image(png: Vec<u8>, metadata: Value) -> Self {
		use base64::Engine as _;
		let data = base64::engine::general_purpose::STANDARD.encode(png);
		let text = serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string());
		Self {
			content: vec![
				ContentBlock::image(data, "image/png"),
				ContentBlock::text(text),
			],
			structured: Some(metadata),
			is_error: false,
		}
	}

	/// Converts to the `tools/call` result `rmcp` sends to the client.
	pub fn into_call_tool_result(self) -> CallToolResult {
		let mut result = if self.is_error {
			CallToolResult::error(self.content)
		} else {
			CallToolResult::success(self.content)
		};
		result.structured_content = self.structured;
		result
	}
}

/// Typed accessors over a tool's arguments object.
pub struct Args<'a> {
	value: &'a Value,
}

impl<'a> Args<'a> {
	/// Wraps an arguments object.
	pub fn new(value: &'a Value) -> Self {
		Self { value }
	}

	/// The raw arguments object.
	pub fn raw(&self) -> &'a Value {
		self.value
	}

	fn get(&self, key: &str) -> Option<&'a Value> {
		self.value.get(key).filter(|value| !value.is_null())
	}

	/// An optional string argument.
	pub fn opt_str(&self, key: &str) -> Option<String> {
		self.get(key).and_then(Value::as_str).map(str::to_owned)
	}

	/// A required string argument.
	pub fn str(&self, key: &str) -> Result<String, String> {
		self.opt_str(key)
			.ok_or_else(|| format!("missing required string argument `{key}`"))
	}

	/// An optional number argument.
	pub fn opt_f64(&self, key: &str) -> Result<Option<f64>, String> {
		match self.get(key) {
			None => Ok(None),
			Some(Value::Number(number)) => number
				.as_f64()
				.map(Some)
				.ok_or_else(|| format!("`{key}` must be a finite number")),
			Some(_) => Err(format!("`{key}` must be a number")),
		}
	}

	/// A required number argument.
	pub fn f64(&self, key: &str) -> Result<f64, String> {
		self.opt_f64(key)?
			.ok_or_else(|| format!("missing required numeric argument `{key}`"))
	}

	/// An optional boolean argument.
	pub fn opt_bool(&self, key: &str) -> Result<Option<bool>, String> {
		match self.get(key) {
			None => Ok(None),
			Some(Value::Bool(value)) => Ok(Some(*value)),
			Some(_) => Err(format!("`{key}` must be a boolean")),
		}
	}

	/// A required boolean argument.
	pub fn bool(&self, key: &str) -> Result<bool, String> {
		self.opt_bool(key)?
			.ok_or_else(|| format!("missing required boolean argument `{key}`"))
	}

	/// An optional non-negative integer argument.
	pub fn opt_usize(&self, key: &str) -> Result<Option<usize>, String> {
		match self.get(key) {
			None => Ok(None),
			Some(Value::Number(number)) => number
				.as_u64()
				.map(|value| Some(value as usize))
				.ok_or_else(|| format!("`{key}` must be a non-negative integer")),
			Some(_) => Err(format!("`{key}` must be a non-negative integer")),
		}
	}

	/// An optional integer argument.
	pub fn opt_i64(&self, key: &str) -> Result<Option<i64>, String> {
		match self.get(key) {
			None => Ok(None),
			Some(Value::Number(number)) => number
				.as_i64()
				.map(Some)
				.ok_or_else(|| format!("`{key}` must be an integer")),
			Some(_) => Err(format!("`{key}` must be an integer")),
		}
	}

	/// A required node id.
	pub fn id(&self, key: &str) -> Result<NodeId, String> {
		let value = self
			.get(key)
			.ok_or_else(|| format!("missing required node id argument `{key}`"))?;
		convert::parse_id(value, key)
	}

	/// An optional node id.
	pub fn opt_id(&self, key: &str) -> Result<Option<NodeId>, String> {
		match self.get(key) {
			None => Ok(None),
			Some(value) => convert::parse_id(value, key).map(Some),
		}
	}

	/// A required array of node ids.
	pub fn ids(&self, key: &str) -> Result<Vec<NodeId>, String> {
		let values = self
			.get(key)
			.and_then(Value::as_array)
			.ok_or_else(|| format!("`{key}` must be an array of node ids"))?;
		if values.is_empty() {
			return Err(format!("`{key}` must not be empty"));
		}
		values
			.iter()
			.enumerate()
			.map(|(index, value)| convert::parse_id(value, &format!("{key}[{index}]")))
			.collect()
	}
}

/// Resolves the `session` argument (or the active session).
pub fn resolve_session(state: &ServerState, args: &Args<'_>) -> Result<String, String> {
	let requested = args
		.opt_str("session")
		.or_else(|| args.opt_str("session_id"));
	state.resolve_session(requested.as_deref())
}

/// The active sequence of a session, or a helpful error.
pub fn current_sequence(
	session: &quercus_engine_rs::editor::EditorSession,
) -> Result<NodeId, String> {
	session.current_sequence().ok_or_else(|| {
		"session has no current sequence: create one with sequence_create or select one with sequence_use"
			.to_string()
	})
}

/// The frame rate of the given sequence.
pub fn sequence_rate(
	session: &quercus_engine_rs::editor::EditorSession,
	sequence: NodeId,
) -> quercus_engine_rs::types::Rational {
	session.project().sequence_frame_rate(sequence)
}
