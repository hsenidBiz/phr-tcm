//! The database tools' SHIPPED defaults: the schema a lookup searches
//! first, plus the named presets the Database Read Access card's dropdown
//! offers. A person's own saved login always wins, and no preset is chosen
//! for them.
//!
//! Shipping credentials in a public binary is a deliberate owner call,
//! same as keeping each saved login on this machine (db/credentials.rs):
//! the database is reachable only from the company network, behind its
//! own sign-in, and device locked - a string extracted from the installer
//! is not a usable credential anywhere else.

/// The schema a lookup ranks first - see `db::query::run_lookup`.
pub const DEFAULT_SCHEMA_FILTER: &str = "PeoplesHR";

/// One shipped environment: a stable id, a label for the dropdown and the full
/// connection string it stands for.
#[derive(Clone, Copy)]
pub struct DbPreset {
    /// What a saved login and a chosen connection are keyed on. The label is
    /// copy and may be reworded; an id, once shipped, never changes.
    pub id: &'static str,
    pub label: &'static str,
    pub connection_string: &'static str,
}

/// The environments the app ships knowing about, in the order the dropdown
/// lists them. None is preselected on a fresh machine; the read-only dev
/// login leads so the first one offered is the least it can hand out.
pub const DB_PRESETS: &[DbPreset] = &[
    DbPreset {
        id: "dev-read",
        label: "Dev - read only",
        connection_string: "Server=sgdev01db02.cloud;Database=hrmmain_philippines;User Id=sgdev01db02_readonly;Password=M5kjapL2H3bEIuZZ4YA4;TrustServerCertificate=True;",
    },
    DbPreset {
        id: "dev-login",
        label: "Dev - dev login",
        connection_string: "Server=sgdev01db02.cloud;Database=hrmmain_philippinesdev;User Id=sgdev01db01_devlogin;Password=abc123@@@###;TrustServerCertificate=True;",
    },
    DbPreset {
        id: "qa-read",
        label: "QA - read only",
        connection_string: "Server=sgqa01db01.cloud;Database=hrmmain_philippines;User Id=sgqa01db01_readonly;Password=nhi5tJF9pfnsgynODZXA;TrustServerCertificate=True;",
    },
];
