import html as _html
import re as _re
import xml.etree.ElementTree as ET

from app.models.test_case import Step


def html_to_text(html: str) -> str:
    """Flatten an Azure DevOps rich-text/HTML field to clean plain text.

    ADO stores fields like Preconditions as HTML, often with inline colours or
    highlight spans that clash with the app's theme when rendered raw. This
    keeps block structure (block ends and list items become line breaks /
    bullets), strips all tags + styling, and unescapes entities. Returns '' for
    empty / whitespace-only input.
    """
    if not html or not html.strip():
        return ""
    s = html
    s = _re.sub(r"(?i)<\s*li[^>]*>", "\n• ", s)
    s = _re.sub(r"(?i)<\s*(br|/p|/div|/h[1-6]|/tr)\s*/?>", "\n", s)
    s = _re.sub(r"<[^>]+>", "", s)            # strip any remaining tags
    s = _html.unescape(s).replace("\xa0", " ")
    lines, blank = [], False
    for ln in s.split("\n"):
        ln = ln.strip()
        if ln:
            lines.append(ln)
            blank = False
        elif lines and not blank:
            lines.append("")
            blank = True
    return "\n".join(lines).strip()


def parse_steps_xml(xml_str: str) -> list:
    """
    Parse the Microsoft.VSTS.TCM.Steps XML into a list of Step objects.
    Handles HTML-encoded content inside parameterizedString elements.
    Returns [] on empty or malformed input.
    """
    if not xml_str or not xml_str.strip():
        return []

    def _el_text(el) -> str:
        raw = ET.tostring(el, encoding="unicode")
        raw = _re.sub(r"^<[^>]+>", "", raw)
        raw = _re.sub(r"</[^>]+>$", "", raw)
        raw = _html.unescape(raw)
        raw = _re.sub(r"<[^>]+>", " ", raw)
        return " ".join(raw.split())

    try:
        root = ET.fromstring(xml_str)
    except ET.ParseError:
        return []

    steps = []
    for step_el in root.findall("step"):
        parts = step_el.findall("parameterizedString")
        action = _el_text(parts[0]) if parts else ""
        expected = _el_text(parts[1]) if len(parts) > 1 else ""
        steps.append(Step(action=action, expected=expected))
    return steps


def build_steps_xml(steps) -> str:
    """
    Build the XML string for the Microsoft.VSTS.TCM.Steps field.
    steps: list of Step objects with .action and .expected attributes.
    Step IDs start at 2; 'last' attribute equals the last step ID.
    """
    if not steps:
        return '<steps id="0" last="1"><step id="2" type="ActionStep"><parameterizedString isformatted="true"></parameterizedString><parameterizedString isformatted="true"></parameterizedString></step></steps>'

    root = ET.Element("steps")
    root.set("id", "0")
    root.set("last", str(len(steps) + 1))

    for i, step in enumerate(steps):
        step_el = ET.SubElement(root, "step")
        step_el.set("id", str(i + 2))
        step_el.set("type", "ActionStep")

        action_el = ET.SubElement(step_el, "parameterizedString")
        action_el.set("isformatted", "true")
        action_el.text = step.action or ""

        expected_el = ET.SubElement(step_el, "parameterizedString")
        expected_el.set("isformatted", "true")
        expected_el.text = step.expected or ""

    return ET.tostring(root, encoding="unicode")
