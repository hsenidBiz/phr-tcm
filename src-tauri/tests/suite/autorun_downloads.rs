//! Checking a downloaded file: its name, and for a spreadsheet, CSV or text
//! file what is in it. All of it is pure - it reads the committed fixtures
//! under `tests/fixtures/downloads/` and a few files written to a temporary
//! folder, and never starts a browser.

use std::path::{Path, PathBuf};
use v2_lib::autorun::downloads::{
    check_file, name_matches, parse_a1, CellCheck, DownloadCheck, HeaderCheck, MAX_CHECK_BYTES,
};
use v2_lib::test_files::human_size;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/downloads").join(name)
}

fn size_of(path: &Path) -> String {
    human_size(std::fs::metadata(path).unwrap().len())
}

fn named(name: &str) -> DownloadCheck {
    DownloadCheck {
        name: name.to_string(),
        sheet: None,
        headers: None,
        cells: Vec::new(),
        contains_text: Vec::new(),
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn exact(items: &[&str]) -> Option<HeaderCheck> {
    Some(HeaderCheck::Exact(strings(items)))
}

fn contains(items: &[&str]) -> Option<HeaderCheck> {
    Some(HeaderCheck::Contains(strings(items)))
}

fn cell(r: &str, text: &str, contains: bool) -> CellCheck {
    CellCheck { r#ref: r.to_string(), text: text.to_string(), contains }
}

/// The passed sentence for a fixture, with its real size.
fn passed(file: &str, clauses: &[&str]) -> String {
    let mut s = format!("downloaded \"{file}\" ({})", size_of(&fixture(file)));
    for c in clauses {
        s.push_str(", ");
        s.push_str(c);
    }
    s
}

// ---------------------------------------------------------------- names

#[test]
fn a_name_without_a_star_must_match_the_whole_name_ignoring_case() {
    assert!(name_matches("Template.xlsx", "Template.xlsx"));
    assert!(name_matches("template.XLSX", "Template.xlsx"));
    assert!(!name_matches("Template", "Template.xlsx"));
    assert!(!name_matches("Template.xlsx", "My Template.xlsx"));
    assert!(!name_matches("Template.xlsx", "Template.xlsx.crdownload"));
}

#[test]
fn a_star_stands_for_any_run_of_characters() {
    assert!(name_matches("Template*.xlsx", "Template.xlsx"));
    assert!(name_matches("Template*.xlsx", "Template (2).xlsx"));
    assert!(name_matches("template*.xlsx", "TEMPLATE_2026-10-05.XLSX"));
    assert!(name_matches("*", "anything at all.bin"));
    assert!(name_matches("*.csv", "error log.csv"));
    assert!(name_matches("*log*", "Import log 5.txt"));
    assert!(name_matches("a*b*c", "a--b--b--c"));
    assert!(!name_matches("Template*.xlsx", "MyTemplate.xlsx"));
    assert!(!name_matches("Template*.xlsx", "Template.xlsx.part"));
    assert!(!name_matches("*.csv", "errors.txt"));
}

#[test]
fn every_other_character_in_a_name_is_taken_literally() {
    assert!(name_matches("report (1).csv", "Report (1).csv"));
    assert!(!name_matches("a.b", "axb"));
    assert!(!name_matches("a?c.txt", "abc.txt"));
    assert!(name_matches("a?c.txt", "a?c.txt"));
    assert!(name_matches("[x]*.csv", "[x] list.csv"));
}

// ---------------------------------------------------------------- A1 refs

#[test]
fn an_a1_reference_is_a_zero_based_row_and_column() {
    assert_eq!(parse_a1("A1"), Some((0, 0)));
    assert_eq!(parse_a1("B2"), Some((1, 1)));
    assert_eq!(parse_a1("Z1"), Some((0, 25)));
    assert_eq!(parse_a1("AA10"), Some((9, 26)));
    assert_eq!(parse_a1("b2"), Some((1, 1)));
    assert_eq!(parse_a1("XFD1048576"), Some((1_048_575, 16_383)));
}

#[test]
fn a_reference_that_is_not_a1_style_is_refused() {
    for bad in ["", "1A", "A0", "A", "1", "A1B", " A1", "A 1", "A-1", "XFE1", "A1048577", "$A$1"] {
        assert_eq!(parse_a1(bad), None, "{bad:?}");
    }
}

// ---------------------------------------------------------------- name and size

#[test]
fn a_check_of_the_name_alone_says_what_was_downloaded_and_its_size() {
    let path = fixture("comma.csv");
    assert_eq!(check_file(&path, "comma.csv", &named("*.csv")), Ok(passed("comma.csv", &[])));
}

#[test]
fn a_file_with_another_name_says_what_it_got() {
    let path = fixture("comma.csv");
    assert_eq!(
        check_file(&path, "Report.csv", &named("Template*.xlsx")),
        Err("got \"Report.csv\", expected a file named \"Template*.xlsx\"".to_string())
    );
}

#[test]
fn a_file_over_the_cap_is_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.csv");
    let f = std::fs::File::create(&path).unwrap();
    f.set_len(MAX_CHECK_BYTES + 1).unwrap();
    drop(f);
    let mut check = named("big.csv");
    check.headers = exact(&["a"]);
    assert_eq!(
        check_file(&path, "big.csv", &check),
        Err(format!("\"big.csv\" is {}, over the 50 MB that can be checked", human_size(MAX_CHECK_BYTES + 1)))
    );
    // Only reading it is capped: its name can still be checked.
    assert_eq!(
        check_file(&path, "big.csv", &named("big.csv")),
        Ok(format!("downloaded \"big.csv\" ({})", human_size(MAX_CHECK_BYTES + 1)))
    );
}

#[test]
fn the_cap_is_fifty_megabytes() {
    assert_eq!(MAX_CHECK_BYTES, 50 * 1024 * 1024);
}

// ---------------------------------------------------------------- headers (Review Focus 4)

#[test]
fn exact_headers_pass_on_every_spreadsheet_kind() {
    let want = ["Employee No", "Name", "Department"];
    for file in ["template.xlsx", "legacy.xls", "comma.csv", "comma-bom.csv"] {
        let mut check = named(file);
        check.headers = exact(&want);
        assert_eq!(check_file(&fixture(file), file, &check), Ok(passed(file, &["headers match"])), "{file}");
    }
}

/// Review Focus 4: a European CSV - semicolons, Windows-1252, accents in
/// its header - reads its headers as the person sees them.
#[test]
fn a_semicolon_windows_1252_csv_reads_its_accented_headers() {
    let file = "semicolon-1252.csv";
    let mut check = named(file);
    check.headers = exact(&["Numéro", "Nom", "Département"]);
    assert_eq!(check_file(&fixture(file), file, &check), Ok(passed(file, &["headers match"])));
}

/// The byte-order mark is not part of the first header.
#[test]
fn a_utf8_csv_with_a_bom_reads_its_first_header_without_it() {
    let file = "comma-bom.csv";
    let mut check = named(file);
    check.headers = exact(&["Employee No", "Name"]);
    assert_eq!(
        check_file(&fixture(file), file, &check),
        Err("headers are [\"Employee No\", \"Name\", \"Department\"], expected [\"Employee No\", \"Name\"]".to_string())
    );
}

#[test]
fn exact_headers_in_another_order_fail_naming_both() {
    for file in ["template.xlsx", "legacy.xls", "comma.csv"] {
        let mut check = named(file);
        check.headers = exact(&["Name", "Employee No", "Department"]);
        assert_eq!(
            check_file(&fixture(file), file, &check),
            Err("headers are [\"Employee No\", \"Name\", \"Department\"], expected [\"Name\", \"Employee No\", \"Department\"]"
                .to_string()),
            "{file}"
        );
    }
}

#[test]
fn contains_headers_pass_in_any_order() {
    for file in ["template.xlsx", "legacy.xls", "comma.csv", "comma-bom.csv"] {
        let mut check = named(file);
        check.headers = contains(&["Department", "Employee No"]);
        assert_eq!(check_file(&fixture(file), file, &check), Ok(passed(file, &["headers match"])), "{file}");
    }
    let file = "semicolon-1252.csv";
    let mut check = named(file);
    check.headers = contains(&["Département"]);
    assert_eq!(check_file(&fixture(file), file, &check), Ok(passed(file, &["headers match"])));
}

#[test]
fn contains_headers_with_one_missing_fail_naming_both() {
    let mut check = named("template.xlsx");
    check.headers = contains(&["Department", "Manager"]);
    assert_eq!(
        check_file(&fixture("template.xlsx"), "template.xlsx", &check),
        Err("headers are [\"Employee No\", \"Name\", \"Department\"], expected [\"Department\", \"Manager\"] among them"
            .to_string())
    );
}

#[test]
fn a_tab_delimited_csv_is_read_by_its_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tabs.csv");
    std::fs::write(&path, "Employee No\tName, first\tDepartment\nE1\tJane\tSales\n").unwrap();
    let mut check = named("tabs.csv");
    check.headers = exact(&["Employee No", "Name, first", "Department"]);
    check.cells = vec![cell("C2", "Sales", false)];
    let size = human_size(std::fs::metadata(&path).unwrap().len());
    assert_eq!(
        check_file(&path, "tabs.csv", &check),
        Ok(format!("downloaded \"tabs.csv\" ({size}), headers match, C2 is \"Sales\""))
    );
}

#[test]
fn quoted_header_text_never_runs_past_two_hundred_characters() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wide.csv");
    let long = "y".repeat(300);
    std::fs::write(&path, format!("{long},b\n1,2\n")).unwrap();
    let mut check = named("wide.csv");
    check.headers = exact(&["a", "b"]);
    let err = check_file(&path, "wide.csv", &check).unwrap_err();
    assert!(err.starts_with("headers are [\"yyy"), "{err}");
    assert!(!err.contains(&"y".repeat(201)), "{err}");
}

