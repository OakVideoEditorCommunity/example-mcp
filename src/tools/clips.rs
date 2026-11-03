//! Timeline clip tools: placement, trimming, moving, splitting, transitions.
//!
//! All times are seconds. The engine's semantic edit commands use integer frame
//! timestamps relative to the current sequence's frame rate; the conversion
//! happens here.

use quercus_engine_rs::edit::EditCommand;
use quercus_engine_rs::editor::{EditorSession, TrackKind};
use quercus_engine_rs::types::Rational;
use serde_json::{Value, json};

use super::{Args, ToolDef, ToolReply, current_sequence, resolve_session, sequence_rate};
use crate::convert::{self, id_json, parse_track_kind, round, track_kind_str};
use crate::state::ServerState;

/// The tools in this module.
pub fn tools() -> Vec<ToolDef> {
	vec![
		ToolDef {
			name: "clip_place",
			title: "Place clip",
			description: "Place a footage clip on the current sequence: timeline range [start, end) \
				in seconds, starting at `media_in` inside the source. Use clip_place_linked to place \
				linked video+audio at once.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"footage": {"type": "string", "description": "Footage node id from media_import"},
						"kind": {"type": "string", "enum": ["video", "audio", "subtitle"], "description": "Track kind (default video)"},
						"track_index": {"type": "integer", "minimum": 0, "description": "Index among tracks of that kind (default 0)"},
						"start": {"type": "number", "description": "Timeline start in seconds"},
						"end": {"type": "number", "description": "Timeline end in seconds (exclusive)"},
						"media_in": {"type": "number", "description": "Source in-point in seconds (default 0)"}
					},
					"required": ["footage", "start", "end"]
				})
			},
			handler: clip_place,
		},
		ToolDef {
			name: "clip_place_linked",
			title: "Place linked clip",
			description: "Place one footage as linked video+audio clips across several tracks, so \
				edits keep them in sync.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"footage": {"type": "string", "description": "Footage node id"},
						"placements": {
							"type": "array",
							"description": "Tracks to place on (default: video 0 and audio 0)",
							"items": {
								"type": "object",
								"properties": {
									"kind": {"type": "string", "enum": ["video", "audio", "subtitle"]},
									"track_index": {"type": "integer", "minimum": 0}
								},
								"required": ["kind"]
							}
						},
						"start": {"type": "number"},
						"end": {"type": "number"},
						"media_in": {"type": "number"}
					},
					"required": ["footage", "start", "end"]
				})
			},
			handler: clip_place_linked,
		},
		ToolDef {
			name: "clip_move",
			title: "Move clip",
			description: "Move a clip to a new timeline start, keeping its duration and media window.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"clip": {"type": "string", "description": "Clip node id"},
						"start": {"type": "number", "description": "New timeline start in seconds"}
					},
					"required": ["clip", "start"]
				})
			},
			handler: clip_move,
		},
		ToolDef {
			name: "clip_trim",
			title: "Trim clip",
			description: "Set a clip's timeline range [start, end) in seconds. Adjusting one edge only \
				also adjusts the source media window.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"clip": {"type": "string", "description": "Clip node id"},
						"start": {"type": "number"},
						"end": {"type": "number"}
					},
					"required": ["clip", "start", "end"]
				})
			},
			handler: clip_trim,
		},
		ToolDef {
			name: "clip_split",
			title: "Split clip",
			description: "Split a clip at a time in seconds into two clips.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"clip": {"type": "string", "description": "Clip node id"},
						"time": {"type": "number", "description": "Split point in seconds (must be inside the clip)"}
					},
					"required": ["clip", "time"]
				})
			},
			handler: clip_split,
		},
		ToolDef {
			name: "clip_delete",
			title: "Delete clip",
			description: "Delete a clip, leaving a gap (default) or ripple-deleting the gap.",
			read_only: false,
			destructive: true,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"clip": {"type": "string", "description": "Clip node id"},
						"ripple": {"type": "boolean", "description": "Close the gap after deleting (default false)"}
					},
					"required": ["clip"]
				})
			},
			handler: clip_delete,
		},
		ToolDef {
			name: "clip_speed",
			title: "Set clip speed",
			description: "Change playback speed for one or more clips (2.0 = double speed).",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"clips": {"type": "array", "items": {"type": "string"}, "description": "Clip node ids"},
						"speed": {"type": "number", "exclusiveMinimum": 0, "description": "Playback speed multiplier"}
					},
					"required": ["clips", "speed"]
				})
			},
			handler: clip_speed,
		},
		ToolDef {
			name: "clip_slide",
			title: "Slide clip",
			description: "Move a clip on the timeline while keeping its source media window fixed.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"clip": {"type": "string"},
						"start": {"type": "number", "description": "New timeline start in seconds"}
					},
					"required": ["clip", "start"]
				})
			},
			handler: clip_slide,
		},
		ToolDef {
			name: "clip_slip",
			title: "Slip clip",
			description: "Shift a clip's source media window without moving it on the timeline.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"clip": {"type": "string"},
						"media_in": {"type": "number", "description": "New source in-point in seconds"}
					},
					"required": ["clip", "media_in"]
				})
			},
			handler: clip_slip,
		},
		ToolDef {
			name: "clip_set",
			title: "Rename or enable clip",
			description: "Rename or enable/disable one or more clips.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"clips": {"type": "array", "items": {"type": "string"}},
						"name": {"type": "string", "description": "New clip name"},
						"enabled": {"type": "boolean", "description": "Enable or disable the clips"}
					},
					"required": ["clips"]
				})
			},
			handler: clip_set,
		},
		ToolDef {
			name: "transition_add",
			title: "Add transition",
			description: "Add a transition at the seam between two adjacent clips. `half` is the \
				duration added on each side of the seam, in seconds.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"previous": {"type": "string", "description": "Clip before the seam"},
						"next": {"type": "string", "description": "Clip after the seam"},
						"half": {"type": "number", "exclusiveMinimum": 0, "description": "Transition half-width in seconds"}
					},
					"required": ["previous", "next", "half"]
				})
			},
			handler: transition_add,
		},
	]
}

