//! The database side of the assistant's tools: what a statement is allowed
//! to do, the one process that runs it, and a ranked lookup over the
//! database's own catalogue.
//!
//! Nothing here is a Tauri command or a route - these are plain functions
//! the MCP bridge calls. The shape is deliberate: `guard::allowed` is the
//! only way a statement becomes runnable, and `sqlcmd::run_sql` asks it
//! again itself, so there is no path to the process that skips the gate.

pub mod guard;
pub mod schema;
pub mod sqlcmd;

pub use guard::{
    access_for, access_for_user, allowed, classify, Access, Verdict, MAX_SQL_CHARS,
    READ_ONLY_SENTENCE,
};
pub use schema::{lookup_sql, render_lookup};
pub use sqlcmd::{
    find_sqlcmd, parse_connection, run_sql, sqlcmd_args, Connection, Output, RealRunner, Runner,
    CHAR_CAP, NOT_INSTALLED, ROW_CAP, TIMEOUT_SECS,
};
