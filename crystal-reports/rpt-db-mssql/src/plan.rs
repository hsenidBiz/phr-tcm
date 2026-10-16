//! The fetch plan — pure, socket-free: which stored procedure (or SQL command) feeds a
//! report scope, with which bound parameters. Everything here is testable without a server.

use rpt_data::{normalize_param_name, Parameters};
use rpt_formula::eval::Value;
use rpt_model::{FieldValueType, Report, Table};

/// One bound proc parameter. `value_type` is the table's declaration (kept for the bind
/// decision and diagnostics); the value comes from the caller's [`Parameters`] resolved by
/// name, falling back to NULL.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamBind {
    /// The parameter name as the table declares it (usually `@`-prefixed).
    pub name: String,
    /// The table's declared value type.
    pub value_type: FieldValueType,
    /// The resolved value to bind; `None` binds NULL.
    pub value: Option<BindValue>,
}

/// A scalar in tiberius-bindable form.
#[derive(Debug, Clone, PartialEq)]
pub enum BindValue {
    /// Text (SQL Server converts to the proc parameter's type).
    Str(String),
    /// A number (bound as float8; SQL Server converts to int/decimal/…).
    Num(f64),
    /// A boolean.
    Bool(bool),
    /// Raw bytes.
    Bytes(Vec<u8>),
}

impl BindValue {
    /// A short display form for diagnostics — values are report parameters, not credentials.
    pub fn display(&self) -> String {
        match self {
            BindValue::Str(s) => {
                if s.len() > 40 {
                    format!("\"{}…\"", &s[..37])
                } else {
                    format!("\"{s}\"")
                }
            }
            BindValue::Num(n) => n.to_string(),
            BindValue::Bool(b) => b.to_string(),
            BindValue::Bytes(b) => format!("<{} bytes>", b.len()),
        }
    }
}

/// How one table is fetched.
#[derive(Debug, Clone, PartialEq)]
pub enum TableFetch {
    /// A stored-procedure table (`usp_…;1`): an RPC call by name with bound parameters.
    StoredProcedure {
        /// The proc name with the `;N` result-set marker stripped.
        rpc_name: String,
        /// Parameters in declaration order, `@RETURN_VALUE` excluded.
        binds: Vec<ParamBind>,
    },
    /// A SQL-command table: the author's command text, verbatim.
    Command { sql: String },
    /// Not fetchable by this driver; the reason says why.
    NotSupported { table: String, reason: String },
}

impl TableFetch {
    /// A one-line description for logs/diagnostics (no credentials involved).
    pub fn describe(&self) -> String {
        match self {
            TableFetch::StoredProcedure { rpc_name, binds } => {
                let p: Vec<String> = binds
                    .iter()
                    .map(|b| match &b.value {
                        Some(v) => format!("{}={}", b.name, v.display()),
                        None => format!("{}=NULL", b.name),
                    })
                    .collect();
                format!("EXEC {rpc_name} {}", p.join(", "))
            }
            TableFetch::Command { sql } => format!("command ({} chars)", sql.len()),
            TableFetch::NotSupported { table, reason } => format!("{table}: {reason}"),
        }
    }
}

/// A `@`-prefixed proc parameter name normalized for lookup: lowercase, `@` and the formula
/// decorations stripped — so the report field `@performance_cycle_id`, the caller's
/// `performance_cycle_id`, and a formula's `{?@Performance_Cycle_ID}` all match. Public so
/// the coercion layer (rpt-core) keys its values identically to the bind resolution here.
pub fn param_key(name: &str) -> String {
    let n = normalize_param_name(name);
    n.strip_prefix('@').unwrap_or(&n).to_string()
}

/// Classify one table.
pub fn plan_table(table: &Table) -> TableFetch {
    if let Some(cmd) = table.command_text.as_deref().filter(|c| !c.is_empty()) {
        if cmd.contains("{?") {
            return TableFetch::NotSupported {
                table: table.name.clone(),
                reason: "the SQL command references report parameters ({?…}), which are not substituted in v1".to_string(),
            };
        }
        return TableFetch::Command { sql: cmd.to_string() };
    }
    let is_proc = table.name.contains(';') || !table.parameters.is_empty();
    if !is_proc {
        return TableFetch::NotSupported {
            table: table.name.clone(),
            reason: "plain joined tables are not generated for SQL Server in v1 (stored procedures and SQL commands are)".to_string(),
        };
    }
    let rpc_name = table.name.split(';').next().unwrap_or(&table.name).to_string();
    let binds = table
        .parameters
        .iter()
        .filter(|p| !param_key(&p.name).is_empty() && !param_key(&p.name).starts_with("return_value"))
        .map(|p| ParamBind {
            name: p.name.clone(),
            value_type: p.value_type,
            value: None,
        })
        .collect();
    TableFetch::StoredProcedure { rpc_name, binds }
}

