// SPDX-License-Identifier: GPL-3.0-or-later
//! Protocol-level integration test: spawn the real server and drive it over
//! stdio, exactly as an MCP client would.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Server {
	child: Child,
	stdin: ChildStdin,
	stdout: BufReader<ChildStdout>,
	next_id: u64,
}

impl Server {
	fn start() -> Self {
		let mut child = Command::new(env!("CARGO_BIN_EXE_oak-mcp"))
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::null())
			.spawn()
			.expect("spawn oak-mcp");
		let stdin = child.stdin.take().expect("stdin");
		let stdout = BufReader::new(child.stdout.take().expect("stdout"));
		Self {
			child,
			stdin,
			stdout,
			next_id: 0,
		}
	}

	fn send(&mut self, message: Value) {
		self.stdin
			.write_all(format!("{message}\n").as_bytes())
			.expect("write request");
		self.stdin.flush().expect("flush");
	}

	fn recv(&mut self) -> Value {
		let mut line = String::new();
		self.stdout.read_line(&mut line).expect("read response");
		serde_json::from_str(line.trim()).unwrap_or_else(|error| {
			panic!("invalid response {line:?}: {error}");
		})
	}

	fn request(&mut self, method: &str, params: Value) -> Value {
		self.next_id += 1;
		let id = self.next_id;
		self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
		let response = self.recv();
		assert_eq!(response["id"], json!(id), "unexpected response: {response}");
		response
	}

	fn call(&mut self, tool: &str, arguments: Value) -> Value {
		let response = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
		response["result"].clone()
	}

	fn text_of(&mut self, tool: &str, arguments: Value) -> Value {
		let result = self.call(tool, arguments);
		let text = result["content"]
			.as_array()
			.expect("content array")
			.iter()
			.find(|block| block["type"] == "text")
			.and_then(|block| block["text"].as_str())
			.expect("text block");
		serde_json::from_str(text).expect("tool text is JSON")
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}

#[test]
fn lifecycle_tools_timeline_and_errors() {
	let mut server = Server::start();

	let init = server.request(
		"initialize",
		json!({
			"protocolVersion": "2025-06-18",
			"capabilities": {},
			"clientInfo": {"name": "integration", "version": "0"}
		}),
	);
	assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
	assert_eq!(init["result"]["serverInfo"]["name"], "oak-mcp");
	assert!(init["result"]["capabilities"]["tools"].is_object());

	server.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

	let tools = server.request("tools/list", json!({}));
	let names: Vec<&str> = tools["result"]["tools"]
		.as_array()
		.expect("tools array")
		.iter()
		.filter_map(|tool| tool["name"].as_str())
		.collect();
	assert!(
		names.len() >= 40,
		"expected a broad tool set, got {names:?}"
	);
	for expected in [
		"project_new",
		"media_import",
		"clip_place",
		"timeline_get",
		"preview_frame",
		"export_start",
		"task_wait",
		"effect_set_param",
		"undo",
	] {
		assert!(names.contains(&expected), "missing tool {expected}");
	}

	let project = server.text_of(
		"project_new",
		json!({"name": "Integration", "width": 640, "height": 360, "fps": 25}),
	);
	assert_eq!(project["width"], 640);
	assert_eq!(project["fps"], 25.0);

	let timeline = server.text_of("timeline_get", json!({}));
	let sequences = timeline["sequences"].as_array().expect("sequences");
	assert_eq!(sequences.len(), 1);
	assert_eq!(sequences[0]["tracks"].as_array().unwrap().len(), 4);

	// Unknown tools are JSON-RPC errors, not tool results.
	let unknown = server.request(
		"tools/call",
		json!({"name": "does_not_exist", "arguments": {}}),
	);
	assert_eq!(unknown["error"]["code"], -32602, "{unknown}");

	// Tool-level failures are content with isError, so the model can recover.
	let bad = server.call(
		"clip_place",
		json!({"footage": "999999", "start": 0, "end": 1}),
	);
	assert_eq!(bad["isError"], true, "{bad}");

	// Unknown methods produce -32601.
	let missing = server.request("not/a/method", json!({}));
	assert_eq!(missing["error"]["code"], -32601);

	// Resources and prompts are advertised and readable.
	let resources = server.request("resources/list", json!({}));
	let uris: Vec<&str> = resources["result"]["resources"]
		.as_array()
		.unwrap()
		.iter()
		.filter_map(|item| item["uri"].as_str())
		.collect();
	assert!(uris.contains(&"quercus://server/info"));
	assert!(uris.iter().any(|uri| uri.ends_with("/timeline")));

	let info = server.request("resources/read", json!({"uri": "quercus://server/info"}));
	let body = info["result"]["contents"][0]["text"]
		.as_str()
		.expect("text");
	assert!(body.contains("oak-mcp"));

	let prompts = server.request("prompts/list", json!({}));
	assert!(prompts["result"]["prompts"].as_array().unwrap().len() >= 2);
}

#[test]
fn save_reopen_restores_format_and_markers() {
	use std::path::Path;

	let mut server = Server::start();
	server.request(
		"initialize",
		json!({
			"protocolVersion": "2025-06-18",
			"capabilities": {},
			"clientInfo": {"name": "integration", "version": "0"}
		}),
	);
	server.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

	server.text_of(
		"project_new",
		json!({"name": "Roundtrip", "width": 1024, "height": 576, "fps": 24}),
	);
	server.text_of("marker_add", json!({"time": 2.0, "name": "beat"}));

	let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/e2e-test");
	std::fs::create_dir_all(&dir).expect("create test dir");
	let path = dir.join(format!("roundtrip-{}.ove", std::process::id()));
	let path_text = path.to_string_lossy().into_owned();

	let saved = server.text_of("project_save", json!({"path": path_text}));
	assert_eq!(saved["saved"], true, "{saved}");
	assert!(path.is_file(), "project file was not written");
	assert!(
		dir.join(format!("roundtrip-{}.ove.oak-mcp.json", std::process::id()))
			.is_file()
	);

	let reopened = server.text_of("project_open", json!({"path": path_text}));
	assert_eq!(reopened["sidecar_restored"], true, "{reopened}");

	let timeline = server.text_of("timeline_get", json!({}));
	let sequence = &timeline["sequences"][0];
	assert_eq!(sequence["width"], 1024);
	assert_eq!(sequence["height"], 576);
	assert_eq!(sequence["frame_rate"]["seconds"], 24.0);

	let markers = server.text_of("markers_list", json!({}));
	assert_eq!(markers["count"], 1, "{markers}");

	let _ = std::fs::remove_file(&path);
	let _ = std::fs::remove_file(
		dir.join(format!("roundtrip-{}.ove.oak-mcp.json", std::process::id())),
	);
}
