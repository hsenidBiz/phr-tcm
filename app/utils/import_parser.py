import csv
from pathlib import Path
from app.models.test_case import TestCase, Step

VALID_STATUSES = {"Not Automated", "Planned"}

REQUIRED_COLUMNS = {"TestCaseName", "StepNumber", "StepAction"}
ALL_COLUMNS = {"TestCaseName", "StepNumber", "StepAction", "StepExpected",
               "Tags", "AutomationStatus", "ModuleValue"}


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


def _parse_rows(rows: list, headers: list) -> tuple:
    warnings = []

    missing = REQUIRED_COLUMNS - set(headers)
    if missing:
        raise ValueError(
            f"Missing required columns: {', '.join(sorted(missing))}.\n"
            f"Expected columns: {', '.join(sorted(ALL_COLUMNS))}"
        )

    # Group rows by TestCaseName
    groups: dict[str, list] = {}
    order: list[str] = []
    for row_num, row in enumerate(rows, start=2):
        name = row.get("TestCaseName", "").strip()
        if not name:
            warnings.append(f"Row {row_num}: skipped — TestCaseName is empty.")
            continue
        if name not in groups:
            groups[name] = []
            order.append(name)
        groups[name].append((row_num, row))

    test_cases = []
    for name in order:
        group = sorted(groups[name], key=_step_sort_key)
        first_row = group[0][1]

        tags = first_row.get("Tags", "").strip()
        automation_status = first_row.get("AutomationStatus", "").strip()
        module_value = first_row.get("ModuleValue", "").strip()

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
                warnings.append(f"Row {row_num} ('{name}'): StepAction is empty — step skipped.")
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
        ))

    return test_cases, warnings


def export_queue_to_excel(queue: list, path: str):
    """Export the in-memory test case queue to an Excel file (same 7-column format as template)."""
    try:
        import openpyxl
        from openpyxl.styles import Font, PatternFill, Alignment
    except ImportError:
        raise ImportError("openpyxl is required. Run: pip install openpyxl")

    wb = openpyxl.Workbook()
    ws = wb.active
    ws.title = "Test Cases"

    headers = ["TestCaseName", "StepNumber", "StepAction", "StepExpected",
               "Tags", "AutomationStatus", "ModuleValue"]
    header_fill = PatternFill(start_color="366092", end_color="366092", fill_type="solid")
    header_font = Font(color="FFFFFF", bold=True)
    for col, h in enumerate(headers, start=1):
        cell = ws.cell(row=1, column=col, value=h)
        cell.fill = header_fill
        cell.font = header_font
        cell.alignment = Alignment(horizontal="center")

    for tc in queue:
        for i, step in enumerate(tc.steps):
            ws.append([
                tc.title if i == 0 else "",
                i + 1,
                step.action,
                step.expected,
                tc.tags if i == 0 else "",
                tc.automation_status if i == 0 else "",
                tc.module_value if i == 0 else "",
            ])

    col_widths = [30, 12, 45, 45, 20, 18, 20]
    for col, width in enumerate(col_widths, start=1):
        ws.column_dimensions[ws.cell(row=1, column=col).column_letter].width = width
    ws.freeze_panes = "A2"
    wb.save(path)


def export_cases_to_excel(cases: list, path: str, module_ref: str | None,
                          preconditions_ref: str | None):
    """Export DevOps API test case dicts (from edit screen) to Excel."""
    try:
        import openpyxl
        from openpyxl.styles import Font, PatternFill, Alignment
    except ImportError:
        raise ImportError("openpyxl is required. Run: pip install openpyxl")

    from app.utils.xml_builder import parse_steps_xml

    wb = openpyxl.Workbook()
    ws = wb.active
    ws.title = "Test Cases"

    headers = ["TestCaseName", "StepNumber", "StepAction", "StepExpected",
               "Tags", "AutomationStatus", "ModuleValue"]
    header_fill = PatternFill(start_color="366092", end_color="366092", fill_type="solid")
    header_font = Font(color="FFFFFF", bold=True)
    for col, h in enumerate(headers, start=1):
        cell = ws.cell(row=1, column=col, value=h)
        cell.fill = header_fill
        cell.font = header_font
        cell.alignment = Alignment(horizontal="center")

    for tc in cases:
        title = tc.get("System.Title", "")
        tags = tc.get("System.Tags", "") or ""
        auto_status = tc.get("Microsoft.VSTS.TCM.AutomationStatus", "Not Automated") or "Not Automated"
        module_val = tc.get(module_ref, "") if module_ref else ""
        steps = parse_steps_xml(tc.get("Microsoft.VSTS.TCM.Steps", "") or "")

        if not steps:
            ws.append([title, 1, "", "", tags, auto_status, module_val or ""])
            continue

        for i, step in enumerate(steps):
            ws.append([
                title if i == 0 else "",
                i + 1,
                step.action,
                step.expected,
                tags if i == 0 else "",
                auto_status if i == 0 else "",
                (module_val or "") if i == 0 else "",
            ])

    col_widths = [30, 12, 45, 45, 20, 18, 20]
    for col, width in enumerate(col_widths, start=1):
        ws.column_dimensions[ws.cell(row=1, column=col).column_letter].width = width
    ws.freeze_panes = "A2"
    wb.save(path)


def generate_template(save_path: str):
    """Write a blank Excel template to the given path."""
    try:
        import openpyxl
        from openpyxl.styles import Font, PatternFill, Alignment
    except ImportError:
        raise ImportError("openpyxl is required. Run: pip install openpyxl")

    wb = openpyxl.Workbook()
    ws = wb.active
    ws.title = "Test Cases"

    headers = ["TestCaseName", "StepNumber", "StepAction", "StepExpected",
               "Tags", "AutomationStatus", "ModuleValue"]
    header_fill = PatternFill(start_color="366092", end_color="366092", fill_type="solid")
    header_font = Font(color="FFFFFF", bold=True)

    for col, header in enumerate(headers, start=1):
        cell = ws.cell(row=1, column=col, value=header)
        cell.fill = header_fill
        cell.font = header_font
        cell.alignment = Alignment(horizontal="center")

    example_rows = [
        ["Login as admin", 1, "Navigate to the login page", "Login page is displayed", "smoke", "Not Automated", "Authentication"],
        ["Login as admin", 2, "Enter valid username and password", "Fields accept the input", "", "", ""],
        ["Login as admin", 3, "Click the Login button", "User is redirected to the dashboard", "", "", ""],
        ["Invalid login attempt", 1, "Navigate to the login page", "Login page is displayed", "regression", "Not Automated", "Authentication"],
        ["Invalid login attempt", 2, "Enter an invalid password", "Error message is displayed", "", "", ""],
    ]
    for row_data in example_rows:
        ws.append(row_data)

    col_widths = [30, 12, 45, 45, 20, 18, 20]
    for col, width in enumerate(col_widths, start=1):
        ws.column_dimensions[ws.cell(row=1, column=col).column_letter].width = width

    ws.freeze_panes = "A2"
    wb.save(save_path)
