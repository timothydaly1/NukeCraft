//! The NukeCraft session: the open script, undo history, and the command registry.
//!
//! **Everything is a command.** Every edit the UI, the command line or an agent makes goes
//! through [`Session::execute`] with a command id and JSON parameters, so each one gets undo,
//! the journal and the crash guard for free, and every frontend reaches every feature. The
//! registry ([`command_specs`]) carries each command's label, menu path, shortcut and a
//! parameter description that the MCP server publishes to agents.
//!
//! A command either succeeds or leaves the script exactly as it was: failures and caught panics
//! restore the snapshot taken before it ran.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod commands;
pub mod demo;
pub mod fragment;
mod groups;
mod params;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use nukecraft_graph::{GraphError, Script};
use nukecraft_nodes::Registry;
use nukecraft_time::FrameTime;
use serde_json::{Value, json};

pub use commands::{CommandSpec, command_specs, find_command, render_plan};

/// Undo steps kept.
const MAX_UNDO: usize = 500;

/// Journal entries kept.
const MAX_JOURNAL: usize = 10_000;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command `{0}` (query.commands lists them)")]
    UnknownCommand(String),
    #[error("{id} is not available: {reason}")]
    Disabled { id: String, reason: String },
    #[error("{0}")]
    BadParams(String),
    #[error(transparent)]
    Graph(#[from] GraphError),
    #[error("{0}")]
    Render(String),
    #[error("{0}")]
    Io(String),
    #[error("internal error (the script was left unchanged): {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

impl From<nukecraft_render::RenderError> for EngineError {
    fn from(e: nukecraft_render::RenderError) -> EngineError {
        EngineError::Render(e.to_string())
    }
}

impl From<nukecraft_io::IoError> for EngineError {
    fn from(e: nukecraft_io::IoError) -> EngineError {
        EngineError::Io(e.to_string())
    }
}

/// One open script and its editing state.
pub struct Session {
    pub script: Script,
    /// Where the script was opened from or last saved to.
    pub path: Option<PathBuf>,
    pub registry: Registry,
    /// The current frame (the viewer's time).
    pub time: FrameTime,
    /// Unsaved changes.
    pub dirty: bool,
    /// What loading the script repaired.
    pub load_warnings: Vec<String>,
    /// Bumped every time the script changes (edits, undo, open), so views can tell their caches
    /// are stale without comparing scripts.
    pub revision: u64,
    /// An open undo group: its label and the script before it began.
    group: Option<(String, Script)>,
    undo: Vec<(String, Script)>,
    redo: Vec<(String, Script)>,
    journal: Vec<Value>,
}

impl Default for Session {
    fn default() -> Session {
        Session::new()
    }
}

impl Session {
    /// An empty script with every node class (built-ins, Read and Write).
    pub fn new() -> Session {
        Session {
            script: Script::new(),
            path: None,
            registry: nukecraft_io::full_registry(),
            time: FrameTime::from_frame(1),
            dirty: false,
            load_warnings: Vec::new(),
            revision: 0,
            group: None,
            undo: Vec::new(),
            redo: Vec::new(),
            journal: Vec::new(),
        }
    }

    /// Runs a command. Undoable commands that change the script push one undo step labelled
    /// with the command; a failing command (or one that panics) leaves the script unchanged.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find_command(id).ok_or_else(|| EngineError::UnknownCommand(id.into()))?;
        (spec.enabled)(self).map_err(|reason| EngineError::Disabled { id: id.into(), reason })?;
        let before = self.script.clone();
        let result = match catch_unwind(AssertUnwindSafe(|| (spec.run)(self, params))) {
            Ok(r) => r,
            Err(panic) => {
                let msg = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic".into());
                log::error!("command {id} panicked: {msg}");
                Err(EngineError::Internal(format!("{id}: {msg}")))
            }
        };
        match result {
            Ok(v) => {
                if self.script != before {
                    self.revision = self.revision.wrapping_add(1);
                    if spec.undoable {
                        self.dirty = true;
                        if self.group.is_none() {
                            self.push_undo(spec.label, before);
                        }
                    }
                }
                if spec.journal {
                    if self.journal.len() >= MAX_JOURNAL {
                        self.journal.remove(0);
                    }
                    self.journal.push(json!({ "command": id, "params": params }));
                }
                Ok(v)
            }
            Err(e) => {
                self.script = before;
                Err(e)
            }
        }
    }

    fn push_undo(&mut self, label: &str, before: Script) {
        if self.undo.len() >= MAX_UNDO {
            self.undo.remove(0);
        }
        self.undo.push((label.to_string(), before));
        self.redo.clear();
    }

    /// Starts an undo group: until [`Session::end_group`], undoable commands change the script
    /// without pushing undo steps, and the group becomes one step labelled `label` (a slider drag,
    /// a node drag, a node inserted and rewired). Nested calls join the open group.
    pub fn begin_group(&mut self, label: &str) {
        if self.group.is_none() {
            self.group = Some((label.to_string(), self.script.clone()));
        }
    }

    /// Ends the open undo group, pushing one undo step if the script changed during it.
    pub fn end_group(&mut self) {
        if let Some((label, before)) = self.group.take()
            && self.script != before
        {
            self.push_undo(&label, before);
        }
    }

    pub fn in_group(&self) -> bool {
        self.group.is_some()
    }

    /// Undo step labels, oldest first.
    pub fn history(&self) -> Vec<&str> {
        self.undo.iter().map(|(l, _)| l.as_str()).collect()
    }

    /// The commands run so far (id and parameters), oldest first.
    pub fn journal(&self) -> &[Value] {
        &self.journal
    }

    /// Forgets undo history (after opening a script, or building the demo).
    pub fn clear_history(&mut self) {
        self.group = None;
        self.undo.clear();
        self.redo.clear();
    }

    pub(crate) fn undo_step(&mut self) -> Option<String> {
        self.end_group();
        let (label, prev) = self.undo.pop()?;
        let cur = std::mem::replace(&mut self.script, prev);
        self.redo.push((label.clone(), cur));
        self.dirty = true;
        Some(label)
    }

    pub(crate) fn redo_step(&mut self) -> Option<String> {
        self.end_group();
        let (label, next) = self.redo.pop()?;
        let cur = std::mem::replace(&mut self.script, next);
        self.undo.push((label.clone(), cur));
        self.dirty = true;
        Some(label)
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.group.as_ref().is_some_and(|(_, before)| *before != self.script)
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests;
