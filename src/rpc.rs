//! JSON-RPC 2.0 message model and stdio framing for MCP.
//!
//! MCP's stdio transport is newline-delimited JSON: one JSON-RPC message per
//! line, no `Content-Length` headers. Diagnostics must go to stderr; stdout is
//! reserved for protocol messages.

use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

/// The only JSON-RPC version this server speaks.
pub const JSONRPC_VERSION: &str = "2.0";

/// A JSON-RPC error object.
#[derive(Debug, Clone)]
pub struct RpcError {
	/// Numeric JSON-RPC error code.
	pub code: i64,
	/// Human-readable message.
	pub message: String,
	/// Optional structured detail.
	pub data: Option<Value>,
}

impl RpcError {
	/// Creates an error with the given code and message.
	pub fn new(code: i64, message: impl Into<String>) -> Self {
		Self {
			code,
			message: message.into(),
			data: None,
		}
	}

	/// Parse error (`-32700`).
	pub fn parse(message: impl Into<String>) -> Self {
		Self::new(-32700, message)
	}

	/// Invalid request (`-32600`).
	pub fn invalid_request(message: impl Into<String>) -> Self {
		Self::new(-32600, message)
	}

	/// Method not found (`-32601`).
	pub fn method_not_found(method: &str) -> Self {
		Self::new(-32601, format!("method not found: {method}"))
	}

	/// Invalid params (`-32602`).
	pub fn invalid_params(message: impl Into<String>) -> Self {
		Self::new(-32602, message)
	}
}

/// A parsed JSON-RPC request or notification.
#[derive(Debug, Clone)]
pub struct Request {
	/// Request id; `None` marks a notification (no response is sent).
	pub id: Option<Value>,
	/// The method name.
	pub method: String,
	/// Optional params object.
	pub params: Option<Value>,
}

impl Request {
	/// Parses a JSON value into a request, rejecting malformed envelopes.
	pub fn from_value(value: &Value) -> Result<Self, RpcError> {
		let object = value
			.as_object()
			.ok_or_else(|| RpcError::invalid_request("request must be a JSON object"))?;
		let method = object
			.get("method")
			.and_then(Value::as_str)
			.ok_or_else(|| RpcError::invalid_request("missing string member `method`"))?
			.to_owned();
		let id = match object.get("id") {
			None | Some(Value::Null) => None,
			Some(Value::String(_)) | Some(Value::Number(_)) => object.get("id").cloned(),
			Some(_) => {
				return Err(RpcError::invalid_request(
					"`id` must be a string, a number, or null",
				));
			}
		};
		let params = match object.get("params") {
			None | Some(Value::Null) => None,
			Some(Value::Object(_)) => object.get("params").cloned(),
			Some(_) => {
				return Err(RpcError::invalid_request("`params` must be a JSON object"));
			}
		};
		Ok(Self { id, method, params })
	}

	/// The params object, or an empty object.
	pub fn params_or_empty(&self) -> Value {
		self.params.clone().unwrap_or_else(|| json!({}))
	}
}

/// Builds a successful JSON-RPC response value.
pub fn response(id: Value, result: Value) -> Value {
	json!({ "jsonrpc": JSONRPC_VERSION, "id": id, "result": result })
}

/// Builds a JSON-RPC error response value.
pub fn error_response(id: Value, error: &RpcError) -> Value {
	let mut body = json!({ "code": error.code, "message": error.message });
	if let Some(data) = &error.data {
		body["data"] = data.clone();
	}
	json!({ "jsonrpc": JSONRPC_VERSION, "id": id, "error": body })
}

/// Reads one non-empty line from the transport.
///
/// Returns `Ok(None)` on EOF. Blank lines are skipped, which keeps the server
/// tolerant of clients that separate messages with extra newlines.
pub fn read_line<R: BufRead>(reader: &mut R) -> io::Result<Option<String>> {
	loop {
		let mut line = String::new();
		let read = reader.read_line(&mut line)?;
		if read == 0 {
			return Ok(None);
		}
		if line.trim().is_empty() {
			continue;
		}
		return Ok(Some(line));
	}
}

/// Writes one JSON-RPC message as a single newline-terminated line.
pub fn write_message<W: Write>(writer: &mut W, message: &Value) -> io::Result<()> {
	serde_json::to_writer(&mut *writer, message)?;
	writer.write_all(b"\n")?;
	writer.flush()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_request_and_notification() {
		let request =
			Request::from_value(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();
		assert_eq!(request.id, Some(json!(1)));
		assert_eq!(request.method, "tools/list");

		let notification =
			Request::from_value(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
				.unwrap();
		assert!(notification.id.is_none());
	}

	#[test]
	fn rejects_array_params() {
		let error = Request::from_value(&json!({"id":1,"method":"x","params":[1,2]})).unwrap_err();
		assert_eq!(error.code, -32600);
	}

	#[test]
	fn round_trips_lines() {
		let mut input: &[u8] = b"\n{\"id\":1,\"method\":\"ping\"}\n";
		let line = read_line(&mut input).unwrap().unwrap();
		assert_eq!(line.trim(), "{\"id\":1,\"method\":\"ping\"}");

		let mut out = Vec::new();
		write_message(&mut out, &json!({"ok":true})).unwrap();
		assert_eq!(out, b"{\"ok\":true}\n");
	}
}
