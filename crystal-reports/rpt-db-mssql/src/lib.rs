//!
//! The reports this editor serves (the PMSV10 family) bind to SQL Server **stored procedures**
//! through OLE DB (ADO): each report's table is `usp_…;1`-shaped — a proc name plus the `;1`
//! ODBC result-set marker — with the proc's parameters declared on the table (`@name` + type)
//! and mirrored one-to-one by the report's own `@`-prefixed parameter fields. The result
//! columns are NOT what the wire returns: the report pre-declares them in `Table.data_fields`
//! (name + value type), and the proc's actual result set is matched against that list by name.
//!
//! This crate implements the fetch at the seam the pipeline designed for it — the driver-free
//! needed. Upstream ships Postgres and SQLite drivers only; SQL Server is ours.
//!
//! ## Connection strings
//!
//! Accepted in the application's own format — the ADO/OLE DB keyword form
//! `Server=host;Database=db;User Id=user;Password=***;TrustServerCertificate=True` — with the
//! same key aliases the application's parser takes (`Data Source`/`Address`/`Addr`, `Initial
//! Catalog`, `UID`, `PWD`, …). The password is never printed (custom `Debug`), never placed in
//! an error message, and errors name only the missing KEY, never the string around it.
//!
//! ## Parameters
//!
//! Values arrive as [`rpt_data::Parameters`] (typed), normally coerced upstream by
//! `rpt-core` against the report's declared parameter kinds. A proc parameter left unsupplied
//! binds NULL — many reporting procs treat NULL as "all" — and the report's stored
//! current/default values are consulted first (the same precedence the upstream CLI uses).
//!
//! ## Scope
//!
//! v1 fetches stored-procedure and SQL-command tables (verbatim command text). Plain joined
//! tables are refused with a clear message — the reports in this workspace use none — and a
//! per-row subreport link still filters in memory after a single global fetch, the same
//! semantics as the upstream drivers.

pub mod conn;
pub mod fetch;
pub mod plan;
pub mod scope;
/// Row count of a fetched table, for callers that only need to know emptiness.
pub fn row_count(data: &rpt_data::RowData) -> usize {
    rpt_data::RowSource::rows(data).len()
}

pub use conn::{parse_connection_string, ConnectionConfig};
pub use fetch::{connect, connect_cached, evict_cached, fetch_report_table, query_rows_text, MssqlError};
pub use plan::{param_key, plan_for_report, BindValue, ParamBind, TableFetch};
pub use scope::MssqlScope;

/// Redact a connection string's password for logging: `Password=…` (any case) becomes
/// `Password=(hidden)`. Everything else passes through; used on every string that might leave
/// this crate.
pub fn redact_connection(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, part) in text.split(';').enumerate() {
        if i > 0 {
            out.push(';');
        }
        let key_end = part.find('=').map(|e| part[..e].trim().to_ascii_lowercase());
        out.push_str(match key_end.as_deref() {
            Some("password") | Some("pwd") => "Password=(hidden)",
            _ => part,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::redact_connection;

    #[test]
    fn redaction_hides_every_password_form() {
        let s = "Server=s;Database=d;Password=seekrit;User Id=u";
        assert_eq!(redact_connection(s), "Server=s;Database=d;Password=(hidden);User Id=u");
        assert_eq!(
            redact_connection("pwd=x;Server=s"),
            "Password=(hidden);Server=s"
        );
        assert_eq!(redact_connection("Server=s;Database=d"), "Server=s;Database=d");
    }
}
