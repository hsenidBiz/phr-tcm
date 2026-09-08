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

/// Real markup still goes. ADO stores each step's HTML escaped inside
/// `parameterizedString`, so this is what the rich-text editor's own output
/// looks like coming back.
#[test]
fn parse_strips_real_html_markup() {
    let parsed = parse_steps_xml(&build_steps_xml(&[step(
        "<DIV><P>Click <B>Login</B></P></DIV>",
        "<P>Dashboard shown</P><BR/>",
    )]));
    assert_eq!(parsed[0].action, "Click Login");
    assert_eq!(parsed[0].expected, "Dashboard shown");
}

/// The regression this replaced a test for. v1 deleted every `<...>` run,
/// and so did this port - which cost a developer 62 fragments across 20
/// cases. A SQL step written as `WHERE performance_cycle_id = <cycleId>`
/// came back as `WHERE performance_cycle_id =`: silent, still valid JSON,
/// and still readable enough to skim past in review.
#[test]
fn angle_bracket_text_a_user_typed_survives_the_round_trip() {
    let cases = [
        // The reported payload, near enough verbatim.
        (
            "Run: SELECT stage_status_id FROM perf_cp_stage_status WHERE performance_cycle_id = <cycleId> AND timeline_stage_id = <stageId>;",
            "Rows are returned for <cycle id> and <employee>.",
        ),
        // A spec quote naming an element the editor never emits.
        ("The popup body is a <textarea> labelled \"Reason (optional)\".", ""),
        // Multi-word: not tag-shaped at all, but the old scan ate it too.
        ("Pick <next assessment stage> from the list.", ""),
        // A bare less-than must not swallow the rest of the sentence.
        ("Check start_date < GETUTCDATE() and confirm a < b holds.", ""),
    ];
    for (action, expected) in cases {
        let parsed = parse_steps_xml(&build_steps_xml(&[step(action, expected)]));
        assert_eq!(parsed[0].action, action, "action changed");
        assert_eq!(parsed[0].expected, expected, "expected changed");
    }
}

/// Preconditions take a different road out of Azure DevOps (`html_to_text`,
/// which strips before unescaping) - so it was already lossless. Pinned so
/// the two paths cannot diverge again.
#[test]
fn precondition_text_keeps_its_angle_brackets_too() {
    assert_eq!(html_to_text("A cycle exists with id &lt;cycleId&gt;"), "A cycle exists with id <cycleId>");
    assert_eq!(html_to_text("<P>A cycle exists</P>"), "A cycle exists");
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
fn step_ids_come_from_the_real_xml_not_index_math() {
    // ADO reassigns arbitrary ids once a case is edited in the web UI.
    let xml = concat!(
        "<steps id=\"0\" last=\"9\">",
        "<step id=\"7\" type=\"ActionStep\"><parameterizedString/><parameterizedString/></step>",
        "<step id=\"2\" type=\"ActionStep\"><parameterizedString/><parameterizedString/></step>",
        "<step id=\"9\" type=\"ActionStep\"><parameterizedString/><parameterizedString/></step>",
        "</steps>"
    );
    assert_eq!(
        v2_lib::steps_xml::parse_step_ids(xml),
        vec!["7".to_string(), "2".to_string(), "9".to_string()]
    );
    assert!(v2_lib::steps_xml::parse_step_ids("").is_empty());
    assert!(v2_lib::steps_xml::parse_step_ids("<steps><step").is_empty());
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
