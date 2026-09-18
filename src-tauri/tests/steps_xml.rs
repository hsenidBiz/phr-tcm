//! Golden vectors ported 1:1 from v1 tests/test_xml_builder.py.

use v2_lib::steps_xml::{build_steps_xml, html_to_text, parse_steps_xml, Step};

fn step(action: &str, expected: &str) -> Step {
    Step {
        action: action.to_string(),
        expected: expected.to_string(),
        shared: None,
    }
}

fn shared(reference: i32) -> Step {
    Step { shared: Some(reference), ..Default::default() }
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

/// A step with an Expected Result is a ValidateStep - the kind a run marks
/// Pass/Fail against, and the kind execution automation can judge. The web
/// form writes it that way; this builder wrote every step as an ActionStep
/// and quietly undid the form's work on the next bulk update. A step with
/// nothing to check stays an ActionStep, as does the empty placeholder.
#[test]
fn a_step_with_an_expected_result_is_a_validate_step() {
    let xml = build_steps_xml(&[
        step("Open the login page", "Login page is shown"),
        step("Enter the password", ""),
        step("Submit", "   "),
    ]);
    assert!(xml.contains("<step id=\"2\" type=\"ValidateStep\">"), "{xml}");
    assert!(xml.contains("<step id=\"3\" type=\"ActionStep\">"), "{xml}");
    assert!(xml.contains("<step id=\"4\" type=\"ActionStep\">"), "whitespace is not a result: {xml}");
    assert!(build_steps_xml(&[]).contains("type=\"ActionStep\""), "the placeholder checks nothing");
}

/// Repairing the type of steps the app wrote wrongly must not cost the
/// markup Azure DevOps holds: the original XML is kept as it is - ids,
/// formatting, images, `<description/>` - and only the `type` attribute
/// changes, and only where it is wrong.
#[test]
fn retype_keeps_the_original_xml_and_changes_only_wrong_types() {
    use v2_lib::steps_xml::{parse_steps_xml, retype_steps_xml};
    let xml = concat!(
        "<steps id=\"0\" last=\"9\">",
        "<step id=\"7\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">&lt;B&gt;Open&lt;/B&gt;</parameterizedString>",
        "<parameterizedString isformatted=\"true\">Shown</parameterizedString><description/></step>",
        "<step id=\"9\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Wait</parameterizedString>",
        "<parameterizedString isformatted=\"true\"></parameterizedString></step>",
        "<step id=\"4\" type=\"ValidateStep\"><parameterizedString isformatted=\"true\">Check</parameterizedString>",
        "<parameterizedString isformatted=\"true\">Done</parameterizedString></step>",
        "</steps>"
    );
    let steps = parse_steps_xml(xml);
    let fixed = retype_steps_xml(xml, &steps).expect("step 7 has a result but is an ActionStep");
    assert!(fixed.contains("<step id=\"7\" type=\"ValidateStep\">"), "{fixed}");
    assert!(fixed.contains("<step id=\"9\" type=\"ActionStep\">"), "no result stays an action: {fixed}");
    assert!(fixed.contains("<step id=\"4\" type=\"ValidateStep\">"), "already right, untouched: {fixed}");
    assert!(fixed.contains("&lt;B&gt;Open&lt;/B&gt;"), "markup survives: {fixed}");
    assert!(fixed.contains("<description/>"), "extra elements survive: {fixed}");
    assert_eq!(fixed.len(), xml.len() + ("ValidateStep".len() - "ActionStep".len()));

    // Nothing wrong: nothing to write.
    assert!(retype_steps_xml(&fixed, &steps).is_none());
    // A step count that does not match the XML is not something to guess at.
    assert!(retype_steps_xml(xml, &steps[..2]).is_none());
    assert!(retype_steps_xml("", &steps).is_none());
}

#[test]
fn parse_step_types_reads_them_in_document_order() {
    let xml = concat!(
        "<steps id=\"0\" last=\"3\">",
        "<step id=\"2\" type=\"ValidateStep\"><parameterizedString/><parameterizedString/></step>",
        "<step id=\"3\" type=\"ActionStep\"><parameterizedString/><parameterizedString/></step>",
        "</steps>"
    );
    assert_eq!(v2_lib::steps_xml::parse_step_types(xml), vec!["ValidateStep", "ActionStep"]);
    assert!(v2_lib::steps_xml::parse_step_types("").is_empty());
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
    // Two layers: the XML's, then the HTML's. One pass left "Click &amp; hold".
    assert_eq!(steps[0].action, "Click & hold");
    assert_eq!(steps[0].expected, "Done");
}

/// The bug behind 140 cases showing as changed the moment they were
/// uploaded: Azure DevOps stores a step's HTML escaped inside the XML, so a
/// quote comes back as `&amp;quot;`. Decoding the XML layer alone left the
/// literal text `&quot;My Assessment&quot;`, which never matched the file.
#[test]
fn parse_decodes_the_html_layer_under_the_xml_layer() {
    let xml = concat!(
        "<steps id=\"0\" last=\"2\"><step id=\"2\" type=\"ValidateStep\">",
        "<parameterizedString isformatted=\"true\">&lt;DIV&gt;&lt;P&gt;Open the &amp;quot;My Assessment&amp;quot; tab &amp;amp; wait&lt;/P&gt;&lt;/DIV&gt;</parameterizedString>",
        "<parameterizedString isformatted=\"true\">Reads 3.60 &amp;lt; 5.00; the &amp;lt;cycleId&amp;gt; placeholder stays</parameterizedString>",
        "</step></steps>"
    );
    let steps = parse_steps_xml(xml);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].action, "Open the \"My Assessment\" tab & wait");
    // Typed angle brackets survive: `<cycleId>` is not a tag the editor emits.
    assert_eq!(steps[0].expected, "Reads 3.60 < 5.00; the <cycleId> placeholder stays");

    // What this app writes (both layers escaped, see D3) reads back
    // unchanged too, so a case round-trips whether or not the server
    // normalised it.
    let ours = v2_lib::steps_xml::build_steps_xml(&[step("Open the \"My Assessment\" tab & wait", "Reads 3.60 < 5.00")]);
    let back = parse_steps_xml(&ours);
    assert_eq!(back[0].action, "Open the \"My Assessment\" tab & wait");
    assert_eq!(back[0].expected, "Reads 3.60 < 5.00");
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

