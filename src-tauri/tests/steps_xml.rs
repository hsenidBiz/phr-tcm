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

// ------------------------------------------------------------ merge

/// A merge whose every shared-step reference is writable.
fn merge(original: &str, steps: &[Step]) -> String {
    v2_lib::steps_xml::merge_steps_xml(original, steps).expect("every reference is writable")
}

/// A case Azure DevOps holds with formatting, a screenshot, out-of-order
/// ids and a shared step - everything a rebuild used to destroy.
const RICH: &str = concat!(
    "<steps id=\"0\" last=\"9\">",
    "<step id=\"7\" type=\"ValidateStep\"><parameterizedString isformatted=\"true\">&lt;DIV&gt;&lt;B&gt;Click Save&lt;/B&gt;",
    "&lt;IMG src=\"http://ado/att/1.png\"&gt;&lt;/DIV&gt;</parameterizedString>",
    "<parameterizedString isformatted=\"true\">Saved</parameterizedString><description/></step>",
    "<step id=\"9\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Wait</parameterizedString>",
    "<parameterizedString isformatted=\"true\"></parameterizedString></step>",
    "<compref id=\"4\" ref=\"812\" />",
    "<step id=\"3\" type=\"ValidateStep\"><parameterizedString isformatted=\"true\">Check</parameterizedString>",
    "<parameterizedString isformatted=\"true\">Done</parameterizedString></step>",
    "</steps>"
);

const RICH_SAVE: &str = "<step id=\"7\" type=\"ValidateStep\"><parameterizedString isformatted=\"true\">&lt;DIV&gt;&lt;B&gt;Click Save&lt;/B&gt;&lt;IMG src=\"http://ado/att/1.png\"&gt;&lt;/DIV&gt;</parameterizedString><parameterizedString isformatted=\"true\">Saved</parameterizedString><description/></step>";
const RICH_CHECK: &str = "<step id=\"3\" type=\"ValidateStep\"><parameterizedString isformatted=\"true\">Check</parameterizedString><parameterizedString isformatted=\"true\">Done</parameterizedString></step>";
const RICH_SHARED: &str = "<compref id=\"4\" ref=\"812\" />";

#[test]
fn editing_one_step_keeps_every_other_node_verbatim_and_every_id() {
    use v2_lib::steps_xml::parse_step_ids;
    let mut steps = parse_steps_xml(RICH);
    steps[1].action = "Wait 5 seconds".into();
    let merged = merge(RICH, &steps);
    assert!(merged.contains(RICH_SAVE), "markup and screenshot survive: {merged}");
    assert!(merged.contains(RICH_SHARED), "{merged}");
    assert!(merged.contains(RICH_CHECK), "{merged}");
    assert!(
        merged.contains("<step id=\"9\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Wait 5 seconds</parameterizedString>"),
        "the edited step keeps its id: {merged}"
    );
    assert!(merged.starts_with("<steps id=\"0\" last=\"9\">"), "{merged}");
    assert_eq!(parse_steps_xml(&merged), steps);
    assert_eq!(parse_step_ids(&merged), vec!["7", "9", "", "3"]);
}

#[test]
fn an_inserted_step_gets_a_fresh_id_above_last() {
    use v2_lib::steps_xml::parse_step_ids;
    let mut steps = parse_steps_xml(RICH);
    steps.insert(1, step("Enter the name", "Name shown"));
    let merged = merge(RICH, &steps);
    assert!(merged.contains("<step id=\"10\" type=\"ValidateStep\"><parameterizedString isformatted=\"true\">Enter the name</parameterizedString>"), "{merged}");
    assert!(merged.starts_with("<steps id=\"0\" last=\"10\">"), "{merged}");
    assert!(merged.contains(RICH_SAVE) && merged.contains(RICH_CHECK) && merged.contains(RICH_SHARED));
    assert_eq!(parse_step_ids(&merged), vec!["7", "10", "9", "", "3"]);
    assert_eq!(parse_steps_xml(&merged), steps);
}

#[test]
fn a_deleted_step_goes_and_its_id_is_not_reissued() {
    let mut steps = parse_steps_xml(RICH);
    steps.remove(1); // "Wait", id 9 - the highest id
    let merged = merge(RICH, &steps);
    assert!(!merged.contains("id=\"9\""), "{merged}");
    assert!(merged.starts_with("<steps id=\"0\" last=\"9\">"), "last never goes down: {merged}");
    steps.push(step("One more", ""));
    let again = merge(RICH, &steps);
    assert!(again.contains("<step id=\"10\""), "a new step never reuses 9: {again}");
}

