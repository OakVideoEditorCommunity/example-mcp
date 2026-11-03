//! Effect-chain and parameter tools.

use quercus_engine_rs::edit::EditCommand;
use quercus_engine_rs::editor::{GraphNodeRole, KeyframeInterpolation, ParameterValueType};
use quercus_engine_rs::effect;
use quercus_engine_rs::node::NodeValue;
use quercus_engine_rs::types::Rational;
use serde_json::{Value, json};

use super::{Args, ToolDef, ToolReply, resolve_session};
use crate::convert::{id_json, rational_json, round};
use crate::state::ServerState;

/// The tools in this module.
pub fn tools() -> Vec<ToolDef> {
	vec![
		ToolDef {
			name: "effect_types",
			title: "List effect types",
			description: "List the built-in effect/generator type ids that effect_add accepts.",
			read_only: true,
			destructive: false,
			input_schema: || json!({"type": "object", "properties": {}}),
			handler: effect_types,
		},
		ToolDef {
			name: "effect_add",
			title: "Add effect",
			description: "Insert an effect into a clip's effect chain. Use effect_types for type ids.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"host": {"type": "string", "description": "Clip (or other host node) id"},
						"type_id": {"type": "string", "description": "Effect type id from effect_types"},
						"index": {"type": "integer", "minimum": 0, "description": "Chain position (default: append)"}
					},
					"required": ["host", "type_id"]
				})
			},
			handler: effect_add,
		},
		ToolDef {
			name: "effect_remove",
			title: "Remove effect",
			description: "Remove an effect from a chain.",
			read_only: false,
			destructive: true,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"host": {"type": "string", "description": "Clip that owns the effect"},
						"effect": {"type": "string", "description": "Effect node id"}
					},
					"required": ["host", "effect"]
				})
			},
			handler: effect_remove,
		},
		ToolDef {
			name: "effect_enable",
			title: "Enable effect",
			description: "Enable or bypass an effect.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"effect": {"type": "string", "description": "Effect node id"},
						"enabled": {"type": "boolean"}
					},
					"required": ["effect", "enabled"]
				})
			},
			handler: effect_enable,
		},
		ToolDef {
			name: "effect_chain",
			title: "Read effect chain",
			description: "List the effect chain of a clip in signal order (closest to source first).",
			read_only: true,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"host": {"type": "string", "description": "Clip or other host node id"}
					},
					"required": ["host"]
				})
			},
			handler: effect_chain,
		},
		ToolDef {
			name: "effect_params",
			title: "Read effect parameters",
			description: "Read a node's declared parameters, static values, and keyframes.",
			read_only: true,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"node": {"type": "string", "description": "Effect or clip node id"}
					},
					"required": ["node"]
				})
			},
			handler: effect_params,
		},
		ToolDef {
			name: "effect_set_param",
			title: "Set effect parameter",
			description: "Set a static parameter value, or insert a keyframe when `time` is given. \
				Values are converted to the parameter's declared type (float, int, bool, rational, \
				color, vec2/3/4, text). Use effect_params to discover input ids and types.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"node": {"type": "string", "description": "Effect or clip node id"},
						"input_id": {"type": "string", "description": "Parameter input id"},
						"value": {
							"description": "Number, boolean, string, or array (color/vector); rationals accept [num, den]"
						},
						"element": {"type": "integer", "description": "Element index; -1 for scalar inputs (default -1)"},
						"time": {"type": "number", "description": "Node-local keyframe time in seconds; omit for a static value"},
						"interpolation": {"type": "string", "enum": ["linear", "hold", "bezier"], "description": "Keyframe interpolation (default linear)"},
						"bezier_in": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2},
						"bezier_out": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2},
						"value_type": {"type": "string", "enum": ["float", "int", "bool", "rational", "color", "vec2", "vec3", "vec4"], "description": "Override type inference for unknown parameters"}
					},
					"required": ["node", "input_id", "value"]
				})
			},
			handler: effect_set_param,
		},
		ToolDef {
			name: "effect_keyframe",
			title: "Upsert keyframe",
			description: "Insert or replace a keyframe at an exact node-local time in seconds.",
			read_only: false,
			destructive: false,
			input_schema: || {
				json!({
					"type": "object",
					"properties": {
						"session": {"type": "string", "description": "Session id (defaults to active)"},
						"node": {"type": "string"},
						"input_id": {"type": "string"},
						"time": {"type": "number", "description": "Node-local time in seconds"},
						"value": {},
						"element": {"type": "integer", "description": "Default -1"},
						"interpolation": {"type": "string", "enum": ["linear", "hold", "bezier"]},
						"bezier_in": {"type": "array", "items": {"type": "number"}},
						"bezier_out": {"type": "array", "items": {"type": "number"}},
						"value_type": {"type": "string", "enum": ["float", "int", "bool", "rational", "color", "vec2", "vec3", "vec4"]}
					},
					"required": ["node", "input_id", "time", "value"]
				})
			},
			handler: effect_keyframe,
		},
	]
}

