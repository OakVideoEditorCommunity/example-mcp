//! Preview, export, task polling, and OTIO interchange tools.

use std::time::{Duration, Instant};

use quercus_engine_rs::codec::{Codec, Format};
use quercus_engine_rs::editor::EditorSession;
use quercus_engine_rs::render;
use quercus_engine_rs::task::{self, EncodingParams, TaskState};
use quercus_engine_rs::types::{PixelFormat, Rational};
use serde_json::{Value, json};

use super::{Args, ToolDef, ToolReply, current_sequence, resolve_session};
use crate::convert::{id_json, round};
use crate::image;
use crate::state::{ServerState, task_json};

/// The tools in this module.
pub fn tools() -> Vec<ToolDef> {
	vec![
		ToolDef {
			name: "preview_frame",
			title: "Render preview frame",
			description: "Render one frame of the current sequence at a time in seconds and return it \
				as a PNG image. Use it to check the edit visually.",
			read_only: true,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"time": {"type": "number", "description": "Timeline time in seconds"},
						"width": {"type": "integer", "minimum": 16, "maximum": 7680, "description": "Preview width (default 640)"},
						"height": {"type": "integer", "minimum": 16, "maximum": 4320, "description": "Preview height (default 360)"},
						"sequence": {"type": "string", "description": "Sequence id (defaults to current)"}
					},
					"required": ["time"]
				})
			},
			handler: preview_frame,
		},
		ToolDef {
			name: "render_audio",
			title: "Render audio",
			description: "Render the current sequence's audio for a range and report peak/RMS levels. \
				Optionally writes a 16-bit stereo WAV. Use it to verify sync sound or music placement.",
			read_only: true,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"start": {"type": "number", "description": "Range start in seconds"},
						"end": {"type": "number", "description": "Range end in seconds"},
						"sample_rate": {"type": "integer", "minimum": 8000, "maximum": 192000, "description": "Sample rate (default 48000)"},
						"channels": {"type": "string", "enum": ["stereo", "mono"], "description": "Output layout (default stereo)"},
						"gain_db": {"type": "number", "description": "Gain applied before metering/WAV write, in decibels (default 0)"},
						"filename": {"type": "string", "description": "Optional WAV path to write"}
					},
					"required": ["start", "end"]
				})
			},
			handler: render_audio,
		},
		ToolDef {
			name: "export_start",
			title: "Start export",
			description: "Start an asynchronous export of the current sequence. Poll with task_status \
				or block with task_wait. Format defaults to mp4 (H.264/AAC).",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"filename": {"type": "string", "description": "Output file path"},
						"format": {"type": "string", "enum": ["mp4", "mov", "mkv", "webm", "wav", "mp3", "flac", "m4a", "png", "tiff", "exr"], "description": "Container/format (default mp4)"},
						"width": {"type": "integer", "minimum": 1},
						"height": {"type": "integer", "minimum": 1},
						"video": {"type": "boolean", "description": "Include video (default from format)"},
						"audio": {"type": "boolean", "description": "Include audio (default from format)"},
						"video_codec": {"type": "string", "description": "h264, h265, prores, vp9, av1, png, tiff, exr, dnxhd, cineform"},
						"audio_codec": {"type": "string", "description": "aac, pcm, mp3, opus, vorbis, flac, mp2"},
						"video_bitrate": {"type": "integer", "description": "Bits per second; 0 uses the codec default"},
						"audio_bitrate": {"type": "integer", "description": "Bits per second; 0 uses the codec default"},
						"start": {"type": "number", "description": "Optional range start in seconds"},
						"end": {"type": "number", "description": "Optional range end in seconds"},
						"range": {
							"type": "object",
							"description": "Optional [start, end) range in seconds",
							"properties": {
								"start": {"type": "number"},
								"end": {"type": "number"}
							}
						}
					},
					"required": ["filename"]
				})
			},
			handler: export_start,
		},
		ToolDef {
			name: "task_status",
			title: "Task status",
			description: "Poll an asynchronous task's state, progress, and error.",
			read_only: true,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"task": {"type": "string", "description": "Task id"}
					},
					"required": ["task"]
				})
			},
			handler: task_status,
		},
		ToolDef {
			name: "task_wait",
			title: "Wait for task",
			description: "Block until a task finishes or the timeout expires, then report its state.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"task": {"type": "string", "description": "Task id"},
						"timeout_ms": {"type": "integer", "minimum": 0, "maximum": 600000, "description": "Maximum wait in milliseconds (default 30000)"}
					},
					"required": ["task"]
				})
			},
			handler: task_wait,
		},
		ToolDef {
			name: "task_cancel",
			title: "Cancel task",
			description: "Cooperatively cancel a running task.",
			read_only: false,
			destructive: true,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"task": {"type": "string", "description": "Task id"}
					},
					"required": ["task"]
				})
			},
			handler: task_cancel,
		},
		ToolDef {
			name: "task_list",
			title: "List tasks",
			description: "List asynchronous tasks started by this server.",
			read_only: true,
			destructive: false,
			input_schema: || json!({"type": "object", "properties": {}}),
			handler: task_list,
		},
		ToolDef {
			name: "otio_export",
			title: "Export OTIO",
			description: "Start an asynchronously written OpenTimelineIO (.otio) or FCPXML export.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"filename": {"type": "string", "description": "Output .otio or .fcpxml path"}
					},
					"required": ["filename"]
				})
			},
			handler: otio_export,
		},
		ToolDef {
			name: "otio_import",
			title: "Import OTIO",
			description: "Load an OpenTimelineIO (.otio) or FCPXML document as a new session.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"filename": {"type": "string", "description": "Input .otio or .fcpxml path"}
					},
					"required": ["filename"]
				})
			},
			handler: otio_import,
		},
	]
}