fn timestamp(session: &EditorSession, seconds: f64) -> Result<i64, String> {
	let sequence = current_sequence(session)?;
	let rate = sequence_rate(session, sequence);
	Ok(convert::seconds_to_ts(rate, seconds))
}

fn clip_place(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let footage = args.id("footage")?;
	let kind = match args.opt_str("kind") {
		Some(text) => parse_track_kind(&text)?,
		None => TrackKind::Video,
	};
	let track_index = args.opt_usize("track_index")?.unwrap_or(0);
	let start = args.f64("start")?;
	let end = args.f64("end")?;
	let media_in = args.opt_f64("media_in")?.unwrap_or(0.0);
	if end <= start {
		return Err("`end` must be greater than `start`".to_string());
	}
	let entry = state.entry(&session_id)?;
	let sequence = current_sequence(&entry.session)?;
	let rate = sequence_rate(&entry.session, sequence);
	let result = entry
		.session
		.execute(EditCommand::PlaceFootageClip {
			footage,
			kind,
			track_index,
			in_ts: convert::seconds_to_ts(rate, start),
			out_ts: convert::seconds_to_ts(rate, end),
			media_in_ts: convert::seconds_to_ts(rate, media_in),
		})
		.map_err(|error| format!("place clip failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"clip": result.primary.map(id_json),
		"kind": track_kind_str(kind),
		"track_index": track_index,
		"start": round(start),
		"end": round(end),
		"media_in": round(media_in),
		"revision": result.revision,
	})))
}

fn clip_place_linked(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let footage = args.id("footage")?;
	let start = args.f64("start")?;
	let end = args.f64("end")?;
	let media_in = args.opt_f64("media_in")?.unwrap_or(0.0);
	if end <= start {
		return Err("`end` must be greater than `start`".to_string());
	}
	let mut placements = Vec::new();
	if let Some(items) = args
		.raw()
		.get("placements")
		.filter(|value| !value.is_null())
	{
		let items = items
			.as_array()
			.ok_or_else(|| "`placements` must be an array".to_string())?;
		for (index, item) in items.iter().enumerate() {
			let kind = item
				.get("kind")
				.and_then(Value::as_str)
				.ok_or_else(|| format!("`placements[{index}].kind` is required"))?;
			let kind = parse_track_kind(kind)?;
			let track_index = item.get("track_index").and_then(Value::as_u64).unwrap_or(0) as usize;
			placements.push((kind, track_index));
		}
	}
	if placements.is_empty() {
		placements.push((TrackKind::Video, 0));
		placements.push((TrackKind::Audio, 0));
	}
	let entry = state.entry(&session_id)?;
	let sequence = current_sequence(&entry.session)?;
	let rate = sequence_rate(&entry.session, sequence);
	let result = entry
		.session
		.execute(EditCommand::PlaceFootageClipsLinked {
			footage,
			placements: placements.clone(),
			in_ts: convert::seconds_to_ts(rate, start),
			out_ts: convert::seconds_to_ts(rate, end),
			media_in_ts: convert::seconds_to_ts(rate, media_in),
		})
		.map_err(|error| format!("place linked clips failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"clips": result.ids.iter().copied().map(id_json).collect::<Vec<_>>(),
		"placements": placements.iter().map(|(kind, index)| json!({
			"kind": track_kind_str(*kind),
			"track_index": index,
		})).collect::<Vec<_>>(),
		"start": round(start),
		"end": round(end),
		"revision": result.revision,
	})))
}

fn clip_move(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let clip = args.id("clip")?;
	let start = args.f64("start")?;
	let entry = state.entry(&session_id)?;
	let new_in_ts = timestamp(&entry.session, start)?;
	let result = entry
		.session
		.execute(EditCommand::MoveClip { clip, new_in_ts })
		.map_err(|error| format!("move clip failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"clip": id_json(clip),
		"start": round(start),
		"revision": result.revision,
	})))
}