// ---------------------------------------------------------------- cells

#[test]
fn an_exact_cell_passes_on_every_spreadsheet_kind() {
    for (file, r) in [("template.xlsx", "B2"), ("legacy.xls", "B2"), ("comma.csv", "B3")] {
        let mut check = named(file);
        check.cells = vec![cell(r, "Employee Name", false)];
        let clause = format!("{r} is \"Employee Name\"");
        assert_eq!(check_file(&fixture(file), file, &check), Ok(passed(file, &[&clause])), "{file}");
    }
}

#[test]
fn a_contains_cell_passes_on_part_of_the_text() {
    let mut check = named("template.xlsx");
    check.cells = vec![cell("B2", "Name", true)];
    assert_eq!(
        check_file(&fixture("template.xlsx"), "template.xlsx", &check),
        Ok(passed("template.xlsx", &["B2 contains \"Name\""]))
    );
}

#[test]
fn a_number_reads_as_it_is_shown() {
    for file in ["template.xlsx", "legacy.xls"] {
        let mut check = named(file);
        check.cells = vec![cell("A3", "1001", false)];
        assert_eq!(check_file(&fixture(file), file, &check), Ok(passed(file, &["A3 is \"1001\""])), "{file}");
    }
}

#[test]
fn a_semicolon_csv_keeps_a_comma_inside_a_cell() {
    let file = "semicolon-1252.csv";
    let mut check = named(file);
    check.cells = vec![cell("B2", "Hélène Dupont", false), cell("C2", "Ventes, Est", false)];
    assert_eq!(
        check_file(&fixture(file), file, &check),
        Ok(passed(file, &["B2 is \"Hélène Dupont\"", "C2 is \"Ventes, Est\""]))
    );
}

