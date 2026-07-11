//! Golden vectors ported 1:1 from v1 tests/test_xml_builder.py.

use v2_lib::steps_xml::{build_steps_xml, html_to_text, parse_steps_xml, Step};

fn step(action: &str, expected: &str) -> Step {
    Step {
        action: action.to_string(),
        expected: expected.to_string(),
    }
}

#[test]
fn step_ids_start_at_two_and_last_matches() {
    let xml = build_steps_xml(&[step("a", ""), step("b", ""), step("c", "")]);
    assert!(xml.contains("last=\"4\""));
    assert!(xml.contains("<step id=\"2\""));
    assert!(xml.contains("<step id=\"3\""));
    assert!(xml.contains("<step id=\"4\""));
    assert!(!xml.contains("<step id=\"5\""));
}

#[test]
fn round_trip_preserves_action_and_expected() {
    let steps = vec![
        step("Open the login page", "Login page is shown"),
        step("Enter user & password, then submit", ""),
    ];
    let parsed = parse_steps_xml(&build_steps_xml(&steps));
    assert_eq!(
        parsed
            .iter()
            .map(|s| (s.action.as_str(), s.expected.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("Open the login page", "Login page is shown"),
            ("Enter user & password, then submit", ""),
        ]
    );
}

#[test]
fn parse_strips_angle_bracket_markup() {
    // Same as v1: ADO stores step text as HTML, so anything tag-shaped is
    // stripped - a literal "<placeholder>" does not survive a round-trip.
    let parsed = parse_steps_xml(&build_steps_xml(&[step("Enter <credentials> here", "")]));
    assert_eq!(parsed[0].action, "Enter here");
}

#[test]
fn empty_steps_builds_placeholder_step() {
    let xml = build_steps_xml(&[]);
    let parsed = parse_steps_xml(&xml);
    assert_eq!(parsed.len(), 1);
    assert!(xml.contains("<step id=\"2\""));
    assert!(xml.contains("last=\"1\""));
}

#[test]
fn parse_malformed_xml_returns_empty() {
    assert!(parse_steps_xml("<steps><step").is_empty());
    assert!(parse_steps_xml("").is_empty());
    assert!(parse_steps_xml("   ").is_empty());
}

#[test]
fn parse_html_encoded_content() {
    let xml = concat!(
        "<steps id=\"0\" last=\"2\"><step id=\"2\" type=\"ActionStep\">",
        "<parameterizedString isformatted=\"true\">&lt;div&gt;Click &amp;amp; hold&lt;/div&gt;",
        "</parameterizedString>",
        "<parameterizedString isformatted=\"true\">Done</parameterizedString>",
        "</step></steps>"
    );
    let steps = parse_steps_xml(xml);
    assert_eq!(steps.len(), 1);
    assert!(steps[0].action.contains("Click"));
    assert_eq!(steps[0].expected, "Done");
}

#[test]
fn html_to_text_strips_tags_and_keeps_structure() {
    let txt = html_to_text("<div>First</div><ul><li>one</li><li>two</li></ul>");
    assert!(txt.contains("First"));
    assert!(txt.contains("\u{2022} one"));
    assert!(!txt.contains('<'));
    assert_eq!(html_to_text(""), "");
    assert_eq!(html_to_text("   "), "");
}
