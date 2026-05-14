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
        group = groups[name]
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
