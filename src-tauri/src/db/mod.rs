//! The database side of the assistant's tools: what a statement is allowed
//! to do, the one process that runs it, a ranked lookup over the database's
//! own catalogue, and where each database's login is kept.
//!
//! Nothing here is a Tauri command or a route - these are plain functions
//! the MCP bridge calls. The shape is deliberate: `guard::allowed` is the
//! only way a statement becomes runnable, and `sqlcmd::run_sql` asks it
//! again itself, so there is no path to the process that skips the gate.

pub mod credentials;
pub mod guard;
pub mod query;
pub mod schema;
pub mod sqlcmd;

pub use credentials::{
    CredentialManager, DbCredentialsForm, DbDatabase, DbSecrets, MemoryStore, SecretStore, OWN_ID,
};
pub use guard::{
    access_for, access_for_user, allowed, classify, Access, Verdict, MAX_SQL_CHARS,
    READ_ONLY_SENTENCE,
};
pub use schema::{
    describe_sql, detail_sql, lookup_sql, parse_ranked, render_describe, render_lookup, Picked,
};
pub use sqlcmd::{
    find_sqlcmd, parse_connection, run_sql, sqlcmd_args, sqlcmd_env, sqlcmd_path, Connection,
    Output, RealRunner, Runner, CHAR_CAP, CUT_AT, LOGIN_TIMEOUT_SECS, NOT_INSTALLED, PASSWORD_ENV,
    ROW_CAP, SQLCMD_OVERRIDE, TIMEOUT_SECS,
};
