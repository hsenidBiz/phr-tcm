import json

from app.models.test_case import TestCase, Step
from app.utils import export_formats
from app.utils.import_parser import parse_file


def _sample_queue():
    return [
        TestCase(
            title="Login as admin",
            steps=[Step("Open login page", "Page shown"), Step("Sign in", "Dashboard shown")],
            tags="smoke; auth",
            automation_status="Planned",
            module_value="Authentication",
            preconditions="User exists",
        ),
        TestCase(
            title="Update case <b>",
            steps=[Step("Do a thing")],
            update_id=4242,
        ),
    ]


# --------------------------------------------------------------------- #
#  JSON round-trip                                                       #
# --------------------------------------------------------------------- #

def test_json_export_import_round_trip(tmp_path):
    path = str(tmp_path / "cases.json")
    records = export_formats.queue_to_records(_sample_queue())
    export_formats.export_records_to_json(records, path)

    doc = json.loads(open(path, encoding="utf-8").read())
    assert doc["format"] == export_formats.AI_FORMAT_NAME
    assert doc["instructions"]

    cases, warnings = parse_file(path)
    assert warnings == []
    assert len(cases) == 2

    tc = cases[0]
    assert tc.title == "Login as admin"
    assert tc.update_id is None
    assert tc.tags == "smoke; auth"
    assert tc.automation_status == "Planned"
    assert tc.module_value == "Authentication"
    assert tc.preconditions == "User exists"
    assert [(s.action, s.expected) for s in tc.steps] == [
        ("Open login page", "Page shown"), ("Sign in", "Dashboard shown"),
    ]

    # A kept id round-trips as an update.
    assert cases[1].update_id == 4242
    assert cases[1].automation_status == "Not Automated"


def test_json_import_bare_list_and_alternate_keys(tmp_path):
    path = tmp_path / "ai_edited.json"
    path.write_text(json.dumps([
        {
            "test_case_id": "123.0",
            "name": "Alt keys case",
            "tags": ["smoke", "auth"],
            "steps": [
                {"step": "Do it", "expected_result": "It works"},
                "Bare string step",
            ],
        },
    ]), encoding="utf-8")

    cases, warnings = parse_file(str(path))
    assert warnings == []
    assert len(cases) == 1
    tc = cases[0]
    assert tc.update_id == 123
    assert tc.title == "Alt keys case"
    assert tc.tags == "smoke; auth"
    assert [(s.action, s.expected) for s in tc.steps] == [
        ("Do it", "It works"), ("Bare string step", ""),
    ]


def test_json_import_validation_warnings(tmp_path):
    path = tmp_path / "bad.json"
    path.write_text(json.dumps({"test_cases": [
        {"title": "Bad status", "automation_status": "Automated",
         "steps": [{"action": "Go"}]},
        {"title": "No steps", "steps": []},
        {"steps": [{"action": "No title"}]},
        {"title": "Bad id", "id": "abc", "steps": [{"action": "Go"}]},
    ]}), encoding="utf-8")

    cases, warnings = parse_file(str(path))
    assert [tc.title for tc in cases] == ["Bad status", "Bad id"]
    assert cases[0].automation_status == "Not Automated"
    assert cases[1].update_id is None
    assert len(warnings) == 4


def test_json_import_rejects_invalid_documents(tmp_path):
    import pytest
    bad = tmp_path / "broken.json"
    bad.write_text("{not json", encoding="utf-8")
    with pytest.raises(ValueError):
        parse_file(str(bad))

    wrong = tmp_path / "wrong.json"
    wrong.write_text(json.dumps({"cases": []}), encoding="utf-8")
    with pytest.raises(ValueError):
        parse_file(str(wrong))


# --------------------------------------------------------------------- #
#  HTML export                                                           #
# --------------------------------------------------------------------- #

def test_html_export_contains_cases_and_escapes(tmp_path):
    path = str(tmp_path / "cases.html")
    records = export_formats.queue_to_records(_sample_queue())
    export_formats.export_records_to_html(records, path, subtitle="2 test case(s)")

    text = open(path, encoding="utf-8").read()
    assert text.startswith("<!DOCTYPE html>")
    assert "Login as admin" in text
    assert "#4242" in text
    assert "Open login page" in text and "Dashboard shown" in text
    # HTML in a title must be escaped, never rendered.
    assert "Update case &lt;b&gt;" in text
    assert "Update case <b>" not in text
    # Client-side search: input, counter, empty state and the filter script.
    assert "id='tc-search'" in text
    assert "id='tc-count'" in text
    assert "id='tc-no-match'" in text
    assert "<script>" in text and "tc-search" in text.split("<script>")[1]


def test_cases_to_records_maps_api_fields():
    from app.utils.xml_builder import build_steps_xml
    api_case = {
        "_id": 77,
        "System.Title": "From API",
        "System.Tags": "regression",
        "Microsoft.VSTS.TCM.AutomationStatus": "Planned",
        "Custom.Module": "Billing",
        "Microsoft.VSTS.TCM.Steps": build_steps_xml([Step("A", "B")]),
    }
    recs = export_formats.cases_to_records([api_case], "Custom.Module", None)
    assert recs == [{
        "id": 77, "title": "From API", "tags": "regression",
        "automation_status": "Planned", "module": "Billing", "preconditions": "",
        "steps": [{"action": "A", "expected": "B"}],
    }]


# --------------------------------------------------------------------- #
#  Save-path resolution                                                  #
# --------------------------------------------------------------------- #

def test_resolve_export_path():
    f_html = "HTML report — for people (*.html)"
    f_json = "AI-editable JSON (*.json)"
    assert export_formats.resolve_export_path("a.html", f_json) == ("a.html", ".html")
    assert export_formats.resolve_export_path("a.htm", f_html) == ("a.htm", ".html")
    assert export_formats.resolve_export_path("a.json", f_html) == ("a.json", ".json")
    assert export_formats.resolve_export_path("a", f_json) == ("a.json", ".json")
    assert export_formats.resolve_export_path("a.txt", "") == ("a.txt.html", ".html")
