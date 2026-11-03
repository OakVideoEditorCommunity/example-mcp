//! Project, session, sequence, track, and marker tools.

use quercus_engine_rs::edit::EditCommand;
use quercus_engine_rs::editor::EditorSession;
use quercus_engine_rs::types::{Rational, TimeRange};
use serde_json::{Value, json};

use super::{Args, ToolDef, ToolReply, current_sequence, resolve_session};
use crate::convert::{self, id_json, parse_track_kind, round, track_kind_str};
use crate::state::ServerState;

/// The tools in this module.
pub fn tools() -> Vec<ToolDef> {
	vec![
		ToolDef {
			name: "project_new",
			title: "New project",
			description: "Create a new in-memory Quercus project with one sequence and the default \
				V1/V2/A1/A2 track layout. The new session becomes active. Times are seconds.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"name": {"type": "string", "description": "Project name (default \"Untitled\")"},
						"sequence_name": {"type": "string", "description": "First sequence name (default \"Sequence 1\")"},
						"width": {"type": "integer", "minimum": 1, "maximum": 16384, "description": "Frame width (default 1920)"},
						"height": {"type": "integer", "minimum": 1, "maximum": 16384, "description": "Frame height (default 1080)"},
						"fps": {"type": "number", "exclusiveMinimum": 0, "description": "Frame rate in frames per second (default 30)"},
						"interlaced": {"type": "boolean", "description": "Whether the sequence is interlaced (default false)"}
					}
				})
			},
			handler: project_new,
		},
		ToolDef {
			name: "project_open",
			title: "Open project",
			description: "Open a Quercus project file (.ove) or storage URI and make it the active session.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"path": {"type": "string", "description": "Project path or storage URI"}
					},
					"required": ["path"]
				})
			},
			handler: project_open,
		},
		ToolDef {
			name: "project_save",
			title: "Save project",
			description: "Save the active (or given) session to a .ove file. Defaults to the path the \
				project was opened from or last saved to.",
			read_only: false,
			destructive: true,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"path": {"type": "string", "description": "Output .ove path; defaults to the session's current path"}
					}
				})
			},
			handler: project_save,
		},
		ToolDef {
			name: "project_info",
			title: "Project info",
			description: "Summarize the active project: name, path, revision, sequences, footages, markers.",
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
			handler: project_info,
		},
		ToolDef {
			name: "session_list",
			title: "List sessions",
			description: "List open editor sessions and which one is active.",
			read_only: true,
			destructive: false,
			input_schema: || json!({"type": "object", "properties": {}}),
			handler: session_list,
		},
		ToolDef {
			name: "session_use",
			title: "Select session",
			description: "Make an open session the active one for tools that omit `session`.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id to activate"}
					},
					"required": ["session"]
				})
			},
			handler: session_use,
		},
		ToolDef {
			name: "session_close",
			title: "Close session",
			description: "Release an editor session. Unsaved edits are lost.",
			read_only: false,
			destructive: true,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id to close"}
					},
					"required": ["session"]
				})
			},
			handler: session_close,
		},
		ToolDef {
			name: "sequence_create",
			title: "Create sequence",
			description: "Create a sequence and make it current. Format defaults to the current sequence's.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"name": {"type": "string", "description": "Sequence name"},
						"width": {"type": "integer", "minimum": 1, "maximum": 16384},
						"height": {"type": "integer", "minimum": 1, "maximum": 16384},
						"fps": {"type": "number", "exclusiveMinimum": 0},
						"interlaced": {"type": "boolean"}
					},
					"required": ["name"]
				})
			},
			handler: sequence_create,
		},
		ToolDef {
			name: "sequence_use",
			title: "Select sequence",
			description: "Make a sequence in the session current (the timeline tools operate on it).",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"sequence": {"type": "string", "description": "Sequence node id"}
					},
					"required": ["sequence"]
				})
			},
			handler: sequence_use,
		},
		ToolDef {
			name: "sequence_set_format",
			title: "Set sequence format",
			description: "Change a sequence's name, resolution, frame rate, or interlacing.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"sequence": {"type": "string", "description": "Sequence id (defaults to current)"},
						"name": {"type": "string", "description": "New name"},
						"width": {"type": "integer", "minimum": 1, "maximum": 16384},
						"height": {"type": "integer", "minimum": 1, "maximum": 16384},
						"fps": {"type": "number", "exclusiveMinimum": 0},
						"interlaced": {"type": "boolean"}
					}
				})
			},
			handler: sequence_set_format,
		},
		ToolDef {
			name: "timeline_get",
			title: "Read timeline",
			description: "Read the full timeline: sequences, tracks, and clips with timeline and \
				source-media ranges in seconds. This is the source of truth for node ids.",
			read_only: true,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"sequence": {"type": "string", "description": "Only return this sequence"}
					}
				})
			},
			handler: timeline_get,
		},
		ToolDef {
			name: "playhead_set",
			title: "Set playhead",
			description: "Move the current sequence's playhead to a time in seconds.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"time": {"type": "number", "description": "Playhead position in seconds"}
					},
					"required": ["time"]
				})
			},
			handler: playhead_set,
		},
		ToolDef {
			name: "track_add",
			title: "Add track",
			description: "Append a track of the given kind to the current sequence.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"kind": {"type": "string", "enum": ["video", "audio", "subtitle"], "description": "Track kind"}
					},
					"required": ["kind"]
				})
			},
			handler: track_add,
		},
		ToolDef {
			name: "track_set",
			title: "Update track",
			description: "Mute, lock, or resize a track.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"track": {"type": "string", "description": "Track node id"},
						"muted": {"type": "boolean"},
						"locked": {"type": "boolean"},
						"height": {"type": "number", "exclusiveMinimum": 0, "description": "Track height in pixels"}
					},
					"required": ["track"]
				})
			},
			handler: track_set,
		},
		ToolDef {
			name: "marker_add",
			title: "Add marker",
			description: "Add a marker to the current sequence at a time in seconds.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"time": {"type": "number", "description": "Marker time in seconds"},
						"name": {"type": "string", "description": "Marker label"},
						"color": {"type": "integer", "description": "Marker color index (default 0)"}
					},
					"required": ["time"]
				})
			},
			handler: marker_add,
		},
		ToolDef {
			name: "markers_list",
			title: "List markers",
			description: "List the current sequence's markers.",
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
			handler: markers_list,
		},
		ToolDef {
			name: "workarea_set",
			title: "Set work area",
			description: "Set or clear the current sequence's work area (the export/render range).",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"enabled": {"type": "boolean", "description": "Enable the work area (default true)"},
						"start": {"type": "number", "description": "Work area start in seconds"},
						"end": {"type": "number", "description": "Work area end in seconds"}
					},
					"required": ["start", "end"]
				})
			},
			handler: workarea_set,
		},
	]
}

