//! Preview rendering: `.rpt` bytes → a real, data-driven PDF.
//!
//! Runs the vendored `rpt-render` pipeline (VENDORED.md #10) over an open report: the layout
//! engine walks the report definition over its data tree and paginates into Page IR, and the PDF
//! backend writes it through krilla with the bundled Liberation/DejaVu fonts (`Arial` renders as
//! metric-compatible Liberation Sans — no system font scan, so output is deterministic and
//! identical across machines).
//!
//! Data comes from the report's own saved data — the editor is offline by design, so no database
//! is ever contacted. A report saved without data renders its static bands (title, page
//! header/footer, unbound text) over an empty row set; that is a preview, not an error.
//!
//! Render output is reproducible for any report that does not print a time-dependent special
//! field: same bytes → same PDF, across runs, profiles, and machines (the PDF writer's own
//! timestamp defaults to the epoch and the bundled fonts never scan the host). The one moving
//! input is deliberate: Print Date/Time specials render at the wall clock captured at render
//! start, exactly like Crystal's own preview — so a report that prints one produces a PDF that
//! changes with the second, by design. Tests pin reproducibility on a time-free fixture.
//!
//! `diagnostics` carries the pipeline's own fidelity warnings (an object kind the renderer does
//! not draw, a formula that failed evaluation) rather than failing the render — a preview that
//! degrades is more useful to an editor than one that refuses.

use rpt_reader::Rpt;
use rpt_render::Parameters;
use serde::{Deserialize, Serialize};

/// One preview render's result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewOutput {
    /// The rendered PDF file bytes.
    pub pdf: Vec<u8>,
    /// How many pages the report produced.
    pub page_count: usize,
    /// Human-readable pipeline fidelity warnings; empty when the render was clean.
    pub diagnostics: Vec<String>,
}

/// A live-database preview request: the application's own ADO/OLE DB connection string
/// (`Server=…;Database=…;User Id=…;Password=…;TrustServerCertificate=True` — the same keys and
/// aliases the application's parser accepts) plus any report parameter values the report
/// prompts for (`@name` = stored-proc parameter names, with or without the `@`).
///
/// The string is parsed once, never logged, and never echoed in an error; a failure names only
/// the missing KEY.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveDb {
    /// The ADO/OLE DB connection string, verbatim from the application.
    pub connection_string: String,
    /// Report parameter values as text, coerced against the report's declared parameter
    /// kinds (`@performance_cycle_id=42` → the number 42). Unsupplied proc parameters bind
    /// NULL.
    pub params: Vec<(String, String)>,
}

/// Coerce raw `name=value-text` pairs into typed [`Parameters`] against the report's declared
/// parameter kinds — the same coercion rules as the upstream CLI: Number/Currency parse as
/// `f64`, Boolean takes `true|1|yes|y` / `false|0|no|n`, Date is `YYYY-MM-DD`, Time
/// `HH:MM[:SS]`, DateTime `YYYY-MM-DDTHH:MM:SS`, String verbatim. Names match
/// case-insensitively with or without the `@`.
///
/// # Errors
///
/// A value that does not parse for its declared kind, naming the parameter.
pub fn coerce_params(report: &rpt_reader::model::Report, raw: &[(String, String)]) -> Result<Parameters, String> {
    use rpt_formula::eval::{Date, Time, Value};
    use rpt_reader::model::ParameterValueKind;

    let mut out = Parameters::new();
    for (name, text) in raw {
        let key = rpt_db_mssql::param_key(name);
        let kind = report
            .data_definition
            .parameter_fields()
            .find(|(f, _)| rpt_db_mssql::param_key(&f.name) == key)
            .map(|(_, p)| p.value_kind);
        let value = match kind {
            None => Value::Str(text.clone()), // undeclared: pass through as text
            Some(k) => match k {
                ParameterValueKind::NumberParameter | ParameterValueKind::CurrencyParameter => {
                    Value::Number(text.parse::<f64>().map_err(|_| format!("parameter {name}: `{text}` is not a number"))?)
                }
                ParameterValueKind::BooleanParameter => match text.to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" | "y" => Value::Bool(true),
                    "false" | "0" | "no" | "n" => Value::Bool(false),
                    _ => return Err(format!("parameter {name}: `{text}` is not a boolean")),
                },
                ParameterValueKind::DateParameter => {
                    let (y, m, d) = parse_ymd(text).ok_or_else(|| format!("parameter {name}: `{text}` is not YYYY-MM-DD"))?;
                    Value::Date(Date { year: y, month: m, day: d })
                }
                ParameterValueKind::TimeParameter => {
                    let (h, mi, s) = parse_hms(text).ok_or_else(|| format!("parameter {name}: `{text}` is not HH:MM[:SS]"))?;
                    Value::Time(Time { hour: h, minute: mi, second: s })
                }
                ParameterValueKind::DateTimeParameter => {
                    let (date, time) = text
                        .split_once('T')
                        .or_else(|| text.split_once(' '))
                        .ok_or_else(|| format!("parameter {name}: `{text}` is not YYYY-MM-DDTHH:MM:SS"))?;
                    let (y, m, d) = parse_ymd(date).ok_or_else(|| format!("parameter {name}: `{date}` is not a date"))?;
                    let (h, mi, s) = parse_hms(time).ok_or_else(|| format!("parameter {name}: `{time}` is not a time"))?;
                    Value::DateTime(Date { year: y, month: m, day: d }, Time { hour: h, minute: mi, second: s })
                }
                ParameterValueKind::StringParameter | _ => Value::Str(text.clone()),
            },
        };
        out.insert(key, value);
    }
    Ok(out)
}