#[derive(Clone, Copy)]
enum ValueKind {
	Int,
	Float,
	Bool,
	Rational,
	Color,
	Vec2,
	Vec3,
	Vec4,
}

fn value_type_name(value_type: ParameterValueType) -> String {
	match value_type {
		ParameterValueType::None => "none".to_string(),
		ParameterValueType::Int => "int".to_string(),
		ParameterValueType::Float => "float".to_string(),
		ParameterValueType::Rational => "rational".to_string(),
		ParameterValueType::Boolean => "bool".to_string(),
		ParameterValueType::Color => "color".to_string(),
		ParameterValueType::Matrix => "matrix".to_string(),
		ParameterValueType::Text => "text".to_string(),
		ParameterValueType::Texture => "texture".to_string(),
		ParameterValueType::Samples => "samples".to_string(),
		ParameterValueType::Vec2 => "vec2".to_string(),
		ParameterValueType::Vec3 => "vec3".to_string(),
		ParameterValueType::Vec4 => "vec4".to_string(),
		ParameterValueType::Combo => "combo".to_string(),
		ParameterValueType::StrCombo => "strcombo".to_string(),
		ParameterValueType::VideoParams => "video_params".to_string(),
		ParameterValueType::AudioParams => "audio_params".to_string(),
		ParameterValueType::Binary => "binary".to_string(),
		ParameterValueType::PushButton => "push_button".to_string(),
		ParameterValueType::NodeRef => "node_ref".to_string(),
		ParameterValueType::Parametric => "parametric".to_string(),
		ParameterValueType::Other(code) => format!("other({code})"),
	}
}

fn kind_for(value_type: ParameterValueType) -> Result<ValueKind, String> {
	match value_type {
		ParameterValueType::Int | ParameterValueType::Combo => Ok(ValueKind::Int),
		ParameterValueType::Float => Ok(ValueKind::Float),
		ParameterValueType::Boolean => Ok(ValueKind::Bool),
		ParameterValueType::Rational => Ok(ValueKind::Rational),
		ParameterValueType::Color => Ok(ValueKind::Color),
		ParameterValueType::Vec2 => Ok(ValueKind::Vec2),
		ParameterValueType::Vec3 => Ok(ValueKind::Vec3),
		ParameterValueType::Vec4 => Ok(ValueKind::Vec4),
		other => Err(format!(
			"parameter type `{}` cannot be set through effect_set_param",
			value_type_name(other)
		)),
	}
}

fn parse_hint(hint: &str) -> Result<ValueKind, String> {
	match hint.trim().to_ascii_lowercase().as_str() {
		"int" | "integer" | "combo" => Ok(ValueKind::Int),
		"float" | "double" => Ok(ValueKind::Float),
		"bool" | "boolean" => Ok(ValueKind::Bool),
		"rational" => Ok(ValueKind::Rational),
		"color" => Ok(ValueKind::Color),
		"vec2" => Ok(ValueKind::Vec2),
		"vec3" => Ok(ValueKind::Vec3),
		"vec4" => Ok(ValueKind::Vec4),
		other => Err(format!("unknown `value_type` `{other}`")),
	}
}

fn infer_kind(value: &Value) -> Result<ValueKind, String> {
	match value {
		Value::Bool(_) => Ok(ValueKind::Bool),
		Value::Number(_) => Ok(ValueKind::Float),
		Value::Array(items) => match items.len() {
			2 => Ok(ValueKind::Vec2),
			3 => Ok(ValueKind::Vec3),
			4 => Ok(ValueKind::Color),
			other => Err(format!(
				"arrays of length {other} are not a supported value"
			)),
		},
		Value::String(_) => Err("string values are applied as text parameters".to_string()),
		_ => Err("unsupported value".to_string()),
	}
}

