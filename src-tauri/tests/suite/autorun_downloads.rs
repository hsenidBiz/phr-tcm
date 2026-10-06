//! Checking a downloaded file: its name, and for a spreadsheet, CSV, text
//! or PDF file what is in it. All of it is pure - it reads the committed
//! fixtures under `tests/fixtures/downloads/` and a few files written to a
//! temporary folder, and never starts a browser.

use std::path::{Path, PathBuf};
use v2_lib::autorun::downloads::{
    check_file, name_matches, normalise, parse_a1, pdf_pages, pdf_size_gate, CellCheck, DownloadCheck, HeaderCheck,
    OnPage, PageCount, PdfCheck, MAX_CHECK_BYTES, PDF_UNREADABLE,
};
use v2_lib::browser::actions::Action;
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
        pdf: None,
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

/// A second download of one name is kept numbered; the sentence names both,
/// so a person opening the run's folder finds the one that was checked.
#[test]
fn a_numbered_file_says_the_name_it_was_saved_as() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x (2).csv");
    std::fs::copy(fixture("comma.csv"), &path).unwrap();
    assert_eq!(
        check_file(&path, "x.csv", &named("x.csv")),
        Ok(format!("downloaded \"x.csv\" (saved as \"x (2).csv\", {})", size_of(&path)))
    );
    // Kept under its own name, the sentence is the plain one.
    let plain = dir.path().join("x.csv");
    std::fs::copy(fixture("comma.csv"), &plain).unwrap();
    assert_eq!(check_file(&plain, "x.csv", &named("x.csv")), Ok(format!("downloaded \"x.csv\" ({})", size_of(&plain))));
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

// ---------------------------------------------------------------- damaged files never crash

#[test]
fn a_truncated_xlsx_is_reported_not_a_crash() {
    let whole = std::fs::read(fixture("template.xlsx")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("half.xlsx");
    std::fs::write(&path, &whole[..whole.len() / 2]).unwrap();
    let mut check = named("half.xlsx");
    check.headers = exact(&["Employee No"]);
    let err = check_file(&path, "half.xlsx", &check).unwrap_err();
    assert!(err.starts_with("\"half.xlsx\" could not be read as a spreadsheet: "), "{err}");
}

#[test]
fn a_corrupt_xls_is_reported_not_a_crash() {
    // Fixed pseudo-random bytes, so the test is the same every run.
    let mut x: u32 = 0x2545_F491;
    let bytes: Vec<u8> = (0..8192)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 24) as u8
        })
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("noise.xls");
    std::fs::write(&path, &bytes).unwrap();
    let mut check = named("noise.xls");
    check.cells = vec![cell("A1", "x", false)];
    let err = check_file(&path, "noise.xls", &check).unwrap_err();
    assert!(err.starts_with("\"noise.xls\" could not be read as a spreadsheet: "), "{err}");
}

// ---------------------------------------------------------------- delimiter and encoding edges

/// Commas inside a quoted header are text, not delimiters: this file is
/// split on its one semicolon.
#[test]
fn commas_inside_quotes_do_not_choose_the_delimiter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quoted.csv");
    std::fs::write(&path, "\"Smith, John, Jr\";B\n1;2\n").unwrap();
    let mut check = named("quoted.csv");
    check.headers = exact(&["Smith, John, Jr", "B"]);
    check.cells = vec![cell("B2", "2", false)];
    let size = human_size(std::fs::metadata(&path).unwrap().len());
    assert_eq!(
        check_file(&path, "quoted.csv", &check),
        Ok(format!("downloaded \"quoted.csv\" ({size}), headers match, B2 is \"2\""))
    );
}

#[test]
fn a_windows_1252_euro_sign_reads_as_euro() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("price.txt");
    std::fs::write(&path, b"Total: 12 \x80\r\n").unwrap();
    let mut check = named("price.txt");
    check.contains_text = strings(&["12 €"]);
    assert_eq!(
        check_file(&path, "price.txt", &check),
        Ok(format!("downloaded \"price.txt\" ({}), it contains \"12 €\"", human_size(13)))
    );
}

#[test]
fn a_twelve_letter_column_is_refused() {
    assert_eq!(parse_a1("ABCDEFGHIJKL1"), None);
}

