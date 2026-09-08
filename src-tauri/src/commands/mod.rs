//! The IPC surface: every `#[tauri::command]`, grouped one file per domain.
//! Handlers stay thin — token via `state::get_fresh_token`, then a call into
//! the matching domain module. Command fn names are frozen (bindings.ts).

pub mod ai_bridge;
pub mod ai_tools;
pub mod auth;
pub mod autorun;
pub mod board;
pub mod bugs;
pub mod cases;
pub mod discovery;
pub mod misc;
pub mod prs;
pub mod queue;
pub mod runs;
pub mod testplan;
pub mod workspace;