#[test]
fn a_wrong_exact_cell_fails_with_what_it_is() {
    let mut check = named("template.xlsx");
    check.cells = vec![cell("B2", "Employee Number", false)];
    assert_eq!(
        check_file(&fixture("template.xlsx"), "template.xlsx", &check),
        Err("B2 is \"Employee Name\", expected \"Employee Number\"".to_string())
    );
}

#[test]
fn a_wrong_contains_cell_fails_with_what_it_is() {
    let mut check = named("legacy.xls");
    check.cells = vec![cell("B2", "Surname", true)];
    assert_eq!(
        check_file(&fixture("legacy.xls"), "legacy.xls", &check),
        Err("B2 is \"Employee Name\", expected it to contain \"Surname\"".to_string())
    );
}

#[test]
fn a_cell_past_the_data_is_empty() {
    let mut check = named("comma.csv");
    check.cells = vec![cell("Z99", "x", false)];
    assert_eq!(
        check_file(&fixture("comma.csv"), "comma.csv", &check),
        Err("Z99 is \"\", expected \"x\"".to_string())
    );
}

#[test]
fn quoted_cell_text_never_runs_past_two_hundred_characters() {
    let mut check = named("template.xlsx");
    check.cells = vec![cell("C3", "short", false)];
    let err = check_file(&fixture("template.xlsx"), "template.xlsx", &check).unwrap_err();
    assert!(err.starts_with("C3 is \"Long note xxx"), "{err}");
    assert!(err.ends_with("\", expected \"short\""), "{err}");
    let quoted = &err["C3 is \"".len()..err.len() - "\", expected \"short\"".len()];
    assert!(quoted.chars().filter(|c| *c != '.').count() <= 200, "{quoted}");
    assert!(quoted.ends_with("..."), "{quoted}");
}

