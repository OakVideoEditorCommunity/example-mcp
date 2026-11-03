//! `oak-mcp`: a Model Context Protocol server for the Quercus video engine.
//!
//! Transport is MCP stdio: newline-delimited JSON-RPC on stdin/stdout. Anything
//! printed to stdout that is not a protocol message corrupts the session, so all
//! diagnostics go to stderr.

#![forbid(unsafe_code)]

mod convert;
mod image;
mod rpc;
mod server;
mod sidecar;
mod state;
mod tools;

use std::io::{self, BufWriter};

use quercus_engine_rs::editor::EditorSession;
use quercus_engine_rs::render::{self, RenderBackend};
use serde_json::{Value, json};

fn main() {
	let mut verbose = false;
	for argument in std::env::args().skip(1) {
		match argument.as_str() {
			"-h" | "--help" => {
				print_help();
				return;
			}
			"-V" | "--version" => {
				println!("oak-mcp {}", env!("CARGO_PKG_VERSION"));
				return;
			}
			"--check" => std::process::exit(run_check(verbose)),
			"-v" | "--verbose" => verbose = true,
			other => {
				eprintln!("oak-mcp: unknown argument `{other}` (try --help)");
				std::process::exit(2);
			}
		}
	}

	if !render::manager_init(RenderBackend::Threads) && !render::manager_is_initialized() {
		eprintln!(
			"oak-mcp: warning: the render manager did not initialize; preview and export may fail"
		);
	}

	let mut state = state::ServerState::new();
	state.verbose = verbose;
	if verbose {
		eprintln!(
			"oak-mcp: ready (version {}, pid {}, {} tools)",
			env!("CARGO_PKG_VERSION"),
			std::process::id(),
			tools::all().len()
		);
	}

	let stdin = io::stdin();
	let stdout = io::stdout();
	let mut reader = stdin.lock();
	let mut writer = BufWriter::new(stdout.lock());

	loop {
		match rpc::read_line(&mut reader) {
			Ok(Some(line)) => {
				let message: Value = match serde_json::from_str(line.trim()) {
					Ok(value) => value,
					Err(error) => {
						let response = rpc::error_response(
							Value::Null,
							&rpc::RpcError::parse(error.to_string()),
						);
						if rpc::write_message(&mut writer, &response).is_err() {
							break;
						}
						continue;
					}
				};
				if let Some(response) = server::handle(&mut state, message) {
					if let Err(error) = rpc::write_message(&mut writer, &response) {
						eprintln!("oak-mcp: failed to write a response: {error}");
						break;
					}
				}
			}
			Ok(None) => break,
			Err(error) => {
				eprintln!("oak-mcp: stdin error: {error}");
				break;
			}
		}
	}

	if verbose {
		eprintln!("oak-mcp: stdin closed, shutting down");
	}
}

fn print_help() {
	println!(
		"oak-mcp {} - MCP server for the Quercus video engine

USAGE:
    oak-mcp [OPTIONS]

OPTIONS:
    -v, --verbose   Log lifecycle diagnostics to stderr
        --check     Verify the engine links, a sequence is created, and rendering initializes
    -h, --help      Print this help
    -V, --version   Print the version

The server speaks MCP over stdio: newline-delimited JSON-RPC 2.0 on
stdin/stdout. Configure it in an MCP client with command `oak-mcp` and no
arguments.

The Quercus engine links the project FFmpeg 8.1 build statically and needs
OpenColorIO 2.x at build time; see README.md for setup.",
		env!("CARGO_PKG_VERSION")
	);
}

fn run_check(verbose: bool) -> i32 {
	let session = match EditorSession::new() {
		Ok(session) => session,
		Err(error) => {
			eprintln!("oak-mcp --check: cannot create a session: {error}");
			return 1;
		}
	};
	let sequence = match session.create_sequence("check") {
		Ok(sequence) => sequence,
		Err(error) => {
			eprintln!("oak-mcp --check: cannot create a sequence: {error}");
			return 1;
		}
	};
	let render_ready =
		render::manager_init(RenderBackend::Threads) || render::manager_is_initialized();
	let snapshot = session.snapshot();
	let report = json!({
		"ok": true,
		"version": env!("CARGO_PKG_VERSION"),
		"engine": "quercus-engine-rs",
		"sequence": sequence.raw(),
		"sequences": snapshot.sequences.len(),
		"tracks": snapshot.sequences.first().map(|item| item.tracks.len()).unwrap_or(0),
		"render_manager": render_ready,
		"tools": tools::all().len(),
		"protocol_versions": ["2025-06-18", "2025-03-26", "2024-11-05"],
	});
	println!(
		"{}",
		serde_json::to_string_pretty(&report).unwrap_or_default()
	);
	if !render_ready {
		eprintln!("oak-mcp --check: the render manager did not initialize");
		return 1;
	}
	if verbose {
		eprintln!("oak-mcp --check: engine loaded and usable");
	}
	0
}
