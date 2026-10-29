//! The parameter-prompt surface: extraction of the main report's declared parameters from
//! the already-decoded model — the data a front end renders a prompt form from.

use rpt_core::report_parameters;
use rpt_core::open;

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
}

/// The stored-proc report's five `@`-named parameters, in declaration order, with their
/// kinds (the values `coerce_params` parses) — the exact list a live-preview prompt form
/// needs. Subreport `Pm-` parameters are excluded (bound by links, never prompted).
#[test]
fn proc_parameters_extract_with_kinds() {
    let rpt = open(&fixture("PMSV10_GoalAlignCascade.rpt")).expect("fixture opens");
    let params = report_parameters(rpt.report());
    let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "@performance_cycle_id",
            "@business_unit_level",
            "@business_unit",
            "@emp_number_list",
            "@manager_emp_number",
        ],
        "declaration order"
    );
    let cycle = &params[0];
    assert_eq!(cycle.value_kind, "Number");
    assert_eq!(params[1].value_kind, "String");
    assert_eq!(params[3].value_kind, "String");
    assert!(params.iter().all(|p| !p.has_current_value), "saved without values");
    // No `Pm-` parameter leaks in from the SS-style subreports.
    assert!(!params.iter().any(|p| p.name.starts_with("Pm-")));
}

/// The from-scratch blank declares no parameters — an empty prompt form.
#[test]
fn blank_has_no_parameters() {
    let bytes = rpt_core::blank::create_report(&rpt_core::blank::NewReportOptions::default()).unwrap();
    let rpt = rpt_core::open_bytes(&bytes).unwrap();
    assert!(report_parameters(rpt.report()).is_empty());
}