fn dimension(args: &Args<'_>, key: &str, default: i32) -> Result<i32, String> {
	let value = args.opt_usize(key)?.unwrap_or(default as usize);
	let value = i32::try_from(value).map_err(|_| format!("`{key}` is too large"))?;
	if !(1..=16384).contains(&value) {
		return Err(format!("`{key}` must be between 1 and 16384"));
	}
	Ok(value)
}

fn fps(args: &Args<'_>, default: Rational) -> Result<Rational, String> {
	let Some(fps) = args.opt_f64("fps")? else {
		return Ok(default);
	};
	if !(fps.is_finite() && fps > 0.0) {
		return Err("`fps` must be positive".to_string());
	}
	let rate = Rational::from_f64(fps);
	if rate.is_null() || rate.num <= 0 || rate.den <= 0 {
		return Err(format!("`fps` {fps} cannot be represented as a frame rate"));
	}
	Ok(rate)
}

fn project_new(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let name = args
		.opt_str("name")
		.unwrap_or_else(|| "Untitled".to_string());
	let sequence_name = args
		.opt_str("sequence_name")
		.unwrap_or_else(|| "Sequence 1".to_string());
	let width = dimension(&args, "width", 1920)?;
	let height = dimension(&args, "height", 1080)?;
	let rate = fps(&args, Rational::from_f64(30.0))?;
	let interlaced = args.opt_bool("interlaced")?.unwrap_or(false);

	let session =
		EditorSession::new().map_err(|error| format!("failed to create a project: {error}"))?;
	let created = session
		.execute(EditCommand::CreateSequence {
			name: sequence_name,
			width: Some(width),
			height: Some(height),
			rate: Some(rate),
			interlaced: Some(interlaced),
		})
		.map_err(|error| format!("create sequence failed: {error}"))?;
	let sequence = created
		.primary
		.ok_or_else(|| "engine did not report the new sequence".to_string())?;
	let snapshot = session.snapshot();
	let tracks = snapshot
		.sequences
		.iter()
		.find(|item| item.id == sequence)
		.map(|item| item.tracks.len())
		.unwrap_or(0);
	let session_id = state.add_session(session, name.clone(), None);
	Ok(ToolReply::json(json!({
		"session": session_id,
		"sequence": id_json(sequence),
		"name": name,
		"width": width,
		"height": height,
		"fps": round(rate.to_f64()),
		"interlaced": interlaced,
		"tracks": tracks,
		"next": "media_import, then clip_place",
	})))
}

