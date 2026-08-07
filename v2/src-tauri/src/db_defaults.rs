//! The database MCP server's SHIPPED defaults - the values the AI Bridge
//! form starts from on a machine that has never configured it. A person's
//! own saved config always wins; these only fill an empty form, and
//! registering still takes an explicit click.
//!
//! Shipping credentials in a public binary is a deliberate owner call,
//! same as storing the connection string locally (see lib/dbServer.ts):
//! the database is reachable only from the company network, behind its
//! own sign-in, and device locked - a string extracted from the installer
//! is not a usable credential anywhere else. Blank means "no default";
//! the form simply starts empty as before.

pub const DEFAULT_EXE_PATH: &str = "";
pub const DEFAULT_DB_TYPE: &str = "mssql";
pub const DEFAULT_CONNECTION_STRING: &str = "";
pub const DEFAULT_SCHEMA_FILTER: &str = "";