/// A case as Azure DevOps holds it once somebody inserted Shared Steps in
/// the web form: two `<compref>`s among the case's own steps, one of them
/// carrying child steps.
const WITH_SHARED: &str = concat!(
    "<steps id=\"0\" last=\"6\">",
    "<step id=\"2\" type=\"ValidateStep\"><parameterizedString isformatted=\"true\">Open</parameterizedString>",
    "<parameterizedString isformatted=\"true\">Shown</parameterizedString><description/></step>",
    "<compref id=\"3\" ref=\"812\" />",
    "<step id=\"4\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Wait</parameterizedString>",
    "<parameterizedString isformatted=\"true\"></parameterizedString></step>",
    "<compref id=\"5\" ref=\"901\"><step id=\"6\" type=\"ValidateStep\"><parameterizedString isformatted=\"true\">Inner</parameterizedString>",
    "<parameterizedString isformatted=\"true\"></parameterizedString></step></compref>",
    "</steps>"
);

/// A shared-step reference is one step of its own, in document order. It
/// used to be skipped while its nested steps were read as the case's own.
#[test]
fn a_compref_parses_as_one_shared_step_in_document_order() {
    assert_eq!(
        parse_steps_xml(WITH_SHARED),
        vec![step("Open", "Shown"), shared(812), step("Wait", ""), shared(901)]
    );
}

/// Run results address steps by id, index-aligned with the parsed steps. A
/// compref's slot holds "" (no per-step mark can be recorded against it)
/// and its nested step ids never shift the case's own.
#[test]
fn step_ids_and_types_stay_aligned_with_the_parsed_steps() {
    use v2_lib::steps_xml::{parse_step_ids, parse_step_types};
    assert_eq!(parse_step_ids(WITH_SHARED), vec!["2", "", "4", ""]);
    assert_eq!(parse_step_types(WITH_SHARED), vec!["ValidateStep", "", "ActionStep", ""]);
    assert_eq!(parse_steps_xml(WITH_SHARED).len(), parse_step_ids(WITH_SHARED).len());
}

