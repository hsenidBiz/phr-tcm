//! The hidden module's parameters: the open report lists them, and a
//! saved-data preview takes values for them, checked before anything is
//! drawn and kept for that drawing only.

use std::path::Path;

use v2_lib::commands::crystal as screen;
use v2_lib::commands::crystal::ParameterValue;
use v2_lib::crystal;

use crate::crystal_reports::{fitting_values, typed_parameter, with_value};
use crate::serial;

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../crystal-reports/Sample Report");
const SAMPLE: &str = "PMSV10_GoalAlignCascade.rpt";

fn copy_sample(dir: &Path) -> String {
    let to = dir.join(SAMPLE);
    std::fs::copy(Path::new(SAMPLES).join(SAMPLE), &to).expect("the sample copies");
    to.to_string_lossy().into_owned()
}

#[test]
fn the_sample_lists_its_five_parameters() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    crystal::open(&copy_sample(dir.path())).unwrap();
    let listed = crystal::parameters().unwrap();
    assert_eq!(listed.len(), 5);
    assert_eq!(listed[0].name, "@performance_cycle_id");
    assert_eq!(listed[0].value_kind, "Number");
    crystal::close();
}

#[test]
fn nothing_open_lists_no_parameters() {
    let _lock = serial::crystal();
    crystal::close();
    assert!(crystal::parameters().unwrap_err().contains("No report is open"));
}

#[test]
fn a_preview_with_values_draws_pages() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path());
    crystal::open(&path).unwrap();
    let info = crystal::preview_saved_with(fitting_values(&path)).unwrap();
    assert!(info.page_count > 0, "{info:?}");
    assert!(crystal::last_pdf().unwrap().1.starts_with(b"%PDF"));
    crystal::close();
}

#[test]
fn a_bad_value_or_unknown_name_is_refused_and_the_last_preview_stays() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path());
    crystal::open(&path).unwrap();
    crystal::preview_saved_with(fitting_values(&path)).unwrap();
    let before = crystal::last_pdf().unwrap();

    let name = typed_parameter(&path);
    let bad = with_value(fitting_values(&path), &name, "not-a-number");
    let said = crystal::preview_saved_with(bad).unwrap_err();
    assert!(said.contains(&name) && said.contains("cannot be previewed with these values"), "{said}");

    let said = crystal::preview_saved_with(vec![("perfomance_cycle".into(), "1".into())]).unwrap_err();
    assert!(said.contains("perfomance_cycle"), "{said}");

    let after = crystal::last_pdf().unwrap();
    assert_eq!(after.1, before.1);
    assert_eq!(after.0.page_count, before.0.page_count);
    crystal::close();
}

#[test]
fn values_are_not_saved_into_the_file() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path());
    let original = std::fs::read(&path).unwrap();
    crystal::open(&path).unwrap();
    crystal::preview_saved_with(fitting_values(&path)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    crystal::close();
}

#[test]
fn no_values_is_todays_preview() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    crystal::open(&copy_sample(dir.path())).unwrap();
    let plain = crystal::preview_saved().unwrap();
    let none = crystal::preview_saved_with(vec![]).unwrap();
    assert_eq!(plain.page_count, none.page_count);
    assert_eq!(plain.diagnostics, none.diagnostics);
    crystal::close();
}

#[test]
fn the_screen_commands_follow_the_module_switch_and_drop_blank_values() {
    let _e = serial::extras();
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    crystal::open(&copy_sample(dir.path())).unwrap();
    let off = v2_lib::modules::Module::CrystalReports.off_sentence();
    assert_eq!(screen::parameters_with(false).unwrap_err(), off);
    assert_eq!(screen::draw_with(false, vec![]).unwrap_err(), off);
    assert_eq!(screen::parameters_with(true).unwrap().len(), 5);
    screen::draw_with(true, vec![ParameterValue { name: "@performance_cycle_id".into(), value: "  ".into() }])
        .expect("a blank value is dropped, so this is a plain preview");
    crystal::close();
}

#[test]
fn a_screen_refusal_names_no_path() {
    let _e = serial::extras();
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path());
    crystal::open(&path).unwrap();
    let said = screen::draw_with(true, vec![ParameterValue { name: "nope".into(), value: "1".into() }]).unwrap_err();
    assert!(said.contains("nope"), "{said}");
    assert!(!said.contains(dir.path().to_string_lossy().as_ref()), "{said}");
    crystal::close();
}

#[test]
fn numbers_that_are_not_finite_are_refused_before_drawing() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path());
    crystal::open(&path).unwrap();
    crystal::preview_saved_with(fitting_values(&path)).unwrap();
    let before = crystal::last_pdf().unwrap();
    let name = typed_parameter(&path);
    for bad in ["NaN", "inf", "-infinity", "1e309"] {
        let said = crystal::preview_saved_with(with_value(fitting_values(&path), &name, bad)).unwrap_err();
        assert!(said.contains(&name) && said.contains("not a number"), "{bad}: {said}");
    }
    assert_eq!(crystal::last_pdf().unwrap().1, before.1);
    crystal::close();
}

#[test]
fn dates_out_of_range_are_refused() {
    use rpt_core::rpt_reader::model::ParameterValueKind as K;
    for ok in ["2024-02-29", "2026-01-31", "0001-01-01", "9999-12-31"] {
        crystal::check_typed_value(K::DateParameter, ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
    }
    for bad in ["2024-13-45", "2023-02-29", "2026-00-10", "2026-04-31", "2026-01-00", "0000-01-01", "10000-01-01"] {
        let said = crystal::check_typed_value(K::DateParameter, bad).unwrap_err();
        assert!(said.contains(bad) && said.contains("not a real date"), "{bad}: {said}");
    }
    crystal::check_typed_value(K::DateTimeParameter, "2026-01-31T10:30:00").unwrap();
    assert!(crystal::check_typed_value(K::DateTimeParameter, "2026-02-30 10:30:00").is_err());
    assert!(crystal::check_typed_value(K::DateTimeParameter, "2026-01-31T25:00:00").is_err());
    assert!(crystal::check_typed_value(K::TimeParameter, "24:00:00").is_err());
    assert!(crystal::check_typed_value(K::TimeParameter, "10:61").is_err());
    crystal::check_typed_value(K::TimeParameter, "23:59:59").unwrap();
    crystal::check_typed_value(K::NumberParameter, "-12.5e3").unwrap();
}

#[test]
fn the_screen_never_shows_an_assistant_only_sentence() {
    let _e = serial::extras();
    let _lock = serial::crystal();
    crystal::close();
    let said = [
        screen::parameters_with(true).map(|_| ()).unwrap_err(),
        screen::draw_with(true, vec![]).unwrap_err(),
        screen::find_with(true, "a", false).map(|_| ()).unwrap_err(),
        screen::replace_with(true, "a", "b", false, true, None).map(|_| ()).unwrap_err(),
    ];
    for s in said {
        assert!(s.contains("No report is open") && !s.contains("open_report"), "{s}");
    }
}