// ---------------------------------------------------------------- PDFs

/// `two-pages.pdf`: page 1 "Payslip for October" and "Employee: Ada
/// Lovelace" (three spaces in the PDF), page 2 "Summary", "Total
/// 12,500.00" and "End of report".
fn pdf(check: PdfCheck) -> DownloadCheck {
    let mut c = named("two-pages.pdf");
    c.pdf = Some(check);
    c
}

fn on(page: i32, items: &[&str]) -> OnPage {
    OnPage { page, contains: strings(items) }
}

fn run_pdf(check: PdfCheck) -> Result<String, String> {
    check_file(&fixture("two-pages.pdf"), "two-pages.pdf", &pdf(check))
}

#[test]
fn the_two_page_fixture_reads_page_by_page() {
    let pages = pdf_pages(&std::fs::read(fixture("two-pages.pdf")).unwrap()).unwrap();
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(pages[0].contains("payslip for october"), "{pages:?}");
    assert!(pages[0].contains("employee: ada lovelace"), "{pages:?}");
    assert!(pages[1].contains("total 12,500.00"), "{pages:?}");
}

#[test]
fn normalising_ignores_case_and_collapses_whitespace() {
    assert_eq!(normalise("  Net\tPay \r\n\n  TOTAL  "), "net pay total");
}

#[test]
fn contains_passes_ignoring_case_and_whitespace_and_fails_naming_the_text() {
    let check = PdfCheck { contains: strings(&["PAYSLIP for   october", "end of\nreport"]), ..Default::default() };
    assert_eq!(
        run_pdf(check),
        Ok(passed(
            "two-pages.pdf",
            &["the PDF contains \"PAYSLIP for   october\"", "the PDF contains \"end of\nreport\""]
        ))
    );
    let check = PdfCheck { contains: strings(&["Payslip", "Net pay"]), ..Default::default() };
    assert_eq!(run_pdf(check), Err("the PDF does not contain \"Net pay\"".to_string()));
}

#[test]
fn each_page_count_form_passes_and_fails() {
    let count = |p| PdfCheck { pages: Some(p), ..Default::default() };
    assert_eq!(run_pdf(count(PageCount::Equals(2))), Ok(passed("two-pages.pdf", &["the PDF has 2 pages"])));
    assert_eq!(run_pdf(count(PageCount::Equals(3))), Err("the PDF has 2 pages, not 3".to_string()));
    assert!(run_pdf(count(PageCount::AtLeast(2))).is_ok());
    assert_eq!(run_pdf(count(PageCount::AtLeast(3))), Err("the PDF has 2 pages, not at least 3".to_string()));
    assert!(run_pdf(count(PageCount::AtMost(2))).is_ok());
    assert_eq!(run_pdf(count(PageCount::AtMost(1))), Err("the PDF has 2 pages, not at most 1".to_string()));
}

#[test]
fn on_page_reads_the_first_and_the_last_page() {
    let check = PdfCheck { on_page: vec![on(1, &["Payslip"]), on(-1, &["total 12,500.00"])], ..Default::default() };
    assert_eq!(
        run_pdf(check),
        Ok(passed(
            "two-pages.pdf",
            &["page 1 of the PDF contains \"Payslip\"", "page 2 of the PDF contains \"total 12,500.00\""]
        ))
    );
    // "Total" is on page 2, not page 1.
    let check = PdfCheck { on_page: vec![on(1, &["Total"])], ..Default::default() };
    assert_eq!(run_pdf(check), Err("page 1 of the PDF does not contain \"Total\"".to_string()));
    let check = PdfCheck { on_page: vec![on(-1, &["Payslip"])], ..Default::default() };
    assert_eq!(run_pdf(check), Err("page 2 of the PDF does not contain \"Payslip\"".to_string()));
}

#[test]
fn a_page_past_the_end_says_there_is_no_such_page() {
    let check = PdfCheck { on_page: vec![on(3, &["Total"])], ..Default::default() };
    assert_eq!(run_pdf(check), Err("the PDF has no page 3".to_string()));
}