fn parse_ymd(s: &str) -> Option<(i32, u8, u8)> {
    let mut it = s.split('-');
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

fn parse_hms(s: &str) -> Option<(u8, u8, u8)> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts.get(2).and_then(|x| x.parse().ok()).unwrap_or(0),
    ))
}

/// [`preview`] over an already-decoded report — lets a caller render the `Rpt` it has open
/// without a second decode.
///
/// # Errors
///
/// As [`preview`], minus the decode.
pub fn preview_report(rpt: &Rpt) -> Result<PreviewOutput, String> {
    let doc = rpt_render::render(rpt.report());
    finish_preview(doc, Vec::new(), !rpt.report().has_saved_data)
}

/// [`preview_live`] over an already-decoded report, fetching fresh rows from SQL Server —
/// the preview reflects the database as it stands NOW, not the data saved into the file.
///
/// One connection is opened for the whole render (main scope and every subreport scope share
/// it); stored-procedure tables execute as RPC calls with the resolved parameters, SQL-command
/// tables run verbatim. Subreport fetch failures degrade to the scope's saved data with a
/// diagnostic — the same semantics as the saved-data path.
///
/// # Errors
///
/// As [`preview_report`], plus: the connection string does not parse (the error names only the
/// missing key), the connection/login fails, the main-scope fetch fails, or a parameter value
/// does not coerce. The password never appears in any error.
pub fn preview_live_report(rpt: &Rpt, db: &LiveDb) -> Result<PreviewOutput, String> {
    let report = rpt.report();
    let config = rpt_db_mssql::parse_connection_string(&db.connection_string)?;
    let params = coerce_params(report, &db.params)?;
    let label = config.label();
    let client = rpt_db_mssql::connect_cached(&config).map_err(|e| e.to_string())?;
    let fetched = match client.lock() {
        Ok(mut guard) => rpt_db_mssql::fetch_report_table(&mut guard, &label, report, &params),
        Err(_) => Err(rpt_db_mssql::MssqlError::Unsupported("connection cache poisoned".into())),
    };
    let fetched = match fetched {
        Ok(f) => f,
        Err(e @ rpt_db_mssql::MssqlError::Connect { .. }) => {
            // A cached connection that went stale (server restart, idle timeout): drop it
            // and reconnect once — the next preview reuses the fresh connection.
            rpt_db_mssql::evict_cached(&config);
            let fresh = std::sync::Arc::new(std::sync::Mutex::new(
                rpt_db_mssql::connect(&config).map_err(|e2: rpt_db_mssql::MssqlError| {
                    format!("{e}; reconnect also failed: {e2}")
                })?,
            ));
            let mut guard = fresh.lock().map_err(|_| "connection lock poisoned".to_string())?;
            rpt_db_mssql::fetch_report_table(&mut guard, &label, report, &params)
                .map_err(|e| e.to_string())?
        }
        Err(e) => return Err(e.to_string()),
    };
    let mut scope_notes = fetched.notes;
    let scope = rpt_db_mssql::MssqlScope::new(config, params.clone(), client);
    let opts = rpt_render::RenderOptions {
        datasource: rpt_render::RenderSource::Rows(&fetched.data),
        params,
        ..Default::default()
    };
    let doc = rpt_render::render_with(report, opts);
    scope_notes.extend(scope.take_notes());
    // A live fetch that returned no rows leaves the same empty-page shape as no saved data —
    // same notice, worded for the live path.
    let empty_live = rpt_db_mssql::row_count(&fetched.data) == 0;
    finish_preview(doc, scope_notes, empty_live)
}

fn finish_preview(
    doc: rpt_pages::PagedDocument,
    mut diagnostics: Vec<String>,
    no_data: bool,
) -> Result<PreviewOutput, String> {
    let page_count = doc.pages.len();
    let rendered: Vec<String> = doc.diagnostics.iter().map(|d| d.describe()).collect();
    if no_data {
        // Zero rows means no columns resolve, so every db-field reference a page-level band
        // evaluates surfaces as "formula error: unknown name" — pure consequences of the
        // empty row set, not defects. Say the one thing that matters and drop the noise.
        diagnostics.insert(
            0,
            "this report has no data to render (no saved data / the query returned no rows) — only its static bands are shown; render with the live preview to draw fresh rows".to_string(),
        );
        diagnostics.extend(
            rendered
                .into_iter()
                .filter(|d| !d.starts_with("formula error: unknown name")),
        );
    } else {
        diagnostics.extend(rendered);
    }
    let pdf = rpt_render::try_render_document(&doc, &rpt_render::PdfOptions::default())
        .map_err(|e| format!("PDF export failed: {e}"))?;
    Ok(PreviewOutput { pdf, page_count, diagnostics })
}

/// Render `.rpt` file bytes to a PDF preview.
///
/// # Errors
///
/// Fails only when `bytes` cannot be decoded as a report, or the PDF writer itself fails —
/// render-level problems (no saved data, formula failures, unsupported object kinds) degrade the
/// output and surface in [`PreviewOutput::diagnostics`] instead.
pub fn preview(bytes: &[u8]) -> Result<PreviewOutput, String> {
    let rpt = crate::open_bytes(bytes)?;
    preview_report(&rpt)
}

/// [`preview_live_report`] over `.rpt` file bytes (decode + fetch + render).
///
/// # Errors
///
/// As [`preview`] plus the live-data failures of [`preview_live_report`].
pub fn preview_live(bytes: &[u8], db: &LiveDb) -> Result<PreviewOutput, String> {
    let rpt = crate::open_bytes(bytes)?;
    preview_live_report(&rpt, db)
}