fn float_array(value: &Value, length: usize, field: &str) -> Result<Vec<f64>, String> {
	let items = value
		.as_array()
		.ok_or_else(|| format!("`{field}` must be an array of {length} numbers"))?;
	if items.len() != length {
		return Err(format!("`{field}` must have {length} numbers"));
	}
	items
		.iter()
		.map(|item| {
			item.as_f64()
				.ok_or_else(|| format!("`{field}` must contain only numbers"))
		})
		.collect()
}

fn make_node_value(kind: ValueKind, value: &Value) -> Result<NodeValue, String> {
	match kind {
		ValueKind::Int => {
			Ok(NodeValue::int(value.as_i64().ok_or_else(|| {
				"this parameter expects an integer `value`".to_string()
			})?))
		}
		ValueKind::Float => {
			Ok(NodeValue::float(value.as_f64().ok_or_else(|| {
				"this parameter expects a numeric `value`".to_string()
			})?))
		}
		ValueKind::Bool => {
			Ok(NodeValue::bool(value.as_bool().ok_or_else(|| {
				"this parameter expects a boolean `value`".to_string()
			})?))
		}
		ValueKind::Rational => {
			if let Some(seconds) = value.as_f64() {
				return Ok(NodeValue::rational(Rational::from_f64(seconds)));
			}
			let pair = float_array(value, 2, "value")?;
			Ok(NodeValue::rational(Rational::new(
				pair[0] as i64,
				pair[1] as i64,
			)))
		}
		ValueKind::Color => {
			let color = float_array(value, 4, "value")?;
			Ok(NodeValue {
				kind: 5,
				num: 0,
				den: 0,
				f: [color[0], color[1], color[2], color[3]],
			})
		}
		ValueKind::Vec2 => {
			let vector = float_array(value, 2, "value")?;
			Ok(NodeValue {
				kind: 6,
				num: 0,
				den: 0,
				f: [vector[0], vector[1], 0.0, 0.0],
			})
		}
		ValueKind::Vec3 => {
			let vector = float_array(value, 3, "value")?;
			Ok(NodeValue {
				kind: 7,
				num: 0,
				den: 0,
				f: [vector[0], vector[1], vector[2], 0.0],
			})
		}
		ValueKind::Vec4 => {
			let vector = float_array(value, 4, "value")?;
			Ok(NodeValue {
				kind: 8,
				num: 0,
				den: 0,
				f: [vector[0], vector[1], vector[2], vector[3]],
			})
		}
	}
}

fn parse_interpolation(text: Option<String>) -> Result<KeyframeInterpolation, String> {
	match text
		.as_deref()
		.map(str::trim)
		.map(str::to_ascii_lowercase)
		.as_deref()
	{
		None | Some("linear") => Ok(KeyframeInterpolation::Linear),
		Some("hold") | Some("constant") => Ok(KeyframeInterpolation::Hold),
		Some("bezier") => Ok(KeyframeInterpolation::Bezier),
		Some(other) => Err(format!(
			"unknown interpolation `{other}`; expected linear, hold, or bezier"
		)),
	}
}

fn bezier_handle(args: &Args<'_>, key: &str) -> Result<(f64, f64), String> {
	let Some(value) = args.raw().get(key).filter(|value| !value.is_null()) else {
		return Ok((0.0, 0.0));
	};
	let pair = float_array(value, 2, key)?;
	Ok((pair[0], pair[1]))
}

fn node_value_json(value: NodeValue) -> Value {
	json!({
		"kind": value.kind,
		"num": value.num,
		"den": value.den,
		"f": value.f.to_vec(),
	})
}

fn effect_types(_state: &mut ServerState, _arguments: Value) -> Result<ToolReply, String> {
	let mut entries: Vec<Value> = effect::factory_entries()
		.iter()
		.map(|entry| {
			json!({
				"type_id": entry.type_id,
				"name": entry.name,
				"category": entry.category.map(|category| format!("{category:?}")),
				"flags": entry.flags,
			})
		})
		.collect();
	for entry in effect::dynamic_factory_entries() {
		entries.push(json!({
			"type_id": entry.type_id,
			"name": entry.name,
			"category": entry.sub_category,
			"dynamic": true,
		}));
	}
	Ok(ToolReply::json(json!({
		"count": entries.len(),
		"effects": entries,
	})))
}

fn effect_add(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let host = args.id("host")?;
	let type_id = args.str("type_id")?;
	let index = args.opt_usize("index")?.unwrap_or(usize::MAX);
	let entry = state.entry(&session_id)?;
	let result = entry
		.session
		.execute(EditCommand::EffectInsert {
			host,
			index,
			type_id: type_id.clone(),
		})
		.map_err(|error| format!("add effect failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"host": id_json(host),
		"effect": result.primary.map(id_json),
		"type_id": type_id,
		"revision": result.revision,
	})))
}