#[test]
fn every_check_together_passes_in_order() {
    let check = PdfCheck {
        contains: strings(&["Ada Lovelace"]),
        pages: Some(PageCount::Equals(2)),
        on_page: vec![on(-1, &["Total"])],
    };
    assert_eq!(
        run_pdf(check),
        Ok(passed(
            "two-pages.pdf",
            &["the PDF contains \"Ada Lovelace\"", "the PDF has 2 pages", "page 2 of the PDF contains \"Total\""]
        ))
    );
}

/// A file that cannot be read: encrypted with a password to open, scanned
/// (no text at all), or not a PDF. Each gives the one sentence, with the
/// detail in the log, and never a panic (Review Focus 5).
fn unreadable(path: &Path, shown: &str, why: &str) {
    let _log = crate::serial::log_tail();
    let mut check = named(shown);
    check.pdf = Some(PdfCheck { contains: strings(&["salary"]), ..Default::default() });
    assert_eq!(check_file(path, shown, &check), Err(PDF_UNREADABLE.to_string()), "{shown}");
    let logged = v2_lib::applog::recent(50);
    let line = logged
        .iter()
        .rev()
        .find(|l| l.message.contains(&format!("the text of \"{shown}\" could not be read")))
        .unwrap_or_else(|| panic!("no log line for {shown}"));
    assert_eq!(line.level, "warn");
    assert!(line.message.contains(why), "{}", line.message);
    assert!(!line.message.contains(&path.parent().unwrap().to_string_lossy().to_string()), "{}", line.message);
}

#[test]
fn an_encrypted_pdf_cannot_be_read() {
    unreadable(&fixture("encrypted.pdf"), "encrypted.pdf", "encrypted");
}

#[test]
fn an_image_only_pdf_cannot_be_read() {
    unreadable(&fixture("image-only.pdf"), "image-only.pdf", "no text on any of its 1 pages");
}

#[test]
fn garbage_and_a_truncated_pdf_cannot_be_read_and_never_panic() {
    let dir = tempfile::tempdir().unwrap();
    let noise = dir.path().join("noise.pdf");
    let mut x: u32 = 0x1234_5678;
    let mut bytes = b"%PDF-1.4\n".to_vec();
    bytes.extend((0..4096).map(|_| {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        (x >> 24) as u8
    }));
    std::fs::write(&noise, &bytes).unwrap();
    unreadable(&noise, "noise.pdf", "");
    let whole = std::fs::read(fixture("two-pages.pdf")).unwrap();
    let half = dir.path().join("half.pdf");
    std::fs::write(&half, &whole[..whole.len() / 2]).unwrap();
    unreadable(&half, "half.pdf", "");
    let empty = dir.path().join("empty.pdf");
    std::fs::write(&empty, b"").unwrap();
    unreadable(&empty, "empty.pdf", "");
}

#[test]
fn a_pdf_over_fifty_megabytes_is_refused_before_it_is_read() {
    assert_eq!(pdf_size_gate(MAX_CHECK_BYTES), Ok(()));
    assert_eq!(pdf_size_gate(MAX_CHECK_BYTES + 1), Err("the PDF is larger than 50 MB".to_string()));
    // A sparse file: its size is past the cap, and nothing is written.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.pdf");
    let f = std::fs::File::create(&path).unwrap();
    f.set_len(MAX_CHECK_BYTES + 1).unwrap();
    drop(f);
    let mut check = named("big.pdf");
    check.pdf = Some(PdfCheck { pages: Some(PageCount::AtLeast(1)), ..Default::default() });
    assert_eq!(check_file(&path, "big.pdf", &check), Err("the PDF is larger than 50 MB".to_string()));
}

// ---------------------------------------------------------------- the pdf block when a script is saved

fn action(v: serde_json::Value) -> Action {
    serde_json::from_value(v).expect("the action parses")
}

fn refused(v: serde_json::Value) -> String {
    action(v).validate().expect_err("the action is refused")
}

#[test]
fn a_pdf_block_takes_contains_as_one_phrase_or_a_list() {
    use serde_json::json;
    let one = action(json!({ "kind": "expect_download", "name": "Payslip*.pdf", "pdf": { "contains": "Total" } }));
    assert_eq!(one.validate(), Ok(()));
    let many = action(json!({ "kind": "expect_download", "name": "Payslip*.PDF",
        "pdf": { "contains": ["Total", "Net pay"], "pages": { "at_least": 1 },
                 "on_page": [ { "page": -1, "contains": "Total" }, { "page": 1, "contains": ["Payslip"] } ] } }));
    assert_eq!(many.validate(), Ok(()));
}