fn clip_trim(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let clip = args.id("clip")?;
	let start = args.f64("start")?;
	let end = args.f64("end")?;
	if end <= start {
		return Err("`end` must be greater than `start`".to_string());
	}
	let entry = state.entry(&session_id)?;
	let sequence = current_sequence(&entry.session)?;
	let rate = sequence_rate(&entry.session, sequence);
	let result = entry
		.session
		.execute(EditCommand::TrimClip {
			clip,
			new_in_ts: convert::seconds_to_ts(rate, start),
			new_out_ts: convert::seconds_to_ts(rate, end),
		})
		.map_err(|error| format!("trim clip failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"clip": id_json(clip),
		"start": round(start),
		"end": round(end),
		"revision": result.revision,
	})))
}

fn clip_split(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let clip = args.id("clip")?;
	let time = args.f64("time")?;
	let entry = state.entry(&session_id)?;
	let time_ts = timestamp(&entry.session, time)?;
	let result = entry
		.session
		.execute(EditCommand::SplitClip { clip, time_ts })
		.map_err(|error| format!("split clip failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"clip": id_json(clip),
		"time": round(time),
		"new_clip": result.primary.map(id_json),
		"revision": result.revision,
	})))
}

fn clip_delete(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let clip = args.id("clip")?;
	let ripple = args.opt_bool("ripple")?.unwrap_or(false);
	let entry = state.entry(&session_id)?;
	let command = if ripple {
		EditCommand::RippleDeleteClip { clip }
	} else {
		EditCommand::DeleteClip { clip }
	};
	let result = entry
		.session
		.execute(command)
		.map_err(|error| format!("delete clip failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"clip": id_json(clip),
		"ripple": ripple,
		"revision": result.revision,
	})))
}

fn clip_speed(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let clips = args.ids("clips")?;
	let speed = args.f64("speed")?;
	if !(speed.is_finite() && speed > 0.0) {
		return Err("`speed` must be positive".to_string());
	}
	let entry = state.entry(&session_id)?;
	let result = entry
		.session
		.execute(EditCommand::SetClipsSpeed { clips, speed })
		.map_err(|error| format!("set clip speed failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"speed": speed,
		"revision": result.revision,
	})))
}

fn clip_slide(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let clip = args.id("clip")?;
	let start = args.f64("start")?;
	let entry = state.entry(&session_id)?;
	let new_start = timestamp(&entry.session, start)?;
	let result = entry
		.session
		.execute(EditCommand::SlideClip { clip, new_start })
		.map_err(|error| format!("slide clip failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"clip": id_json(clip),
		"start": round(start),
		"revision": result.revision,
	})))
}

fn clip_slip(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let clip = args.id("clip")?;
	let media_in = args.f64("media_in")?;
	let entry = state.entry(&session_id)?;
	let new_media_in = timestamp(&entry.session, media_in)?;
	let result = entry
		.session
		.execute(EditCommand::SlipClip { clip, new_media_in })
		.map_err(|error| format!("slip clip failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"clip": id_json(clip),
		"media_in": round(media_in),
		"revision": result.revision,
	})))
}

fn clip_set(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let clips = args.ids("clips")?;
	let name = args.opt_str("name");
	let enabled = args.opt_bool("enabled")?;
	if name.is_none() && enabled.is_none() {
		return Err("nothing to change: pass `name` and/or `enabled`".to_string());
	}
	let entry = state.entry(&session_id)?;
	let mut revision = 0;
	if let Some(name) = name {
		let result = entry
			.session
			.execute(EditCommand::RenameBlocks {
				clips: clips.clone(),
				name,
			})
			.map_err(|error| format!("rename clips failed: {error}"))?;
		revision = result.revision;
	}
	if let Some(enabled) = enabled {
		let result = entry
			.session
			.execute(EditCommand::SetBlocksEnabled {
				clips: clips.clone(),
				enabled,
			})
			.map_err(|error| format!("set clips enabled failed: {error}"))?;
		revision = result.revision;
	}
	Ok(ToolReply::json(json!({
		"session": session_id,
		"clips": clips.iter().copied().map(id_json).collect::<Vec<_>>(),
		"revision": revision,
	})))
}

fn transition_add(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let previous = args.id("previous")?;
	let next = args.id("next")?;
	let half = args.f64("half")?;
	if !(half.is_finite() && half > 0.0) {
		return Err("`half` must be positive".to_string());
	}
	let entry = state.entry(&session_id)?;
	let result = entry
		.session
		.execute(EditCommand::AddTransitionAtSeam {
			previous,
			next,
			half: Rational::from_f64(half),
		})
		.map_err(|error| format!("add transition failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"transition": result.primary.map(id_json),
		"half": round(half),
		"revision": result.revision,
	})))
}