#[test]
fn reordering_moves_the_original_nodes() {
    use v2_lib::steps_xml::parse_step_ids;
    let old = parse_steps_xml(RICH);
    let steps = vec![old[3].clone(), old[2].clone(), old[0].clone(), old[1].clone()];
    let merged = merge(RICH, &steps);
    assert!(merged.starts_with(&format!("<steps id=\"0\" last=\"9\">{RICH_CHECK}{RICH_SHARED}{RICH_SAVE}")), "{merged}");
    assert_eq!(parse_step_ids(&merged), vec!["3", "", "7", "9"]);
}

#[test]
fn a_shared_step_is_kept_moved_removed_or_added_by_reference() {
    let old = parse_steps_xml(RICH);
    // Moved to the front, and a local step edited so this is not a no-op.
    let mut moved = vec![old[2].clone(), old[0].clone(), old[1].clone(), old[3].clone()];
    moved[3].expected = "All done".into();
    let merged = merge(RICH, &moved);
    assert!(merged.starts_with(&format!("<steps id=\"0\" last=\"9\">{RICH_SHARED}{RICH_SAVE}")), "{merged}");

    // Removed.
    let without: Vec<Step> = old.iter().filter(|s| s.shared.is_none()).cloned().collect();
    let merged = merge(RICH, &without);
    assert!(!merged.contains("compref"), "{merged}");
    assert_eq!(parse_steps_xml(&merged), without);

    // A reference that was not in the original gets a fresh compref.
    let mut added = old.clone();
    added.push(shared(900));
    let merged = merge(RICH, &added);
    assert!(merged.contains(RICH_SHARED), "{merged}");
    assert!(merged.contains("<compref id=\"10\" ref=\"900\" />"), "{merged}");
    assert_eq!(parse_steps_xml(&merged), added);
}

/// The same Shared Steps inserted twice: each reference keeps its own
/// original node, first unused first.
#[test]
fn repeated_references_each_keep_their_own_node() {
    let xml = concat!(
        "<steps id=\"0\" last=\"5\">",
        "<compref id=\"4\" ref=\"812\" />",
        "<step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Wait</parameterizedString><parameterizedString isformatted=\"true\"></parameterizedString></step>",
        "<compref id=\"5\" ref=\"812\" />",
        "</steps>"
    );
    let steps = vec![shared(812), step("Wait longer", ""), shared(812)];
    let merged = merge(xml, &steps);
    let first = merged.find("<compref id=\"4\"").expect("first kept");
    let second = merged.find("<compref id=\"5\"").expect("second kept");
    assert!(first < second, "{merged}");
}

/// A compref's nested child steps travel with it, byte for byte.
#[test]
fn a_compref_with_children_is_kept_whole() {
    let mut steps = parse_steps_xml(WITH_SHARED);
    steps[0].action = "Open the app".into();
    let merged = merge(WITH_SHARED, &steps);
    assert!(merged.contains("<compref id=\"5\" ref=\"901\"><step id=\"6\" type=\"ValidateStep\"><parameterizedString isformatted=\"true\">Inner</parameterizedString>"), "{merged}");
    assert!(merged.contains("<step id=\"2\" type=\"ValidateStep\"><parameterizedString isformatted=\"true\">Open the app</parameterizedString>"), "{merged}");
}

/// An unchanged step keeps its markup even when its type is wrong: only
/// the attribute changes, as in retype_steps_xml.
#[test]
fn an_unchanged_step_with_a_wrong_type_is_retyped_in_place() {
    let mistyped = RICH.replacen("<step id=\"7\" type=\"ValidateStep\">", "<step id=\"7\" type=\"ActionStep\">", 1);
    let mut steps = parse_steps_xml(&mistyped);
    steps[3].expected = "All done".into();
    let merged = merge(&mistyped, &steps);
    assert!(merged.contains(RICH_SAVE), "retyped, markup intact: {merged}");
}

#[test]
fn merging_the_parsed_steps_back_changes_nothing_they_say() {
    for xml in [RICH, WITH_SHARED] {
        let parsed = parse_steps_xml(xml);
        assert_eq!(parse_steps_xml(&merge(xml, &parsed)), parsed);
    }
}

