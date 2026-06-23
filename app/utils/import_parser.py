import csv
from pathlib import Path
from app.models.test_case import TestCase, Step

VALID_STATUSES = {"Not Automated", "Planned"}

REQUIRED_COLUMNS = {"TestCaseName", "StepNumber", "StepAction"}
ALL_COLUMNS = {"TestCaseID", "TestCaseName", "StepNumber", "StepAction", "StepExpected",
               "Tags", "AutomationStatus", "ModuleValue", "Preconditions"}


def parse_file(path: str) -> tuple:
    """
    Parse an Excel (.xlsx) or CSV file into a list of TestCase objects.
    Returns (list[TestCase], list[str]) — test cases and any warning messages.
    """
    p = Path(path)
    if p.suffix.lower() == ".xlsx":
        return _parse_excel(p)
    elif p.suffix.lower() == ".csv":
        return _parse_csv(p)
    else:
        raise ValueError(f"Unsupported file type: {p.suffix}. Use .xlsx or .csv")


def _parse_excel(path: Path) -> tuple:
    try:
        import openpyxl
    except ImportError:
        raise ImportError("openpyxl is required to import Excel files. Run: pip install openpyxl")

    wb = openpyxl.load_workbook(path, read_only=True, data_only=True)
    ws = wb.active
    rows = list(ws.iter_rows(values_only=True))
    wb.close()

    if not rows:
        raise ValueError("The Excel file is empty.")

    headers = [str(c).strip() if c is not None else "" for c in rows[0]]
    data_rows = [
        {headers[i]: (str(cell).strip() if cell is not None else "") for i, cell in enumerate(row)}
        for row in rows[1:]
        if any(cell is not None and str(cell).strip() for cell in row)
    ]
    return _parse_rows(data_rows, headers)


def _parse_csv(path: Path) -> tuple:
    with open(path, newline="", encoding="utf-8-sig") as f:
        reader = csv.DictReader(f)
        headers = reader.fieldnames or []
        data_rows = [{k: (v.strip() if v else "") for k, v in row.items()} for row in reader]
    return _parse_rows(data_rows, list(headers))


def _step_sort_key(item):
    """Sort rows within a test-case group by StepNumber (int), then by file row as tiebreak."""
    row_num, row = item
    try:
        return (int(row.get("StepNumber", 0) or 0), row_num)
    except (ValueError, TypeError):
        return (0, row_num)


def _block_value(block_rows: list, col: str) -> str:
    """First non-empty value of `col` across a block's rows.

    Exported files put the title / ID / metadata only on each case's first step
    row (continuation rows are blank), so per-case fields are read from whichever
    row carries them rather than assuming row 1.
    """
    for _row_num, row in block_rows:
        val = row.get(col, "").strip()
        if val:
            return val
    return ""


def _parse_rows(rows: list, headers: list) -> tuple:
    warnings = []

    missing = REQUIRED_COLUMNS - set(headers)
    if missing:
        raise ValueError(
            f"Missing required columns: {', '.join(sorted(missing))}.\n"
            f"Expected columns: {', '.join(sorted(ALL_COLUMNS))}"
        )

    # Group rows into test-case blocks. A row starts a new block when it carries
    # a TestCaseID (keyed by that ID) or a TestCaseName different from the current
    # block; rows blank in both columns are continuation steps of the open block.
    # This lets a file exported from the Edit tab — where the name, ID and
    # metadata appear only on each case's first step row — round-trip back into
    # multi-step cases, and lets a kept TestCaseID flag the case as an update.
    blocks: list[dict] = []
    current = None
    for row_num, row in enumerate(rows, start=2):
        raw_id = row.get("TestCaseID", "").strip()
        name = row.get("TestCaseName", "").strip()

        if raw_id:
            if current is None or current["raw_id"] != raw_id:
                current = {"raw_id": raw_id, "name": name, "rows": []}
                blocks.append(current)
        elif name:
            if current is None or name != current["name"]:
                current = {"raw_id": "", "name": name, "rows": []}
                blocks.append(current)
        elif current is None:
            warnings.append(
                f"Row {row_num}: skipped — no TestCaseName/TestCaseID and no open test case."
            )
            continue
        current["rows"].append((row_num, row))

    test_cases = []
    for block in blocks:
        group = sorted(block["rows"], key=_step_sort_key)
        name = block["name"] or _block_value(group, "TestCaseName")
        if not name:
            warnings.append("A group of rows was skipped — no TestCaseName found.")
            continue

        update_id = None
        raw_id = block["raw_id"] or _block_value(group, "TestCaseID")
        if raw_id:
            try:
                # Excel numeric cells may render as "123.0"; tolerate that.
                update_id = int(float(raw_id))
            except (ValueError, TypeError):
                warnings.append(
                    f"Test case '{name}': TestCaseID '{raw_id}' is not a valid work item ID — "
                    "this row will be created as a new test case instead of updating."
                )

        tags = _block_value(group, "Tags")
        automation_status = _block_value(group, "AutomationStatus")
        module_value = _block_value(group, "ModuleValue")
        preconditions = _block_value(group, "Preconditions")

        if not automation_status:
            automation_status = "Not Automated"
        elif automation_status not in VALID_STATUSES:
            warnings.append(
                f"Test case '{name}': AutomationStatus '{automation_status}' is invalid. "
                f"Defaulting to 'Not Automated'. Valid values: {', '.join(sorted(VALID_STATUSES))}"
            )
            automation_status = "Not Automated"

        steps = []
        for row_num, row in group:
            action = row.get("StepAction", "").strip()
            expected = row.get("StepExpected", "").strip()
            if not action:
                continue
            steps.append(Step(action=action, expected=expected))

        if not steps:
            warnings.append(f"Test case '{name}': no valid steps found — skipped.")
            continue

        test_cases.append(TestCase(
            title=name,
            steps=steps,
            tags=tags,
            automation_status=automation_status,
            module_value=module_value,
            preconditions=preconditions,
            update_id=update_id,
        ))

    return test_cases, warnings


