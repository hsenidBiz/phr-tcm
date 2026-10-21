//! What one MCP session has been told. One `--mcp` proxy process serves one client, so a value here is
//! per session; Claude desktop keeps one proxy across chats, so the session starts over after
//! `IDLE_RESET_MS` without a call, and on every `initialize`. Never stored, never sent to the app.
use crate::autorun::roles::{self, Role};
use std::collections::BTreeSet;

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
    /// time (and it is now delivered); `roles::pointer(role)` after.
    pub fn note_for(&mut self, tool: &str, args: &serde_json::Value) -> Option<String> {
        if !self.tracked {
            return None;
        }
        let role = roles::role_for_call(tool, args)?;
        if self.delivered.insert(role) {
            Some(role.text())
        } else {
            Some(roles::pointer(role))
        }
    }
}
