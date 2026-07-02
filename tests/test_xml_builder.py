"""Steps-XML round-trip for the Microsoft.VSTS.TCM.Steps field."""

import xml.etree.ElementTree as ET

from app.models.test_case import Step
from app.utils.xml_builder import build_steps_xml, html_to_text, parse_steps_xml


def test_step_ids_start_at_two_and_last_matches():
    xml = build_steps_xml([Step(action="a"), Step(action="b"), Step(action="c")])
    root = ET.fromstring(xml)
    ids = [el.get("id") for el in root.findall("step")]
    assert ids == ["2", "3", "4"]
    assert root.get("last") == "4"


def test_round_trip_preserves_action_and_expected():
    steps = [
        Step(action="Open the login page", expected="Login page is shown"),
        Step(action="Enter user & password, then submit", expected=""),
    ]
    parsed = parse_steps_xml(build_steps_xml(steps))
    assert [(s.action, s.expected) for s in parsed] == [
        ("Open the login page", "Login page is shown"),
        ("Enter user & password, then submit", ""),
    ]


def test_parse_strips_angle_bracket_markup():
    # ADO stores step text as HTML, so the parser strips anything tag-shaped —
    # a literal "<placeholder>" typed by a user does not survive a round-trip.
    parsed = parse_steps_xml(build_steps_xml([Step(action="Enter <credentials> here")]))
    assert parsed[0].action == "Enter here"


def test_empty_steps_builds_placeholder_step():
    xml = build_steps_xml([])
    root = ET.fromstring(xml)
    assert len(root.findall("step")) == 1
    assert root.findall("step")[0].get("id") == "2"


def test_parse_malformed_xml_returns_empty():
    assert parse_steps_xml("<steps><step") == []
    assert parse_steps_xml("") == []
    assert parse_steps_xml("   ") == []


def test_parse_html_encoded_content():
    xml = (
        '<steps id="0" last="2"><step id="2" type="ActionStep">'
        "<parameterizedString isformatted=\"true\">&lt;div&gt;Click &amp;amp; hold&lt;/div&gt;"
        "</parameterizedString>"
        '<parameterizedString isformatted="true">Done</parameterizedString>'
        "</step></steps>"
    )
    steps = parse_steps_xml(xml)
    assert len(steps) == 1
    assert "Click" in steps[0].action
    assert steps[0].expected == "Done"


def test_html_to_text_strips_tags_and_keeps_structure():
    txt = html_to_text("<div>First</div><ul><li>one</li><li>two</li></ul>")
    assert "First" in txt
    assert "• one" in txt
    assert "<" not in txt
    assert html_to_text("") == ""
    assert html_to_text("   ") == ""