#[test]
fn new_text_in_a_merge_is_escaped_for_both_layers() {
    let mut steps = parse_steps_xml(RICH);
    steps[1].action = "Run WHERE id = <cycleId>".into();
    let merged = merge(RICH, &steps);
    assert!(merged.contains("Run WHERE id = &amp;lt;cycleId&amp;gt;"), "{merged}");
}

#[test]
fn with_no_usable_original_merge_is_a_build() {
    let steps = vec![step("Open", "Shown"), shared(812)];
    for original in ["", "   ", "<steps><step", "<steps id=\"0\" last=\"1\"/>"] {
        assert_eq!(merge(original, &steps), build_steps_xml(&steps), "{original:?}");
    }
}

/// A compref whose `ref` cannot be read parses as `shared: Some(0)`, and
/// `ref="0"` must never reach Azure DevOps. Such a reference is the
/// original node, verbatim, matched in order among the original's
/// unreadable comprefs; when no original node can supply it, there is
/// nothing safe to write and the merge refuses.
#[test]
fn an_unreadable_shared_reference_is_kept_verbatim_or_refused() {
    use v2_lib::steps_xml::merge_steps_xml;
    let xml = concat!(
        "<steps id=\"0\" last=\"4\">",
        "<compref id=\"2\" ref=\"abc\" />",
        "<step id=\"3\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Wait</parameterizedString><parameterizedString isformatted=\"true\"></parameterizedString></step>",
        "<compref id=\"4\" ref=\"\"><step id=\"5\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Inner</parameterizedString><parameterizedString isformatted=\"true\"></parameterizedString></step></compref>",
        "</steps>"
    );
    let mut steps = parse_steps_xml(xml);
    assert_eq!(steps, vec![shared(0), step("Wait", ""), shared(0)]);

    // Edited, and the local step moved last: both originals kept, in order.
    steps[1].action = "Wait longer".into();
    let moved = vec![steps[0].clone(), steps[2].clone(), steps[1].clone()];
    let merged = merge(xml, &moved);
    let first = merged.find("<compref id=\"2\" ref=\"abc\" />").expect("first kept verbatim");
    let second = merged.find("<compref id=\"4\" ref=\"\"><step id=\"5\"").expect("second kept whole");
    assert!(first < second, "{merged}");
    assert!(!merged.contains("ref=\"0\""), "{merged}");
    assert!(merged.contains("<step id=\"3\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Wait longer"), "{merged}");

    // One more unreadable reference than the original can supply: refused.
    let mut extra = moved.clone();
    extra.push(shared(0));
    assert_eq!(merge_steps_xml(xml, &extra), None);

    // No usable original to supply it: refused, never built as ref="0".
    for original in ["", "<steps><step"] {
        assert_eq!(merge_steps_xml(original, &[step("Open", ""), shared(0)]), None, "{original:?}");
    }
}

/// A prolog or a comment around the root belongs to the original, and a
/// merge keeps it where it was.
#[test]
fn a_merge_keeps_what_sits_outside_the_root() {
    let xml = concat!(
        "<?xml version=\"1.0\"?><!-- kept -->",
        "<steps id=\"0\" last=\"2\"><step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Open</parameterizedString><parameterizedString isformatted=\"true\"></parameterizedString></step></steps>",
        "<!-- after -->"
    );
    let merged = merge(xml, &[step("Open the page", "")]);
    assert!(merged.starts_with("<?xml version=\"1.0\"?><!-- kept --><steps "), "{merged}");
    assert!(merged.ends_with("</steps><!-- after -->"), "{merged}");
    assert!(merged.contains("Open the page"), "{merged}");
    assert!(merged.contains("<step id=\"2\""), "the edited step keeps its id: {merged}");
}

/// Only a `<steps>` document is edited in place. Anything else used to be
/// closed with `</steps>` and stop being XML; it is rebuilt instead.
#[test]
fn a_root_that_is_not_steps_is_rebuilt_not_closed_as_steps() {
    let xml = "<list id=\"0\" last=\"2\"><step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Open</parameterizedString><parameterizedString isformatted=\"true\"></parameterizedString></step></list>";
    let steps = vec![step("Open the page", "")];
    assert_eq!(merge(xml, &steps), build_steps_xml(&steps));
}