struct FormatSpec {
	format: Format,
	video_codec: Codec,
	audio_codec: Codec,
	has_video: bool,
	has_audio: bool,
}

fn format_spec(name: &str) -> Result<FormatSpec, String> {
	let spec = match name.trim().to_ascii_lowercase().as_str() {
		"mp4" | "mpeg4" => FormatSpec {
			format: Format::MPEG4Video,
			video_codec: Codec::H264,
			audio_codec: Codec::AAC,
			has_video: true,
			has_audio: true,
		},
		"mov" | "quicktime" => FormatSpec {
			format: Format::QuickTime,
			video_codec: Codec::H264,
			audio_codec: Codec::AAC,
			has_video: true,
			has_audio: true,
		},
		"mkv" | "matroska" => FormatSpec {
			format: Format::Matroska,
			video_codec: Codec::H264,
			audio_codec: Codec::AAC,
			has_video: true,
			has_audio: true,
		},
		"webm" => FormatSpec {
			format: Format::WebM,
			video_codec: Codec::VP9,
			audio_codec: Codec::Opus,
			has_video: true,
			has_audio: true,
		},
		"wav" => FormatSpec {
			format: Format::WAV,
			video_codec: Codec::H264,
			audio_codec: Codec::PCM,
			has_video: false,
			has_audio: true,
		},
		"mp3" => FormatSpec {
			format: Format::MP3,
			video_codec: Codec::H264,
			audio_codec: Codec::MP3,
			has_video: false,
			has_audio: true,
		},
		"flac" => FormatSpec {
			format: Format::FLAC,
			video_codec: Codec::H264,
			audio_codec: Codec::FLAC,
			has_video: false,
			has_audio: true,
		},
		"m4a" => FormatSpec {
			format: Format::MPEG4Audio,
			video_codec: Codec::H264,
			audio_codec: Codec::AAC,
			has_video: false,
			has_audio: true,
		},
		"png" => FormatSpec {
			format: Format::PNG,
			video_codec: Codec::PNG,
			audio_codec: Codec::PCM,
			has_video: true,
			has_audio: false,
		},
		"tiff" => FormatSpec {
			format: Format::TIFF,
			video_codec: Codec::TIFF,
			audio_codec: Codec::PCM,
			has_video: true,
			has_audio: false,
		},
		"exr" => FormatSpec {
			format: Format::OpenEXR,
			video_codec: Codec::OpenEXR,
			audio_codec: Codec::PCM,
			has_video: true,
			has_audio: false,
		},
		other => {
			return Err(format!(
				"unsupported export format `{other}`; use mp4, mov, mkv, webm, wav, mp3, flac, m4a, png, tiff, or exr"
			));
		}
	};
	Ok(spec)
}