fn project_open(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let path = args.str("path")?;
	let session =
		EditorSession::open(&path).map_err(|error| format!("failed to open `{path}`: {error}"))?;
	// The .ove serializer drops sequence formats and markers; restore them from
	// the sidecar this server wrote when the project was saved.
	let (sidecar_restored, warnings) = match crate::sidecar::read(&path) {
		Some(sidecar) => (true, crate::sidecar::restore(&session, &sidecar)),
		None => (false, Vec::new()),
	};
	let snapshot = session.snapshot();
	let label = if snapshot.name.is_empty() {
		path.clone()
	} else {
		snapshot.name.clone()
	};
	let sequences: Vec<Value> = snapshot
		.sequences
		.iter()
		.map(|sequence| {
			json!({
				"id": id_json(sequence.id),
				"name": sequence.name,
				"width": sequence.width,
				"height": sequence.height,
				"fps": round(sequence.frame_rate.to_f64()),
				"length_seconds": round(sequence.length.to_f64()),
				"clips": sequence.tracks.iter().map(|track| track.clips.len()).sum::<usize>(),
			})
		})
		.collect();
	let session_id = state.add_session(session, label.clone(), Some(path.clone()));
	Ok(ToolReply::json(json!({
		"session": session_id,
		"name": label,
		"path": path,
		"filename": snapshot.filename,
		"sequences": sequences,
		"footages": snapshot.footages.len(),
		"sidecar_restored": sidecar_restored,
		"warnings": warnings,
	})))
}

fn project_save(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let requested = args.opt_str("path");
	let entry = state.entry_mut(&session_id)?;
	let path = requested.or_else(|| entry.path.clone()).ok_or_else(|| {
		"missing `path`: this project has never been saved, so there is no default".to_string()
	})?;
	entry
		.session
		.save(&path)
		.map_err(|error| format!("save failed: {error}"))?;
	entry.path = Some(path.clone());
	let snapshot = entry.session.snapshot();
	let (sidecar, warnings) = match crate::sidecar::write(&path, &snapshot) {
		Ok(sidecar) => (Some(sidecar.display().to_string()), None),
		Err(error) => (None, Some(error)),
	};
	Ok(ToolReply::json(json!({
		"session": session_id,
		"path": path,
		"saved": true,
		"sidecar": sidecar,
		"warnings": warnings,
	})))
}

fn sequence_set_format(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let entry = state.entry(&session_id)?;
	let sequence = match args.opt_id("sequence")? {
		Some(sequence) => sequence,
		None => current_sequence(&entry.session)?,
	};
	let snapshot = entry.session.snapshot();
	let current = snapshot
		.sequences
		.iter()
		.find(|item| item.id == sequence)
		.ok_or_else(|| "sequence not found in this session".to_string())?;
	let width = dimension(&args, "width", current.width)?;
	let height = dimension(&args, "height", current.height)?;
	let rate = fps(&args, current.frame_rate)?;
	let interlaced = args.opt_bool("interlaced")?.unwrap_or(current.interlaced);
	let name = args.opt_str("name").unwrap_or_else(|| current.name.clone());
	let result = entry
		.session
		.execute(EditCommand::SetSequenceParameters {
			sequence,
			name: name.clone(),
			width,
			height,
			rate,
			interlaced,
		})
		.map_err(|error| format!("set sequence format failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"sequence": id_json(sequence),
		"name": name,
		"width": width,
		"height": height,
		"fps": round(rate.to_f64()),
		"interlaced": interlaced,
		"revision": result.revision,
	})))
}