#[test]
fn each_refusal_of_a_pdf_block() {
    use serde_json::json;
    assert_eq!(
        refused(json!({ "kind": "expect_download", "name": "report.xlsx", "pdf": { "contains": "Total" } })),
        "pdf checks need a name ending in .pdf"
    );
    assert_eq!(
        refused(json!({ "kind": "expect_download", "name": "report*", "pdf": { "contains": "Total" } })),
        "pdf checks need a name ending in .pdf"
    );
    assert_eq!(
        refused(json!({ "kind": "expect_download", "name": "report.pdf", "headers": { "exact": ["A"] },
                        "pdf": { "contains": "Total" } })),
        "a download check is either a PDF check or a spreadsheet check"
    );
    assert_eq!(
        refused(json!({ "kind": "expect_download", "name": "report.pdf", "cells": [ { "ref": "A1", "text": "x" } ],
                        "pdf": { "contains": "Total" } })),
        "a download check is either a PDF check or a spreadsheet check"
    );
    for page in [0, -2] {
        assert_eq!(
            refused(json!({ "kind": "expect_download", "name": "report.pdf",
                            "pdf": { "on_page": [ { "page": page, "contains": "Total" } ] } })),
            "on_page: page counts from 1, or -1 for the last page"
        );
    }
    assert_eq!(
        refused(json!({ "kind": "expect_download", "name": "report.pdf", "pdf": { "pages": { "equals": 2, "at_most": 3 } } })),
        "pdf pages takes exactly one of equals, at_least or at_most"
    );
    assert_eq!(
        refused(json!({ "kind": "expect_download", "name": "report.pdf", "pdf": { "pages": {} } })),
        "pdf pages takes exactly one of equals, at_least or at_most"
    );
    assert_eq!(
        refused(json!({ "kind": "expect_download", "name": "report.pdf", "pdf": { "pages": { "at_least": 0 } } })),
        "pdf pages counts from 1"
    );
    assert_eq!(
        refused(json!({ "kind": "expect_download", "name": "report.pdf", "pdf": {} })),
        "pdf is empty - give contains, pages or on_page, or leave pdf out"
    );
    assert_eq!(
        refused(json!({ "kind": "expect_download", "name": "report.pdf", "pdf": { "contains": [] } })),
        "pdf contains is an empty list - give at least one text"
    );
    assert_eq!(
        refused(json!({ "kind": "expect_download", "name": "report.pdf", "pdf": { "contains": ["Total", " "] } })),
        "pdf contains has an empty text - every PDF contains nothing"
    );
    assert_eq!(
        refused(json!({ "kind": "expect_download", "name": "report.pdf", "pdf": { "on_page": [] } })),
        "pdf on_page is an empty list - give at least one page, or leave on_page out"
    );
    assert_eq!(
        refused(json!({ "kind": "expect_download", "name": "report.pdf", "pdf": { "page_count": 2 } })),
        "pdf has no \"page_count\" - it takes contains, pages and on_page"
    );
}

#[test]
fn old_and_new_download_checks_round_trip_byte_identical() {
    for text in [
        r#"{"kind":"expect_download","name":"Template*.xlsx","headers":{"exact":["Employee No","Name"]}}"#,
        r#"{"kind":"expect_download","name":"*Error*.csv","within_ms":30000,"contains_text":["Row 4"]}"#,
        r#"{"kind":"expect_download","name":"report.pdf"}"#,
        r#"{"kind":"expect_download","name":"Payslip*.pdf","pdf":{"contains":"Total"}}"#,
        r#"{"kind":"expect_download","name":"Payslip*.pdf","pdf":{"contains":["Total","Net pay"],"pages":{"equals":3},"on_page":[{"page":-1,"contains":"Total"}]}}"#,
    ] {
        let a: Action = serde_json::from_str(text).unwrap();
        assert_eq!(serde_json::to_string(&a).unwrap(), text);
    }
}
