//! Connecting and fetching: tiberius (pure-Rust TDS) behind a shared tokio runtime, so the
//! crate's public surface stays synchronous like the rest of the pipeline.
//!
//! Everything a result-set cell can be arrives as text (or bytes for binary fields) — the same
//! everything-is-text design as the upstream drivers: the report's declared
//! [`rpt_model::FieldValueType`] re-types each column afterwards via `RowData`.

use std::sync::{Arc, Mutex, OnceLock};

use rpt_data::{Cell, Column, Parameters, RowData};
use rpt_model::Report;
use tiberius::{Client, ColumnType};
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use crate::conn::ConnectionConfig;
use crate::plan::{plan_for_report, BindValue, TableFetch};

/// A connected client: tiberius over tokio's TCP stream (futures-io compat wrapper).
pub type TdsClient = Client<Compat<tokio::net::TcpStream>>;

/// Errors, carrying the `server/database` label and the statement that failed — never the
/// credential.
#[derive(Debug, thiserror::Error)]
pub enum MssqlError {
    /// The TCP/TDS connection or login failed.
    #[error("cannot connect to {label}: {source}")]
    Connect {
        /// `server/database` label.
        label: String,
        /// The driver's error.
        source: tiberius::error::Error,
    },
    /// The fetch statement failed (missing proc, permission, bad parameter, …).
    #[error("query failed against {label} — {statement}: {source}")]
    Query {
        /// `server/database` label.
        label: String,
        /// The RPC name or command that failed.
        statement: String,
        /// The driver's error.
        source: tiberius::error::Error,
    },
    /// The scope's tables are not fetchable by this driver; the reason says why.
    #[error("{0}")]
    Unsupported(String),
}

/// One fetched scope: the materialized rows plus non-fatal notes (unmatched columns, skipped
/// result sets) worth surfacing as preview diagnostics.
pub struct Fetched {
    /// The rows, ready for [`rpt_render::RenderSource::Rows`].
    pub data: RowData,
    /// Non-fatal notes (column mismatches, ignored extra tables).
    pub notes: Vec<String>,
}

/// The shared tokio runtime every blocking call runs on.
fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

const OP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// A shared, cached connection: previews repeat against the same server, and a fresh
/// TCP+TLS+login per render is pure latency. Keyed by a hash of the full config (the password
/// contributes to the hash, never to storage). A `Connect`-class failure evicts the slot so
/// the next call reconnects; `Query`-class failures keep the connection (a server-side
/// statement error does not poison TDS).
pub fn connect_cached(config: &ConnectionConfig) -> Result<Arc<Mutex<TdsClient>>, MssqlError> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<Mutex<TdsClient>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        config.server.hash(&mut h);
        config.port.hash(&mut h);
        config.database.hash(&mut h);
        config.user.hash(&mut h);
        config.password.hash(&mut h);
        config.trust_cert.hash(&mut h);
        h.finish()
    };

    let mut guard = cache.lock().map_err(|_| MssqlError::Unsupported("connection cache poisoned".into()))?;
    if let Some(hit) = guard.get(&key) {
        return Ok(Arc::clone(hit));
    }
    let client = connect(config)?;
    let arc = Arc::new(Mutex::new(client));
    guard.insert(key, Arc::clone(&arc));
    Ok(arc)
}

/// Evict a broken cached connection (called by the preview path on `Connect`-class failures).
pub fn evict_cached(config: &ConnectionConfig) {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<Mutex<TdsClient>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        config.server.hash(&mut h);
        config.port.hash(&mut h);
        config.database.hash(&mut h);
        config.user.hash(&mut h);
        config.password.hash(&mut h);
        config.trust_cert.hash(&mut h);
        h.finish()
    };
    if let Ok(mut guard) = cache.lock() {
        guard.remove(&key);
    }
}

/// Connect (TCP + TDS login). Errors carry only the `server/database` label.
pub fn connect(config: &ConnectionConfig) -> Result<TdsClient, MssqlError> {
    let label = config.label();
    let addr = (config.server.clone(), config.port);
    let tcfg = config.tiberius();
    runtime().block_on(async move {
        let tcp = tokio::time::timeout(OP_TIMEOUT, tokio::net::TcpStream::connect(addr))
            .await
            .map_err(|_| MssqlError::Unsupported(format!("connect to {label} timed out after 30s")))?
            .map_err(|source| MssqlError::Connect {
                label: label.clone(),
                source: tiberius::error::Error::from(source),
            })?;
        tcp.set_nodelay(true).ok();
        let client = tokio::time::timeout(OP_TIMEOUT, Client::connect(tcfg, tcp.compat_write()))
            .await
            .map_err(|_| MssqlError::Unsupported(format!("login to {label} timed out after 30s")))?
            .map_err(|source| MssqlError::Connect { label, source })?;
        Ok(client)
    })
}

