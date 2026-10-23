//! What one MCP session has been told. One `--mcp` proxy process serves one client, so a value here is
//! per session; Claude desktop keeps one proxy across chats, so the session starts over after
//! `IDLE_RESET_MS` without a call, and on every `initialize`. Never stored, never sent to the app.
use crate::autorun::roles::{self, Role};
use std::collections::{BTreeSet, HashMap};

pub const IDLE_RESET_MS: u64 = 20 * 60 * 1000;

#[derive(Debug, Default)]
pub struct Session {
    tracked: bool,
    delivered: BTreeSet<Role>,
    last_call_ms: u64,
}

impl Session {
    /// The proxy's own session.
    pub fn new() -> Session {
        Session { tracked: true, ..Session::default() }
    }

    /// A session that keeps nothing and adds nothing: `mcp::handle_message` runs with one.
    pub fn untracked() -> Session {
        Session::default()
    }

    /// Before each message: when tracked, a gap of IDLE_RESET_MS or more since the last call clears
    /// what was told; then the call time is kept. Untracked: nothing.
    pub fn tick(&mut self, now_ms: u64) {
        if !self.tracked {
            return;
        }
        if self.last_call_ms != 0 && now_ms.saturating_sub(self.last_call_ms) >= IDLE_RESET_MS {
            self.delivered.clear();
        }
        self.last_call_ms = now_ms;
    }

    /// `initialize`: clears what was told (stays tracked or untracked).
    pub fn reset(&mut self) {
        self.delivered.clear();
    }

    pub fn delivered(&self, role: Role) -> bool {
        self.delivered.contains(&role)
    }

    /// A `get_guide` role answer reached the assistant: the role counts as delivered. Untracked: nothing.
    pub fn mark_read(&mut self, role: Role) {
        if self.tracked {
            self.delivered.insert(role);
        }
    }

    /// What goes ahead of the answer of `tool` called with `args`, when the call reached the app:
    /// None untracked or for a call that starts no role (`roles::role_for_call`); the role's text the first
    /// time (and it is now delivered), as `text` gives it; `roles::pointer(role)` after.
    pub fn note_for(&mut self, tool: &str, args: &serde_json::Value, text: &dyn Fn(Role) -> String) -> Option<String> {
        Some(match self.note_kind(tool, args)? {
            Note::Rules(role) => text(role),
            Note::Pointer(role) => roles::pointer(role),
        })
    }

    /// What kind of note goes ahead of the answer, without its text: `note_for` is this plus the text.
    /// The role counts as delivered from here on.
    pub fn note_kind(&mut self, tool: &str, args: &serde_json::Value) -> Option<Note> {
        if !self.tracked {
            return None;
        }
        let role = roles::role_for_call(tool, args)?;
        Some(if self.delivered.insert(role) { Note::Rules(role) } else { Note::Pointer(role) })
    }
}

/// What goes ahead of an answer: a role's rules the first time, a pointer to them after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    Rules(Role),
    Pointer(Role),
}

/// One tracked `Session` per agent name (`""` for none), made on first use, so each subagent of one
/// proxy is told a role's rules once. Its lock is held only for these calls, never across a bridge call.
/// Per-process state of the proxy, not a cache.
#[derive(Debug, Default)]
pub struct SessionBook {
    sessions: std::sync::Mutex<HashMap<String, Session>>,
}

impl SessionBook {
    pub fn new() -> SessionBook {
        SessionBook::default()
    }

    fn with<T>(&self, agent: Option<&str>, f: impl FnOnce(&mut Session) -> T) -> T {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        f(sessions.entry(agent.unwrap_or("").to_string()).or_insert_with(Session::new))
    }

    /// Before each message, for the agent it names: as `Session::tick`.
    pub fn tick(&self, agent: Option<&str>, now_ms: u64) {
        self.with(agent, |s| s.tick(now_ms));
    }

    /// `initialize`: every agent's session starts over.
    pub fn reset_all(&self) {
        for session in self.sessions.lock().unwrap_or_else(|e| e.into_inner()).values_mut() {
            session.reset();
        }
    }

    pub fn mark_read(&self, agent: Option<&str>, role: Role) {
        self.with(agent, |s| s.mark_read(role));
    }

    pub fn note_kind(&self, agent: Option<&str>, tool: &str, args: &serde_json::Value) -> Option<Note> {
        self.with(agent, |s| s.note_kind(tool, args))
    }
}