fn project_info(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let entry = state.entry(&session_id)?;
	let snapshot = entry.session.snapshot();
	let sequences: Vec<Value> = snapshot
		.sequences
		.iter()
		.map(|sequence| {
			json!({
				"id": id_json(sequence.id),
				"name": sequence.name,
				"width": sequence.width,
				"height": sequence.height,
				"fps": round(sequence.frame_rate.to_f64()),
				"length_seconds": round(sequence.length.to_f64()),
				"tracks": sequence.tracks.len(),
				"clips": sequence.tracks.iter().map(|track| track.clips.len()).sum::<usize>(),
				"current": snapshot.current_sequence == Some(sequence.id),
			})
		})
		.collect();
	Ok(ToolReply::json(json!({
		"session": session_id,
		"label": entry.label,
		"path": entry.path,
		"name": snapshot.name,
		"filename": snapshot.filename,
		"uuid": snapshot.uuid,
		"revision": snapshot.revision,
		"modified": snapshot.modified,
		"current_sequence": snapshot.current_sequence.map(id_json),
		"sequences": sequences,
		"footages": snapshot.footages.iter().map(convert::footage_json).collect::<Vec<_>>(),
		"markers": snapshot.markers.len(),
		"storage_bound": entry.session.storage_bound(),
		"storage_error": entry.session.storage_last_error(),
	})))
}

fn session_list(state: &mut ServerState, _arguments: Value) -> Result<ToolReply, String> {
	let active = state.active_session_id().map(str::to_owned);
	let sessions: Vec<Value> = state
		.session_entries()
		.map(|entry| {
			let snapshot = entry.session.snapshot();
			json!({
				"session": entry.id,
				"label": entry.label,
				"path": entry.path,
				"active": active.as_deref() == Some(entry.id.as_str()),
				"sequences": snapshot.sequences.len(),
				"footages": snapshot.footages.len(),
				"modified": snapshot.modified,
			})
		})
		.collect();
	Ok(ToolReply::json(
		json!({ "sessions": sessions, "active": active }),
	))
}

fn session_use(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session = args.str("session")?;
	state.set_active(&session)?;
	Ok(ToolReply::json(json!({ "active": session })))
}

fn session_close(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session = args.str("session")?;
	let closed = state.remove_session(&session);
	if !closed {
		return Err(format!("unknown session `{session}`"));
	}
	let active = state.active_session_id().map(str::to_owned);
	Ok(ToolReply::json(json!({
		"session": session,
		"closed": true,
		"active": active,
	})))
}

fn sequence_create(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let name = args.str("name")?;
	let entry = state.entry(&session_id)?;
	let current = entry.session.current_sequence();
	let defaults = current.map(|sequence| {
		let project = entry.session.project();
		let (width, height) = project.sequence_geometry(sequence).unwrap_or((1920, 1080));
		(
			sequence,
			width,
			height,
			project.sequence_frame_rate(sequence),
		)
	});
	let default_rate = defaults
		.as_ref()
		.map(|(_, _, _, rate)| *rate)
		.unwrap_or_else(|| Rational::from_f64(30.0));
	let width = dimension(
		&args,
		"width",
		defaults.as_ref().map(|(_, w, _, _)| *w).unwrap_or(1920),
	)?;
	let height = dimension(
		&args,
		"height",
		defaults.as_ref().map(|(_, _, h, _)| *h).unwrap_or(1080),
	)?;
	let rate = fps(&args, default_rate)?;
	let interlaced = args.opt_bool("interlaced")?.unwrap_or(false);
	let created = entry
		.session
		.execute(EditCommand::CreateSequence {
			name: name.clone(),
			width: Some(width),
			height: Some(height),
			rate: Some(rate),
			interlaced: Some(interlaced),
		})
		.map_err(|error| format!("create sequence failed: {error}"))?;
	let sequence = created
		.primary
		.ok_or_else(|| "engine did not report the new sequence".to_string())?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"sequence": id_json(sequence),
		"name": name,
		"width": width,
		"height": height,
		"fps": round(rate.to_f64()),
		"interlaced": interlaced,
	})))
}

fn sequence_use(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let sequence = args.id("sequence")?;
	let entry = state.entry(&session_id)?;
	if !entry.session.set_current_sequence(sequence) {
		return Err("sequence not found in this session".to_string());
	}
	Ok(ToolReply::json(json!({
		"session": session_id,
		"sequence": id_json(sequence),
	})))
}

fn timeline_get(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let filter = args.opt_id("sequence")?;
	let entry = state.entry(&session_id)?;
	let snapshot = entry.session.snapshot();
	let sequences: Vec<Value> = snapshot
		.sequences
		.iter()
		.filter(|sequence| filter.is_none_or(|id| id == sequence.id))
		.map(convert::sequence_json)
		.collect();
	Ok(ToolReply::json(json!({
		"session": session_id,
		"revision": snapshot.revision,
		"current_sequence": snapshot.current_sequence.map(id_json),
		"sequences": sequences,
	})))
}