#[test]
fn the_first_failing_check_is_the_one_said() {
    let mut check = named("template.xlsx");
    check.headers = exact(&["Employee No", "Name", "Department"]);
    check.cells = vec![cell("B2", "Employee Name", false), cell("A2", "nope", false), cell("C2", "also nope", false)];
    assert_eq!(
        check_file(&fixture("template.xlsx"), "template.xlsx", &check),
        Err("A2 is \"Employee Number\", expected \"nope\"".to_string())
    );
}

#[test]
fn every_passing_check_adds_its_clause_in_order() {
    let mut check = named("Template*.xlsx");
    check.sheet = Some("Employees".to_string());
    check.headers = exact(&["Employee No", "Name", "Department"]);
    check.cells = vec![cell("B2", "Employee Name", false), cell("C2", "Department", true)];
    assert_eq!(
        check_file(&fixture("template.xlsx"), "Template.xlsx", &check),
        Ok(format!(
            "downloaded \"Template.xlsx\" ({}), headers match, B2 is \"Employee Name\", C2 contains \"Department\"",
            size_of(&fixture("template.xlsx"))
        ))
    );
}

// ---------------------------------------------------------------- sheets

#[test]
fn a_named_sheet_is_the_one_read() {
    for file in ["template.xlsx", "legacy.xls"] {
        let mut check = named(file);
        check.sheet = Some("Lookups".to_string());
        check.headers = exact(&["Department", "Code"]);
        assert_eq!(check_file(&fixture(file), file, &check), Ok(passed(file, &["headers match"])), "{file}");
    }
}

#[test]
fn a_sheet_name_ignores_case() {
    let mut check = named("template.xlsx");
    check.sheet = Some("lookups".to_string());
    check.cells = vec![cell("A2", "Sales", false)];
    assert_eq!(
        check_file(&fixture("template.xlsx"), "template.xlsx", &check),
        Ok(passed("template.xlsx", &["A2 is \"Sales\""]))
    );
}

#[test]
fn a_missing_sheet_lists_the_sheets_the_file_has() {
    for file in ["template.xlsx", "legacy.xls"] {
        let mut check = named(file);
        check.sheet = Some("Staff".to_string());
        check.headers = exact(&["Employee No"]);
        assert_eq!(
            check_file(&fixture(file), file, &check),
            Err(format!("sheet \"Staff\" is not in \"{file}\" (it has: Employees, Lookups)")),
            "{file}"
        );
    }
}

#[test]
fn a_csv_has_one_sheet_and_ignores_the_sheet_named() {
    let mut check = named("comma.csv");
    check.sheet = Some("Anything".to_string());
    check.cells = vec![cell("A2", "E001", false)];
    assert_eq!(check_file(&fixture("comma.csv"), "comma.csv", &check), Ok(passed("comma.csv", &["A2 is \"E001\""])));
}

