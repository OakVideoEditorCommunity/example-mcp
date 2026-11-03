# Tool Recipes

These are compiled, tested patterns. Adapt names and descriptions; keep the
`ToolDef`/`Args`/`ToolReply` shape.

## `studio_info` (no engine)

```rust
use serde_json::{Value, json};
use super::{Args, ToolDef, ToolReply};
use crate::state::ServerState;

pub fn tools() -> Vec<ToolDef> {
	vec![ToolDef {
		name: "studio_info",
		title: "Studio information",
		description: "Return this MCP server's studio name and version.",
		read_only: true,
		destructive: false,
		input_schema: || json!({
			"type": "object",
			"properties": {
				"studio": {"type": "string", "description": "Studio name (optional)"}
			}
		}),
		handler: studio_info,
	}]
}

fn studio_info(_state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let studio = args.opt_str("studio").unwrap_or_else(|| "My Studio".to_string());
	Ok(ToolReply::json(json!({
		"studio": studio,
		"server": env!("CARGO_PKG_NAME"),
		"version": env!("CARGO_PKG_VERSION"),
	})))
}
```

`opt_str` returns `Option<String>`; `str` returns `Result` and is for required
arguments.

## `import_folder` (engine)

```rust
use std::path::Path;
use quercus_engine_rs::edit::EditCommand;
use serde_json::{Value, json};
use super::{Args, ToolDef, ToolReply, resolve_session};
use crate::convert;
use crate::state::ServerState;

fn import_folder(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let folder = args.str("path")?;
	let recursive = args.opt_bool("recursive")?.unwrap_or(false);
	let folder = std::fs::canonicalize(&folder)
		.map_err(|error| format!("cannot open folder `{folder}`: {error}"))?;
	let mut files = Vec::new();
	collect_media(&folder, recursive, &mut files)?;
	files.sort();
	if files.is_empty() {
		return Err(format!("no media files found in `{}`", folder.display()));
	}
	let entry = state.entry(&session_id)?;
	let mut imported = Vec::new();
	for file in &files {
		let result = entry
			.session
			.execute(EditCommand::ImportFootage { path: file.clone() })
			.map_err(|error| format!("import `{file}` failed: {error}"))?;
		if let Some(footage) = result.primary {
			imported.push(footage);
		}
	}
	let snapshot = entry.session.snapshot();
	let footages: Vec<Value> = imported
		.iter()
		.filter_map(|id| snapshot.footages.iter().find(|footage| footage.id == *id))
		.map(convert::footage_json)
		.collect();
	Ok(ToolReply::json(json!({
		"session": session_id,
		"count": footages.len(),
		"footages": footages,
	})))
}

fn collect_media(dir: &Path, recursive: bool, out: &mut Vec<String>) -> Result<(), String> {
	let entries = std::fs::read_dir(dir)
		.map_err(|error| format!("cannot read `{}`: {error}", dir.display()))?;
	for entry in entries {
		let entry = entry.map_err(|error| error.to_string())?;
		let path = entry.path();
		if path.is_dir() {
			if recursive {
				collect_media(&path, true, out)?;
			}
		} else if is_media(&path) {
			let path = path.canonicalize().map_err(|error| error.to_string())?;
			out.push(path.to_string_lossy().into_owned());
		}
	}
	Ok(())
}

fn is_media(path: &Path) -> bool {
	let extension = path
		.extension()
		.and_then(|value| value.to_str())
		.map(str::to_ascii_lowercase)
		.unwrap_or_default();
	matches!(
		extension.as_str(),
		"mp4" | "mov" | "mkv" | "mxf" | "avi" | "m4v" | "webm" | "wav" | "mp3" | "flac" | "aif"
			| "aiff"
	)
}
```

## `assemble_rushes` (compose with `import_folder`)

Add this after `import_folder`; the agent calls `import_folder`, then passes
the returned footage IDs to this tool.

```rust
use quercus_engine_rs::editor::TrackKind;
use super::{current_sequence, sequence_rate};

fn assemble_rushes(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let footages = args.ids("footages")?;
	let entry = state.entry(&session_id)?;
	let sequence = current_sequence(&entry.session)?;
	let rate = sequence_rate(&entry.session, sequence);
	let snapshot = entry.session.snapshot();
	let sequence_snapshot = snapshot
		.sequences
		.iter()
		.find(|item| item.id == sequence)
		.ok_or_else(|| "sequence not found in snapshot".to_string())?;
	let mut cursor = convert::sequence_content_length(sequence_snapshot).to_f64();
	let mut placed = Vec::new();
	for footage in footages {
		let metadata = snapshot
			.footages
			.iter()
			.find(|item| item.id == footage)
			.ok_or_else(|| format!("footage {} is not in the project bin", convert::id_json(footage)))?;
		let duration = metadata.duration.to_f64();
		if duration <= 0.0 {
			continue;
		}
		let result = entry
			.session
			.execute(EditCommand::PlaceFootageClip {
				footage,
				kind: TrackKind::Video,
				track_index: 0,
				in_ts: convert::seconds_to_ts(rate, cursor),
				out_ts: convert::seconds_to_ts(rate, cursor + duration),
				media_in_ts: 0,
			})
			.map_err(|error| format!("place footage failed: {error}"))?;
		if let Some(clip) = result.primary {
			placed.push(convert::id_json(clip));
		}
		cursor += duration;
	}
	Ok(ToolReply::json(json!({
		"session": session_id,
		"clips": placed,
		"content_length": cursor,
	})))
}
```

ToolDef schema:

```json
{
  "type": "object",
  "properties": {
    "session": {"type": "string"},
    "footages": {"type": "array", "items": {"type": "string"}, "minItems": 1}
  },
  "required": ["footages"]
}
```

## Prompt

`src/server.rs`, in `prompts()`:

```rust
Prompt::new(
	"studio_brief",
	Some("Turn a client brief into a rough cut"),
	Some(vec![{
		let mut argument = PromptArgument::new("brief");
		argument.description = Some("What the client wants".into());
		argument.required = Some(true);
		argument
	}]),
)
```

`PromptArgument` is `#[non_exhaustive]`; use `PromptArgument::new(...)` and
mutate fields. Add the matching arm in `prompt_text()`.

## Effects and LUTs

Do not add a new engine effect node. Use the existing 68 effects through
`effect_add` / `effect_set_param` (or the engine effect-chain API), and wrap
only the studio-level loop ("apply the show LUT to every clip on V1"). For a
new shader effect, the extension point is an OFX plugin (C++), not this skill.