fn codec_by_name(name: &str) -> Result<Codec, String> {
	match name.trim().to_ascii_lowercase().as_str() {
		"h264" | "avc" => Ok(Codec::H264),
		"h264rgb" => Ok(Codec::H264RGB),
		"h265" | "hevc" => Ok(Codec::H265),
		"prores" => Ok(Codec::ProRes),
		"vp9" => Ok(Codec::VP9),
		"av1" => Ok(Codec::AV1),
		"dnxhd" => Ok(Codec::DNxHD),
		"cineform" => Ok(Codec::CineForm),
		"png" => Ok(Codec::PNG),
		"tiff" => Ok(Codec::TIFF),
		"exr" | "openexr" => Ok(Codec::OpenEXR),
		"aac" => Ok(Codec::AAC),
		"pcm" => Ok(Codec::PCM),
		"mp3" => Ok(Codec::MP3),
		"mp2" => Ok(Codec::MP2),
		"opus" => Ok(Codec::Opus),
		"vorbis" => Ok(Codec::Vorbis),
		"flac" => Ok(Codec::FLAC),
		other => Err(format!("unknown codec `{other}`")),
	}
}

fn i32_field(value: i64, field: &str) -> Result<i32, String> {
	i32::try_from(value).map_err(|_| format!("`{field}` is out of range"))
}

fn preview_frame(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let time = args.f64("time")?;
	let width = args.opt_usize("width")?.unwrap_or(640);
	let height = args.opt_usize("height")?.unwrap_or(360);
	if !(16..=7680).contains(&width) || !(16..=4320).contains(&height) {
		return Err("preview dimensions are out of range".to_string());
	}
	state.ensure_render()?;
	let entry = state.entry(&session_id)?;
	let sequence = match args.opt_id("sequence")? {
		Some(sequence) => sequence,
		None => current_sequence(&entry.session)?,
	};
	let project = entry.session.project();
	let mut pixels = render::render_frame_f32(
		&project,
		sequence,
		Rational::from_f64(time),
		(width as u32, height as u32),
	)
	.ok_or_else(|| {
		format!(
			"rendering failed at {time}s; check that the sequence has clips and the time is inside it"
		)
	})?;
	// The renderer works in the project's linear working space; the export path
	// applies the delivery transform, so preview must too or it looks crushed.
	quercus_engine_rs::utils::working_to_display_target(
		&mut pixels,
		&quercus_engine_rs::state::color_working_space(),
		&quercus_engine_rs::state::color_output_gamut(),
		&quercus_engine_rs::state::color_output_transfer(),
	);
	let png = image::encode_rgba_f32(width as u32, height as u32, &pixels)?;
	Ok(ToolReply::image(
		png,
		json!({
			"session": session_id,
			"sequence": id_json(sequence),
			"time": round(time),
			"width": width,
			"height": height,
		}),
	))
}

