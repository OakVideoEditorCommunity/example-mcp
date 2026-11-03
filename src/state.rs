//! Process-wide server state: editor sessions and asynchronous tasks.

use std::collections::HashMap;
use std::time::Instant;

use quercus_engine_rs::editor::EditorSession;
use quercus_engine_rs::render::{self, RenderBackend};
use quercus_engine_rs::task::{Task, TaskState};
use serde_json::{Value, json};

use crate::convert::round;

/// One live editor session and the metadata the MCP layer tracks for it.
pub struct SessionEntry {
	/// Stable MCP session id (`s1`, `s2`, ...).
	pub id: String,
	/// The engine session; dropping it releases engine-side state.
	pub session: EditorSession,
	/// Display label (project name or file stem).
	pub label: String,
	/// Path the project was opened from / last saved to.
	pub path: Option<String>,
}

/// One asynchronous engine task owned by the server.
pub struct TaskEntry {
	/// Stable MCP task id (`t1`, `t2`, ...).
	pub id: String,
	/// The engine task handle; dropping it cancels and joins a running task.
	pub task: Task,
	/// `export`, `otio_save`, ...
	pub kind: &'static str,
	/// Human-readable description (usually the output path).
	pub label: String,
	/// Session the task was started from, when applicable.
	pub session: Option<String>,
	/// When the task was started, for elapsed-time reporting.
	pub created: Instant,
}

/// All mutable server state. The server is single-threaded: requests are
/// handled sequentially, so `&mut` access is enough and no locks are needed.
pub struct ServerState {
	sessions: HashMap<String, SessionEntry>,
	order: Vec<String>,
	active: Option<String>,
	next_session: u64,
	tasks: HashMap<String, TaskEntry>,
	task_order: Vec<String>,
	next_task: u64,
	/// Negotiated MCP protocol version.
	pub protocol_version: String,
	/// MCP logging level requested by the client.
	pub log_level: String,
	/// Whether stderr diagnostics are enabled.
	pub verbose: bool,
	/// When the server started, for the info resource.
	pub started: Instant,
	render_ready: bool,
}

impl ServerState {
	/// Creates empty state.
	pub fn new() -> Self {
		Self {
			sessions: HashMap::new(),
			order: Vec::new(),
			active: None,
			next_session: 0,
			tasks: HashMap::new(),
			task_order: Vec::new(),
			next_task: 0,
			protocol_version: "2025-06-18".to_string(),
			log_level: "info".to_string(),
			verbose: false,
			started: Instant::now(),
			render_ready: false,
		}
	}

	/// Initializes the engine render manager on first use.
	pub fn ensure_render(&mut self) -> Result<(), String> {
		if self.render_ready && render::manager_is_initialized() {
			return Ok(());
		}
		let _ = render::manager_init(RenderBackend::Threads);
		if !render::manager_is_initialized() {
			return Err("failed to initialize the Quercus render manager".to_string());
		}
		self.render_ready = true;
		Ok(())
	}

	/// Registers a new session and makes it active.
	pub fn add_session(
		&mut self,
		session: EditorSession,
		label: String,
		path: Option<String>,
	) -> String {
		self.next_session += 1;
		let id = format!("s{}", self.next_session);
		self.sessions.insert(
			id.clone(),
			SessionEntry {
				id: id.clone(),
				session,
				label,
				path,
			},
		);
		self.order.push(id.clone());
		self.active = Some(id.clone());
		id
	}

	/// The active session id, if any.
	pub fn active_session_id(&self) -> Option<&str> {
		self.active.as_deref()
	}

	/// Resolves an optional client-supplied session id against the active one.
	pub fn resolve_session(&self, requested: Option<&str>) -> Result<String, String> {
		match requested {
			Some(id) => {
				if self.sessions.contains_key(id) {
					Ok(id.to_string())
				} else {
					Err(format!(
						"unknown session `{id}`; use session_list to inspect open sessions"
					))
				}
			}
			None => self.active.clone().ok_or_else(|| {
				"no active session: call project_new or project_open first".to_string()
			}),
		}
	}

