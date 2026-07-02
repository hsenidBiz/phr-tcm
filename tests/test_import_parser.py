"""Import/export: the 9-column spreadsheet format, update-ID detection,
row-numbered warnings, and the Excel round-trip."""

import pytest

from app.models.test_case import Step, TestCase
from app.utils.import_parser import (
    export_queue_to_excel,
    generate_template,
    parse_file,
    _parse_rows,
)

HEADERS = [
    "TestCaseID", "TestCaseName", "StepNumber", "StepAction", "StepExpected",
    "Tags", "AutomationStatus", "ModuleValue", "Preconditions",
]


def _row(**kv):
    row = {h: "" for h in HEADERS}
    row.update(kv)
    return row


def test_missing_required_columns_raises():
    with pytest.raises(ValueError, match="Missing required columns"):
        _parse_rows([], ["TestCaseName", "StepAction"])


def test_basic_case_with_continuation_rows():
    rows = [
        (2, _row(TestCaseName="Login", StepNumber="1", StepAction="Open", Tags="smoke")),
        (3, _row(StepNumber="2", StepAction="Type", StepExpected="Accepted")),
        (4, _row(StepNumber="3", StepAction="Submit")),
    ]
    cases, warnings = _parse_rows(rows, HEADERS)
    assert warnings == []
    assert len(cases) == 1
    tc = cases[0]
    assert tc.title == "Login"
    assert tc.tags == "smoke"
    assert [s.action for s in tc.steps] == ["Open", "Type", "Submit"]
    assert tc.update_id is None


def test_testcaseid_marks_update_and_tolerates_excel_floats():
    rows = [(2, _row(TestCaseID="123.0", TestCaseName="Existing", StepNumber="1", StepAction="Do"))]
    cases, _ = _parse_rows(rows, HEADERS)
    assert cases[0].update_id == 123


def test_invalid_testcaseid_warns_and_creates_new():
    rows = [(5, _row(TestCaseID="abc", TestCaseName="Bad", StepNumber="1", StepAction="Do"))]
    cases, warnings = _parse_rows(rows, HEADERS)
    assert cases[0].update_id is None
    assert any("Row 5" in w and "abc" in w for w in warnings)


def test_invalid_automation_status_warns_with_row():
    rows = [(7, _row(TestCaseName="X", StepNumber="1", StepAction="Do", AutomationStatus="Nope"))]
    cases, warnings = _parse_rows(rows, HEADERS)
    assert cases[0].automation_status == "Not Automated"
    assert any("Row 7" in w and "Nope" in w for w in warnings)


def test_steps_sorted_by_step_number():
    rows = [
        (2, _row(TestCaseName="Order", StepNumber="2", StepAction="Second")),
        (3, _row(StepNumber="1", StepAction="First")),
    ]
    cases, _ = _parse_rows(rows, HEADERS)
    assert [s.action for s in cases[0].steps] == ["First", "Second"]


def test_comma_tags_warns():
    rows = [(2, _row(TestCaseName="T", StepNumber="1", StepAction="Do", Tags="a, b"))]
    _cases, warnings = _parse_rows(rows, HEADERS)
    assert any("comma" in w for w in warnings)


def test_long_title_warns_but_case_kept():
    rows = [(3, _row(TestCaseName="X" * 300, StepNumber="1", StepAction="Do"))]
    cases, warnings = _parse_rows(rows, HEADERS)
    assert len(cases) == 1
    assert any("Row 3" in w and "255" in w for w in warnings)


def test_expected_without_action_warns_and_skips_step():
    rows = [
        (2, _row(TestCaseName="T", StepNumber="1", StepAction="Do")),
        (3, _row(StepNumber="2", StepExpected="Orphan expected")),
    ]
    cases, warnings = _parse_rows(rows, HEADERS)
    assert len(cases[0].steps) == 1
    assert any("Row 3" in w and "StepAction is empty" in w for w in warnings)


def test_case_without_steps_skipped_with_row():
    rows = [(4, _row(TestCaseName="Empty"))]
    cases, warnings = _parse_rows(rows, HEADERS)
    assert cases == []
    assert any("Row 4" in w and "Empty" in w for w in warnings)


def test_csv_row_numbers_survive_blank_lines(tmp_path):
    csv_file = tmp_path / "cases.csv"
    csv_file.write_text(
        "TestCaseID,TestCaseName,StepNumber,StepAction,StepExpected,"
        "Tags,AutomationStatus,ModuleValue,Preconditions\n"
        ",Case A,1,Open,,,,,\n"
        "\n"
        ",Case B,1,Do,,,Wrong,,\n",
        encoding="utf-8",
    )
    cases, warnings = parse_file(str(csv_file))
    assert [c.title for c in cases] == ["Case A", "Case B"]
    # Case B sits on physical line 4 (blank line 3 must not shift the number).
    assert any("Row 4" in w and "Wrong" in w for w in warnings)


def test_excel_round_trip_preserves_update_id_and_steps(tmp_path):
    queue = [
        TestCase(
            title="Round trip",
            steps=[Step(action="One", expected="A"), Step(action="Two", expected="")],
            tags="smoke; nightly",
            automation_status="Planned",
            module_value="Auth",
            preconditions="Logged out",
            update_id=4242,
        ),
        TestCase(title="New case", steps=[Step(action="Go")]),
    ]
    path = tmp_path / "queue.xlsx"
    export_queue_to_excel(queue, str(path))
    cases, warnings = parse_file(str(path))

    assert warnings == []
    assert len(cases) == 2
    first, second = cases
    assert first.update_id == 4242
    assert first.title == "Round trip"
    assert [(s.action, s.expected) for s in first.steps] == [("One", "A"), ("Two", "")]
    assert first.tags == "smoke; nightly"
    assert first.automation_status == "Planned"
    assert first.module_value == "Auth"
    assert first.preconditions == "Logged out"
    assert second.update_id is None
    assert second.title == "New case"


def test_generate_template_parses_back(tmp_path):
    path = tmp_path / "template.xlsx"
    generate_template(str(path))
    cases, warnings = parse_file(str(path))
    assert warnings == []
    assert [c.title for c in cases] == ["Login as admin", "Invalid login attempt"]
    assert len(cases[0].steps) == 3