/// Fetch one report scope's rows: the scope's stored procedure (or SQL command) executed with
/// the resolved parameters, its result set matched by name against the report's declared
/// `data_fields`.
///
/// v1 fetches the FIRST fetchable table of the scope (every report in this workspace binds
/// exactly one); extra tables are a note, plain joined tables a refusal.
pub fn fetch_report_table(
    client: &mut TdsClient,
    label: &str,
    report: &Report,
    params: &Parameters,
) -> Result<Fetched, MssqlError> {
    let plans = plan_for_report(report, params);
    let mut notes = Vec::new();
    let Some(plan) = plans.iter().find(|p| !matches!(p, TableFetch::NotSupported { .. })) else {
        let reasons: Vec<String> = plans.iter().map(|p| p.describe()).collect();
        return Err(MssqlError::Unsupported(if reasons.is_empty() {
            "the report scope binds no database table".to_string()
        } else {
            reasons.join("; ")
        }));
    };
    if plans.iter().filter(|p| !matches!(p, TableFetch::NotSupported { .. })).count() > 1 {
        notes.push(format!(
            "scope binds more than one table; only {} was fetched",
            plan.describe()
        ));
    }

    let stream = runtime().block_on(async {
        match plan {
            TableFetch::StoredProcedure { rpc_name, binds } => {
                // EXEC with @P1-style placeholders bound positionally — tiberius sends the
                // values as typed RPC parameters; no SQL text is built from user input.
                let placeholders: Vec<String> =
                    (1..=binds.len()).map(|i| format!("@P{i}")).collect();
                let sql = if placeholders.is_empty() {
                    format!("EXEC [{rpc_name}]")
                } else {
                    format!("EXEC [{rpc_name}] {}", placeholders.join(", "))
                };
                let values: Vec<Box<dyn tiberius::ToSql + '_>> = binds
                    .iter()
                    .map(|b| match &b.value {
                        Some(BindValue::Str(s)) => Box::new(s.clone()) as Box<dyn tiberius::ToSql>,
                        Some(BindValue::Num(n)) => Box::new(*n),
                        Some(BindValue::Bool(v)) => Box::new(*v),
                        Some(BindValue::Bytes(bytes)) => Box::new(bytes.clone()),
                        None => Box::new(Option::<String>::None),
                    })
                    .collect();
                let refs: Vec<&dyn tiberius::ToSql> = values.iter().map(|v| v.as_ref()).collect();
                client
                    .query(sql, &refs)
                    .await
                    .map_err(|source| MssqlError::Query {
                        label: label.to_string(),
                        statement: plan.describe(),
                        source,
                    })
            }
            TableFetch::Command { sql } => client
                .simple_query(sql.as_str())
                .await
                .map_err(|source| MssqlError::Query {
                    label: label.to_string(),
                    statement: format!("command, {} chars", sql.len()),
                    source,
                }),
            TableFetch::NotSupported { .. } => unreachable!("filtered above"),
        }
    })?;

    collect_rows(stream, report, &mut notes).map(|data| Fetched { data, notes })
}

/// One diagnostic query: every cell of every row as text. Used by the gated live smoke test
/// (`tests/live.rs`) — not part of the render path.
pub fn query_rows_text(client: &mut TdsClient, sql: &str) -> Result<Vec<Vec<String>>, MssqlError> {
    runtime().block_on(async {
        let stream = client
            .simple_query(sql)
            .await
            .map_err(|source| MssqlError::Query {
                label: "diagnostic query".into(),
                statement: sql.chars().take(60).collect(),
                source,
            })?;
        let sets = stream
            .into_results()
            .await
            .map_err(|source| MssqlError::Query {
                label: "diagnostic query".into(),
                statement: "fetch rows".into(),
                source,
            })?;
        Ok(sets
            .into_iter()
            .flatten()
            .map(|row| {
                row.columns()
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        row.try_get::<&str, _>(i)
                            .ok()
                            .flatten()
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("<{:?}>", c.column_type()))
                    })
                    .collect::<Vec<String>>()
            })
            .collect())
    })
}