fn render_audio(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let start = args.f64("start")?;
	let end = args.f64("end")?;
	if end <= start {
		return Err("`end` must be greater than `start`".to_string());
	}
	let sample_rate = args.opt_usize("sample_rate")?.unwrap_or(48_000);
	if !(8_000..=192_000).contains(&sample_rate) {
		return Err("`sample_rate` is out of range".to_string());
	}
	let layout = match args.opt_str("channels").as_deref() {
		None | Some("stereo") => 0x3u64,
		Some("mono") => 0x4u64,
		Some(other) => return Err(format!("unknown `channels` value `{other}`")),
	};
	state.ensure_render()?;
	let entry = state.entry(&session_id)?;
	let sequence = match args.opt_id("sequence")? {
		Some(sequence) => sequence,
		None => current_sequence(&entry.session)?,
	};
	let project = entry.session.project();
	let task = task::render_audio(
		&project,
		sequence,
		quercus_engine_rs::types::TimeRange::new(
			Rational::from_f64(start),
			Rational::from_f64(end),
		),
		sample_rate as i32,
		layout,
	)
	.ok_or_else(|| "audio render could not start".to_string())?;
	if !task.wait() {
		return Err(format!("audio render failed: {}", task.error()));
	}
	let audio = task
		.take_audio()
		.ok_or_else(|| "audio render produced no buffer".to_string())?;
	let info = audio.info();
	let samples = audio.samples();
	let frames = audio.frame_count();
	let gain_db = args.opt_f64("gain_db")?.unwrap_or(0.0);
	let gain = 10f32.powf(gain_db as f32 / 20.0);
	let samples: Vec<f32> = if gain_db == 0.0 {
		samples
	} else {
		samples
			.iter()
			.map(|sample| (sample * gain).clamp(-1.0, 1.0))
			.collect()
	};
	let peak = samples
		.iter()
		.fold(0f32, |peak, sample| peak.max(sample.abs()));
	let rms = if samples.is_empty() {
		0.0
	} else {
		(samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt()
	};
	let wav = match args.opt_str("filename") {
		Some(path) => {
			write_wav(
				&path,
				&samples,
				info.sample_rate.max(sample_rate as i32) as u32,
				info.channel_count.max(1) as u16,
			)?;
			Some(path)
		}
		None => None,
	};
	Ok(ToolReply::json(json!({
		"session": session_id,
		"sequence": id_json(sequence),
		"range": { "start": round(start), "end": round(end) },
		"sample_rate": info.sample_rate,
		"channels": info.channel_count,
		"frames": frames,
		"peak": round(peak as f64),
		"rms": round(rms as f64),
		"peak_db": if peak > 0.0 { round(20.0 * (peak as f64).log10()) } else { -120.0 },
		"gain_db": round(gain_db),
		"wav": wav,
	})))
}

fn write_wav(path: &str, samples: &[f32], sample_rate: u32, channels: u16) -> Result<(), String> {
	let data_len = samples.len() * 2;
	let mut out = Vec::with_capacity(44 + data_len);
	out.extend_from_slice(b"RIFF");
	out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
	out.extend_from_slice(b"WAVEfmt ");
	out.extend_from_slice(&16u32.to_le_bytes());
	out.extend_from_slice(&1u16.to_le_bytes());
	out.extend_from_slice(&channels.to_le_bytes());
	out.extend_from_slice(&sample_rate.to_le_bytes());
	out.extend_from_slice(&(sample_rate * channels as u32 * 2).to_le_bytes());
	out.extend_from_slice(&(channels * 2).to_le_bytes());
	out.extend_from_slice(&16u16.to_le_bytes());
	out.extend_from_slice(b"data");
	out.extend_from_slice(&(data_len as u32).to_le_bytes());
	for &sample in samples {
		let value = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
		out.extend_from_slice(&value.to_le_bytes());
	}
	std::fs::write(path, out).map_err(|error| format!("cannot write `{path}`: {error}"))
}

fn export_start(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let filename = args
		.opt_str("filename")
		.or_else(|| args.opt_str("output"))
		.ok_or_else(|| "missing required argument `filename`".to_string())?;
	let spec = format_spec(&args.opt_str("format").unwrap_or_else(|| "mp4".to_string()))?;
	state.ensure_render()?;

	let entry = state.entry(&session_id)?;
	let sequence = match args.opt_id("sequence")? {
		Some(sequence) => sequence,
		None => current_sequence(&entry.session)?,
	};
	let snapshot = entry.session.snapshot();
	let sequence_snapshot = snapshot
		.sequences
		.iter()
		.find(|item| item.id == sequence)
		.ok_or_else(|| "sequence is not mounted in this project".to_string())?;
	let rate = sequence_snapshot.frame_rate;
	// `sequence.length` is the engine's cached length, which an SDK-only
	// producer never refreshes; fall back to (and prefer) the clip extent.
	let content_length = crate::convert::sequence_content_length(sequence_snapshot);
	let length = if content_length > sequence_snapshot.length {
		content_length
	} else {
		sequence_snapshot.length
	};
	if length.to_f64() <= 0.0 {
		return Err("the sequence is empty; place clips before exporting".to_string());
	}
	let default_width = if sequence_snapshot.width > 0 {
		sequence_snapshot.width as usize
	} else {
		1920
	};
	let default_height = if sequence_snapshot.height > 0 {
		sequence_snapshot.height as usize
	} else {
		1080
	};
	let width = args.opt_usize("width")?.unwrap_or(default_width);
	let height = args.opt_usize("height")?.unwrap_or(default_height);
	if width == 0 || height == 0 {
		return Err("export dimensions must be positive".to_string());
	}
	let video_enabled = args.opt_bool("video")?.unwrap_or(spec.has_video);
	let audio_enabled = args.opt_bool("audio")?.unwrap_or(spec.has_audio);
	let video_codec = match args.opt_str("video_codec") {
		Some(name) => codec_by_name(&name)?,
		None => spec.video_codec,
	};
	let audio_codec = match args.opt_str("audio_codec") {
		Some(name) => codec_by_name(&name)?,
		None => spec.audio_codec,
	};

	let mut params = EncodingParams {
		format: spec.format as i32,
		video_enabled,
		video_codec: video_codec as i32,
		video_width: i32_field(width as i64, "width")?,
		video_height: i32_field(height as i64, "height")?,
		// The encoder wants a frame *duration* (seconds per frame), not the rate.
		video_time_base_num: i32_field(rate.den, "frame_rate")?,
		video_time_base_den: i32_field(rate.num, "frame_rate")?,
		video_pixel_format: PixelFormat::U8 as i32,
		audio_enabled,
		audio_codec: audio_codec as i32,
		audio_sample_rate: 48_000,
		audio_channel_layout: 0x3,
		export_length_num: i32_field(length.num, "sequence length")?,
		export_length_den: i32_field(length.den, "sequence length")?,
		video_bit_rate: args.opt_i64("video_bitrate")?.unwrap_or(0),
		audio_bit_rate: args.opt_i64("audio_bitrate")?.unwrap_or(0),
		..EncodingParams::default()
	};

	let range = read_range(&args)?;
	if let Some((start, end)) = range {
		if end <= start {
			return Err("`end` must be greater than `start`".to_string());
		}
		params.has_custom_range = true;
		let start = Rational::from_f64(start);
		let end = Rational::from_f64(end);
		params.custom_range_in_num = i32_field(start.num, "range start")?;
		params.custom_range_in_den = i32_field(start.den, "range start")?;
		params.custom_range_out_num = i32_field(end.num, "range end")?;
		params.custom_range_out_den = i32_field(end.den, "range end")?;
	}

	let project = entry.session.project();
	let task = task::export(&project, sequence, &filename, &params)
		.ok_or_else(|| "export could not start: the engine rejected the parameters".to_string())?;
	let task_id = state.add_task(task, "export", filename.clone(), Some(session_id.clone()));
	Ok(ToolReply::json(json!({
		"session": session_id,
		"task": task_id,
		"filename": filename,
		"format": args.opt_str("format").unwrap_or_else(|| "mp4".to_string()),
		"width": width,
		"height": height,
		"video": video_enabled,
		"audio": audio_enabled,
		"length_seconds": round(length.to_f64()),
		"range": range.map(|(start, end)| json!({ "start": round(start), "end": round(end) })),
		"next": "call task_wait or task_status with this task id",
	})))
}

fn read_range(args: &Args<'_>) -> Result<Option<(f64, f64)>, String> {
	let mut start = args.opt_f64("start")?;
	let mut end = args.opt_f64("end")?;
	if let Some(range) = args.raw().get("range").filter(|value| !value.is_null()) {
		let object = range
			.as_object()
			.ok_or_else(|| "`range` must be an object".to_string())?;
		if let Some(value) = object.get("start") {
			start = Some(
				value
					.as_f64()
					.ok_or_else(|| "`range.start` must be a number".to_string())?,
			);
		}
		if let Some(value) = object.get("end") {
			end = Some(
				value
					.as_f64()
					.ok_or_else(|| "`range.end` must be a number".to_string())?,
			);
		}
	}
	match (start, end) {
		(None, None) => Ok(None),
		(Some(start), Some(end)) => Ok(Some((start, end))),
		_ => Err("a range needs both `start` and `end`".to_string()),
	}
}

fn task_status(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let task_id = args.str("task")?;
	let entry = state.task(&task_id)?;
	Ok(ToolReply::json(task_json(entry)))
}

fn task_wait(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let task_id = args.str("task")?;
	let timeout_ms = args
		.opt_i64("timeout_ms")?
		.unwrap_or(30_000)
		.clamp(0, 600_000) as u64;
	let deadline = Instant::now() + Duration::from_millis(timeout_ms);
	loop {
		let entry = state.task(&task_id)?;
		let finished = entry.task.state() != TaskState::Running;
		if finished || Instant::now() >= deadline {
			return Ok(ToolReply::json(task_json(entry)));
		}
		std::thread::sleep(Duration::from_millis(20));
	}
}

fn task_cancel(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let task_id = args.str("task")?;
	let entry = state.task(&task_id)?;
	entry.task.cancel();
	Ok(ToolReply::json(task_json(entry)))
}

fn task_list(state: &mut ServerState, _arguments: Value) -> Result<ToolReply, String> {
	let tasks: Vec<Value> = state.task_entries().map(task_json).collect();
	Ok(ToolReply::json(
		json!({ "count": tasks.len(), "tasks": tasks }),
	))
}

fn otio_export(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let filename = args.str("filename")?;
	let entry = state.entry(&session_id)?;
	let project = entry.session.project();
	let task = task::save_otio(&project, &filename)
		.ok_or_else(|| "OTIO export could not start".to_string())?;
	let task_id = state.add_task(
		task,
		"otio_save",
		filename.clone(),
		Some(session_id.clone()),
	);
	Ok(ToolReply::json(json!({
		"session": session_id,
		"task": task_id,
		"filename": filename,
	})))
}

fn otio_import(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let filename = args.str("filename")?;
	let path = std::fs::canonicalize(&filename)
		.map_err(|error| format!("interchange file is not readable: `{filename}`: {error}"))?
		.to_string_lossy()
		.into_owned();
	let task = task::load_otio(&path).ok_or_else(|| "OTIO import could not start".to_string())?;
	if !task.wait() {
		return Err(format!("OTIO import failed: {}", task.error()));
	}
	let project = task
		.take_project()
		.ok_or_else(|| "OTIO import produced no project".to_string())?;
	let session = EditorSession::from_project(project)
		.map_err(|error| format!("session setup failed: {error}"))?;
	let snapshot = session.snapshot();
	let label = snapshot.name.clone();
	let session_id = state.add_session(session, label.clone(), None);
	Ok(ToolReply::json(json!({
		"session": session_id,
		"path": path,
		"name": label,
		"sequences": snapshot.sequences.len(),
		"footages": snapshot.footages.len(),
	})))
}
