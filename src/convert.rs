// SPDX-License-Identifier: GPL-3.0-or-later
//! Conversions between Quercus SDK DTOs and the JSON shapes the MCP tools use.
//!
//! Time conventions:
//! * Every MCP-facing time is **seconds** (f64), because that is what a model
//!   reasons about; the engine's rational seconds and per-sequence integer
//!   frame timestamps are derived here.
//! * Node ids are 64-bit packed tokens. They are emitted as **decimal
//!   strings** so JavaScript clients cannot lose precision above 2^53.

use quercus_engine_rs::editor::{
	ClipSnapshot, FootageSnapshot, MarkerSnapshot, ProjectSnapshot, SequenceSnapshot, TrackKind,
	TrackSnapshot,
};
use quercus_engine_rs::node::NodeId;
use quercus_engine_rs::types::Rational;
use serde_json::{Value, json};

/// Rounds a seconds value to microsecond precision for compact JSON output.
pub fn round(seconds: f64) -> f64 {
	(seconds * 1_000_000.0).round() / 1_000_000.0
}

/// Serializes a rational time as `{num, den, seconds}`.
pub fn rational_json(value: Rational) -> Value {
	json!({
		"num": value.num,
		"den": value.den,
		"seconds": round(value.to_f64()),
	})
}

/// Serializes a node id as a decimal string.
pub fn id_json(id: NodeId) -> Value {
	Value::String(id.raw().to_string())
}

/// Parses a node id from a decimal string or a JSON integer.
pub fn parse_id(value: &Value, field: &str) -> Result<NodeId, String> {
	let raw = match value {
		Value::Number(number) => number.as_u64(),
		Value::String(text) => text.trim().parse::<u64>().ok(),
		_ => None,
	}
	.ok_or_else(|| format!("`{field}` must be a node id (decimal string or integer)"))?;
	let id = NodeId::from_raw(raw);
	if id.is_valid() {
		Ok(id)
	} else {
		Err(format!("`{field}` is not a valid node id: {raw}"))
	}
}

/// The lowercase name of a track kind.
pub fn track_kind_str(kind: TrackKind) -> &'static str {
	match kind {
		TrackKind::Video => "video",
		TrackKind::Audio => "audio",
		TrackKind::Subtitle => "subtitle",
	}
}

/// Parses `video` / `audio` / `subtitle` (case-insensitive).
pub fn parse_track_kind(text: &str) -> Result<TrackKind, String> {
	match text.trim().to_ascii_lowercase().as_str() {
		"video" | "v" => Ok(TrackKind::Video),
		"audio" | "a" => Ok(TrackKind::Audio),
		"subtitle" | "sub" | "s" => Ok(TrackKind::Subtitle),
		other => Err(format!(
			"unknown track kind `{other}`; expected video, audio, or subtitle"
		)),
	}
}

/// Converts seconds to a sequence frame timestamp.
///
/// The engine's `Rational::time_to_timestamp` takes a *timebase*
/// (seconds per frame, e.g. `1/25`), not the frame rate (`25/1`) that
/// `ProjectSnapshot` exposes, so the rate is inverted first.
pub fn seconds_to_ts(rate: Rational, seconds: f64) -> i64 {
	if rate.num <= 0 || rate.den <= 0 {
		return 0;
	}
	let timebase = Rational::new(rate.den, rate.num);
	timebase.time_to_timestamp(Rational::from_f64(seconds))
}

/// The end of the last clip on any track.
///
/// `SequenceSnapshot::length` is the engine's cached `last_length`; it is only
/// refreshed by the app's verify pass, so an SDK-only producer must derive the
/// content length from the clips for commands that need one (e.g. export).
pub fn sequence_content_length(sequence: &SequenceSnapshot) -> Rational {
	let mut best = Rational::new(0, 1);
	for track in &sequence.tracks {
		for clip in &track.clips {
			if clip.out_time > best {
				best = clip.out_time;
			}
		}
	}
	best
}

/// One sequence, including its flattened tracks and clips.
pub fn sequence_json(sequence: &SequenceSnapshot) -> Value {
	json!({
		"id": id_json(sequence.id),
		"name": sequence.name,
		"width": sequence.width,
		"height": sequence.height,
		"frame_rate": rational_json(sequence.frame_rate),
		"interlaced": sequence.interlaced,
		"length": rational_json(sequence.length),
		"content_length": rational_json(sequence_content_length(sequence)),
		"playhead": rational_json(sequence.playhead),
		"tracks": sequence.tracks.iter().map(track_json).collect::<Vec<_>>(),
	})
}