// ---------------------------------------------------------------- text

#[test]
fn text_in_a_txt_file_is_found() {
    let mut check = named("errors.txt");
    check.contains_text = strings(&["Row 4: Department is required", "Row 2: Saved"]);
    assert_eq!(
        check_file(&fixture("errors.txt"), "errors.txt", &check),
        Ok(passed("errors.txt", &["it contains \"Row 4: Department is required\"", "it contains \"Row 2: Saved\""]))
    );
}

#[test]
fn text_a_txt_file_lacks_fails_naming_the_text() {
    let mut check = named("errors.txt");
    check.contains_text = strings(&["Row 4: Department is required", "Row 5: Name is required"]);
    assert_eq!(
        check_file(&fixture("errors.txt"), "errors.txt", &check),
        Err("\"errors.txt\" does not contain \"Row 5: Name is required\"".to_string())
    );
}

#[test]
fn text_in_a_csv_is_found_in_its_raw_text() {
    let mut check = named("comma.csv");
    check.contains_text = strings(&["Smith, Jane"]);
    assert_eq!(
        check_file(&fixture("comma.csv"), "comma.csv", &check),
        Ok(passed("comma.csv", &["it contains \"Smith, Jane\""]))
    );
    let file = "semicolon-1252.csv";
    let mut check = named(file);
    check.contains_text = strings(&["Hélène"]);
    assert_eq!(check_file(&fixture(file), file, &check), Ok(passed(file, &["it contains \"Hélène\""])));
}

#[test]
fn a_windows_1252_text_file_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.txt");
    // "Café: ligne 4 rejetée" in Windows-1252: é is 0xE9, not valid UTF-8.
    std::fs::write(&path, b"Caf\xe9: ligne 4 rejet\xe9e\r\n").unwrap();
    let mut check = named("log.txt");
    check.contains_text = strings(&["ligne 4 rejetée"]);
    assert_eq!(
        check_file(&path, "log.txt", &check),
        Ok(format!("downloaded \"log.txt\" ({}), it contains \"ligne 4 rejetée\"", human_size(23)))
    );
}

// ---------------------------------------------------------------- what cannot be read

#[test]
fn an_unreadable_xlsx_says_why_without_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.xlsx");
    std::fs::write(&path, b"this is not a zip file").unwrap();
    let mut check = named("broken.xlsx");
    check.headers = exact(&["a"]);
    let err = check_file(&path, "broken.xlsx", &check).unwrap_err();
    assert!(err.starts_with("\"broken.xlsx\" could not be read as a spreadsheet: "), "{err}");
    assert!(err.len() > "\"broken.xlsx\" could not be read as a spreadsheet: ".len(), "{err}");
    let dir_text = dir.path().to_string_lossy().to_string();
    assert!(!err.contains(&dir_text), "{err}");
}

#[test]
fn spreadsheet_checks_on_a_text_file_say_it_is_not_a_spreadsheet() {
    let mut check = named("errors.txt");
    check.cells = vec![cell("A1", "x", false)];
    let err = check_file(&fixture("errors.txt"), "errors.txt", &check).unwrap_err();
    assert!(err.starts_with("\"errors.txt\" could not be read as a spreadsheet: "), "{err}");
}

#[test]
fn text_checks_on_a_workbook_say_it_is_not_text() {
    let mut check = named("template.xlsx");
    check.contains_text = strings(&["Employee"]);
    let err = check_file(&fixture("template.xlsx"), "template.xlsx", &check).unwrap_err();
    assert!(err.starts_with("\"template.xlsx\" could not be read as text: "), "{err}");
}

#[test]
fn a_missing_file_says_it_could_not_be_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gone.csv");
    let err = check_file(&path, "gone.csv", &named("gone.csv")).unwrap_err();
    assert!(err.starts_with("\"gone.csv\" could not be read"), "{err}");
    assert!(!err.contains(&dir.path().to_string_lossy().to_string()), "{err}");
}