/// Take the first result set with rows and map it to the report's declared columns.
fn collect_rows(
    stream: tiberius::QueryStream<'_>,
    report: &Report,
    notes: &mut Vec<String>,
) -> Result<RowData, MssqlError> {
    let sets = runtime()
        .block_on(async { stream.into_results().await })
        .map_err(|source| MssqlError::Query {
            label: "result stream".into(),
            statement: "fetch rows".into(),
            source,
        })?;

    let table = report
        .database
        .tables
        .first()
        .expect("planner only runs with a table");
    let columns: Vec<Column> = table
        .data_fields
        .iter()
        .map(|f| Column { name: f.name.clone(), value_type: f.value_type })
        .collect();

    // The first result set that actually returned rows (procs commonly emit row-count
    // chatter first); an entirely empty stream is still a valid (empty) fetch.
    let Some(rows) = sets.into_iter().find(|s| !s.is_empty()) else {
        let mut next: std::vec::IntoIter<Vec<Option<Cell>>> = Vec::new().into_iter();
        return Ok(from_columns_and_cells(columns, move || Ok(next.next())));
    };

    // Column identity from the result set's own metadata (every row carries it).
    let result_cols: Vec<(String, ColumnType)> = rows[0]
        .columns()
        .iter()
        .map(|c| (c.name().to_string(), c.column_type()))
        .collect();
    let mut index_of: Vec<Option<usize>> = Vec::with_capacity(columns.len());
    for col in &columns {
        let idx = result_cols
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(&col.name));
        if idx.is_none() {
            notes.push(format!("column `{}` is not in the result set (renders blank)", col.name));
        }
        index_of.push(idx);
    }

    let cells: Vec<Vec<Option<Cell>>> = rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .zip(index_of.iter())
                .map(|(col, idx)| match idx {
                    None => None,
                    Some(i) => sql_cell(row, *i, result_cols[*i].1, col.value_type),
                })
                .collect()
        })
        .collect();

    let mut next = cells.into_iter();
    Ok(from_columns_and_cells(columns, move || Ok(next.next())))
}

/// One SQL Server value → one pipeline cell. Binary-declared report fields read raw bytes;
/// everything else is stringified in a form the pipeline's re-typing accepts (ISO dates,
/// `true`/`false` booleans, plain decimal digits).
fn sql_cell(
    row: &tiberius::Row,
    idx: usize,
    ct: ColumnType,
    declared: rpt_model::FieldValueType,
) -> Option<Cell> {
    if declared.is_binary() {
        return row
            .try_get::<&[u8], _>(idx)
            .ok()
            .flatten()
            .map(|b| Cell::Bytes(b.to_vec()));
    }
    let text: Option<String> = match ct {
        ColumnType::Bit | ColumnType::Bitn => {
            row.try_get::<bool, _>(idx).ok().flatten().map(|b| b.to_string())
        }
        ColumnType::Int1 => row.try_get::<u8, _>(idx).ok().flatten().map(|v| v.to_string()),
        ColumnType::Int2 => row.try_get::<i16, _>(idx).ok().flatten().map(|v| v.to_string()),
        ColumnType::Int4 | ColumnType::Intn => {
            row.try_get::<i32, _>(idx).ok().flatten().map(|v| v.to_string())
        }
        ColumnType::Int8 => row.try_get::<i64, _>(idx).ok().flatten().map(|v| v.to_string()),
        ColumnType::Float4 => row.try_get::<f32, _>(idx).ok().flatten().map(|v| v.to_string()),
        ColumnType::Float8 | ColumnType::Floatn => {
            row.try_get::<f64, _>(idx).ok().flatten().map(|v| v.to_string())
        }
        ColumnType::Decimaln | ColumnType::Numericn | ColumnType::Money | ColumnType::Money4 => row
            .try_get::<rust_decimal::Decimal, _>(idx)
            .ok()
            .flatten()
            .map(|d| d.to_string()),
        ColumnType::Guid => row
            .try_get::<tiberius::Uuid, _>(idx)
            .ok()
            .flatten()
            .map(|u| u.to_string()),
        ColumnType::Daten => row
            .try_get::<chrono::NaiveDate, _>(idx)
            .ok()
            .flatten()
            .map(|d| d.format("%Y-%m-%d").to_string()),
        ColumnType::Timen => row
            .try_get::<chrono::NaiveTime, _>(idx)
            .ok()
            .flatten()
            .map(|t| t.format("%H:%M:%S%.f").to_string()),
        ColumnType::Datetime4 | ColumnType::Datetime | ColumnType::Datetimen | ColumnType::Datetime2 => row
            .try_get::<chrono::NaiveDateTime, _>(idx)
            .ok()
            .flatten()
            .map(|d| d.format("%Y-%m-%d %H:%M:%S%.f").to_string()),
        ColumnType::DatetimeOffsetn => row
            .try_get::<chrono::DateTime<chrono::FixedOffset>, _>(idx)
            .ok()
            .flatten()
            .map(|d| d.format("%Y-%m-%d %H:%M:%S%.f%:z").to_string()),
        _ => row.try_get::<&str, _>(idx).map(|s| s.map(str::to_string)).ok().flatten(),
    };
    text.map(Cell::Text)
}

/// `RowData::from_cells` over pre-materialized cells — the closure can never fail.
fn from_columns_and_cells(
    columns: Vec<Column>,
    next: impl FnMut() -> Result<Option<Vec<Option<Cell>>>, std::convert::Infallible>,
) -> RowData {
    RowData::from_cells(columns, next).map_err(|e| match e {}).expect("Infallible")
}