/// One track and its clips.
pub fn track_json(track: &TrackSnapshot) -> Value {
	json!({
		"id": id_json(track.id),
		"kind": track.kind.map(track_kind_str),
		"height": track.height,
		"muted": track.muted,
		"locked": track.locked,
		"clips": track.clips.iter().map(clip_json).collect::<Vec<_>>(),
	})
}

/// One clip block with both timeline and source-media ranges.
pub fn clip_json(clip: &ClipSnapshot) -> Value {
	json!({
		"id": id_json(clip.id),
		"kind": clip.kind.map(|kind| format!("{kind:?}")),
		"name": clip.name,
		"in": rational_json(clip.in_time),
		"out": rational_json(clip.out_time),
		"media_in": rational_json(clip.media_in),
		"media_out": rational_json(clip.media_out),
		"enabled": clip.enabled,
	})
}

/// One project-bin footage entry.
pub fn footage_json(footage: &FootageSnapshot) -> Value {
	let video = footage
		.video
		.as_ref()
		.map(|params| {
			json!({
				"width": params.width,
				"height": params.height,
				"frame_rate": rational_json(params.frame_rate),
				"pixel_format": params.pixel_format,
				"channels": params.channels,
				"interlaced": params.interlaced,
			})
		})
		.unwrap_or(Value::Null);
	let audio = footage
		.audio
		.as_ref()
		.map(|params| {
			json!({
				"sample_rate": params.sample_rate,
				"channel_layout": params.channel_layout,
				"format": params.format,
			})
		})
		.unwrap_or(Value::Null);
	json!({
		"id": id_json(footage.id),
		"name": footage.name,
		"filename": footage.filename,
		"duration": rational_json(footage.duration),
		"video_streams": footage.video_streams,
		"audio_streams": footage.audio_streams,
		"subtitle_streams": footage.subtitle_streams,
		"video": video,
		"audio": audio,
	})
}

/// One sequence-local marker.
pub fn marker_json(marker: &MarkerSnapshot) -> Value {
	json!({
		"name": marker.name,
		"time": rational_json(marker.time.in_),
		"end": rational_json(marker.time.out),
		"color": marker.color,
	})
}

/// A whole project snapshot (bin, sequences, markers, work area).
pub fn project_json(snapshot: &ProjectSnapshot) -> Value {
	json!({
		"name": snapshot.name,
		"filename": snapshot.filename,
		"uuid": snapshot.uuid,
		"revision": snapshot.revision,
		"modified": snapshot.modified,
		"current_sequence": snapshot.current_sequence.map(id_json),
		"sequences": snapshot.sequences.iter().map(sequence_json).collect::<Vec<_>>(),
		"footages": snapshot.footages.iter().map(footage_json).collect::<Vec<_>>(),
		"markers": snapshot.markers.iter().map(marker_json).collect::<Vec<_>>(),
		"workarea": snapshot.workarea.as_ref().map(|workarea| json!({
			"start": rational_json(workarea.range.in_),
			"end": rational_json(workarea.range.out),
		})),
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_node_ids_as_strings_and_numbers() {
		let id = parse_id(&Value::String("42".to_string()), "node").unwrap();
		assert_eq!(id.raw(), 42);
		let id = parse_id(&json!(7), "node").unwrap();
		assert_eq!(id.raw(), 7);
		assert!(parse_id(&json!("nope"), "node").is_err());
		assert!(parse_id(&json!(-1), "node").is_err());
	}

	#[test]
	fn converts_seconds_using_the_sequence_timebase() {
		let rate = Rational::new(25, 1);
		assert_eq!(seconds_to_ts(rate, 0.0), 0);
		assert_eq!(seconds_to_ts(rate, 5.0), 125);
		assert_eq!(seconds_to_ts(rate, 1.5), 38);
		// NTSC-style rates keep their fractional frame timing.
		let ntsc = Rational::new(30000, 1001);
		assert_eq!(seconds_to_ts(ntsc, 1.0), 30);
	}

	#[test]
	fn rounds_seconds_for_json() {
		assert_eq!(round(29.970029970), 29.97003);
		assert_eq!(round(1.0 / 3.0), 0.333333);
	}
}
