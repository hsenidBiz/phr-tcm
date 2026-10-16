//! The gated live smoke — runs ONLY when `RPT_MSSQL_CONN` holds a real ADO/OLE DB connection
//! string (`cargo test -p rpt-db-mssql --test live -- --ignored`). Never printed, never
//! echoed; failures carry the `server/database` label only.
//!
//! Stages: parse → connect (TCP + TDS login + TLS) → `SELECT @@VERSION` → optional raw SQL
//! from `RPT_MSSQL_SQL` (rows printed as text). This is the harness for verifying the driver
//! against a real server before wiring it into a preview.

#[test]
#[ignore = "set RPT_MSSQL_CONN (and optionally RPT_MSSQL_SQL) to run the live smoke"]
fn connect_and_query() {
    let conn = std::env::var("RPT_MSSQL_CONN").expect("RPT_MSSQL_CONN");
    let config = rpt_db_mssql::parse_connection_string(&conn).expect("parse");
    println!("target: {}", config.label());
    let mut client = rpt_db_mssql::connect(&config).expect("connect + login");

    let sql = std::env::var("RPT_MSSQL_SQL").unwrap_or_else(|_| "SELECT @@VERSION AS v".to_string());
    let rows = rpt_db_mssql::query_rows_text(&mut client, &sql).expect("query");
    for row in rows {
        println!("row: {row:?}");
    }
}