_EXCEL_HEADERS = [
    "TestCaseID", "TestCaseName", "StepNumber", "StepAction", "StepExpected",
    "Tags", "AutomationStatus", "ModuleValue", "Preconditions",
]
_EXCEL_COL_WIDTHS = [12, 30, 12, 45, 45, 20, 18, 20, 40]

_ID_COLUMN_HELP = (
    "Work item ID. Leave blank to create a NEW test case. "
    "When you re-import a file exported from the Edit Test Cases tab, keep this "
    "value to UPDATE that existing test case instead of creating a duplicate."
)


def _write_excel_headers(ws) -> None:
    """Write the standard styled header row and column widths to a worksheet."""
    from openpyxl.styles import Font, PatternFill, Alignment
    from openpyxl.comments import Comment
    header_fill = PatternFill(start_color="366092", end_color="366092", fill_type="solid")
    header_font = Font(color="FFFFFF", bold=True)
    for col, h in enumerate(_EXCEL_HEADERS, start=1):
        cell = ws.cell(row=1, column=col, value=h)
        cell.fill = header_fill
        cell.font = header_font
        cell.alignment = Alignment(horizontal="center")
    # Explain the round-trip behaviour of the TestCaseID column.
    ws.cell(row=1, column=1).comment = Comment(_ID_COLUMN_HELP, "Test Case Creator")
    for col, width in enumerate(_EXCEL_COL_WIDTHS, start=1):
        ws.column_dimensions[ws.cell(row=1, column=col).column_letter].width = width
    ws.freeze_panes = "A2"


def export_queue_to_excel(queue: list, path: str):
    """Export the in-memory test case queue to an Excel file (same 8-column format as template)."""
    try:
        import openpyxl
    except ImportError:
        raise ImportError("openpyxl is required. Run: pip install openpyxl")

    wb = openpyxl.Workbook()
    ws = wb.active
    ws.title = "Test Cases"
    _write_excel_headers(ws)

    for tc in queue:
        # Preserve update_id so a queued update still round-trips as an update.
        wid = tc.update_id or ""
        for i, step in enumerate(tc.steps):
            ws.append([
                wid if i == 0 else "",
                tc.title if i == 0 else "",
                i + 1,
                step.action,
                step.expected,
                tc.tags if i == 0 else "",
                tc.automation_status if i == 0 else "",
                tc.module_value if i == 0 else "",
                tc.preconditions if i == 0 else "",
            ])

    wb.save(path)


def export_cases_to_excel(cases: list, path: str, module_ref: str | None,
                          preconditions_ref: str | None):
    """Export DevOps API test case dicts (from edit screen) to Excel."""
    try:
        import openpyxl
    except ImportError:
        raise ImportError("openpyxl is required. Run: pip install openpyxl")

    from app.utils.xml_builder import parse_steps_xml

    wb = openpyxl.Workbook()
    ws = wb.active
    ws.title = "Test Cases"
    _write_excel_headers(ws)

    for tc in cases:
        wid = tc.get("_id", "")
        title = tc.get("System.Title", "")
        tags = tc.get("System.Tags", "") or ""
        auto_status = tc.get("Microsoft.VSTS.TCM.AutomationStatus", "Not Automated") or "Not Automated"
        module_val = tc.get(module_ref, "") if module_ref else ""
        preconditions_val = tc.get(preconditions_ref, "") if preconditions_ref else ""
        steps = parse_steps_xml(tc.get("Microsoft.VSTS.TCM.Steps", "") or "")

        if not steps:
            ws.append([wid, title, 1, "", "", tags, auto_status, module_val or "", preconditions_val or ""])
            continue

        for i, step in enumerate(steps):
            ws.append([
                wid if i == 0 else "",
                title if i == 0 else "",
                i + 1,
                step.action,
                step.expected,
                tags if i == 0 else "",
                auto_status if i == 0 else "",
                (module_val or "") if i == 0 else "",
                (preconditions_val or "") if i == 0 else "",
            ])

    wb.save(path)


def generate_template(save_path: str):
    """Write a blank Excel template to the given path."""
    try:
        import openpyxl
    except ImportError:
        raise ImportError("openpyxl is required. Run: pip install openpyxl")

    wb = openpyxl.Workbook()
    ws = wb.active
    ws.title = "Test Cases"
    _write_excel_headers(ws)

    # TestCaseID (first column) is left blank in the template — these are new cases.
    example_rows = [
        ["", "Login as admin", 1, "Navigate to the login page", "Login page is displayed", "smoke", "Not Automated", "Authentication", "User is logged out"],
        ["", "Login as admin", 2, "Enter valid username and password", "Fields accept the input", "", "", "", ""],
        ["", "Login as admin", 3, "Click the Login button", "User is redirected to the dashboard", "", "", "", ""],
        ["", "Invalid login attempt", 1, "Navigate to the login page", "Login page is displayed", "regression", "Not Automated", "Authentication", "User is logged out"],
        ["", "Invalid login attempt", 2, "Enter an invalid password", "Error message is displayed", "", "", "", ""],
    ]
    for row_data in example_rows:
        ws.append(row_data)

    wb.save(save_path)