/// Retyping works around shared steps and never touches them, or the steps
/// nested in them.
#[test]
fn retype_skips_shared_steps_and_their_children() {
    use v2_lib::steps_xml::retype_steps_xml;
    let steps = parse_steps_xml(WITH_SHARED);
    assert!(retype_steps_xml(WITH_SHARED, &steps).is_none(), "every local type is already right");

    let mistyped = WITH_SHARED.replacen("<step id=\"2\" type=\"ValidateStep\">", "<step id=\"2\" type=\"ActionStep\">", 1);
    let fixed = retype_steps_xml(&mistyped, &steps).expect("step 2 has a result");
    assert_eq!(fixed, WITH_SHARED, "only the one attribute changes; the comprefs are byte-for-byte");

    // A list that says "local step" where the XML has a compref does not line up.
    let mut wrong = steps.clone();
    wrong[1] = step("Log in", "");
    assert!(retype_steps_xml(&mistyped, &wrong).is_none());
}

/// D4: a self-closing step with no type used to come out as
/// `<step id="2"/ type="...">` - broken XML.
#[test]
fn retype_of_a_self_closing_step_stays_well_formed() {
    use v2_lib::steps_xml::{parse_step_types, retype_steps_xml};
    for xml in [
        "<steps id=\"0\" last=\"2\"><step id=\"2\"/></steps>",
        "<steps id=\"0\" last=\"2\"><step id=\"2\" /></steps>",
    ] {
        let fixed = retype_steps_xml(xml, &[step("", "Shown")]).expect("no type is a wrong type");
        assert_eq!(fixed, "<steps id=\"0\" last=\"2\"><step id=\"2\" type=\"ValidateStep\"/></steps>");
        assert_eq!(parse_step_types(&fixed), vec!["ValidateStep"]);
    }
}

/// A new case with a shared step writes a compref in its place, with the
/// same id sequence the local steps use.
#[test]
fn build_writes_a_shared_step_as_a_compref() {
    let steps = vec![step("Open", ""), shared(812), step("Check", "Done")];
    let xml = build_steps_xml(&steps);
    assert!(xml.contains("<step id=\"2\" type=\"ActionStep\">"), "{xml}");
    assert!(xml.contains("<compref id=\"3\" ref=\"812\" />"), "{xml}");
    assert!(xml.contains("<step id=\"4\" type=\"ValidateStep\">"), "{xml}");
    assert!(xml.contains("last=\"4\""), "{xml}");
    assert_eq!(parse_steps_xml(&xml), steps);
}

/// D3: Azure DevOps renders a step's text as HTML, so text the user typed
/// has to be escaped for that layer as well as the XML one. With the XML
/// layer alone, a typed `<cycleId>` reached ADO as a tag and disappeared
/// from its UI, and a typed `&lt;` came back as `<`.
#[test]
fn step_text_is_escaped_for_the_html_layer_too() {
    let xml = build_steps_xml(&[step("Use <cycleId> & \"quotes\"", "Reads &lt; literally")]);
    assert!(xml.contains("Use &amp;lt;cycleId&amp;gt; &amp;amp; &amp;quot;quotes&amp;quot;"), "{xml}");
    assert!(xml.contains("Reads &amp;amp;lt; literally"), "{xml}");
    let back = parse_steps_xml(&xml);
    assert_eq!(back[0].action, "Use <cycleId> & \"quotes\"");
    assert_eq!(back[0].expected, "Reads &lt; literally", "no double decode");
}

/// Shared steps carry no action of their own; that is not an empty step.
#[test]
fn a_case_with_a_shared_step_is_valid() {
    let tc = v2_lib::model::TestCase {
        title: "T".into(),
        steps: vec![shared(812), step("Check", "Done")],
        automation_status: "Planned".into(),
        ..Default::default()
    };
    assert_eq!(tc.is_valid(), Ok(()));
    let empty = v2_lib::model::TestCase { steps: vec![step(" ", "x")], ..tc };
    assert!(empty.is_valid().unwrap_err().contains("Step 1 action is empty"));
}

/// The shape a case file carries: `shared` only when set.
#[test]
fn shared_serialises_only_when_set() {
    assert_eq!(serde_json::to_value(step("a", "b")).unwrap(), serde_json::json!({"action": "a", "expected": "b"}));
    assert_eq!(serde_json::to_value(shared(812)).unwrap()["shared"], 812);
    let old: Step = serde_json::from_str("{\"action\":\"a\",\"expected\":\"b\"}").unwrap();
    assert_eq!(old, step("a", "b"));
}