fn effect_remove(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let host = args.id("host")?;
	let effect = args.id("effect")?;
	let entry = state.entry(&session_id)?;
	let result = entry
		.session
		.execute(EditCommand::EffectRemove { host, effect })
		.map_err(|error| format!("remove effect failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"host": id_json(host),
		"effect": id_json(effect),
		"revision": result.revision,
	})))
}

fn effect_enable(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let effect = args.id("effect")?;
	let enabled = args.bool("enabled")?;
	let entry = state.entry(&session_id)?;
	let result = entry
		.session
		.execute(EditCommand::EffectSetEnabled { effect, enabled })
		.map_err(|error| format!("set effect enabled failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"effect": id_json(effect),
		"enabled": enabled,
		"revision": result.revision,
	})))
}

fn effect_chain(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let host = args.id("host")?;
	let entry = state.entry(&session_id)?;
	let sequence = match args.opt_id("sequence")? {
		Some(sequence) => sequence,
		None => super::current_sequence(&entry.session)?,
	};
	let project = entry.session.project();
	// The engine's chain includes the upstream source node; the graph snapshot
	// supplies roles so the source can be reported separately from effects.
	let graph = entry
		.session
		.graph_snapshot(sequence, Some(host))
		.ok_or_else(|| "cannot read the node graph for this clip".to_string())?;
	let mut source = Value::Null;
	let mut effects = Vec::new();
	for id in project.effect_chain(host) {
		let Some(node) = graph.nodes.iter().find(|node| node.id == id) else {
			continue;
		};
		let label = if node.label.is_empty() {
			effect::factory_name(&node.type_id).unwrap_or_default()
		} else {
			node.label.clone()
		};
		let entry = json!({
			"node": id_json(id),
			"type_id": node.type_id,
			"name": label,
			"enabled": node.enabled,
		});
		if node.role == GraphNodeRole::Effect {
			effects.push(entry);
		} else if source.is_null() {
			source = entry;
		}
	}
	Ok(ToolReply::json(json!({
		"session": session_id,
		"host": id_json(host),
		"source": source,
		"count": effects.len(),
		"effects": effects,
	})))
}

fn effect_params(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let node = args.id("node")?;
	let entry = state.entry(&session_id)?;
	let snapshot = entry
		.session
		.parameters(node)
		.ok_or_else(|| format!("node `{}` has no parameters", node.raw()))?;
	let parameters: Vec<Value> = snapshot
		.params
		.iter()
		.map(|parameter| {
			json!({
				"input_id": parameter.input_id,
				"label": parameter.label,
				"type": value_type_name(parameter.value_type),
				"flags": parameter.flags,
				"values": parameter.values.iter().map(|(element, value)| json!({
					"element": element,
					"pod": value.pod.map(node_value_json),
					"text": value.text.as_deref(),
				})).collect::<Vec<_>>(),
				"keyframes": parameter.keyframes.iter().map(|(element, keys)| json!({
					"element": element,
					"keys": keys.iter().map(|key| json!({
						"time": rational_json(key.time),
						"pod": key.value.pod.map(node_value_json),
						"text": key.value.text.as_deref(),
						"interpolation": format!("{:?}", key.interpolation).to_ascii_lowercase(),
						"bezier_in": [key.bezier_in.0, key.bezier_in.1],
						"bezier_out": [key.bezier_out.0, key.bezier_out.1],
					})).collect::<Vec<_>>(),
				})).collect::<Vec<_>>(),
			})
		})
		.collect();
	Ok(ToolReply::json(json!({
		"session": session_id,
		"node": id_json(node),
		"revision": snapshot.revision,
		"parameters": parameters,
	})))
}

