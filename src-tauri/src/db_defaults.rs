//! The database MCP server's SHIPPED defaults - the values the AI Bridge
//! form starts from on a machine that has never configured it, plus the
//! named presets its dropdown offers. A person's own saved config always
//! wins; the first preset only fills an empty form, and registering still
//! takes an explicit click.
//!
//! Shipping credentials in a public binary is a deliberate owner call,
//! same as storing the connection string locally (see lib/dbServer.ts):
//! the database is reachable only from the company network, behind its
//! own sign-in, and device locked - a string extracted from the installer
//! is not a usable credential anywhere else.

pub const DEFAULT_EXE_PATH: &str = "";
pub const DEFAULT_DB_TYPE: &str = "mssql";
pub const DEFAULT_SCHEMA_FILTER: &str = "PeoplesHR";

/// One shipped environment: a label for the dropdown and the full
/// connection string it stands for.
pub struct DbPreset {
    pub label: &'static str,
    pub connection_string: &'static str,
}

/// The environments the app ships knowing about. ORDER MATTERS: the first
/// entry is the automatic prefill for a never-configured machine, so the
/// read-only dev login leads - the least a set of defaults can hand out.
pub const DB_PRESETS: &[DbPreset] = &[
    DbPreset {
        label: "Dev — read only",
        connection_string: "Server=sgdev01db02.cloud;Database=hrmmain_philippines;User Id=sgdev01db02_readonly;Password=M5kjapL2H3bEIuZZ4YA4;TrustServerCertificate=True;",
    },
    DbPreset {
        label: "Dev — dev login",
        connection_string: "Server=sgdev01db02.cloud;Database=hrmmain_philippinesdev;User Id=sgdev01db01_devlogin;Password=abc123@@@###;TrustServerCertificate=True;",
    },
    DbPreset {
        label: "QA — read only",
        connection_string: "Server=sgqa01db01.cloud;Database=hrmmain_philippines;User Id=sgqa01db01_readonly;Password=nhi5tJF9pfnsgynODZXA;TrustServerCertificate=True;",
    },
];

/// The automatic-prefill string: the first preset, or blank when none
/// ship (blank means the form simply starts empty, as before).
pub fn default_connection_string() -> &'static str {
    DB_PRESETS.first().map(|p| p.connection_string).unwrap_or("")
}