fn playhead_set(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let time = args.f64("time")?;
	let entry = state.entry(&session_id)?;
	let sequence = current_sequence(&entry.session)?;
	entry
		.session
		.execute(EditCommand::SetSequencePlayhead {
			sequence,
			time: Rational::from_f64(time),
		})
		.map_err(|error| format!("set playhead failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"playhead_seconds": round(time),
	})))
}

fn track_add(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let kind = parse_track_kind(&args.str("kind")?)?;
	let entry = state.entry(&session_id)?;
	let sequence = current_sequence(&entry.session)?;
	let before: Vec<u64> = entry
		.session
		.snapshot()
		.sequences
		.iter()
		.find(|item| item.id == sequence)
		.map(|item| item.tracks.iter().map(|track| track.id.raw()).collect())
		.unwrap_or_default();
	let result = entry
		.session
		.execute(EditCommand::AddTrack { kind })
		.map_err(|error| format!("add track failed: {error}"))?;
	// `AddTrack` does not report the new track, so diff the track lists.
	let created = entry
		.session
		.snapshot()
		.sequences
		.iter()
		.find(|item| item.id == sequence)
		.and_then(|item| {
			item.tracks
				.iter()
				.rev()
				.find(|track| !before.contains(&track.id.raw()))
				.map(|track| track.id)
		});
	Ok(ToolReply::json(json!({
		"session": session_id,
		"track": created.map(id_json),
		"kind": track_kind_str(kind),
		"revision": result.revision,
	})))
}

fn track_set(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let track = args.id("track")?;
	let muted = args.opt_bool("muted")?;
	let locked = args.opt_bool("locked")?;
	let height = args.opt_f64("height")?;
	if muted.is_none() && locked.is_none() && height.is_none() {
		return Err("nothing to change: pass `muted`, `locked`, and/or `height`".to_string());
	}
	let entry = state.entry(&session_id)?;
	let mut updated = Vec::new();
	if let Some(muted) = muted {
		entry
			.session
			.execute(EditCommand::SetTrackMuted { track, muted })
			.map_err(|error| format!("set track muted failed: {error}"))?;
		updated.push("muted");
	}
	if let Some(locked) = locked {
		entry
			.session
			.execute(EditCommand::SetTrackLocked { track, locked })
			.map_err(|error| format!("set track locked failed: {error}"))?;
		updated.push("locked");
	}
	if let Some(height) = height {
		entry
			.session
			.execute(EditCommand::SetTrackHeight { track, height })
			.map_err(|error| format!("set track height failed: {error}"))?;
		updated.push("height");
	}
	Ok(ToolReply::json(json!({
		"session": session_id,
		"track": id_json(track),
		"updated": updated,
	})))
}

fn marker_add(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let time = args.f64("time")?;
	let name = args.opt_str("name").unwrap_or_else(|| "Marker".to_string());
	let color = args.opt_i64("color")?.unwrap_or(0) as i32;
	let entry = state.entry(&session_id)?;
	entry
		.session
		.execute(EditCommand::MarkerAdd {
			time: Rational::from_f64(time),
			name: name.clone(),
			color,
		})
		.map_err(|error| format!("add marker failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"marker": { "name": name, "seconds": round(time), "color": color },
	})))
}

fn markers_list(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let entry = state.entry(&session_id)?;
	let markers = entry.session.markers();
	Ok(ToolReply::json(json!({
		"session": session_id,
		"count": markers.len(),
		"markers": markers.iter().map(convert::marker_json).collect::<Vec<_>>(),
	})))
}

fn workarea_set(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let enabled = args.opt_bool("enabled")?.unwrap_or(true);
	let start = args.f64("start")?;
	let end = args.f64("end")?;
	if end <= start {
		return Err("`end` must be greater than `start`".to_string());
	}
	let entry = state.entry(&session_id)?;
	entry
		.session
		.execute(EditCommand::WorkareaSet {
			enabled,
			range: TimeRange::new(Rational::from_f64(start), Rational::from_f64(end)),
		})
		.map_err(|error| format!("set work area failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"enabled": enabled,
		"start": round(start),
		"end": round(end),
	})))
}