/// Plan every table of a report scope (main report or a subreport), resolving parameter
/// values from `params` by name. Multi-value arrays of strings join with `,` (the common
/// `@emp_number_list` convention); any other array is a refusal.
pub fn plan_for_report(report: &Report, params: &Parameters) -> Vec<TableFetch> {
    report
        .database
        .tables
        .iter()
        .map(|t| match plan_table(t) {
            TableFetch::StoredProcedure { rpc_name, binds } => TableFetch::StoredProcedure {
                rpc_name,
                binds: binds
                    .into_iter()
                    .map(|mut b| {
                        b.value = params.get(&param_key(&b.name)).and_then(value_to_bind);
                        b
                    })
                    .collect(),
            },
            other => other,
        })
        .collect()
}

/// Convert a resolved parameter [`Value`] to a bind scalar. `None` means "cannot express"
/// (caller treats it as NULL rather than failing the whole render).
fn value_to_bind(v: &Value) -> Option<BindValue> {
    match v {
        Value::Str(s) => Some(BindValue::Str(s.clone())),
        Value::Number(n) => Some(BindValue::Num(*n)),
        Value::Currency(n) => Some(BindValue::Num(*n)),
        Value::Bool(b) => Some(BindValue::Bool(*b)),
        Value::Bytes(b) => Some(BindValue::Bytes(b.clone())),
        // ISO text; SQL Server converts to the parameter's date/datetime type.
        Value::Date(d) => Some(BindValue::Str(format!("{:04}-{:02}-{:02}", d.year, d.month, d.day))),
        Value::Time(t) => Some(BindValue::Str(format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second))),
        Value::DateTime(d, t) => Some(BindValue::Str(format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            d.year, d.month, d.day, t.hour, t.minute, t.second
        ))),
        Value::Null => None,
        Value::Array(items) => {
            let strs: Option<Vec<String>> = items.iter().map(value_to_display_string).collect();
            strs.map(|s| BindValue::Str(s.join(",")))
        }
        Value::Range { .. } => None,
    }
}

fn value_to_display_string(v: &Value) -> Option<String> {
    match value_to_bind(v) {
        Some(BindValue::Str(s)) => Some(s),
        Some(BindValue::Num(n)) => Some(n.to_string()),
        Some(BindValue::Bool(b)) => Some(b.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn table(name: &str, params: &[(&str, FieldValueType)], command: Option<&str>) -> Table {
        Table {
            name: name.to_string(),
            alias: "t".to_string(),
            class_name: None,
            qualified_name: None,
            connection: Default::default(),
            data_fields: Vec::new(),
            command_text: command.map(str::to_string),
            parameters: params
                .iter()
                .map(|(n, t)| rpt_model::CommandParameter {
                    name: n.to_string(),
                    value_type: *t,
                })
                .collect(),
        }
    }

    #[test]
    fn proc_table_plans_to_rpc_with_marker_stripped() {
        let t = table(
            "usp_perf_ind_perf_sheet;1",
            &[
                ("@RETURN_VALUE", FieldValueType::Number),
                ("@performance_cycle_id", FieldValueType::Number),
                ("@business_unit_level", FieldValueType::String),
            ],
            None,
        );
        let TableFetch::StoredProcedure { rpc_name, binds } = plan_table(&t) else {
            panic!("expected a proc plan");
        };
        assert_eq!(rpc_name, "usp_perf_ind_perf_sheet");
        // @RETURN_VALUE excluded, order preserved.
        let names: Vec<&str> = binds.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["@performance_cycle_id", "@business_unit_level"]);
    }

    #[test]
    fn binds_resolve_by_name_with_or_without_the_at() {
        let t = table("p;1", &[("@performance_cycle_id", FieldValueType::Number)], None);
        let mut params = HashMap::new();
        params.insert("performance_cycle_id".to_string(), Value::Number(7.0));
        let planned = plan_for_report(
            &Report {
                database: rpt_model::Database {
                    tables: vec![t],
                    ..Default::default()
                },
                ..Default::default()
            },
            &params,
        );
        let TableFetch::StoredProcedure { binds, .. } = &planned[0] else { panic!() };
        assert_eq!(binds[0].value, Some(BindValue::Num(7.0)));
    }

    #[test]
    fn unsupplied_binds_null_and_command_tables_stay_verbatim() {
        let planned = plan_for_report(
            &Report {
                database: rpt_model::Database {
                    tables: vec![
                        table("p;1", &[("@x", FieldValueType::String)], None),
                        table("cmd", &[], Some("SELECT 1 AS one")),
                        table("plain", &[], None),
                    ],
                    ..Default::default()
                },
                ..Default::default()
            },
            &Parameters::new(),
        );
        assert!(matches!(&planned[0], TableFetch::StoredProcedure { binds, .. } if binds[0].value.is_none()));
        assert!(matches!(&planned[1], TableFetch::Command { sql } if sql == "SELECT 1 AS one"));
        assert!(matches!(&planned[2], TableFetch::NotSupported { .. }));
    }

    #[test]
    fn multi_value_string_arrays_join_with_commas() {
        let v = value_to_bind(&Value::Array(vec![Value::Str("a".into()), Value::Str("b".into())]));
        assert_eq!(v, Some(BindValue::Str("a,b".into())));
    }
}
