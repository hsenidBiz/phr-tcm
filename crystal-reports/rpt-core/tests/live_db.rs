//! Live-database preview tests — everything provable without a server: parameter coercion
//! against a real fixture's declared kinds, the stored-procedure fetch plan over that fixture,
//! and the credential-discipline rule (no error, label, or diagnostic ever contains the
//! password). The live path itself is exercised by the gated smoke test in
//! `rpt-db-mssql/tests/live.rs` (`RPT_MSSQL_CONN`).

use rpt_core::preview::{coerce_params, preview_live, LiveDb};
use rpt_core::open;

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

/// The fixture's `@`-prefixed parameter fields coerce by their declared kinds: the cycle id is
/// a number, the business-unit level a string; names match with or without the `@`,
/// case-insensitively.
#[test]
fn coerce_params_over_fixture() {
    let rpt = open(&fixture("PMSV10_IndPerfOverview.rpt")).expect("fixture opens");
    let params = coerce_params(
        rpt.report(),
        &[
            ("@performance_cycle_id".to_string(), "42".to_string()),
            ("BUSINESS_UNIT_LEVEL".to_string(), "company".to_string()),
        ],
    )
    .expect("coerce");
    let cycle = params.get("performance_cycle_id").expect("normalized key");
    assert_eq!(cycle, &rpt_formula::eval::Value::Number(42.0));
    let level = params.get("business_unit_level").expect("string key");
    assert_eq!(level, &rpt_formula::eval::Value::Str("company".to_string()));
}

/// A value that does not parse for its declared kind is a refusal naming the parameter.
#[test]
fn coerce_params_rejects_bad_numbers() {
    let rpt = open(&fixture("PMSV10_IndPerfOverview.rpt")).expect("fixture opens");
    let e = coerce_params(
        rpt.report(),
        &[("@performance_cycle_id".to_string(), "not-a-number".to_string())],
    )
    .unwrap_err();
    assert!(e.contains("performance_cycle_id"), "`{e}` names the parameter");
}

/// The fetch plan for the fixture's main scope: the proc name with the `;1` marker stripped,
/// `@RETURN_VALUE` excluded, and the resolved parameter values in declaration order.
#[test]
fn plan_over_fixture() {
    use rpt_db_mssql::{plan_for_report, BindValue, TableFetch};

    let rpt = open(&fixture("PMSV10_IndPerfOverview.rpt")).expect("fixture opens");
    let mut params = std::collections::HashMap::new();
    params.insert(
        "performance_cycle_id".to_string(),
        rpt_formula::eval::Value::Number(7.0),
    );
    let plans = plan_for_report(rpt.report(), &params);
    assert_eq!(plans.len(), 1, "one table in the main scope");
    let TableFetch::StoredProcedure { rpc_name, binds } = &plans[0] else {
        panic!("expected a stored-procedure plan, got {:?}", plans[0]);
    };
    assert_eq!(rpc_name, "usp_perf_ind_perf_sheet");
    assert!(!binds.iter().any(|b| b.name.eq_ignore_ascii_case("@RETURN_VALUE")));
    let cycle = binds
        .iter()
        .find(|b| b.name.eq_ignore_ascii_case("@performance_cycle_id"))
        .expect("cycle bind");
    assert_eq!(cycle.value, Some(BindValue::Num(7.0)));
    // Unsupplied proc parameters bind NULL, not an error.
    let level = binds
        .iter()
        .find(|b| b.name.eq_ignore_ascii_case("@business_unit_level"))
        .expect("level bind");
    assert_eq!(level.value, None);
}

/// The credential rule, end to end at the API surface: a connection failure names the
/// server/database (the label) and NEVER the password. Port 1 on localhost is refused
/// instantly — no server, no network dependency.
#[test]
fn preview_live_never_leaks_the_password() {
    let bytes = std::fs::read(fixture("PMSV10_IndPerfOverview.rpt")).expect("fixture bytes");
    let db = LiveDb {
        connection_string: "Server=127.0.0.1,1;Database=probe;User Id=u;Password=sekrit999;TrustServerCertificate=True".to_string(),
        params: Vec::new(),
    };
    let e = preview_live(&bytes, &db).expect_err("connect must fail");
    assert!(!e.contains("sekrit999"), "error leaks the password: {e}");
    assert!(e.contains("127.0.0.1") || e.contains("probe"), "error should name the target: {e}");
}

/// `LiveDb` serializes in the snake_case shape the TS side mirrors.
#[test]
fn live_db_serde_shape() {
    let db = LiveDb {
        connection_string: "Server=s".to_string(),
        params: vec![("performance_cycle_id".to_string(), "42".to_string())],
    };
    let json = serde_json::to_value(&db).unwrap();
    assert_eq!(json["connection_string"], "Server=s");
    assert_eq!(json["params"][0][0], "performance_cycle_id");
}
