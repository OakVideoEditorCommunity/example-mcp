// SPDX-License-Identifier: GPL-3.0-or-later
//! Sidecar persistence for engine data the `.ove` serializer drops.
//!
//! The published engine's project serializer writes the graph (nodes, clips,
//! connections) but not a sequence's video format or its markers: after a
//! save/reopen, sequences come back at the default 1920x1080@29.97 and markers
//! are gone. `oak-mcp` therefore writes a small JSON sidecar next to the
//! project (`<project>.oak-mcp.json`) and re-applies the missing values when it
//! reopens that project. Other consumers are unaffected: they simply ignore the
//! sidecar.

use std::path::PathBuf;

use quercus_engine_rs::edit::EditCommand;
use quercus_engine_rs::editor::{EditorSession, ProjectSnapshot};
use quercus_engine_rs::types::Rational;
use serde::{Deserialize, Serialize};

/// Sidecar schema version.
pub const VERSION: u32 = 1;

/// Data that must survive a save/reopen round trip.
#[derive(Debug, Serialize, Deserialize)]
pub struct Sidecar {
	/// Schema version; readers ignore unknown future versions.
	pub version: u32,
	/// Sequence formats, matched back by sequence name.
	pub sequences: Vec<SequenceMeta>,
	/// Markers of the current sequence at save time.
	#[serde(default)]
	pub markers: Vec<MarkerMeta>,
	/// Name of the sequence the markers belong to.
	#[serde(default)]
	pub marker_sequence: Option<String>,
}

/// One sequence's video format.
#[derive(Debug, Serialize, Deserialize)]
pub struct SequenceMeta {
	/// Sequence name (the engine's reload identity is the parsed name).
	pub name: String,
	/// Frame width in pixels.
	pub width: i32,
	/// Frame height in pixels.
	pub height: i32,
	/// Frame rate numerator.
	pub fps_num: i64,
	/// Frame rate denominator.
	pub fps_den: i64,
	/// Whether frames are interlaced.
	pub interlaced: bool,
}

/// One sequence-local marker.
#[derive(Debug, Serialize, Deserialize)]
pub struct MarkerMeta {
	/// Marker label.
	pub name: String,
	/// Marker time in seconds.
	pub seconds: f64,
	/// Marker color index.
	pub color: i32,
}

/// The sidecar path for a project file.
pub fn sidecar_path(project: &str) -> PathBuf {
	PathBuf::from(format!("{project}.oak-mcp.json"))
}

/// Writes (or refreshes) the sidecar for a project.
pub fn write(project: &str, snapshot: &ProjectSnapshot) -> Result<PathBuf, String> {
	let sequences = snapshot
		.sequences
		.iter()
		.map(|sequence| SequenceMeta {
			name: sequence.name.clone(),
			width: sequence.width,
			height: sequence.height,
			fps_num: sequence.frame_rate.num,
			fps_den: sequence.frame_rate.den,
			interlaced: sequence.interlaced,
		})
		.collect();
	let marker_sequence = snapshot
		.current_sequence
		.and_then(|id| snapshot.sequences.iter().find(|sequence| sequence.id == id))
		.map(|sequence| sequence.name.clone())
		.or_else(|| {
			snapshot
				.sequences
				.first()
				.map(|sequence| sequence.name.clone())
		});
	let markers = snapshot
		.markers
		.iter()
		.map(|marker| MarkerMeta {
			name: marker.name.clone(),
			seconds: marker.time.in_.to_f64(),
			color: marker.color,
		})
		.collect();
	let sidecar = Sidecar {
		version: VERSION,
		sequences,
		markers,
		marker_sequence,
	};
	let path = sidecar_path(project);
	let text = serde_json::to_string_pretty(&sidecar)
		.map_err(|error| format!("cannot serialize the sidecar: {error}"))?;
	std::fs::write(&path, text)
		.map_err(|error| format!("cannot write `{}`: {error}", path.display()))?;
	Ok(path)
}

/// Reads a project's sidecar, if it exists and parses.
pub fn read(project: &str) -> Option<Sidecar> {
	let text = std::fs::read_to_string(sidecar_path(project)).ok()?;
	let sidecar: Sidecar = serde_json::from_str(&text).ok()?;
	(sidecar.version <= VERSION).then_some(sidecar)
}

/// Re-applies the sidecar's format and markers to a freshly opened session.
///
/// Returns human-readable warnings for values that could not be restored; the
/// project still opens in that case.
pub fn restore(session: &EditorSession, sidecar: &Sidecar) -> Vec<String> {
	let mut warnings = Vec::new();
	let original_current = session.current_sequence();
	let snapshot = session.snapshot();

	for meta in &sidecar.sequences {
		let Some(sequence) = snapshot
			.sequences
			.iter()
			.find(|sequence| sequence.name == meta.name)
		else {
			warnings.push(format!(
				"sidecar names sequence `{}`, which is not in the project",
				meta.name
			));
			continue;
		};
		let rate = Rational::new(meta.fps_num.max(1), meta.fps_den.max(1));
		if let Err(error) = session.execute(EditCommand::SetSequenceParameters {
			sequence: sequence.id,
			name: sequence.name.clone(),
			width: meta.width,
			height: meta.height,
			rate,
			interlaced: meta.interlaced,
		}) {
			warnings.push(format!(
				"could not restore the format of sequence `{}`: {error}",
				meta.name
			));
		}
	}

	if let Some(name) = &sidecar.marker_sequence {
		if let Some(sequence) = snapshot
			.sequences
			.iter()
			.find(|sequence| &sequence.name == name)
		{
			if !sidecar.markers.is_empty() {
				session.set_current_sequence(sequence.id);
				for marker in &sidecar.markers {
					if let Err(error) = session.execute(EditCommand::MarkerAdd {
						time: Rational::from_f64(marker.seconds),
						name: marker.name.clone(),
						color: marker.color,
					}) {
						warnings.push(format!(
							"could not restore marker `{}`: {error}",
							marker.name
						));
					}
				}
			}
		}
	}

	if let Some(sequence) = original_current {
		session.set_current_sequence(sequence);
	}
	warnings
}