fn effect_set_param(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let node = args.id("node")?;
	let input_id = args.str("input_id")?;
	let element = args.opt_i64("element")?.unwrap_or(-1) as i32;
	let value = args
		.raw()
		.get("value")
		.ok_or_else(|| "missing required argument `value`".to_string())?;
	let time = args.opt_f64("time")?;
	let entry = state.entry(&session_id)?;

	let declared = entry.session.parameters(node).and_then(|snapshot| {
		snapshot
			.get(&input_id)
			.map(|parameter| parameter.value_type)
	});

	if matches!(declared, Some(ParameterValueType::PushButton)) {
		let result = entry
			.session
			.execute(EditCommand::PushNodeButton {
				node,
				input_id: input_id.clone(),
			})
			.map_err(|error| format!("press parameter button failed: {error}"))?;
		return Ok(ToolReply::json(json!({
			"session": session_id,
			"node": id_json(node),
			"input_id": input_id,
			"pushed": true,
			"revision": result.revision,
		})));
	}

	let is_text = match declared {
		Some(ParameterValueType::Text | ParameterValueType::StrCombo) => true,
		Some(_) => false,
		None => value.is_string(),
	};
	if is_text {
		let text = value
			.as_str()
			.ok_or_else(|| "this parameter expects a string `value`".to_string())?;
		let result = entry
			.session
			.execute(EditCommand::SetNodeTextValue {
				node,
				input_id: input_id.clone(),
				element,
				value: text.to_string(),
			})
			.map_err(|error| format!("set text parameter failed: {error}"))?;
		return Ok(ToolReply::json(json!({
			"session": session_id,
			"node": id_json(node),
			"input_id": input_id,
			"text": text,
			"revision": result.revision,
		})));
	}

	let kind = match args.opt_str("value_type") {
		Some(hint) => parse_hint(&hint)?,
		None => match declared {
			Some(value_type) => kind_for(value_type)?,
			None => infer_kind(value)?,
		},
	};
	let node_value = make_node_value(kind, value)?;
	let interpolation = parse_interpolation(args.opt_str("interpolation"))?;
	let bezier_in = bezier_handle(&args, "bezier_in")?;
	let bezier_out = bezier_handle(&args, "bezier_out")?;

	match time {
		Some(seconds) => {
			let result = entry
				.session
				.execute(EditCommand::UpsertNodeKeyframe {
					node,
					input_id: input_id.clone(),
					element,
					time: Rational::from_f64(seconds),
					value: node_value,
					interpolation,
					bezier_in,
					bezier_out,
				})
				.map_err(|error| format!("set keyframe failed: {error}"))?;
			Ok(ToolReply::json(json!({
				"session": session_id,
				"node": id_json(node),
				"input_id": input_id,
				"element": element,
				"keyframe_time": round(seconds),
				"revision": result.revision,
			})))
		}
		None => {
			let result = entry
				.session
				.execute(EditCommand::SetNodeStandardValue {
					node,
					input_id: input_id.clone(),
					element,
					value: node_value,
				})
				.map_err(|error| format!("set parameter failed: {error}"))?;
			Ok(ToolReply::json(json!({
				"session": session_id,
				"node": id_json(node),
				"input_id": input_id,
				"element": element,
				"revision": result.revision,
			})))
		}
	}
}

fn effect_keyframe(state: &mut ServerState, arguments: Value) -> Result<ToolReply, String> {
	let args = Args::new(&arguments);
	let session_id = resolve_session(state, &args)?;
	let node = args.id("node")?;
	let input_id = args.str("input_id")?;
	let element = args.opt_i64("element")?.unwrap_or(-1) as i32;
	let seconds = args.f64("time")?;
	let value = args
		.raw()
		.get("value")
		.ok_or_else(|| "missing required argument `value`".to_string())?;
	let entry = state.entry(&session_id)?;
	let declared = entry.session.parameters(node).and_then(|snapshot| {
		snapshot
			.get(&input_id)
			.map(|parameter| parameter.value_type)
	});
	let kind = match args.opt_str("value_type") {
		Some(hint) => parse_hint(&hint)?,
		None => match declared {
			Some(value_type) => kind_for(value_type)?,
			None => infer_kind(value)?,
		},
	};
	let node_value = make_node_value(kind, value)?;
	let result = entry
		.session
		.execute(EditCommand::UpsertNodeKeyframe {
			node,
			input_id: input_id.clone(),
			element,
			time: Rational::from_f64(seconds),
			value: node_value,
			interpolation: parse_interpolation(args.opt_str("interpolation"))?,
			bezier_in: bezier_handle(&args, "bezier_in")?,
			bezier_out: bezier_handle(&args, "bezier_out")?,
		})
		.map_err(|error| format!("upsert keyframe failed: {error}"))?;
	Ok(ToolReply::json(json!({
		"session": session_id,
		"node": id_json(node),
		"input_id": input_id,
		"element": element,
		"time": round(seconds),
		"revision": result.revision,
	})))
}