	/// Looks up one session.
	pub fn entry(&self, id: &str) -> Result<&SessionEntry, String> {
		self.sessions
			.get(id)
			.ok_or_else(|| format!("unknown session `{id}`"))
	}

	/// Looks up one session mutably (for saving or path updates).
	pub fn entry_mut(&mut self, id: &str) -> Result<&mut SessionEntry, String> {
		self.sessions
			.get_mut(id)
			.ok_or_else(|| format!("unknown session `{id}`"))
	}

	/// Makes an existing session active.
	pub fn set_active(&mut self, id: &str) -> Result<(), String> {
		if self.sessions.contains_key(id) {
			self.active = Some(id.to_string());
			Ok(())
		} else {
			Err(format!("unknown session `{id}`"))
		}
	}

	/// Iterates sessions in creation order.
	pub fn session_entries(&self) -> impl Iterator<Item = &SessionEntry> {
		self.order.iter().filter_map(|id| self.sessions.get(id))
	}

	/// Removes a session, resetting the active pointer when needed.
	pub fn remove_session(&mut self, id: &str) -> bool {
		if self.sessions.remove(id).is_none() {
			return false;
		}
		self.order.retain(|entry| entry != id);
		if self.active.as_deref() == Some(id) {
			self.active = self.order.last().cloned();
		}
		true
	}

	/// Registers a task and returns its MCP id.
	pub fn add_task(
		&mut self,
		task: Task,
		kind: &'static str,
		label: String,
		session: Option<String>,
	) -> String {
		self.next_task += 1;
		let id = format!("t{}", self.next_task);
		self.tasks.insert(
			id.clone(),
			TaskEntry {
				id: id.clone(),
				task,
				kind,
				label,
				session,
				created: Instant::now(),
			},
		);
		self.task_order.push(id.clone());
		id
	}

	/// Looks up one task.
	pub fn task(&self, id: &str) -> Result<&TaskEntry, String> {
		self.tasks
			.get(id)
			.ok_or_else(|| format!("unknown task `{id}`"))
	}

	/// Iterates tasks in creation order.
	pub fn task_entries(&self) -> impl Iterator<Item = &TaskEntry> {
		self.task_order.iter().filter_map(|id| self.tasks.get(id))
	}

	/// Drops finished tasks so long-lived servers do not grow forever.
	pub fn prune_finished_tasks(&mut self) {
		let finished: Vec<String> = self
			.tasks
			.iter()
			.filter(|(_, entry)| entry.task.state() != TaskState::Running)
			.filter(|(_, entry)| entry.created.elapsed().as_secs() > 600)
			.map(|(id, _)| id.clone())
			.collect();
		for id in finished {
			self.tasks.remove(&id);
		}
		self.task_order.retain(|id| self.tasks.contains_key(id));
	}
}

impl Default for ServerState {
	fn default() -> Self {
		Self::new()
	}
}

/// A task's state name for JSON output.
pub fn task_state_name(state: TaskState) -> &'static str {
	match state {
		TaskState::Running => "running",
		TaskState::Succeeded => "succeeded",
		TaskState::Failed => "failed",
		TaskState::Cancelled => "cancelled",
	}
}

/// Serializes a task entry for `task_status`, `task_wait`, and `task_list`.
pub fn task_json(entry: &TaskEntry) -> Value {
	let state = entry.task.state();
	json!({
		"task": entry.id,
		"kind": entry.kind,
		"label": entry.label,
		"session": entry.session,
		"state": task_state_name(state),
		"running": state == TaskState::Running,
		"progress": round(entry.task.progress()),
		"error": if state == TaskState::Failed { Some(entry.task.error()) } else { None },
		"elapsed_seconds": round(entry.created.elapsed().as_secs_f64()),
	})
}
