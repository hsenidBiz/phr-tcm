//! The bridge's router, tested without sockets: routing, validation, and
//! the ADO-backed routes (Task 2) via wiremock. The TCP loop (Task 3) is
//! deliberately thin - everything interesting lives in `route`.

use v2_lib::ai_bridge::{bridge_may_write, new_token, q, route, BridgeContext};

fn ctx() -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        module_ref: Some("Custom.Module".into()),
        preconditions_ref: Some("Custom.Preconditions".into()),
        disabled_tools: vec![],
        working_dir: None,
        db_id: None,
        db_secrets: None,
        db_writes: false,
        api_writes: false,
    }
}

#[test]
fn tokens_are_32_hex_and_unique() {
    let a = new_token();
    let b = new_token();
    assert_eq!(a.len(), 32);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}

#[tokio::test]
async fn ping_answers_without_a_client() {
    let (status, body) = route(&ctx(), None, "GET", "/ping", "", "1.10.3").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["app"], "tcm");
    assert_eq!(v["org"], "acme");
    assert_eq!(v["version"], "1.10.3", "reports the real app version, not this crate's");
}

#[tokio::test]
async fn unknown_routes_404() {
    let (status, _) = route(&ctx(), None, "GET", "/secrets", "", "1.10.3").await;
    assert_eq!(status, 404);
    // DELETE used to fall through to the same 404; it is now a refused
    // write - see the refusal tests at the bottom of this file.
}

#[test]
fn query_parsing_survives_valueless_pairs() {
    assert_eq!(q("/test-cases?flag&pbi=42", "pbi").as_deref(), Some("42"));
    assert_eq!(q("/x?module=Pay+roll%20HR", "module").as_deref(), Some("Pay roll HR"));
    assert_eq!(q("/x?a=1", "b"), None);
    assert_eq!(q("/noquery", "a"), None);
}

#[test]
fn query_parsing_decodes_arbitrary_percent_escapes() {
    // mcp.rs percent-encodes search text with a generic RFC 3986 encoder
    // (not just spaces) so literal '&'/'='/etc. in the query text survive
    // the naive '&'-split in `q`. The decoder here must be the matching
    // generic counterpart, not a %20-only special case.
    assert_eq!(
        q("/search-pbis?q=Search%20%26%20Filter", "q").as_deref(),
        Some("Search & Filter")
    );
}

use v2_lib::ado::AdoClient;
use wiremock::matchers::{method as wm_method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Wiremock host standing in for ADO; the routes hit the same endpoints
/// the app's own screens use.
async fn ado_stub() -> (MockServer, AdoClient) {
    let server = MockServer::start().await;
    let client = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri());
    (server, client)
}

/// The two warnings that came out of round-2 feedback, exercised through
/// the route rather than against the checker directly - the wiring is the
/// part that was missing, not the detection.
#[tokio::test]
async fn validate_warns_about_markup_and_merged_branches() {
    let draft = serde_json::json!({
        "test_cases": [
            {
                "title": "Quote a spec element",
                "automation_status": "Not Automated",
                "steps": [
                    { "action": "Open the reject popup.", "expected": "A <textarea> is shown." },
                    { "action": "Read the body.", "expected": "It is <div> wrapped." }
                ]
            },
            {
                "title": "Actions and Measures visibility",
                "automation_status": "Not Automated",
                "steps": [
                    { "action": "Sign in as the manager.", "expected": "The dashboard is shown." },
                    { "action": "Open the appraisal.", "expected": "The form is shown." },
                    { "action": "Expand the goal row.", "expected": "An Actions and Measures section is shown." },
                    { "action": "Turn off the action_measure_enabled setting.", "expected": "Saved." },
                    { "action": "Expand the goal row again.", "expected": "No Actions and Measures section is shown." }
                ]
            },
            {
                "title": "A plain case with a placeholder",
                "automation_status": "Not Automated",
                "steps": [
                    { "action": "Run SELECT * FROM t WHERE id = <cycleId>;", "expected": "One row is returned." }
                ]
            }
        ]
    })
    .to_string();

    let (status, body) = route(&ctx(), None, "POST", "/validate", &draft, "1.18.6").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], serde_json::Value::Null, "{body}");
    assert_eq!(v["cases"], 3, "{body}");
    let warnings = v["warnings"].as_array().unwrap();
    let all = warnings
        .iter()
        .map(|w| w.as_str().unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" | ");

    // A real element name will be eaten by the round trip; say so.
    assert!(
        all.contains("Quote a spec element") && all.contains("HTML tag name"),
        "expected a markup warning, got: {all}"
    );
    // The merged positive/negative - an ADVISORY, not a warning, since
    // round 3: "fix every warning" must stay followable as written, and a
    // judgement call in that channel devalues the warnings that are not.
    assert!(!all.contains("both branches"), "the split suggestion must not be a warning: {all}");
    let advisories = v["advisories"]
        .as_array()
        .expect("advisories key present when the branch check fires")
        .iter()
        .map(|w| w.as_str().unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        advisories.contains("Actions and Measures visibility") && advisories.contains("both branches"),
        "expected a merged-branch advisory, got: {advisories}"
    );
    assert!(
        v["advisories_note"].as_str().unwrap_or_default().contains("judgement"),
        "{body}"
    );
    // And the ordinary case stays quiet - <cycleId> survives now, so
    // warning about it would be noise.
    assert!(
        !all.contains("A plain case with a placeholder"),
        "a safe placeholder must not warn: {all}"
    );
}

/// A `comment` on a new case, or `reviewer_notes` that reads like a problem
/// report, both point the assistant at the case's own `findings` list -
/// the removed `record_finding` tool's replacement. A `comment` on a case
/// with an id is the developer's own, round-tripped through the file, and
/// draws no advisory.
#[tokio::test]
async fn validate_advises_when_the_human_fields_carry_the_assistants_words() {
    let draft = serde_json::json!({
        "test_cases": [
            { "title": "New case with a comment", "automation_status": "Not Automated",
              "comment": "Spec and code disagree here",
              "steps": [{ "action": "Open the page.", "expected": "It opens." }] },
            { "id": 155170, "title": "Existing case with the developer's comment", "automation_status": "Not Automated",
              "comment": "Blocked until the API lands",
              "steps": [{ "action": "Open the page.", "expected": "It opens." }] },
            { "title": "Note that reports a problem", "automation_status": "Not Automated",
              "reviewer_notes": "Checks the cut-off. Spec: S.md 7.7\n> \"closed at cut-off\"\nNote: the code contradicts the spec here.",
              "steps": [{ "action": "Open the page.", "expected": "It opens." }] }
        ]
    })
    .to_string();
    let (status, body) = route(&ctx(), None, "POST", "/validate", &draft, "1.10.3").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let adv: Vec<String> = v["advisories"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect();
    assert!(adv.iter().any(|a| a.contains("Test case 1") && a.contains("comment") && a.contains("`findings`")), "{adv:?}");
    assert!(!adv.iter().any(|a| a.contains("Test case 2") && a.contains("comment")), "{adv:?}");
    assert!(adv.iter().any(|a| a.contains("Test case 3") && a.contains("reviewer_notes") && a.contains("`findings`")), "{adv:?}");
}

/// A clean draft gets no advisories KEY at all - an empty list would read
/// as "the check ran and might have said something", and the shape of a
/// clean response should not change because a new check exists.
#[tokio::test]
async fn a_clean_draft_carries_no_advisories_key() {
    let draft = serde_json::json!({
        "test_cases": [{
            "title": "Submit saves the form",
            "automation_status": "Not Automated",
            "steps": [
                { "action": "Sign in.", "expected": "The dashboard is shown." },
                { "action": "Click Submit.", "expected": "A confirmation is shown." }
            ]
        }]
    })
    .to_string();
    let (status, body) = route(&ctx(), None, "POST", "/validate", &draft, "1.18.12").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v.get("advisories").is_none(), "{body}");
}

/// validate_cases runs the real importer, so a case of only Shared Steps -
/// no action anywhere - is one case with no warning, not "skipped".
#[tokio::test]
async fn validate_accepts_a_step_that_is_only_a_shared_reference() {
    let draft = serde_json::json!({ "test_cases": [{
        "title": "Reuses the login steps",
        "automation_status": "Not Automated",
        "steps": [{ "shared": 812 }]
    }] })
    .to_string();
    let (status, body) = route(&ctx(), None, "POST", "/validate", &draft, "1.25.15").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"], serde_json::Value::Null, "{body}");
    assert_eq!(v["cases"], 1, "{body}");
    assert!(!body.contains("has no steps"), "{body}");
}

/// A `Spec:` citation with neither a quote nor the fixed exemption form is
/// a judgement call flagged as an advisory, not a warning - the same
/// register as the both-branches check. Uses `speccov::parse_citations`
/// (Task 1's parser) rather than a second regex over `reviewer_notes`, so
/// the guide's citation grammar and this check can never drift apart.
#[tokio::test]
async fn a_spec_cited_case_without_quote_or_exemption_is_an_advisory_not_a_warning() {
    let draft = serde_json::json!({
        "test_cases": [
            {
                "title": "Cited with no quote or exemption",
                "reviewer_notes": "Checks the export button.\nSpec: S.md 7.7",
                "automation_status": "Not Automated",
                "steps": [{ "action": "Open the page.", "expected": "The button is shown." }]
            },
            {
                "title": "Cited with the exemption form",
                "reviewer_notes": "Checks the state table.\nSpec: S.md 7.9 - no quotable text (requirement is a state table)",
                "automation_status": "Not Automated",
                "steps": [{ "action": "Open the page.", "expected": "The table matches." }]
            },
            {
                "title": "Code-only citation",
                "reviewer_notes": "Checks the helper.\nCode: IndexModel.CanCopyFromPreviousCycle",
                "automation_status": "Not Automated",
                "steps": [{ "action": "Run the helper.", "expected": "It returns true." }]
            }
        ]
    })
    .to_string();

    let (status, body) = route(&ctx(), None, "POST", "/validate", &draft, "1.19.17").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["warnings"].as_array().unwrap(), &Vec::<serde_json::Value>::new(), "{body}");

    let advisories = v["advisories"]
        .as_array()
        .expect("advisories key present when an uncited quote is missing")
        .iter()
        .map(|w| w.as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(advisories.len(), 1, "{body}");
    assert!(
        advisories[0].contains("Cited with no quote or exemption"),
        "the advisory has to name the case: {:?}",
        advisories
    );
    assert!(
        !advisories.iter().any(|a| a.contains("Cited with the exemption form")),
        "the exemption form must clear the advisory: {:?}",
        advisories
    );
    assert!(
        !advisories.iter().any(|a| a.contains("Code-only citation")),
        "a Code:-only citation must not trip the quote-rule advisory: {:?}",
        advisories
    );
}

/// Round 8 §1-§4: three of five writers, with a verbatim quote already in
/// the note, went looking for a MISSING quote - because that is what the
/// advisory said. When a blockquote exists but sits above the `Spec:`
/// line, the message has to name the position, not the absence.
#[tokio::test]
async fn a_misordered_quote_is_told_where_the_checker_reads_it() {
    let draft = serde_json::json!({
        "test_cases": [
            {
                "title": "Quote above the pointer",
                "reviewer_notes": "Checks the layout.\n\n> | Employee Details | Name, ID |\n\nSpec: S.md Report Design",
                "automation_status": "Not Automated",
                "steps": [{ "action": "Open the report.", "expected": "Four fields are shown." }]
            },
            {
                "title": "No quote anywhere",
                "reviewer_notes": "Checks the export button.\nSpec: S.md 7.7",
                "automation_status": "Not Automated",
                "steps": [{ "action": "Open the page.", "expected": "The button is shown." }]
            }
        ]
    })
    .to_string();

    let (status, body) = route(&ctx(), None, "POST", "/validate", &draft, "1.23.2").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let advisories: Vec<String> = v["advisories"]
        .as_array()
        .expect("both cases are advisories")
        .iter()
        .map(|a| a.as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(advisories.len(), 2, "{body}");
    let above = advisories.iter().find(|a| a.contains("Quote above the pointer")).unwrap();
    assert!(above.contains("not where the checker reads it"), "{above}");
    assert!(above.contains("table or code block is not a quote"), "{above}");
    let bare = advisories.iter().find(|a| a.contains("No quote anywhere")).unwrap();
    assert!(!bare.contains("not where the checker reads it"), "a bare citation keeps the plain advice: {bare}");
    assert!(bare.contains("no quotable text"), "{bare}");
}

#[tokio::test]
async fn guide_carries_format_rules_and_live_modules() {
    let _style = crate::serial::writing_style();
    let (server, client) = ado_stub().await;
    // Module picklist: allowedValues empty -> falls back to values-in-use,
    // exactly like the app's own module picker.
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/wit/workitemtypes/Test%20Case/fields/Custom.Module"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "allowedValues": ["Login", "Payroll"]
        })))
        .mount(&server)
        .await;

    let (status, body) = route(&ctx(), Some(&client), "GET", "/guide", "", "1.10.3").await;
    assert_eq!(status, 200);
    assert!(body.contains("Not Automated"), "statuses come from VALID_STATUSES");
    assert!(body.contains("Planned"));
    assert!(body.contains("semicolon"), "tag separator rule");
    assert!(body.contains("Login") && body.contains("Payroll"), "live modules");
    assert!(body.contains("optimize_cases"), "the guide points at the optimizer");
    assert!(body.contains("\"shared\": N"), "the guide names the Shared Steps form");

    // Every case gets an area path, so the Test map draws itself from
    // files assistants write - nobody edits JSON to get a tree.
    let area = body
        .split("## area")
        .nth(1)
        .and_then(|rest| rest.split("## Findings").next())
        .expect("the guide has an area section");
    assert!(area.contains("Manage Events / Create / Validation"), "{area}");
    assert!(area.contains("same spelling"), "{area}");
    assert!(area.contains("Never sent to Azure DevOps"), "{area}");
    assert!(area.contains("not the work item's Area Path"), "{area}");

    // The style rules are scoped to the case text - an assistant that read
    // them as a rule for its own replies would stop explaining itself.
    let style = body
        .split("## Writing style")
        .nth(1)
        .and_then(|rest| rest.split("## area").next())
        .expect("the guide has a writing style section");
    assert!(style.contains("TEST CASES THEMSELVES"), "{style}");
    assert!(style.contains("not bound by it"), "{style}");
    assert!(style.contains("AVOID em dashes"), "{style}");
    assert!(style.contains("active voice"), "{style}");

    // Reviewer notes are two things and no more: what this case checks,
    // in words anyone can read, and where the requirement lives. Both
    // halves are pinned because the field has drifted twice - first into
    // paragraphs of reasoning, then into per-case boilerplate repeating
    // where the SET came from, which the developer had already settled at
    // intake and was reading on every single case.
    let notes = body
        .split("## reviewer_notes")
        .nth(1)
        .and_then(|rest| rest.split("## One branch per case").next())
        .expect("the guide still has a reviewer_notes section");

    // 1. Say what it checks, plainly.
    assert!(
        notes.contains("What this case checks"),
        "the note has to start with the point of the case: {notes}"
    );
    assert!(
        notes.contains("plain sentences"),
        "and in terms a non-specialist can read: {notes}"
    );

    // 2. Then the pointer - a shape to copy, not just a prohibition.
    assert!(notes.contains("Spec:") && notes.contains("Code:"), "{notes}");

    // And the three things that must NOT be in it.
    assert!(
        notes.contains("Leave OUT"),
        "the exclusions have to be stated, not implied: {notes}"
    );
    assert!(
        notes.contains("authority = app"),
        "names the provenance boilerplate it is banning, by example: {notes}"
    );
    assert!(
        notes.contains("SET's scope"),
        "a note is about one case, not the whole set: {notes}"
    );

    // Findings live in the case's own `findings` list now - the
    // record_finding tool is gone, and a problem noticed in a
    // reviewer_notes review is redirected there too.
    let findings = body
        .split("## Findings")
        .nth(1)
        .and_then(|rest| rest.split("## reviewer_notes").next())
        .expect("the guide has a findings section");
    assert!(findings.contains("`findings`"), "{findings}");
    assert!(findings.contains("test_case, spec or code"), "{findings}");
    assert!(findings.contains("never write `comment`"), "{findings}");
    assert!(!findings.contains("record_finding"), "the tool is gone: {findings}");
    assert!(
        notes.contains("`findings`"),
        "a problem in a note is redirected to the case's findings: {notes}"
    );

    // Reaching for a hand-rolled generator when a tool falls short is not
    // hypothetical: a draft too large for validate_cases once led to a
    // workaround that reported a pass over cases it never checked. The
    // guide has to forbid it AND say what to do instead, or the
    // prohibition just leaves the assistant stuck.
    let rebuild = body
        .split("## Use these tools")
        .nth(1)
        .and_then(|rest| rest.split("## Format").next())
        .expect("the guide still tells the assistant not to rebuild the tools");
    assert!(rebuild.contains("Do NOT write your own"), "{rebuild}");
    assert!(
        rebuild.contains("generator") && rebuild.contains("validate"),
        "names what not to rebuild: {rebuild}"
    );
    // The escape hatch, and that it routes to the developer rather than
    // to a workaround.
    assert!(rebuild.contains("STOP"), "{rebuild}");
    assert!(
        rebuild.contains("ask the") && rebuild.contains("developer"),
        "a blocked assistant has to ask, not improvise: {rebuild}"
    );
    // The database tools belong in the same section - they are a source to
    // check against, and the one thing to say about them is what they are
    // NOT: the expected result still comes from the specification.
    assert!(
        rebuild.contains("`db_lookup`") && rebuild.contains("`db_query`"),
        "the guide names the database tools: {rebuild}"
    );
    assert!(
        rebuild.contains("current state"),
        "and says the database reports the present, not the requirement: {rebuild}"
    );
}

/// Slices the guide body to the text under `heading`, up to (not
/// including) the next `##` heading, or the end of the body when `heading`
/// is the last one. Every guide test that reads a single section should
/// use this instead of hand-rolling `.split(...).nth(1)...` at the call
/// site.
fn section<'a>(body: &'a str, heading: &str) -> &'a str {
    let start = body.find(heading).unwrap_or_else(|| panic!("heading {heading:?} not found in guide")) + heading.len();
    let rest = &body[start..];
    match rest.find("\n## ") {
        Some(end) => &rest[..end],
        None => rest,
    }
}

/// The owner asked for two things: a section teaching what a "reasonable"
/// edge case is (one a tester can run from the application itself, not one
/// that needs developer tools or a database change), and a rule that puts
/// on-screen names in double quotation marks so "save" the word and "Save"
/// the button are never confused.
#[tokio::test]
async fn the_guide_asks_for_reasonable_edge_cases_and_quoted_names() {
    let _style = crate::serial::writing_style();
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/wit/workitemtypes/Test%20Case/fields/Custom.Module"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "allowedValues": ["Login"]
        })))
        .mount(&server)
        .await;

    let (status, g) = route(&ctx(), Some(&client), "GET", "/guide", "", "1.23.2").await;
    assert_eq!(status, 200, "{g}");

    let edge = section(&g, "## Edge cases worth writing");
    assert!(edge.contains("without signing in"), "the one edge case the owner named must be there: {edge}");
    assert!(
        edge.contains("Do NOT write cases that need developer tools"),
        "the boundary of a reasonable edge case must be stated: {edge}"
    );
    assert!(edge.contains("reviewer_notes"), "{edge}");

    let style = section(&g, "## Writing style");
    assert!(style.contains("double quotation marks"), "{style}");
    assert!(style.contains("the \"Save\" button"), "{style}");

    // Order: edge cases come after one-branch and before the live modules.
    let a = g.find("## One branch per case").unwrap();
    let b = g.find("## Edge cases worth writing").unwrap();
    let c = g.find("## Allowed Module values").unwrap();
    assert!(a < b && b < c, "edge cases must sit between one-branch and the live modules: {g}");
}

/// The quote rule: a citation either carries the source's own words
/// verbatim, or states one of the fixed exemptions - never a paraphrase
/// dressed up as a quote. Pinned separately from the round-3 assertions
/// above because round-round-6 feedback found the old single sentence
/// ("one short quote only when the exact wording IS the requirement")
/// left "no quote" as a silent third option nobody was told to justify.
#[tokio::test]
async fn the_guide_teaches_the_quote_rule_and_its_exemptions() {
    let _style = crate::serial::writing_style();
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/wit/workitemtypes/Test%20Case/fields/Custom.Module"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "allowedValues": ["Login"]
        })))
        .mount(&server)
        .await;

    let (status, body) = route(&ctx(), Some(&client), "GET", "/guide", "", "1.19.17").await;
    assert_eq!(status, 200);

    // The always-attempt-a-quote rule replaces the old "one short quote
    // only when..." clause outright - the two must never coexist, or an
    // assistant reading top to bottom hits contradictory instructions.
    assert!(
        !body.contains("one short quote only when the exact wording IS"),
        "the old single-quote clause must be replaced, not left alongside the new rule: {body}"
    );
    assert!(
        body.contains("verbatim, or it must not be presented as a quote"),
        "the guide has to state the quote-or-exemption rule in these terms: {body}"
    );
    // The fixed exemption form, ASCII hyphen only - an em dash in the guide
    // prose itself was the round-6 misparse trap, even though the parser
    // also accepts the typographic dashes reviewers paste from Word.
    assert!(
        body.contains("no quotable text"),
        "the guide has to name the fixed exemption form: {body}"
    );
    assert!(
        !body.contains("\u{2013} no quotable text") && !body.contains("\u{2014} no quotable text"),
        "the guide's own example must use a plain ASCII hyphen, not a typographic dash: {body}"
    );
    // The four exemption reasons from spec section 7, named so an author
    // has a menu to pick from rather than inventing wording.
    for reason in ["code-not-prose", "absence", "table/diagram", "synthesis"] {
        assert!(body.contains(reason), "the guide has to name the {reason} exemption: {body}");
    }

    // Workflow step 2.5: check_spec_coverage runs BEFORE optimize_cases,
    // while the draft is still in spec order (round-5 §4) - not after it,
    // and not just mentioned somewhere in the document.
    let workflow = body
        .split("## Workflow")
        .nth(1)
        .expect("the guide still has a Workflow section");
    assert!(
        workflow.contains("check_spec_coverage"),
        "the workflow has to call out check_spec_coverage: {workflow}"
    );
    let optimize_at = workflow.find("optimize_cases").expect("optimize_cases still in the workflow");
    let coverage_at = workflow.find("check_spec_coverage").unwrap();
    let validate_at = workflow.find("validate_cases").expect("validate_cases still in the workflow");
    assert!(
        coverage_at < optimize_at && optimize_at < validate_at,
        "check_spec_coverage has to run BEFORE optimize_cases, while the draft is still in spec order: {workflow}"
    );
    assert!(
        workflow.contains("uncovered") && workflow.contains("out of scope for this batch"),
        "the workflow step has to say what to do with `uncovered`, not just name the tool: {workflow}"
    );
}

#[tokio::test]
async fn test_cases_return_real_cases_in_import_shape() {
    let (server, client) = ado_stub().await;
    // The same two calls the runner/edit screens make: ids-for-PBI, then
    // batch details. Match loosely on path; the client's own tests pin the
    // exact query strings.
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 42,
            "relations": [
                {"rel": "Microsoft.VSTS.Common.TestedBy-Forward",
                 "url": format!("{}/acme/Web/_apis/wit/workitems/201", server.uri())}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "id": 201,
                "fields": {
                    "System.Title": "Login - valid credentials",
                    "System.Tags": "smoke",
                    "Microsoft.VSTS.TCM.AutomationStatus": "Planned",
                    "Custom.Module": "Login",
                    "Custom.Preconditions": "Account exists",
                    "Microsoft.VSTS.TCM.Steps": "<steps id=\"0\" last=\"2\"><step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Open page</parameterizedString><parameterizedString isformatted=\"true\">Shown</parameterizedString><description/></step></steps>"
                }
            }]
        })))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/test-cases?pbi=42&limit=5", "", "1.10.3").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let cases = v["test_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0]["id"], 201);
    assert_eq!(cases[0]["title"], "Login - valid credentials");
    assert_eq!(cases[0]["module"], "Login");
    assert_eq!(cases[0]["steps"][0]["action"], "Open page");
}

/// The suite tree an assistant reads is the one the Test Suites tab shows:
/// every plan's suites minus the structural root, one row each with the
/// ids `get_suite_test_cases` takes. The filter reaches plan names, suite
/// names and a requirement suite's PBI id.
#[tokio::test]
async fn suites_list_the_plan_tree_and_filter_by_name_or_pbi() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 9, "name": "Web - Auth Plan", "areaPath": "Web", "rootSuite": { "id": 90 } }]
        })))
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                { "id": 90, "name": "Web - Auth Plan", "suiteType": "staticTestSuite" },
                { "id": 91, "name": "Login", "suiteType": "staticTestSuite", "parentSuite": { "id": 90 } },
                { "id": 92, "name": "42 : Checkout", "suiteType": "requirementTestSuite", "requirementId": 42, "parentSuite": { "id": 91 } }
            ]
        })))
        .mount(&server)
        .await;

    let (status, body) = route(&ctx(), Some(&client), "GET", "/suites", "", "1.10.3").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let rows = v["suites"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "the root suite is structure, not a row: {body}");
    assert_eq!(rows[0]["plan_id"], 9);
    assert_eq!(rows[0]["plan_name"], "Web - Auth Plan");
    assert_eq!(rows[0]["suite_id"], 91);
    assert_eq!(rows[0]["suite_type"], "staticTestSuite");
    assert!(rows[0]["parent_suite_id"].is_null(), "a child of the root is top-level: {body}");
    assert_eq!(rows[1]["requirement_id"], 42);
    assert_eq!(rows[1]["parent_suite_id"], 91);

    // By suite name, and by the PBI a requirement suite is bound to.
    let (_, body) = route(&ctx(), Some(&client), "GET", "/suites?q=login", "", "1.10.3").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["suites"].as_array().unwrap().len(), 1);
    assert_eq!(v["suites"][0]["suite_name"], "Login");
    let (_, body) = route(&ctx(), Some(&client), "GET", "/suites?q=42", "", "1.10.3").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["suites"][0]["suite_id"], 92, "{body}");

    // Three listings, one scan: the tree is cached.
    let scans = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/testplan/plans"))
        .count();
    assert_eq!(scans, 1);
}

/// A suite's cases come by way of its test points, as on Run Tests: each
/// case once even when two configurations give it two points, in the
/// suite's order, in the same record shape `get_test_cases` returns.
/// `children=true` is the endpoint's own `isRecursive`.
#[tokio::test]
async fn suite_cases_follow_the_points_in_suite_order() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/Suites/91/TestPoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                { "id": 1, "testCaseReference": { "id": 202, "name": "Second" }, "configuration": { "name": "Chrome" } },
                { "id": 2, "testCaseReference": { "id": 201, "name": "First" }, "configuration": { "name": "Chrome" } },
                { "id": 3, "testCaseReference": { "id": 202, "name": "Second" }, "configuration": { "name": "Edge" } }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                { "id": 201, "fields": { "System.Title": "First", "Microsoft.VSTS.TCM.AutomationStatus": "Planned" } },
                { "id": 202, "fields": { "System.Title": "Second", "Custom.Module": "Login",
                    "Microsoft.VSTS.TCM.Steps": "<steps id=\"0\" last=\"2\"><step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Open page</parameterizedString><parameterizedString isformatted=\"true\">Shown</parameterizedString><description/></step></steps>" } }
            ]
        })))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/suite-cases?plan=9&suite=91&limit=5", "", "1.10.3").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let cases = v["test_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 2, "one row per case, not per point: {body}");
    assert_eq!(cases[0]["id"], 202, "the suite's order, not id order: {body}");
    assert_eq!(cases[0]["module"], "Login");
    assert_eq!(cases[0]["steps"][0]["action"], "Open page");
    assert_eq!(cases[1]["id"], 201);
    assert_eq!(v["total"], 2);
    assert_eq!(v["suite_id"], 91);

    let (status, _) =
        route(&ctx(), Some(&client), "GET", "/suite-cases?plan=9&suite=91&children=true", "", "1.10.3").await;
    assert_eq!(status, 200);
    let recursive = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path().ends_with("/TestPoint"))
        .filter(|r| r.url.query().unwrap_or("").contains("isRecursive=true"))
        .count();
    assert_eq!(recursive, 1, "children=true asks the endpoint for the child suites' points too");

    let (status, body) = route(&ctx(), Some(&client), "GET", "/suite-cases?plan=9", "", "1.10.3").await;
    assert_eq!(status, 400);
    assert!(body.contains("search_test_suites"), "{body}");
}

#[tokio::test]
async fn search_wiki_503_without_a_client() {
    let (status, body) = route(&ctx(), None, "GET", "/search-wiki?q=auth", "", "1.10.3").await;
    assert_eq!(status, 503);
    assert!(body.contains("sign in"));
}

#[tokio::test]
async fn wiki_page_503_without_a_client() {
    let (status, body) =
        route(&ctx(), None, "GET", "/wiki-page?wiki=w1&path=/Docs/Guide", "", "1.10.3").await;
    assert_eq!(status, 503);
    assert!(body.contains("sign in"));
}

#[tokio::test]
async fn search_wiki_returns_hits_via_wiremock() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("POST"))
        .and(wm_path("/acme/Web/_apis/search/wikisearchresults"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [{
                "fileName": "Auth.md",
                "path": "/Docs/Auth",
                "wiki": {"id": "w1", "name": "Team.wiki"},
                "hits": [{"highlights": ["snippet"]}]
            }]
        })))
        .mount(&server)
        .await;
    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/search-wiki?q=auth", "", "1.10.3").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let results = v["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["wiki_id"], "w1");
    assert_eq!(results[0]["highlights"], "snippet");
}

#[tokio::test]
async fn wiki_page_returns_content_via_wiremock() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/wiki/wikis/w1/pages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "path": "/Docs/Auth",
            "content": "# Auth\nfull text"
        })))
        .mount(&server)
        .await;
    let (status, body) = route(
        &ctx(),
        Some(&client),
        "GET",
        "/wiki-page?wiki=w1&path=/Docs/Auth",
        "",
        "1.10.3",
    )
    .await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["path"], "/Docs/Auth");
    assert!(v["content"].as_str().unwrap().contains("full text"));
}

/// A pasted URL names its own wiki, so demanding `wiki` as well turned the
/// one thing a person has to hand into a 400 they could not act on.
#[tokio::test]
async fn wiki_page_accepts_a_url_without_a_wiki_id() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/wiki/wikis/HRM.wiki/pages/9486"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "path": "/Issue Meal",
            "content": "# Issue Meal"
        })))
        .mount(&server)
        .await;
    let (status, body) = route(
        &ctx(),
        Some(&client),
        "GET",
        "/wiki-page?path=https%3A%2F%2Fdev.azure.com%2FPeoplesHR%2FHRM%2F_wiki%2Fwikis%2FHRM.wiki%2F9486%2FIssue-Meal",
        "",
        "1.10.3",
    )
    .await;
    assert_eq!(status, 200, "body: {body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["path"], "/Issue Meal");
}

/// A bare path still needs its wiki, and the refusal has to say so - the
/// url case is the exception, not the new rule.
#[tokio::test]
async fn wiki_page_still_demands_a_wiki_id_for_a_bare_path() {
    let (server, client) = ado_stub().await;
    drop(server);
    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/wiki-page?path=/Docs/Auth", "", "1.10.3").await;
    assert_eq!(status, 400);
    assert!(body.contains("wiki"), "the refusal must name what is missing: {body}");
}

#[tokio::test]
async fn test_cases_without_pbi_400_with_guidance() {
    let (_server, client) = ado_stub().await;
    let (status, body) = route(&ctx(), Some(&client), "GET", "/test-cases", "", "1.10.3").await;
    assert_eq!(status, 400);
    assert!(body.contains("pbi") && body.contains("ids"), "names both ways in: {body}");
    let (status, body) = route(&ctx(), Some(&client), "GET", "/test-cases?ids=12,abc", "", "1.10.3").await;
    assert_eq!(status, 400, "a malformed id is refused, not silently dropped: {body}");
}

/// The work-item batch answers for one test case the way the PBI and suite
/// listings already parse it.
fn case_json(id: i32, title: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "fields": {
            "System.Title": title,
            "Microsoft.VSTS.TCM.Steps": "<steps id=\"0\" last=\"2\"><step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Open page</parameterizedString><parameterizedString isformatted=\"true\">Shown</parameterizedString><description/></step></steps>"
        }
    })
}

/// A case read by its own id needs no PBI, and comes back in the order the
/// ids were asked for - ADO's batch answers in id order.
#[tokio::test]
async fn test_cases_by_id_alone_return_those_cases_in_the_order_asked() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [case_json(201, "Login - first"), case_json(202, "Login - second")]
        })))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/test-cases?ids=202,201,202", "", "1.10.3").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let ids: Vec<i64> = v["test_cases"].as_array().unwrap().iter().map(|c| c["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, vec![202, 201], "requested order, each case once");
    assert_eq!(v["test_cases"][0]["steps"][0]["action"], "Open page");
}

/// An id that is not a work item at all makes ADO refuse the whole batch;
/// the assistant is told which lookup failed instead of a raw 502.
#[tokio::test]
async fn test_cases_by_an_unknown_id_404_with_guidance() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let (status, body) = route(&ctx(), Some(&client), "GET", "/test-cases?ids=999999", "", "1.10.3").await;
    assert_eq!(status, 404, "{body}");
    assert!(body.contains("999999"), "names the ids it looked for: {body}");
}

/// With a PBI and ids together, the answer is that PBI's cases narrowed to
/// those ids - and an id the PBI is not tested by is named, not ignored.
#[tokio::test]
async fn test_cases_for_a_pbi_narrowed_to_ids_name_the_ones_not_on_it() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 42,
            "relations": [
                {"rel": "Microsoft.VSTS.Common.TestedBy-Forward", "url": format!("{}/acme/Web/_apis/wit/workitems/201", server.uri())},
                {"rel": "Microsoft.VSTS.Common.TestedBy-Forward", "url": format!("{}/acme/Web/_apis/wit/workitems/203", server.uri())}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/_apis/wit/workitems"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [case_json(201, "Login - first"), case_json(203, "Login - third")]
        })))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/test-cases?pbi=42&ids=203,555", "", "1.10.3").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let cases = v["test_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0]["id"], 203);
    assert_eq!(v["not_on_pbi"], serde_json::json!([555]));
}

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn tcp_server_guards_with_token_and_serves_ping() {
    let shared = v2_lib::ai_bridge::BridgeState::new(ctx(), "0.0.0-test".into());
    // Test-specific handshake path: the real handshake_path() location belongs
    // to a live app - a test run must never clobber it (it did once: proxies
    // then saw a dead port + test token until Settings was reopened).
    let hs_path = std::env::temp_dir().join(format!("tcm-v2-hs-test-{}.json", std::process::id()));
    let (port, token) =
        v2_lib::ai_bridge::start_listener(Arc::clone(&shared), None, Some(hs_path.clone()))
            .await
            .unwrap();

    async fn send(port: u16, req: String) -> String {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(req.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await.unwrap();
        String::from_utf8_lossy(&buf).to_string()
    }

    // Wrong token -> 401, no body.
    let resp = send(
        port,
        "GET /ping HTTP/1.1\r\nHost: x\r\nx-bridge-token: wrong\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(resp.starts_with("HTTP/1.1 401"), "got: {resp}");
    assert!(!resp.contains("tcm"));

    // Right token -> 200 with the ping payload.
    let resp = send(
        port,
        format!("GET /ping HTTP/1.1\r\nHost: x\r\nx-bridge-token: {token}\r\nConnection: close\r\n\r\n"),
    )
    .await;
    assert!(resp.starts_with("HTTP/1.1 200"), "got: {resp}");
    assert!(resp.contains("\"app\":\"tcm\""));

    // The handshake file exists and matches, including the version tcm-mcp
    // reads for its own serverInfo.version.
    let hs: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&hs_path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&hs_path);
    assert_eq!(hs["port"].as_u64().unwrap() as u16, port);
    assert_eq!(hs["token"].as_str().unwrap(), token);
    assert_eq!(hs["version"].as_str().unwrap(), "0.0.0-test");
}


/// Tool toggles: the app publishes what is switched off, and the MCP layer
/// honours it.
#[tokio::test]
async fn the_bridge_publishes_the_disabled_tool_set() {
    let mut ctx = ctx();
    ctx.disabled_tools = vec!["search_wiki".into(), "get_wiki_page".into()];
    let (status, body) = route(&ctx, None, "GET", "/tools", "", "1.0.0").await;

    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["disabled"][0], "search_wiki");
    assert_eq!(v["disabled"].as_array().unwrap().len(), 2);
}

/// Nothing configured means nothing disabled - a fresh install must not
/// come up with an empty toolset.
#[tokio::test]
async fn no_configuration_disables_nothing() {
    let (_, body) = route(&ctx(), None, "GET", "/tools", "", "1.0.0").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["disabled"].as_array().unwrap().is_empty());
}

/// The bridge tells the proxy whether the Auto Run tools are offered - the
/// proxy is a separate process and cannot read the switch itself. This
/// test binary is a development build, so they always are here.
#[tokio::test]
async fn the_bridge_says_whether_auto_run_tools_are_offered() {
    let (status, body) = route(&ctx(), None, "GET", "/tools", "", "1.0.0").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["autorun"], true);
}

// ---- round 7 §§1-5: optimize_cases takes a path, like every other tool --

struct TempDir(std::path::PathBuf);
impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir()
            .join(format!("tcm-optimize-{nanos}-{}", N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn draft_on_disk(dir: &TempDir) -> std::path::PathBuf {
    let path = dir.0.join("draft.json");
    std::fs::write(
        &path,
        serde_json::json!({ "test_cases": [
            { "title": "A", "steps": [{ "action": "Open the module.", "expected": "It opens." }] },
            { "title": "B", "steps": [{ "action": "Open the module.", "expected": "It opens." }] }
        ]})
        .to_string(),
    )
    .unwrap();
    path
}

/// A 457 KB finished draft cannot travel inline both ways (round 7 §1) -
/// the optimizer reads it from disk like every other tool in the family.
#[tokio::test]
async fn a_draft_can_be_optimized_from_a_path() {
    let dir = TempDir::new();
    let path = draft_on_disk(&dir);
    let target = format!("/optimize?path={}", path.to_string_lossy().replace('\\', "%5C"));
    let (status, out) = route(&ctx(), None, "POST", &target, "", "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let cases = v["test_cases"].as_array().expect("transformed draft returned");
    assert_eq!(cases.len(), 2);
    assert!(
        cases.iter().all(|c| c["spec_order"].is_number() && c["tester_order"].is_number()),
        "every case carries both orders: {out}"
    );
}

/// `in_place` writes the optimized draft back and answers with the report
/// alone - the return half of the quarter-million-token cost was the other
/// half of §1's complaint.
#[tokio::test]
async fn optimize_in_place_writes_back_and_returns_only_the_report() {
    let dir = TempDir::new();
    let ctx = repo_ctx(&dir);
    let path = in_repo(&dir, &draft_on_disk(&dir));
    let target = format!(
        "/optimize?in_place=true&path={}",
        path.to_string_lossy().replace('\\', "%5C")
    );
    let (status, out) = route(&ctx, None, "POST", &target, "", "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v.get("test_cases").is_none(), "in_place must not echo the draft: {out}");
    assert_eq!(v["cases"], 2);
    assert!(v["report"].is_object(), "the report is the response: {out}");
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.contains("tester_order"), "the file was not rewritten: {on_disk}");
    assert!(!std::path::Path::new(&format!("{}.tmp", path.display())).exists());
}

/// Same contract as transform_cases: two sources is a refusal, not a
/// silent preference, and in_place with nothing to write back to is a 400.
#[tokio::test]
async fn optimize_refuses_both_sources_and_pathless_in_place() {
    let dir = TempDir::new();
    let path = draft_on_disk(&dir);
    let target = format!("/optimize?path={}", path.to_string_lossy().replace('\\', "%5C"));
    let (status, out) = route(&ctx(), None, "POST", &target, "[]", "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("not both"), "{out}");

    let (status, out) = route(&ctx(), None, "POST", "/optimize?in_place=true", "[]", "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("path"), "{out}");
}

/// A path that does not resolve names itself in the error - the assistant
/// is usually one typo away from the real file.
#[tokio::test]
async fn optimize_names_a_missing_path() {
    let (status, out) =
        route(&ctx(), None, "POST", "/optimize?path=C%3A%5Cnope%5Cmissing.json", "", "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains("missing.json"), "{out}");
}

/// The importer skips a case it cannot read and says why. Both tools threw
/// those warnings away, so a draft came back shorter with no indication -
/// "success" over work that had quietly gone missing.
#[tokio::test]
async fn optimize_and_transform_report_what_the_importer_could_not_read() {
    // Two cases; the second has no steps, which the importer skips.
    let draft = serde_json::json!({
        "test_cases": [
            { "id": null, "title": "Good", "steps": [{ "action": "Do", "expected": "Done" }] },
            { "id": null, "title": "No steps at all", "steps": [] }
        ]
    })
    .to_string();

    let (status, body) = route(&ctx(), None, "POST", "/optimize", &draft, "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let warned = v["import_warnings"].as_array().expect("import_warnings present");
    assert!(
        warned.iter().any(|w| w.as_str().unwrap_or_default().contains("No steps at all")),
        "the dropped case must be named: {warned:?}"
    );
    // And the output really is shorter, which is why silence was wrong.
    assert_eq!(v["test_cases"].as_array().unwrap().len(), 1);

    let t_body = serde_json::json!({
        "test_cases": draft,
        "operations": [{ "op": "set_tags", "value": "smoke" }],
    })
    .to_string();
    let (status, body) = route(&ctx(), None, "POST", "/transform", &t_body, "test").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        v["import_warnings"]
            .as_array()
            .expect("import_warnings present")
            .iter()
            .any(|w| w.as_str().unwrap_or_default().contains("No steps at all")),
        "transform must report it too"
    );
}

/// Offsets used to come from `String::from_utf8_lossy(raw)` and then index
/// `raw`. Those are not the same string - every invalid byte becomes a
/// three-byte U+FFFD - so one bad byte in the headers slid the body offset
/// and the request was read from the wrong place.
#[test]
fn the_body_is_found_by_byte_not_by_a_lossy_copy() {
    use v2_lib::ai_bridge::{parse_http, Parsed};

    // A header carrying a byte that is not valid UTF-8. Under the old
    // parser the decoded head was longer than the real one, so body_start
    // pointed past the start of the body.
    let mut raw = b"POST /validate?x=1 HTTP/1.1\r\nX-Note: ".to_vec();
    raw.push(0xFF);
    raw.extend_from_slice(b"\r\nContent-Length: 9\r\n\r\n{\"a\":123}");
    match parse_http(&raw) {
        // Headers that are not text at all is a fine thing to refuse - what
        // must never happen is reading the body from the wrong offset and
        // treating the result as a real request.
        Parsed::Malformed => {}
        Parsed::Complete { body, .. } => assert_eq!(body, "{\"a\":123}", "body read at the wrong offset"),
        Parsed::Incomplete => panic!("a complete request was read as incomplete"),
    }

    // The ordinary case still works, body and all.
    let ok = b"POST /validate HTTP/1.1\r\nX-Bridge-Token: abc\r\nContent-Length: 9\r\n\r\n{\"a\":123}";
    let Parsed::Complete { method, target, token, body, .. } = parse_http(ok) else {
        panic!("a well-formed request did not parse");
    };
    assert_eq!((method.as_str(), target.as_str()), ("POST", "/validate"));
    assert_eq!(token.as_deref(), Some("abc"));
    assert_eq!(body, "{\"a\":123}");
}

/// "Keep reading" and "this will never be a request" used to be the same
/// answer (None). A malformed request therefore read as incomplete, and the
/// connection sat there - to the 64 KB cap if the client kept sending, or
/// for the life of the process if it simply stopped.
#[test]
fn a_malformed_request_is_told_apart_from_an_unfinished_one() {
    use v2_lib::ai_bridge::{parse_http, Parsed};

    // Genuinely unfinished: no blank line yet, and short.
    assert!(matches!(parse_http(b"POST /validate HTTP/1.1\r\nX-A: 1\r\n"), Parsed::Incomplete));
    // Headers complete, body still arriving.
    assert!(matches!(
        parse_http(b"POST /v HTTP/1.1\r\nContent-Length: 20\r\n\r\nshort"),
        Parsed::Incomplete
    ));

    // Never going to be a request.
    assert!(matches!(
        parse_http(b"POST /v HTTP/1.1\r\nContent-Length: not-a-number\r\n\r\n"),
        Parsed::Malformed
    ));
    assert!(matches!(parse_http(b"\r\n\r\n"), Parsed::Malformed), "no request line");
    assert!(matches!(parse_http(b"GET\r\n\r\n"), Parsed::Malformed), "no target");

    // A header line without a colon is not a reason to refuse the request -
    // only Content-Length has to be right.
    assert!(matches!(
        parse_http(b"GET /ping HTTP/1.1\r\ngarbage-line\r\n\r\n"),
        Parsed::Complete { .. }
    ));

    // Endless garbage with no blank line stops being "incomplete".
    let flood = vec![b'x'; 17 * 1024];
    assert!(matches!(parse_http(&flood), Parsed::Malformed));
}

/// The failure-reading loop, end to end against wiremock: plan scan (find
/// only, never create), points, and the per-failure detail with the
/// tester's comment and linked bugs.
#[tokio::test]
async fn run_results_returns_failed_cases_with_comment_and_bugs() {
    let (server, client) = ado_stub().await;

    // One plan, whose suite list holds PBI 42's requirement suite.
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 9, "name": "Web - Auth Plan", "areaPath": "Web", "rootSuite": { "id": 90 } }]
        })))
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 91, "name": "42 : Login flow", "suiteType": "requirementTestSuite", "requirementId": 42 }]
        })))
        .mount(&server)
        .await;
    // Three points: one failed, one passed, one never run.
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/Suites/91/TestPoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {
                    "id": 7,
                    "testCaseReference": { "id": 201, "name": "Valid login" },
                    "configuration": { "name": "Windows 10" },
                    "results": { "outcome": "failed", "lastTestRunId": 3, "lastResultId": 30 }
                },
                {
                    "id": 8,
                    "testCaseReference": { "id": 202, "name": "Invalid login" },
                    "configuration": { "name": "Windows 10" },
                    "results": { "outcome": "passed", "lastTestRunId": 3, "lastResultId": 31 }
                },
                {
                    "id": 9,
                    "testCaseReference": { "id": 203, "name": "Session timeout" },
                    "configuration": { "name": "Windows 10" },
                    "results": { "outcome": "unspecified" }
                }
            ]
        })))
        .mount(&server)
        .await;
    // The failed result's report info: the tester's comment + a linked bug.
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/test/Runs/3/Results/30"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "comment": "Redirect loops back to the sign-in page on the second attempt.",
            "associatedBugs": [{ "id": "777" }]
        })))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/run-results?pbi=42", "", "1.18.11").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["outcomes"], serde_json::json!(["Failed"]), "no outcome asked for is Failed, as before");
    assert_eq!(v["matched"], 1, "only the failed point is listed - passed and never-run are not");
    assert_eq!(v["results"].as_array().unwrap().len(), 1);
    let f = &v["results"][0];
    assert_eq!(f["case_id"], 201);
    assert_eq!(f["title"], "Valid login");
    assert_eq!(f["outcome"], "Failed");
    assert_eq!(f["comment"], "Redirect loops back to the sign-in page on the second attempt.");
    assert_eq!(f["bug_ids"][0], 777);
    assert_eq!(v["plan"]["name"], "Web - Auth Plan");
    assert_eq!(v["cases_in_suite"], 3);
    // Every outcome is counted, whatever was listed.
    assert_eq!(
        v["summary"],
        serde_json::json!([
            { "outcome": "Failed", "count": 1 },
            { "outcome": "Passed", "count": 1 },
            { "outcome": "Never run", "count": 1 },
        ])
    );
}

/// A PBI's suite with one point of each common outcome, for the filter
/// tests below. Only the blocked point's result has detail to read.
async fn mount_mixed_suite(server: &wiremock::MockServer) {
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 9, "name": "Web - Auth Plan", "areaPath": "Web", "rootSuite": { "id": 90 } }]
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 91, "name": "45 : Mixed", "suiteType": "requirementTestSuite", "requirementId": 45 }]
        })))
        .mount(server)
        .await;
    let point = |id: i64, case: i64, name: &str, results: serde_json::Value| {
        serde_json::json!({
            "id": id,
            "testCaseReference": { "id": case, "name": name },
            "configuration": { "name": "Windows 10" },
            "results": results,
        })
    };
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/Suites/91/TestPoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                point(1, 301, "Passes", serde_json::json!({ "outcome": "passed", "lastTestRunId": 5, "lastResultId": 50 })),
                point(2, 302, "Passes too", serde_json::json!({ "outcome": "passed", "lastTestRunId": 5, "lastResultId": 51 })),
                point(3, 303, "Stuck", serde_json::json!({ "outcome": "blocked", "lastTestRunId": 5, "lastResultId": 52 })),
                point(4, 304, "Irrelevant here", serde_json::json!({ "outcome": "notApplicable", "lastTestRunId": 5, "lastResultId": 53 })),
                point(5, 305, "Not reached", serde_json::json!({ "outcome": "unspecified" })),
                point(6, 306, "Flaky", serde_json::json!({ "outcome": "failed", "lastTestRunId": 5, "lastResultId": 55 })),
            ]
        })))
        .mount(server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/test/Runs/5/Results/52"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "comment": "The approver list is empty, so nothing can be sent.",
            "associatedBugs": []
        })))
        .mount(server)
        .await;
}

fn listed(v: &serde_json::Value) -> Vec<i64> {
    v["results"].as_array().unwrap().iter().map(|r| r["case_id"].as_i64().unwrap()).collect()
}

#[tokio::test]
async fn run_results_lists_the_outcome_asked_for_with_its_comment() {
    let (server, client) = ado_stub().await;
    mount_mixed_suite(&server).await;
    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/run-results?pbi=45&outcome=Blocked", "", "1.18.11").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["outcomes"], serde_json::json!(["Blocked"]));
    assert_eq!(listed(&v), vec![303]);
    assert_eq!(v["results"][0]["outcome"], "Blocked");
    assert_eq!(v["results"][0]["comment"], "The approver list is empty, so nothing can be sent.");
    assert_eq!(
        v["summary"],
        serde_json::json!([
            { "outcome": "Failed", "count": 1 },
            { "outcome": "Blocked", "count": 1 },
            { "outcome": "Not applicable", "count": 1 },
            { "outcome": "Passed", "count": 2 },
            { "outcome": "Never run", "count": 1 },
        ])
    );
}

#[tokio::test]
async fn run_results_takes_several_outcomes_in_any_spelling_and_never_run() {
    let (server, client) = ado_stub().await;
    mount_mixed_suite(&server).await;
    // A list as the MCP proxy sends it: comma-joined, then URL-encoded.
    let (status, body) = route(
        &ctx(),
        Some(&client),
        "GET",
        "/run-results?pbi=45&outcome=Not%20Applicable%2Cnot_run",
        "",
        "1.18.11",
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["outcomes"], serde_json::json!(["Not applicable", "Never run"]));
    assert_eq!(listed(&v), vec![304, 305]);
    // A point that never ran has no result, so no comment is read for it.
    assert!(v["results"][1].get("comment").is_none(), "{body}");
}

#[tokio::test]
async fn run_results_all_lists_every_case() {
    let (server, client) = ado_stub().await;
    mount_mixed_suite(&server).await;
    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/run-results?pbi=45&outcome=all", "", "1.18.11").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["matched"], 6);
    assert_eq!(listed(&v), vec![301, 302, 303, 304, 305, 306], "in the suite's own order");
}

#[tokio::test]
async fn run_results_refuses_an_outcome_it_does_not_know_and_names_the_real_ones() {
    let (server, client) = ado_stub().await;
    mount_mixed_suite(&server).await;
    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/run-results?pbi=45&outcome=blokced", "", "1.18.11").await;
    assert_eq!(status, 400, "a typo must not read as \"nothing had that outcome\": {body}");
    assert!(body.contains("blokced") && body.contains("Blocked") && body.contains("Never run"), "{body}");
}

#[test]
fn outcomes_parse_in_any_spelling_and_default_to_failed() {
    use v2_lib::ai_bridge::parse_outcomes;
    assert_eq!(parse_outcomes(None).unwrap(), vec!["failed"]);
    assert_eq!(parse_outcomes(Some("  ")).unwrap(), vec!["failed"]);
    assert_eq!(parse_outcomes(Some("Failed, BLOCKED")).unwrap(), vec!["failed", "blocked"]);
    assert_eq!(parse_outcomes(Some("In Progress")).unwrap(), vec!["inprogress"]);
    assert_eq!(parse_outcomes(Some("not_applicable")).unwrap(), vec!["notapplicable"]);
    for never in ["never run", "Not run", "active", "unspecified"] {
        assert_eq!(parse_outcomes(Some(never)).unwrap(), vec!["neverrun"], "{never}");
    }
    assert_eq!(parse_outcomes(Some("passed,pass")).unwrap(), vec!["passed"], "no duplicates");
    assert_eq!(parse_outcomes(Some("all")).unwrap().len(), v2_lib::ai_bridge::RUN_OUTCOMES.len());
    assert!(parse_outcomes(Some("passed,nope")).is_err());
}

#[test]
fn a_point_with_no_verdict_is_never_run_and_an_unknown_outcome_keeps_its_name() {
    use v2_lib::ai_bridge::{outcome_key, outcome_label};
    assert_eq!(outcome_key(""), "neverrun");
    assert_eq!(outcome_label(&outcome_key("")), "Never run");
    assert_eq!(outcome_label(&outcome_key("notApplicable")), "Not applicable");
    assert_eq!(outcome_label(&outcome_key("somethingNew")), "somethingnew");
}

/// Resolving a suite scans every test plan in the project (~60s on a
/// large org) - which is why the bridge remembers the answer: the second
/// call must reuse it and go straight to the points.
#[tokio::test]
async fn run_results_resolves_the_suite_once_and_reuses_it() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 9, "name": "Web - Auth Plan", "areaPath": "Web", "rootSuite": { "id": 90 } }]
        })))
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 91, "name": "43 : Login flow", "suiteType": "requirementTestSuite", "requirementId": 43 }]
        })))
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/Suites/91/TestPoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "id": 7,
                "testCaseReference": { "id": 201, "name": "Valid login" },
                "configuration": { "name": "W10" },
                "results": { "outcome": "passed", "lastTestRunId": 3, "lastResultId": 30 }
            }]
        })))
        .mount(&server)
        .await;

    for _ in 0..2 {
        let (status, body) =
            route(&ctx(), Some(&client), "GET", "/run-results?pbi=43", "", "1.18.11").await;
        assert_eq!(status, 200, "{body}");
    }
    let scans = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/acme/Web/_apis/testplan/plans")
        .count();
    assert_eq!(scans, 1, "the plan scan must run once, not per call");
}

/// The cached suite is not immortal: when its points come back 404 (the
/// suite was deleted in Azure DevOps), the bridge forgets it, re-scans
/// once, and answers from whatever suite the PBI has now.
#[tokio::test]
async fn run_results_re_resolves_a_cached_suite_that_was_deleted() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 9, "name": "Web - Auth Plan", "areaPath": "Web", "rootSuite": { "id": 90 } }]
        })))
        .mount(&server)
        .await;
    // The suite listing names 91 exactly once - after that, the PBI's
    // requirement suite is 92 (91 "was deleted and recreated").
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 91, "name": "44 : Login flow", "suiteType": "requirementTestSuite", "requirementId": 44 }]
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 92, "name": "44 : Login flow", "suiteType": "requirementTestSuite", "requirementId": 44 }]
        })))
        .mount(&server)
        .await;
    // Suite 91's points answer once, then the suite is gone: an unmatched
    // request gets wiremock's 404, exactly what ADO returns for a deleted
    // suite. Suite 92 answers steadily.
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/Suites/91/TestPoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{
                "id": 7,
                "testCaseReference": { "id": 201, "name": "Valid login" },
                "configuration": { "name": "W10" },
                "results": { "outcome": "passed", "lastTestRunId": 3, "lastResultId": 30 }
            }]
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/Suites/92/TestPoint"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                {
                    "id": 8,
                    "testCaseReference": { "id": 201, "name": "Valid login" },
                    "configuration": { "name": "W10" },
                    "results": { "outcome": "passed", "lastTestRunId": 4, "lastResultId": 40 }
                },
                {
                    "id": 9,
                    "testCaseReference": { "id": 202, "name": "Invalid login" },
                    "configuration": { "name": "W10" },
                    "results": { "outcome": "passed", "lastTestRunId": 4, "lastResultId": 41 }
                }
            ]
        })))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/run-results?pbi=44", "", "1.18.11").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["cases_in_suite"], 1);

    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/run-results?pbi=44", "", "1.18.11").await;
    assert_eq!(status, 200, "the 404 must trigger a re-resolve, not an error: {body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["cases_in_suite"], 2, "the answer must come from the NEW suite");
}

/// A PBI with no requirement suite is an ANSWER, not an error - and above
/// all not a reason to create one. The bridge never writes to Azure DevOps.
#[tokio::test]
async fn run_results_on_a_pbi_with_no_suite_says_so_without_creating_one() {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/plans"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [{ "id": 9, "name": "Web - Auth Plan", "areaPath": "Web", "rootSuite": { "id": 90 } }]
        })))
        .mount(&server)
        .await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/testplan/Plans/9/suites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "value": [] })))
        .mount(&server)
        .await;

    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/run-results?pbi=42", "", "1.18.11").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["results"].as_array().unwrap().len(), 0);
    assert!(v["note"].as_str().unwrap().contains("never had a test run"));
    // No POST reached the mock server - wiremock 404s any unmatched
    // request, and a create would have errored the route before this line.
}

#[tokio::test]
async fn run_results_requires_a_pbi_and_a_signed_in_client() {
    let (status, _) = route(&ctx(), None, "GET", "/run-results?pbi=42", "", "1.18.11").await;
    assert_eq!(status, 503, "no client means sign in first");

    let (_, client) = ado_stub().await;
    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/run-results", "", "1.18.11").await;
    assert_eq!(status, 400);
    assert!(body.contains("?pbi="));
}

// ---- Writes are refused with a sentence, not a 404 ---------------------
//
// The bridge is read-only toward Azure DevOps by design. An assistant
// that tries to mutate gets told the action is impossible and unsafe to
// automate - a bare 404 reads as "wrong spelling, try again".

#[tokio::test]
async fn a_mutating_verb_is_refused_with_the_warning() {
    for method in ["PUT", "PATCH", "DELETE"] {
        let (status, body) = route(&ctx(), None, method, "/cases", "", "1.0.0").await;
        assert_eq!(status, 403, "{method} was not refused");
        assert!(
            body.contains("must be done through the app"),
            "{method} refusal lost its message: {body}"
        );
        assert!(
            body.contains("unsafe"),
            "the refusal must say WHY, not just no: {body}"
        );
    }
}

#[tokio::test]
async fn a_write_shaped_path_is_refused_even_as_a_post() {
    for path in ["/update-case", "/create-case", "/delete-case?id=7"] {
        let (status, body) = route(&ctx(), None, "POST", path, "{}", "1.0.0").await;
        assert_eq!(status, 403, "{path} was not refused");
        assert!(body.contains("must be done through the app"), "{path}: {body}");
    }
}

/// A plain unknown GET is still a quiet 404 - the refusal is for write
/// INTENT, not for every typo.
#[tokio::test]
async fn an_unknown_read_is_still_a_bare_404() {
    let (status, body) = route(&ctx(), None, "GET", "/nonsense", "", "1.0.0").await;
    assert_eq!(status, 404);
    assert!(body.is_empty(), "a 404 grew a body: {body}");
}

/// Round 8 dogfooding: a query-less get_tags on a 2,000-tag project dumped
/// the whole list into the assistant's context. Capped at 300, the note
/// says so and how to search the rest - and ?query= still searches ALL of
/// them, not just the first 300.
#[tokio::test]
async fn a_query_less_get_tags_is_capped_and_a_query_still_searches_everything() {
    // A dedicated org/project keeps this test's cache entry out of every
    // other test's way (the cache is process-global).
    let c = BridgeContext {
        org: "cap-org".into(),
        project: "CapProj".into(),
        module_ref: None,
        preconditions_ref: None,
        disabled_tools: vec![],
        working_dir: None,
        db_id: None,
        db_secrets: None,
        db_writes: false,
        api_writes: false,
    };
    let key = v2_lib::cache::keys::tags("cap-org", "CapProj");
    let values: Vec<String> = (0..350).map(|i| format!("tag-{i:03}")).collect();
    v2_lib::cache::put(&key, &values);

    let (status, out) = route(&c, None, "GET", "/tags", "", "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["count"], 300, "{out}");
    assert_eq!(v["total"], 350, "{out}");
    assert!(v["note"].as_str().unwrap().contains("Showing 300 of 350"), "{out}");

    // tag-349 is beyond the cap, but a query reaches it.
    let (status, out) = route(&c, None, "GET", "/tags?query=tag-349", "", "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["count"], 1, "{out}");
    assert_eq!(v["tags"][0], "tag-349", "{out}");
    assert!(!v["note"].as_str().unwrap().contains("Showing"), "{out}");
}

/// The pane is only as good as the list that feeds it: the guide tells the
/// assistant to copy the intake's documents into the file's `specs`.
#[tokio::test]
async fn the_guide_asks_for_the_specs_list() {
    let _style = crate::serial::writing_style();
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/wit/workitemtypes/Test%20Case/fields/Custom.Module"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "allowedValues": ["Login", "Payroll"]
        })))
        .mount(&server)
        .await;

    let (status, body) = route(&ctx(), Some(&client), "GET", "/guide", "", "1.10.3").await;
    assert_eq!(status, 200, "{body}");

    let section = body
        .split("## specs")
        .nth(1)
        .and_then(|rest| rest.split("## Findings").next())
        .expect("the guide has a specs section");
    assert!(section.contains("\"specs\""), "{section}");
    assert!(section.contains("wiki"), "wiki URLs are valid entries: {section}");
    assert!(section.contains("relative"), "paths relative to the file: {section}");
    // Only markdown files and wiki links are specs; code is cited, not listed.
    assert!(section.contains("Only `.md` files and Azure DevOps wiki links may be listed"), "{section}");
    assert!(section.contains(".cshtml"), "{section}");
    assert!(section.contains("reviewer_notes"), "{section}");
    assert!(section.contains("refused"), "{section}");
}

/// A draft on disk with everything a file carries besides its cases: the
/// `specs` list, the whole-set `comments`, and a key this app has never
/// heard of. An in-place tool run must leave all three exactly as written.
fn draft_with_specs_on_disk(dir: &TempDir) -> std::path::PathBuf {
    let path = dir.0.join("draft-with-specs.json");
    std::fs::write(
        &path,
        serde_json::json!({
            "format": "azure-devops-test-cases",
            "specs": ["Step13-CalculationEngine.md", "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine"],
            "comments": "whole-set note",
            "unknown_key": { "kept": true },
            "test_cases": [
                { "title": "A", "steps": [{ "action": "Open the module.", "expected": "It opens." }] },
                { "title": "B", "steps": [{ "action": "Open the module.", "expected": "It opens." }] }
            ]
        })
        .to_string(),
    )
    .unwrap();
    path
}

fn assert_file_kept_its_other_keys(on_disk: &str) {
    let doc: serde_json::Value = serde_json::from_str(on_disk).unwrap();
    assert_eq!(
        doc["specs"],
        serde_json::json!(["Step13-CalculationEngine.md", "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine"]),
        "specs survive the rewrite: {on_disk}"
    );
    assert_eq!(doc["comments"], "whole-set note", "{on_disk}");
    assert_eq!(doc["unknown_key"]["kept"], true, "{on_disk}");
}

/// An in-place optimize used to write a fresh wrapper holding only
/// `test_cases`, so the file's `specs` (and `comments`) vanished on the
/// first run. The cases are the tool's; the file is not.
#[tokio::test]
async fn optimize_in_place_keeps_the_files_specs_and_comments() {
    let dir = TempDir::new();
    let ctx = repo_ctx(&dir);
    let path = in_repo(&dir, &draft_with_specs_on_disk(&dir));
    let target = format!("/optimize?in_place=true&path={}", path.to_string_lossy().replace('\\', "%5C"));
    let (status, out) = route(&ctx, None, "POST", &target, "", "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.contains("tester_order"), "the cases were rewritten: {on_disk}");
    assert_file_kept_its_other_keys(&on_disk);
}

#[tokio::test]
async fn transform_in_place_keeps_the_files_specs_and_comments() {
    let dir = TempDir::new();
    let ctx = repo_ctx(&dir);
    let path = in_repo(&dir, &draft_with_specs_on_disk(&dir));
    let body = serde_json::json!({
        "path": path.to_string_lossy(),
        "in_place": true,
        "operations": [{ "op": "set_tags", "value": "smoke" }],
    })
    .to_string();
    let (status, out) = route(&ctx, None, "POST", "/transform", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.contains("\"smoke\""), "the cases were rewritten: {on_disk}");
    assert_file_kept_its_other_keys(&on_disk);
}

/// A file that lists a `.cshtml` (an assistant attached the views it read)
/// comes back from an in-place tool run with only the allowed specs, and
/// the response names each one it dropped.
#[tokio::test]
async fn an_in_place_tool_run_keeps_only_allowed_specs_and_warns() {
    let refused = "Views/Payroll/Index.cshtml";
    let sentence = format!(
        "specs: {refused} cannot be a spec - only .md files and Azure DevOps wiki links can be added"
    );
    let write_draft = |dir: &TempDir| {
        let path = dir.0.join("draft-with-cshtml.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "specs": ["Spec.md", refused, "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine"],
                "comments": "whole-set note",
                "test_cases": [{ "title": "A", "steps": [{ "action": "Open the module.", "expected": "It opens." }] }]
            })
            .to_string(),
        )
        .unwrap();
        path
    };
    let allowed = serde_json::json!(["Spec.md", "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine"]);

    // transform_cases
    let dir = TempDir::new();
    let rc = repo_ctx(&dir);
    let path = in_repo(&dir, &write_draft(&dir));
    let body = serde_json::json!({
        "path": path.to_string_lossy(),
        "in_place": true,
        "operations": [{ "op": "set_tags", "value": "smoke" }],
    })
    .to_string();
    let (status, out) = route(&rc, None, "POST", "/transform", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["import_warnings"].as_array().unwrap().iter().any(|w| w.as_str() == Some(sentence.as_str())), "{out}");
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("smoke"), "the cases were rewritten: {raw}");
    let on_disk: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(on_disk["specs"], allowed, "{raw}");
    assert_eq!(on_disk["comments"], "whole-set note", "{raw}");

    // optimize_cases
    let dir = TempDir::new();
    let rc = repo_ctx(&dir);
    let path = in_repo(&dir, &write_draft(&dir));
    let target = format!("/optimize?in_place=true&path={}", path.to_string_lossy().replace('\\', "%5C"));
    let (status, out) = route(&rc, None, "POST", &target, "", "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    assert!(out.contains(&sentence), "{out}");
    let raw = std::fs::read_to_string(&path).unwrap();
    let on_disk: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(on_disk["specs"], allowed, "{raw}");

    // Without in_place, the echoed specs carry only the allowed entries.
    let dir = TempDir::new();
    let path = write_draft(&dir);
    let body = serde_json::json!({ "path": path.to_string_lossy(), "operations": [{ "op": "set_tags", "value": "smoke" }] })
        .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/transform", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["specs"], allowed, "{out}");
    assert!(out.contains(&sentence), "{out}");
}

/// Without in_place the tool echoes the cases for the assistant to write
/// back itself - so the file's `specs` ride along in the response, or the
/// assistant would drop them when it rewrites the file.
#[tokio::test]
async fn transform_from_a_file_echoes_its_specs() {
    let dir = TempDir::new();
    let path = draft_with_specs_on_disk(&dir);
    let body = serde_json::json!({
        "path": path.to_string_lossy(),
        "operations": [{ "op": "set_tags", "value": "smoke" }],
    })
    .to_string();
    let (status, out) = route(&ctx(), None, "POST", "/transform", &body, "1.0.0").await;
    assert_eq!(status, 200, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["specs"], serde_json::json!(["Step13-CalculationEngine.md", "https://dev.azure.com/o/p/_wiki/wikis/p.wiki/12/Engine"]), "{out}");
    // An inline draft has no file and no specs: the key is absent, not empty.
    let inline = serde_json::json!({
        "test_cases": [{ "title": "A", "steps": [{ "action": "a", "expected": "b" }] }],
        "operations": [{ "op": "set_tags", "value": "smoke" }],
    })
    .to_string();
    let (_, out) = route(&ctx(), None, "POST", "/transform", &inline, "1.0.0").await;
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v.get("specs").is_none(), "{out}");
}

// ============================================================ the database
//
// Two routes, one guard. The route itself can only be tested as far as its
// refusals here - past them it spawns sqlcmd - so the work happens in
// `db::query`, which takes the process as a `Runner` and is exercised with
// a fake one below. The company database is not reachable from this
// machine at all, which is precisely why none of this may depend on it.

mod db_tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use v2_lib::ai_bridge::{route, BridgeContext};
    use v2_lib::db::credentials::{save, DbCredentialsForm, MemoryStore, SecretStore, OWN_ID};
    use v2_lib::db::query::{run_lookup, run_query, NO_CONNECTION, NO_LOGIN_SAVED, WRITES_OFF};
    use v2_lib::db::{
        classify, parse_connection, Connection, Output, Runner, Verdict, NOT_INSTALLED,
        READ_ONLY_SENTENCE, SQLCMD_OVERRIDE,
    };
    use v2_lib::db_defaults::DB_PRESETS;

    /// A stand-in for sqlcmd: answers with what it was built with, and
    /// keeps every call so a test can assert it was never reached.
    #[derive(Default)]
    struct FakeRunner {
        status: i32,
        stdout: String,
        stderr: String,
        calls: Mutex<Vec<Vec<String>>>,
        /// Answers handed out one per call, before `stdout` takes over.
        in_turn: Mutex<std::collections::VecDeque<String>>,
    }

    impl FakeRunner {
        fn answering(stdout: &str) -> FakeRunner {
            FakeRunner { stdout: stdout.to_string(), ..Default::default() }
        }
        fn answering_in_turn(answers: &[&str]) -> FakeRunner {
            FakeRunner {
                in_turn: Mutex::new(answers.iter().map(|a| a.to_string()).collect()),
                ..Default::default()
            }
        }
        fn failing(stderr: &str) -> FakeRunner {
            FakeRunner { status: 1, stderr: stderr.to_string(), ..Default::default() }
        }
        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Runner for FakeRunner {
        async fn run(
            &self,
            _exe: &Path,
            args: &[String],
            _env: &[(String, String)],
            _timeout: Duration,
        ) -> Result<Output, String> {
            self.calls.lock().unwrap().push(args.to_vec());
            let stdout = self.in_turn.lock().unwrap().pop_front().unwrap_or_else(|| self.stdout.clone());
            Ok(Output { status: self.status, stdout, stderr: self.stderr.clone() })
        }
    }

    fn preset(id: &str) -> Connection {
        let p = DB_PRESETS.iter().find(|p| p.id == id).expect("the preset");
        parse_connection(p.connection_string).expect("the preset parses")
    }

    fn read_only() -> Connection {
        preset("dev-read")
    }

    fn dev_login() -> Connection {
        preset("dev-login")
    }

    fn exe() -> PathBuf {
        PathBuf::from("sqlcmd.exe")
    }

    /// The context names a database by id and carries a store to resolve it
    /// in - an empty one here, so a shipped id resolves to its shipped login.
    fn with_connection(id: &str, writes: bool) -> BridgeContext {
        with_store(id, writes, Arc::new(MemoryStore::default()))
    }

    fn with_store(id: &str, writes: bool, store: Arc<dyn SecretStore>) -> BridgeContext {
        BridgeContext {
            db_id: Some(id.to_string()),
            db_secrets: Some(store),
            db_writes: writes,
            ..BridgeContext::default()
        }
    }

    /// A store whose every entry is `value` - how a test sees which string
    /// the route actually read, without ever reaching sqlcmd.
    struct Holding(Result<Option<String>, String>);
    impl SecretStore for Holding {
        fn get(&self, _: &str) -> Result<Option<String>, String> {
            self.0.clone()
        }
        fn put(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn remove(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn targets(&self, _: &str) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
    }

    // ---------------------------------------------------------- the routes

    #[tokio::test]
    async fn the_routes_read_the_login_saved_for_the_chosen_id() {
        // No password in the saved override: the route refuses on the
        // missing key, which only happens if it read the store rather than
        // the shipped string.
        let saved = Holding(Ok(Some("Server=x;Database=y;User Id=u".into())));
        let c = with_store("dev-read", false, Arc::new(saved));
        let (status, said) = route(&c, None, "POST", "/db-query", r#"{"sql":"SELECT 1"}"#, "1.0.0").await;
        assert_eq!(status, 409, "{said}");
        assert!(said.contains("Password="), "{said}");
    }

    #[tokio::test]
    async fn an_unreadable_vault_says_so_and_names_no_login() {
        let c = with_store("dev-read", false, Arc::new(Holding(Err("Could not read the saved login.".into()))));
        let (status, said) = route(&c, None, "POST", "/db-lookup", r#"{"query":"leave"}"#, "1.0.0").await;
        assert_eq!(status, 409, "{said}");
        assert_eq!(said, "Could not read the saved login.");
    }

    /// An id saved for a preset a later release removed: nothing to sign in
    /// with, so the same sentence as nothing chosen, on both routes.
    #[tokio::test]
    async fn an_id_this_build_does_not_know_is_no_connection() {
        let c = with_connection("a-preset-since-removed", false);
        for (path, body) in [
            ("/db-lookup", r#"{"query":"leave"}"#),
            ("/db-query", r#"{"sql":"SELECT 1"}"#),
        ] {
            let (status, said) = route(&c, None, "POST", path, body, "1.0.0").await;
            assert_eq!(status, 409, "{path}: {said}");
            assert_eq!(said, NO_CONNECTION, "{path}");
        }
    }

    /// The route reads a custom id's own login - not another database's:
    /// one saved without a password is refused on that missing key.
    #[tokio::test]
    async fn the_routes_read_the_login_saved_for_a_custom_id() {
        use v2_lib::db::credentials::{add_custom, WithList};
        let dir = tempfile::tempdir().unwrap();
        let store = WithList { store: MemoryStore::default(), list: dir.path().join("databases.json") };
        let form = DbCredentialsForm {
            server: "own-host".into(),
            port: None,
            database: "own-db".into(),
            user: "me".into(),
            password: Some("pw".into()),
            trust_cert: false,
        };
        let id = add_custom(&store, "Staging", &form).unwrap().id;
        store.put(&format!("tcm-v2/db/{id}"), "Server=own-host;Database=own-db;User Id=me").unwrap();
        let c = with_store(&id, false, Arc::new(store));
        let (status, said) = route(&c, None, "POST", "/db-query", r#"{"sql":"SELECT 1"}"#, "1.0.0").await;
        assert_eq!(status, 409, "{said}");
        assert!(said.contains("Password="), "{said}");
    }

    /// One of the person's own databases is known for as long as the list
    /// names it: Debug shows its id, and once it is removed the routes say
    /// nothing is chosen - never a login of some other database.
    #[tokio::test]
    async fn an_own_database_is_known_until_it_is_removed() {
        use v2_lib::db::credentials::{add_custom, remove_custom, WithList};
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(WithList { store: MemoryStore::default(), list: dir.path().join("databases.json") });
        let form = DbCredentialsForm {
            server: "own-host".into(),
            port: None,
            database: "own-db".into(),
            user: "me".into(),
            password: Some("pw-Zq9".into()),
            trust_cert: false,
        };
        let id = add_custom(&*store, "Staging", &form).unwrap().id;
        let shown = format!("{:?}", with_store(&id, false, store.clone()));
        assert!(shown.contains(&id) && !shown.contains("pw-Zq9"), "{shown}");
        // A store whose list does not name it does not know it.
        assert!(format!("{:?}", with_connection(&id, false)).contains("(unknown)"));

        remove_custom(&*store, &id).unwrap();
        let c = with_store(&id, false, store.clone());
        assert!(format!("{c:?}").contains("(unknown)"));
        for (path, body) in [
            ("/db-lookup", r#"{"query":"leave"}"#),
            ("/db-query", r#"{"sql":"SELECT 1"}"#),
        ] {
            let (status, said) = route(&c, None, "POST", path, body, "1.0.0").await;
            assert_eq!(status, 409, "{path}: {said}");
            assert_eq!(said, NO_CONNECTION, "{path}");
        }
    }

    /// The id comes from the webview, which could send anything - a whole
    /// connection string included. Debug shows a known id and nothing else.
    #[test]
    fn debug_prints_a_known_id_and_hides_anything_else() {
        let leaked = "Server=h;Database=d;User Id=u;Password=pw-Zq9";
        let shown = format!("{:?}", with_connection(leaked, false));
        assert!(!shown.contains("pw-Zq9") && !shown.contains("Server=h"), "{shown}");
        assert!(shown.contains("(unknown)"), "{shown}");
        assert!(format!("{:?}", with_connection("dev-read", false)).contains("dev-read"));
    }

    /// Your own database IS a choice - the person picked it - so telling
    /// them to pick one sends them to a control they already used. What is
    /// missing is the login, and that is what the sentence asks for.
    #[tokio::test]
    async fn your_own_database_with_nothing_saved_asks_for_a_login() {
        let c = with_connection(OWN_ID, false);
        for (path, body) in [
            ("/db-lookup", r#"{"query":"leave"}"#),
            ("/db-query", r#"{"sql":"SELECT 1"}"#),
        ] {
            let (status, said) = route(&c, None, "POST", path, body, "1.0.0").await;
            assert_eq!(status, 409, "{path}: {said}");
            assert_eq!(said, NO_LOGIN_SAVED, "{path}");
            assert!(said.contains("Edit button"), "{said}");
        }
    }

    #[tokio::test]
    async fn both_routes_need_a_connection_to_have_been_chosen() {
        let c = BridgeContext::default();
        for (path, body) in [
            ("/db-lookup", r#"{"query":"leave"}"#),
            ("/db-query", r#"{"sql":"SELECT 1"}"#),
        ] {
            let (status, said) = route(&c, None, "POST", path, body, "1.0.0").await;
            assert_eq!(status, 409, "{path}: {said}");
            assert_eq!(said, NO_CONNECTION, "{path}");
            // The sentence has to name where the choice is made.
            assert!(said.contains("AI Bridge tab"), "{said}");
        }
    }

    /// The override is authoritative, so pointing it at a path that does
    /// not exist is how a machine WITH sqlcmd installed (this one) tests
    /// the answer a machine without it gets.
    #[tokio::test]
    async fn both_routes_say_so_when_sqlcmd_is_not_installed() {
        // db_sqlcmd's own override test sets the same variable.
        let _env = crate::serial::sqlcmd_env();
        let before = std::env::var(SQLCMD_OVERRIDE).ok();
        std::env::set_var(SQLCMD_OVERRIDE, "Z:\\no\\such\\sqlcmd.exe");

        // Your own database, once a login is saved for it, gets as far as
        // looking for sqlcmd - the same as a shipped one. Kept in this test
        // because both set the process-wide override.
        let own = MemoryStore::default();
        let form = DbCredentialsForm {
            server: "own-host".into(),
            port: None,
            database: "own-db".into(),
            user: "me".into(),
            password: Some("pw".into()),
            trust_cert: false,
        };
        save(&own, OWN_ID, &form).unwrap();

        // One of the person's own databases, by its custom id: the route
        // resolves its login and gets as far as sqlcmd - the same as `own`.
        let dir = tempfile::tempdir().unwrap();
        let listed = v2_lib::db::credentials::WithList {
            store: MemoryStore::default(),
            list: dir.path().join("databases.json"),
        };
        let custom = v2_lib::db::credentials::add_custom(&listed, "Staging", &form).unwrap().id;

        for c in [
            with_connection("dev-read", false),
            with_store(OWN_ID, false, Arc::new(own)),
            with_store(&custom, false, Arc::new(listed)),
        ] {
            for (path, body) in [
                ("/db-lookup", r#"{"query":"leave"}"#),
                ("/db-query", r#"{"sql":"SELECT 1"}"#),
            ] {
                let (status, said) = route(&c, None, "POST", path, body, "1.0.0").await;
                assert_eq!(status, 409, "{path}: {said}");
                assert_eq!(said, NOT_INSTALLED, "{path}");
            }
        }

        match before {
            Some(v) => std::env::set_var(SQLCMD_OVERRIDE, v),
            None => std::env::remove_var(SQLCMD_OVERRIDE),
        }
    }

    #[tokio::test]
    async fn a_lookup_with_no_words_is_refused_before_anything_runs() {
        let c = with_connection("dev-read", false);
        for body in [r#"{"query":""}"#, r#"{"query":"   "}"#, "{}"] {
            let (status, said) = route(&c, None, "POST", "/db-lookup", body, "1.0.0").await;
            assert_eq!(status, 400, "{body}: {said}");
            assert!(said.contains("query"), "{said}");
        }
        let (status, said) = route(&c, None, "POST", "/db-query", "{}", "1.0.0").await;
        assert_eq!(status, 400, "{said}");
        assert!(said.contains("sql"), "{said}");
    }

    /// A batch body that is not the shape it says is named back before
    /// anything runs - a statement dropped in parsing would change what the
    /// transaction does.
    #[tokio::test]
    async fn a_batch_body_of_the_wrong_shape_is_refused_before_anything_runs() {
        let c = with_connection("dev-read", false);
        for (body, expect) in [
            (r#"{"sql":"SELECT 1","statements":["SELECT 2"]}"#, "not both"),
            (r#"{"statements":"SELECT 1"}"#, "must be a list"),
            (r#"{"statements":[{"sql":"SELECT 1","expect_rows":"two"}]}"#, "whole number"),
            (r#"{"statements":[{"sql":"  "}]}"#, "statement 1 has no"),
            (r#"{"statements":[42]}"#, "statement 1 must be an object"),
        ] {
            let (status, said) = route(&c, None, "POST", "/db-query", body, "1.0.0").await;
            assert_eq!(status, 400, "{body}: {said}");
            assert!(said.contains(expect), "{body}: {said}");
        }
    }

    // ------------------------------------------------------- the statement

    #[tokio::test]
    async fn a_write_on_a_read_only_connection_never_reaches_the_process() {
        // Reads its lines back from the shared log tail and the activity
        // log's own directory lock (crate::serial).
        let _log = crate::serial::log_tail();
        let _act = crate::serial::activity_log();
        let dir = tempfile::tempdir().unwrap();
        v2_lib::activity_log::init(dir.path().to_path_buf());
        let fake = FakeRunner::answering("");
        let sql = "INSERT INTO dbo.Leave (marker) VALUES ('task6fix-readonly-marker')";
        let refused = run_query(
            &fake,
            &exe(),
            &read_only(),
            // Even with the switch ON: the connection decides too.
            true,
            sql,
        )
        .await
        .unwrap_err();

        assert_eq!(refused.0, 400);
        assert_eq!(refused.1, READ_ONLY_SENTENCE);
        assert!(fake.calls().is_empty(), "a refused write reached sqlcmd");

        // A refusal is still worth a trail: the activity log should show
        // the attempt in full, why it was refused, and never the
        // connection's own user or password.
        let read_only = read_only();
        let recs = v2_lib::activity_log::directory()
            .map(|d| crate::common::activity_records(&d, "db"))
            .unwrap_or_default();
        let rec = recs.iter().find(|r| r["sql"] == sql).expect("the refusal is in the activity log");
        assert_eq!(rec["verdict"], "refused", "{rec}");
        assert_eq!(rec["why"], READ_ONLY_SENTENCE, "{rec}");
        let rec_str = rec.to_string();
        assert!(!rec_str.contains(&read_only.user), "the user is in the activity log: {rec_str}");
        assert!(!rec_str.contains(&read_only.password), "the password is in the activity log: {rec_str}");

        // The app log gets a short summary only - never the SQL.
        let lines: Vec<_> = v2_lib::applog::recent(400).into_iter().map(|l| l.message).collect();
        assert!(
            !lines.iter().any(|m| m.contains("task6fix-readonly-marker")),
            "the SQL leaked into the app log: {lines:?}"
        );
        assert!(
            lines.iter().any(|m| m.starts_with("db query ") && m.contains(READ_ONLY_SENTENCE)),
            "no summary line in the app log: {lines:?}"
        );
    }

    #[tokio::test]
    async fn a_write_with_the_switch_off_never_reaches_the_process_either() {
        let _log = crate::serial::log_tail();
        let _act = crate::serial::activity_log();
        let dir = tempfile::tempdir().unwrap();
        v2_lib::activity_log::init(dir.path().to_path_buf());
        let fake = FakeRunner::answering("");
        for verb_sql in [
            "INSERT INTO dbo.Leave (marker) VALUES ('task6fix-switch-off-marker')",
            "UPDATE dbo.Leave SET marker = 'task6fix-switch-off-marker'",
            "DELETE FROM dbo.Leave WHERE marker = 'task6fix-switch-off-marker'",
        ] {
            let refused = run_query(&fake, &exe(), &dev_login(), false, verb_sql).await.unwrap_err();
            assert_eq!(refused.0, 400, "{verb_sql}");
            assert_eq!(refused.1, WRITES_OFF, "{verb_sql}");
            assert!(refused.1.contains("AI Bridge tab"), "{}", refused.1);

            let recs = v2_lib::activity_log::directory()
                .map(|d| crate::common::activity_records(&d, "db"))
                .unwrap_or_default();
            let rec =
                recs.iter().find(|r| r["sql"] == verb_sql).expect("the refusal is in the activity log");
            assert_eq!(rec["verdict"], "refused", "{rec}");
            assert_eq!(rec["why"], WRITES_OFF, "{rec}");

            let dev = dev_login();
            let rec_str = rec.to_string();
            assert!(!rec_str.contains(&dev.user), "the user is in the activity log: {rec_str}");
            assert!(!rec_str.contains(&dev.password), "the password is in the activity log: {rec_str}");
        }
        assert!(fake.calls().is_empty(), "a switched-off write reached sqlcmd");

        let lines: Vec<_> = v2_lib::applog::recent(400).into_iter().map(|l| l.message).collect();
        assert!(
            !lines.iter().any(|m| m.contains("task6fix-switch-off-marker")),
            "the SQL leaked into the app log: {lines:?}"
        );
    }

    #[tokio::test]
    async fn a_write_needs_both_the_switch_and_the_dev_login_and_is_logged_whole() {
        let _log = crate::serial::log_tail();
        let _act = crate::serial::activity_log();
        let dir = tempfile::tempdir().unwrap();
        v2_lib::activity_log::init(dir.path().to_path_buf());
        let fake = FakeRunner::answering("");
        // A marker no other test writes, so the record is findable even
        // though several tests share this activity_log lock in sequence.
        let sql = "INSERT INTO dbo.Leave (marker) VALUES ('task6-write-marker')";
        let out = run_query(&fake, &exe(), &dev_login(), true, sql).await.expect("it runs");
        assert_eq!(out, "");
        assert_eq!(fake.calls().len(), 1, "the write reached sqlcmd exactly once");
        assert!(fake.calls()[0].contains(&sql.to_string()), "{:?}", fake.calls()[0]);

        let recs = v2_lib::activity_log::directory()
            .map(|d| crate::common::activity_records(&d, "db"))
            .unwrap_or_default();
        let rec = recs.iter().find(|r| r["sql"] == sql).expect("the write is in the activity log");
        assert_eq!(rec["verdict"], "write", "{rec}");
        assert_eq!(rec["ok"], true, "{rec}");
        assert!(rec["duration_ms"].is_u64(), "{rec}");
        assert!(
            rec["connection"].as_str().unwrap().contains("sgdev01db02.cloud/hrmmain_philippinesdev"),
            "{rec}"
        );
        // The whole statement, and never the credentials.
        let rec_str = rec.to_string();
        assert!(!rec_str.contains("abc123"), "the password is in the activity log: {rec_str}");
        assert!(!rec_str.contains("devlogin"), "the user is in the activity log: {rec_str}");

        let lines: Vec<_> = v2_lib::applog::recent(400).into_iter().map(|l| l.message).collect();
        assert!(
            !lines.iter().any(|m| m.contains("task6-write-marker")),
            "the SQL leaked into the app log: {lines:?}"
        );
        assert!(
            lines.iter().any(|m| m.starts_with("db query (Write) on ") && m.contains("ok")),
            "no summary line in the app log: {lines:?}"
        );
    }

    /// Renamed from `a_read_is_logged_short_and_without_the_credentials`:
    /// a read is no longer cut at 200 characters - the activity log is the
    /// audit record and is never shipped in a bug report, so it keeps a
    /// read's statement in full, the same as a write's.
    #[tokio::test]
    async fn a_read_is_logged_in_full_and_without_the_credentials() {
        let _log = crate::serial::log_tail();
        let _act = crate::serial::activity_log();
        let dir = tempfile::tempdir().unwrap();
        v2_lib::activity_log::init(dir.path().to_path_buf());
        let fake = FakeRunner::answering("n\n1\n");
        let long = format!("SELECT 'task6-read-marker' AS a, '{}' AS b", "x".repeat(400));
        run_query(&fake, &exe(), &read_only(), false, &long).await.expect("a read runs");

        let recs = v2_lib::activity_log::directory()
            .map(|d| crate::common::activity_records(&d, "db"))
            .unwrap_or_default();
        let rec =
            recs.iter().find(|r| r["sql"] == long.as_str()).expect("the read is in the activity log");
        assert_eq!(rec["verdict"], "read", "{rec}");
        assert_eq!(rec["sql"], long.as_str(), "a read is kept in full: {rec}");

        let read_only = read_only();
        let rec_str = rec.to_string();
        assert!(!rec_str.contains(&read_only.user), "the user is in the activity log: {rec_str}");
        assert!(!rec_str.contains(&read_only.password), "the password is in the activity log: {rec_str}");

        let lines: Vec<_> = v2_lib::applog::recent(400).into_iter().map(|l| l.message).collect();
        assert!(
            !lines.iter().any(|m| m.contains("task6-read-marker")),
            "the SQL leaked into the app log: {lines:?}"
        );
        assert!(
            !lines.iter().any(|m| m.contains(&"x".repeat(400))),
            "the SQL leaked into the app log: {lines:?}"
        );
        assert!(
            lines.iter().any(|m| m.starts_with("db query (Read) on ")),
            "no summary line in the app log: {lines:?}"
        );
    }

    #[tokio::test]
    async fn a_refused_statement_is_refused_on_the_dev_login_too() {
        let _log = crate::serial::log_tail();
        let _act = crate::serial::activity_log();
        let dir = tempfile::tempdir().unwrap();
        v2_lib::activity_log::init(dir.path().to_path_buf());
        let fake = FakeRunner::answering("");
        let dev = dev_login();
        for sql in ["DROP TABLE dbo.Leave", "SELECT 1\nGO\nSELECT 2", "EXEC sp_who"] {
            let refused = run_query(&fake, &exe(), &dev_login(), true, sql).await.unwrap_err();
            assert_eq!(refused.0, 400, "{sql}");
            assert!(!refused.1.is_empty(), "{sql}");

            // The guard's own refusal never even reaches sqlcmd's process,
            // but it still belongs in the activity log, with the reason,
            // the raw (possibly multi-line) SQL text, and never the dev
            // login's credentials.
            let recs = v2_lib::activity_log::directory()
                .map(|d| crate::common::activity_records(&d, "db"))
                .unwrap_or_default();
            let rec = recs
                .iter()
                .filter(|r| r["verdict"] == "refused" && r["why"] == refused.1.as_str())
                .last()
                .expect("the refusal is in the activity log");
            assert_eq!(rec["sql"], sql, "{sql}: {rec}");
            let rec_str = rec.to_string();
            assert!(!rec_str.contains(&dev.user), "the user is in the activity log: {rec_str}");
            assert!(!rec_str.contains(&dev.password), "the password is in the activity log: {rec_str}");
        }
        assert!(fake.calls().is_empty(), "a refused statement reached sqlcmd");
    }

    /// What sqlcmd says when it cannot reach the server - the one failure
    /// this machine CAN produce for real. It echoes the command line,
    /// password and all, which is why nothing leaves `run_sql` unredacted.
    #[tokio::test]
    async fn a_connection_failure_comes_back_as_502_without_the_password() {
        // `run_query` also touches `activity_log`'s process-wide directory
        // now - held so a concurrent test's own tempdir assertions never
        // see a stray write from this one (see serial::activity_log).
        let _act = crate::serial::activity_log();
        let password = "abc123@@@###";
        let fake = FakeRunner::failing(&format!(
            "Sqlcmd: Error: Microsoft ODBC Driver 17: Login failed (-P {password})"
        ));
        let refused =
            run_query(&fake, &exe(), &dev_login(), false, "SELECT 1 AS n").await.unwrap_err();

        assert_eq!(refused.0, 502);
        assert!(refused.1.starts_with("the database refused the statement: "), "{}", refused.1);
        assert!(refused.1.contains("Login failed"), "{}", refused.1);
        assert!(!refused.1.contains(password), "the password came back: {}", refused.1);
        assert!(refused.1.contains("(hidden)"), "{}", refused.1);
    }

    #[tokio::test]
    async fn a_capped_answer_says_so_on_its_first_line() {
        let _act = crate::serial::activity_log();
        let mut rows = String::from("id\n");
        for i in 0..250 {
            rows.push_str(&format!("{i}\n"));
        }
        let fake = FakeRunner::answering(&rows);
        let out = run_query(&fake, &exe(), &read_only(), false, "SELECT id FROM dbo.Leave")
            .await
            .expect("a read runs");
        let first = out.lines().next().unwrap();
        assert!(first.starts_with("rows: "), "{first}");
        assert!(first.contains("(capped)"), "{first}");

        let small = FakeRunner::answering("id\n1\n2\n");
        let out = run_query(&small, &exe(), &read_only(), false, "SELECT id FROM dbo.Leave")
            .await
            .expect("a read runs");
        assert!(!out.contains("capped"), "an uncapped answer says nothing about caps: {out}");
    }

    /// A realistic answer - header, the dashes rule, and the footer sqlcmd
    /// actually writes - counted exactly. Wide rows trip the CHARACTER cap
    /// rather than the row cap, so the count is not entangled with how many
    /// "rows" the row cap itself would have kept; it just has to prove the
    /// rule line, the blank separator and the footer are never mistaken
    /// for a row of data.
    #[tokio::test]
    async fn a_capped_answer_counts_only_its_own_data_rows() {
        let _act = crate::serial::activity_log();
        let mut stdout = String::from("id\tblob\n----\t----\n");
        for i in 0..150 {
            stdout.push_str(&format!("{i:03}\t{}\n", "x".repeat(600)));
        }
        stdout.push_str("\n(150 rows affected)\n");

        let fake = FakeRunner::answering(&stdout);
        let out = run_query(&fake, &exe(), &read_only(), false, "SELECT id, blob FROM dbo.Leave")
            .await
            .expect("a read runs");

        let first = out.lines().next().unwrap();
        // 99 whole rows fit under the 60,000-character cap before the cut
        // lands mid-row; the dashes rule is not counted as one of them, and
        // the blank line and footer never made it into the answer at all -
        // the cut lands well before either.
        assert_eq!(first, "rows: 99 (capped)", "{first}");
        assert!(!out.contains("rows affected"), "the footer must not count as a row: {out}");
    }

    /// A value that happens to contain the literal text "(capped)" must not
    /// fake a cap notice - `run_sql` now says whether it capped, rather
    /// than `with_cap_note` searching the answer for the word.
    #[tokio::test]
    async fn an_uncapped_answer_with_the_word_capped_in_a_value_gets_no_note() {
        let _act = crate::serial::activity_log();
        let stdout = "id\tnote\n----\t----\n1\tstatus is (capped) apparently\n\n(1 rows affected)\n";
        let fake = FakeRunner::answering(stdout);
        let out = run_query(&fake, &exe(), &read_only(), false, "SELECT id, note FROM dbo.Leave")
            .await
            .expect("a read runs");

        assert!(!out.starts_with("rows: "), "a data value faked a cap note: {out}");
        assert!(out.contains("(capped)"), "the real value must still be there: {out}");
    }

    // ---------------------------------------------------------- the lookup

    const TWO_TABLES: &str = "sch\ttab\trows_est\tcols\tfks\n\
----\t---\t--------\t----\t---\n\
dbo\tLeaveRequest\t1240\tLeaveRequestId int | LeaveTypeId int\tLeaveTypeId -> dbo.LeaveType(LeaveTypeId)\n\
\n\
(1 rows affected)\n";

    const RANKED: &str = "sch\ttab\tscore\n\
---\t---\t-----\n\
dbo\tLeaveRequest\t160\n\
\n\
(1 rows affected)\n";

    #[tokio::test]
    async fn a_lookup_ranks_then_reads_the_details_of_the_tables_it_picked() {
        let _act = crate::serial::activity_log();
        let fake = FakeRunner::answering_in_turn(&[RANKED, TWO_TABLES]);
        let out = run_lookup(&fake, &exe(), &read_only(), "leave request", 10)
            .await
            .expect("a lookup runs");

        assert!(out.contains("dbo.LeaveRequest (1240 rows est.)"), "{out}");
        assert!(out.contains("foreign key: LeaveTypeId -> dbo.LeaveType(LeaveTypeId)"), "{out}");
        let calls = fake.calls();
        assert_eq!(calls.len(), 2, "the ranking, then the details - never a trip per table");
        let rank = calls[0].last().unwrap().clone();
        let detail = calls[1].last().unwrap().clone();
        // PeoplesHR first: the HR databases also hold PeoplesHRDAP copies.
        assert!(rank.contains("TABLE_SCHEMA = N'PeoplesHR'"), "{rank}");
        assert!(detail.contains("(N'dbo', N'LeaveRequest', 160)"), "{detail}");
        // Whatever else they are, both statements are reads.
        assert_eq!(classify(&rank), Verdict::Read, "{rank}");
        assert_eq!(classify(&detail), Verdict::Read, "{detail}");
    }

    /// The lookup used to log its search words before any of its reads ran,
    /// so the record never said whether it worked or how long it took. It
    /// now records once, after the last read returns, with `ok`,
    /// `duration_ms`, and - since the ranking's own table count is known
    /// directly here - `rows` too.
    #[tokio::test]
    async fn a_lookup_records_ok_and_duration_after_it_runs() {
        let _act = crate::serial::activity_log();
        let dir = tempfile::tempdir().unwrap();
        v2_lib::activity_log::init(dir.path().to_path_buf());
        let fake = FakeRunner::answering_in_turn(&[RANKED, TWO_TABLES]);
        run_lookup(&fake, &exe(), &read_only(), "leave request", 10).await.expect("a lookup runs");

        let recs = crate::common::activity_records(dir.path(), "db");
        let rec = recs
            .iter()
            .find(|r| r["verdict"] == "lookup" && r["sql"] == "leave request")
            .expect("the lookup is in the activity log");
        assert_eq!(rec["ok"], true, "{rec}");
        assert!(rec["duration_ms"].is_u64(), "{rec}");
        // One table was ranked and read - a directly known count, not a
        // guess at what the rendered text contains.
        assert_eq!(rec["rows"], 1, "{rec}");

        let lines: Vec<_> = v2_lib::applog::recent(400).into_iter().map(|l| l.message).collect();
        assert!(
            lines.iter().any(|m| m.starts_with("db lookup on ") && m.contains("ok")),
            "no summary line in the app log: {lines:?}"
        );
        assert!(
            !lines.iter().any(|m| m.contains("leave request")),
            "the search words leaked into the app log: {lines:?}"
        );
    }

    /// A lookup that never reaches the server is recorded too, `ok: false`
    /// - the activity log is the trail of every attempt, not just the ones
    /// that worked.
    #[tokio::test]
    async fn a_failed_lookup_is_recorded_as_not_ok() {
        let _act = crate::serial::activity_log();
        let dir = tempfile::tempdir().unwrap();
        v2_lib::activity_log::init(dir.path().to_path_buf());
        let fake = FakeRunner::failing("Login failed (-P M5kjapL2H3bEIuZZ4YA4)");
        run_lookup(&fake, &exe(), &read_only(), "leave request", 10).await.unwrap_err();

        let recs = crate::common::activity_records(dir.path(), "db");
        let rec = recs
            .iter()
            .find(|r| r["verdict"] == "lookup" && r["sql"] == "leave request")
            .expect("the failed lookup is in the activity log");
        assert_eq!(rec["ok"], false, "{rec}");
        assert!(rec["duration_ms"].is_u64(), "{rec}");
        assert!(rec.get("rows").is_none(), "a failed lookup has no countable rows: {rec}");
    }

    #[tokio::test]
    async fn a_lookup_with_nothing_in_peopleshr_searches_every_schema() {
        let _act = crate::serial::activity_log();
        let fake = FakeRunner::answering_in_turn(&["", RANKED, TWO_TABLES]);
        let out = run_lookup(&fake, &exe(), &read_only(), "leave request", 10)
            .await
            .expect("a lookup runs");
        assert!(out.contains("dbo.LeaveRequest (1240 rows est.)"), "{out}");
        let calls = fake.calls();
        assert_eq!(calls.len(), 3, "PeoplesHR, every schema, the details");
        assert!(calls[0].last().unwrap().contains("TABLE_SCHEMA = N'PeoplesHR'"));
        assert!(!calls[1].last().unwrap().contains("TABLE_SCHEMA = N'"), "{:?}", calls[1]);
    }

    /// A bare name is a different question from a topic: "what is in this
    /// table", not "which tables are about this". The ranked lookup lists
    /// only the columns that matched the words, so a name gets the whole
    /// column list instead - inside the same tool, since an assistant
    /// should not have to know which shape it is asking for.
    #[tokio::test]
    async fn a_bare_table_name_comes_back_as_that_tables_columns() {
        let _act = crate::serial::activity_log();
        let columns = "sch\ttab\tcol\ttyp\tlen\tnul\n\
----\t---\t---\t---\t---\t---\n\
dbo\tLeaveRequest\tLeaveRequestId\tint\tNULL\tNO\n\
dbo\tLeaveRequest\tReason\tnvarchar\t200\tYES\n\
\n\
(2 rows affected)\n";
        let fake = FakeRunner::answering(columns);
        let out = run_lookup(&fake, &exe(), &read_only(), "dbo.LeaveRequest", 10)
            .await
            .expect("a describe runs");

        assert!(out.contains("LeaveRequestId int not null"), "{out}");
        assert!(out.contains("Reason nvarchar(200) null"), "{out}");
        assert_eq!(fake.calls().len(), 1, "one statement for a name that exists");
    }

    #[tokio::test]
    async fn a_name_that_is_not_a_table_falls_back_to_the_ranked_lookup() {
        let _act = crate::serial::activity_log();
        // Nothing comes back for the describe, so the words are treated as
        // a topic and the ranked lookup answers.
        let fake = FakeRunner::answering("");
        let out = run_lookup(&fake, &exe(), &read_only(), "Leave", 10).await.expect("it runs");
        assert_eq!(out, "no table or column matches those words", "{out}");
        // Nothing ranked, so there are no details to read.
        assert_eq!(
            fake.calls().len(),
            3,
            "the describe, the ranking in PeoplesHR, the ranking in every schema"
        );
    }

    #[tokio::test]
    async fn a_lookup_that_cannot_reach_the_server_says_so_without_the_password() {
        let _act = crate::serial::activity_log();
        let fake = FakeRunner::failing("Login failed (-P M5kjapL2H3bEIuZZ4YA4)");
        let refused =
            run_lookup(&fake, &exe(), &read_only(), "leave request", 10).await.unwrap_err();
        assert_eq!(refused.0, 502);
        assert!(!refused.1.contains("M5kjapL2H3bEIuZZ4YA4"), "{}", refused.1);
    }
}

#[tokio::test]
async fn a_wiki_name_that_climbs_out_is_refused_before_any_request() {
    let server = MockServer::start().await;
    let client = AdoClient::with_base_url("t".into(), server.uri());
    for wiki in ["..%2F..%2FotherOrg", "a%2Fb", "a%5Cb", ".."] {
        let (status, out) = route(
            &ctx(),
            Some(&client),
            "GET",
            &format!("/wiki-page?wiki={wiki}&path=%2FHome"),
            "",
            "1.0.0",
        )
        .await;
        assert_eq!(status, 400, "{wiki}: {out}");
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// A working repository with its `.test-cases` folder: the one place the
/// bridge may write a draft in place without the app following the file.
fn repo_ctx(dir: &TempDir) -> BridgeContext {
    std::fs::create_dir_all(dir.0.join(".test-cases")).unwrap();
    BridgeContext { working_dir: Some(dir.0.to_string_lossy().into_owned()), ..ctx() }
}

fn in_repo(dir: &TempDir, src: &std::path::Path) -> std::path::PathBuf {
    let to = dir.0.join(".test-cases").join(src.file_name().unwrap());
    std::fs::rename(src, &to).unwrap();
    to
}

/// "Reads only" wrote any path that parsed as a draft.
#[tokio::test]
async fn in_place_refuses_a_file_the_app_does_not_own() {
    let dir = TempDir::new();
    let ctx = repo_ctx(&dir);
    let path = draft_on_disk(&dir); // beside .test-cases, not in it
    let before = std::fs::read_to_string(&path).unwrap();

    let target = format!("/optimize?in_place=true&path={}", path.to_string_lossy().replace('\\', "%5C"));
    let (status, out) = route(&ctx, None, "POST", &target, "", "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert!(out.contains(".test-cases"), "{out}");

    let body = serde_json::json!({
        "path": path.to_string_lossy(), "in_place": true,
        "operations": [{ "op": "set_tags", "value": "smoke" }],
    })
    .to_string();
    let (status, out) = route(&ctx, None, "POST", "/transform", &body, "1.0.0").await;
    assert_eq!(status, 400, "{out}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before, "nothing was written");
}

#[test]
fn bridge_writes_only_under_test_cases_or_to_a_watched_file() {
    let dir = TempDir::new();
    std::fs::create_dir_all(dir.0.join(".test-cases")).unwrap();
    let inside = dir.0.join(".test-cases").join("a.json");
    let outside = dir.0.join("b.json");
    std::fs::write(&inside, "{}").unwrap();
    std::fs::write(&outside, "{}").unwrap();
    let root = dir.0.to_string_lossy().into_owned();
    let (inside, outside) = (inside.to_string_lossy().into_owned(), outside.to_string_lossy().into_owned());
    assert!(bridge_may_write(&inside, Some(&root), &[]));
    assert!(!bridge_may_write(&outside, Some(&root), &[]));
    assert!(!bridge_may_write(&inside, None, &[]), "no working repository, no folder rule");
    assert!(bridge_may_write(&outside, None, &[outside.clone()]), "a followed file may be written");
    let sneaky = dir.0.join(".test-cases").join("..").join("b.json");
    assert!(!bridge_may_write(&sneaky.to_string_lossy(), Some(&root), &[]), "`..` is resolved first");

    // A sibling that only shares the prefix is outside the folder.
    std::fs::create_dir_all(dir.0.join(".test-cases-extra")).unwrap();
    let sibling = dir.0.join(".test-cases-extra").join("c.json");
    std::fs::write(&sibling, "{}").unwrap();
    assert!(!bridge_may_write(&sibling.to_string_lossy(), Some(&root), &[]));
}

/// The proxy names its own build on every call, so the app can log a proxy
/// that is not its own build - the one place a person looks.
#[test]
fn parse_http_reads_the_proxys_version() {
    use v2_lib::ai_bridge::{parse_http, Parsed};
    let raw = b"GET /ping HTTP/1.1\r\nx-bridge-token: t\r\nX-TCM-Proxy-Version: 1.22.1\r\n\r\n";
    match parse_http(raw) {
        Parsed::Complete { proxy_version, .. } => assert_eq!(proxy_version.as_deref(), Some("1.22.1")),
        _ => panic!("expected a complete request"),
    }
    // An older proxy sends no such header, and that is not an error.
    match parse_http(b"GET /ping HTTP/1.1\r\nx-bridge-token: t\r\n\r\n") {
        Parsed::Complete { proxy_version, .. } => assert_eq!(proxy_version, None),
        _ => panic!("expected a complete request"),
    }
}

/// The four API template routes - design doc "API templates" §7 and §9.
/// Every one of them is gated with the Auto Run routes, prove and run are
/// refused while the person's switch is off, and guide and list answer
/// either way.
mod api_template_routes {
    use super::{ctx, route, BridgeContext};
    use serde_json::json;
    use v2_lib::ai_bridge::{autorun_guard_for, smells_like_a_write, API_WRITES_OFF};
    use v2_lib::autorun::accounts::save_accounts;
    use v2_lib::autorun::recipe::save_recipe;

    /// Every API template route. The flow ones start with `/api-template`
    /// too, so the path guard covers them without a line of their own.
    const PATHS: [&str; 9] = [
        "/api-template-guide",
        "/api-templates",
        "/api-template-prove",
        "/api-template-run",
        "/api-template-flow-save",
        "/api-template-flow-progress",
        "/api-template-fixtures",
        "/api-template-fixture-save",
        "/api-template-fixture-run",
    ];

    fn on() -> BridgeContext {
        BridgeContext { api_writes: true, ..ctx() }
    }

    /// A data root holding `ctx()`'s project's sign-in recipe and one
    /// account, set as the process-wide root the bridge reads.
    fn root_with_recipe_and_account() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx();
        save_recipe(dir.path(), &c.org, &c.project, &crate::common::recipe()).unwrap();
        save_accounts(dir.path(), &[crate::common::account()]).unwrap();
        v2_lib::autorun::store::set_root(dir.path().to_path_buf());
        dir
    }

    fn draft() -> serde_json::Value {
        json!({
            "id": "pms-create-draft-cycle",
            "title": "Create a draft performance cycle",
            "module": "PMS / Performance Cycle",
            "effect": "create",
            "description": "Cycle setup; leaves the cycle in Draft.",
            "sources": ["Pages/PerformanceCycle/Index.CycleSetup.cshtml.cs:95"],
            "antiforgery": { "page": "/hr/pmsv10/performancecycle?mode=create" },
            "params": [ { "name": "cycleName", "type": "string", "required": true } ],
            "steps": [
                { "name": "Cycle setup", "method": "POST",
                  "path": "/hr/pmsv10/performancecycle", "query": { "handler": "SaveProgress" },
                  "form": { "CycleName": "{{cycleName}}" },
                  "capture": { "cycleId": "$.cycleId" } }
            ],
            "outputs": ["cycleId"]
        })
    }

    #[test]
    fn api_template_routes_are_404_when_auto_run_is_not_offered() {
        for path in PATHS {
            let (status, body) = autorun_guard_for(path, false).unwrap_or_else(|| panic!("{path} was not refused"));
            assert_eq!(status, 404, "{path}");
            assert_eq!(body, "not available in this build", "{path}");
            assert!(autorun_guard_for(path, true).is_none(), "{path} refused where Auto Run is offered");
        }
    }

    #[test]
    fn no_api_template_route_smells_like_a_write() {
        for path in PATHS {
            assert!(!smells_like_a_write("POST", path), "{path}");
            assert!(!smells_like_a_write("GET", path), "{path}");
        }
    }

    /// Off by default, and the refusal comes before anything else is even
    /// looked at: a body that would otherwise fail every check gets the
    /// switch sentence alone, and the one-at-a-time slot is never taken.
    #[tokio::test]
    async fn prove_and_run_are_refused_while_the_switch_is_off() {
        let _slot = crate::serial::api_template_run();
        assert!(!BridgeContext::default().api_writes, "off by default");
        let prove = json!({ "template": draft(), "account": "admin", "values": { "cycleName": "FY27" } }).to_string();
        let run = json!({ "id": "pms-create-draft-cycle", "account": "admin", "values": {} }).to_string();
        for (path, body) in
            [("/api-template-prove", prove.as_str()), ("/api-template-run", run.as_str()), ("/api-template-prove", "not json")]
        {
            let (status, out) = route(&ctx(), None, "POST", path, body, "1.0.0").await;
            assert_eq!(status, 400, "{path}: {out}");
            assert_eq!(out, API_WRITES_OFF, "{path}");
        }
        assert!(API_WRITES_OFF.contains("AI Bridge"), "names where to turn it on");
        assert!(v2_lib::api_templates::runner::claim().is_some(), "nothing was launched, the slot is free");
    }

    #[tokio::test]
    async fn guide_and_list_answer_with_the_switch_off() {
        let _root = crate::serial::autorun();
        let _dir = root_with_recipe_and_account();

        let (status, guide) = route(&ctx(), None, "GET", "/api-template-guide", "", "1.0.0").await;
        assert_eq!(status, 200, "{guide}");
        assert!(guide.contains("prove_api_template"), "{guide}");
        assert!(guide.contains("admin"), "names the account key: {guide}");
        assert!(guide.contains("https://hr.example.internal"), "names the recipe's origin: {guide}");
        assert!(guide.contains("`proven: false`"), "says what an unproven template means: {guide}");
        assert!(!guide.contains(crate::common::PASSWORD), "never a password");

        let (status, list) = route(&ctx(), None, "GET", "/api-templates", "", "1.0.0").await;
        assert_eq!(status, 200, "{list}");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&list).unwrap(), json!({ "templates": [], "flows": [], "test_files": [], "paging": { "total": 0, "offset": 0, "returned": 0 } }));
    }

    /// What a template's `files` may name: the project's Test files, names
    /// and sizes only - in the list and at the end of both guides.
    #[tokio::test]
    async fn the_list_and_the_guides_name_the_projects_test_files() {
        let _root = crate::serial::autorun();
        let dir = root_with_recipe_and_account();
        let c = ctx();
        let files = v2_lib::test_files::folder(dir.path(), &c.org, &c.project);
        std::fs::create_dir_all(&files).unwrap();
        std::fs::write(files.join("appraisal.pdf"), b"%PDF-1.7 secret contents").unwrap();

        let (status, list) = route(&c, None, "GET", "/api-templates", "", "1.0.0").await;
        assert_eq!(status, 200, "{list}");
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(v["test_files"], json!([{ "name": "appraisal.pdf", "size": 24 }]), "{v}");

        let (_, guide) = route(&c, None, "GET", "/api-template-guide", "", "1.0.0").await;
        assert!(guide.contains("## Test files") && guide.contains("`appraisal.pdf` (24 bytes)"), "{guide}");
        let (_, autorun) = route(&c, None, "GET", "/autorun-guide", "", "1.0.0").await;
        assert!(autorun.contains("`appraisal.pdf` (24 bytes)"), "{autorun}");
        for text in [&list, &guide, &autorun] {
            assert!(!text.contains("secret contents") && !text.contains(&files.display().to_string()));
        }
    }

    /// The list is one row per saved template, with the newest run - or
    /// null when it has never run since it was proven.
    #[tokio::test]
    async fn the_list_carries_each_templates_shape_and_last_run() {
        use v2_lib::api_templates::store::{append_run, save, RunRecord};
        let _root = crate::serial::autorun();
        let dir = root_with_recipe_and_account();
        let c = ctx();
        let t: v2_lib::api_templates::ApiTemplate = serde_json::from_value(draft()).unwrap();
        save(dir.path(), &c.org, &c.project, &t).unwrap();

        let (status, list) = route(&c, None, "GET", "/api-templates", "", "1.0.0").await;
        assert_eq!(status, 200, "{list}");
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(v["templates"].as_array().unwrap().len(), 1, "{v}");
        let row = &v["templates"][0];
        assert_eq!(row["id"], "pms-create-draft-cycle");
        assert_eq!(row["title"], "Create a draft performance cycle");
        assert_eq!(row["module"], "PMS / Performance Cycle");
        assert_eq!(row["effect"], "create");
        assert_eq!(row["params"][0]["name"], "cycleName");
        assert_eq!(row["params"][0]["type"], "string");
        assert_eq!(row["outputs"], json!(["cycleId"]));
        assert_eq!(row["last_run"], serde_json::Value::Null);
        assert_eq!(row["stage"], serde_json::Value::Null, "a template on no flow: {row}");
        assert!(row.get("steps").is_none(), "the list is a summary: {row}");
        // Saved without proof - as an import saves one - it says so, and
        // what to do about it.
        assert_eq!(row["proven"], false, "{row}");
        assert_eq!(row["unproven"], v2_lib::api_templates::share::UNPROVEN_FOR_ASSISTANT, "{row}");
        assert!(row["unproven"].as_str().unwrap().contains("prove it here"), "{row}");
        let proven = v2_lib::api_templates::ApiTemplate {
            proven: Some(serde_json::from_value(json!({ "at": "2026-09-28 09:00:00", "origin": "https://hr.example.internal",
                                                          "account": "admin", "outputs": {} })).unwrap()),
            ..t.clone()
        };
        save(dir.path(), &c.org, &c.project, &proven).unwrap();
        let (_, list) = route(&c, None, "GET", "/api-templates", "", "1.0.0").await;
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(v["templates"][0]["proven"], true, "{v}");
        assert_eq!(v["templates"][0]["unproven"], serde_json::Value::Null, "{v}");

        let at = |s: &str, mode: &str| RunRecord {
            at: s.into(),
            mode: mode.into(),
            account: "admin".into(),
            ok: true,
            failed_step: None,
            detail: None,
            outputs: Default::default(),
        };
        // The prove that saved it is not a run.
        append_run(dir.path(), &c.org, &c.project, &t.id, at("2026-09-28 09:00:00", "prove")).unwrap();
        let (_, list) = route(&c, None, "GET", "/api-templates", "", "1.0.0").await;
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(v["templates"][0]["last_run"], serde_json::Value::Null, "a prove is not a run: {v}");

        append_run(dir.path(), &c.org, &c.project, &t.id, at("2026-09-28 10:00:00", "run")).unwrap();
        append_run(dir.path(), &c.org, &c.project, &t.id, at("2026-09-29 11:00:00", "run")).unwrap();
        append_run(dir.path(), &c.org, &c.project, &t.id, at("2026-09-29 12:00:00", "prove")).unwrap();
        let (_, list) = route(&c, None, "GET", "/api-templates", "", "1.0.0").await;
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(v["templates"][0]["last_run"]["at"], "2026-09-29 11:00:00", "the newest run: {v}");
        assert_eq!(v["templates"][0]["last_run"]["mode"], "run", "{v}");
    }

    /// Every problem at once - here three, one per line - and nothing
    /// launched.
    #[tokio::test]
    async fn a_prove_with_a_bad_draft_lists_every_problem() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _dir = root_with_recipe_and_account();
        let mut bad = draft();
        bad["steps"][0]["path"] = json!("https://elsewhere.example/hr");
        bad["steps"][0]["form"]["Extra"] = json!("{{nobody}}");
        bad["outputs"] = json!(["cycleId", "neverCaptured"]);
        let body = json!({ "template": bad, "account": "admin", "values": { "cycleName": "FY27" } }).to_string();

        let (status, out) = route(&on(), None, "POST", "/api-template-prove", &body, "1.0.0").await;
        assert_eq!(status, 400, "{out}");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3, "{out}");
        assert!(lines.iter().any(|l| l.contains("safe relative path")), "{out}");
        assert!(lines.iter().any(|l| l.contains("{{nobody}}")), "{out}");
        assert!(lines.iter().any(|l| l.contains("neverCaptured")), "{out}");
        assert!(v2_lib::api_templates::runner::claim().is_some(), "nothing was launched");
    }

    /// A draft may arrive as a JSON string - the shape every sibling tool
    /// on this server takes its payload in - and reads the same.
    #[tokio::test]
    async fn a_draft_sent_as_a_string_is_read_as_json() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _dir = root_with_recipe_and_account();
        let mut bad = draft();
        bad["outputs"] = json!(["neverCaptured"]);
        let body =
            json!({ "template": bad.to_string(), "account": "admin", "values": { "cycleName": "FY27" } }).to_string();
        let (status, out) = route(&on(), None, "POST", "/api-template-prove", &body, "1.0.0").await;
        assert_eq!(status, 400, "{out}");
        assert_eq!(out, "output 'neverCaptured' is never captured by any step");
    }

    /// Running a template nobody has proven says so by name.
    #[tokio::test]
    async fn running_an_unknown_template_says_so() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _dir = root_with_recipe_and_account();
        let body = json!({ "id": "never-proven", "account": "admin", "values": {} }).to_string();
        let (status, out) = route(&on(), None, "POST", "/api-template-run", &body, "1.0.0").await;
        assert_eq!(status, 400, "{out}");
        assert!(out.contains("never-proven"), "{out}");
        assert!(out.contains("list_api_templates"), "{out}");
    }

    /// Another run holding the slot is a 409 with the sentence, without
    /// touching the other run.
    #[tokio::test]
    async fn a_second_run_while_one_is_going_is_refused() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _dir = root_with_recipe_and_account();
        let held = v2_lib::api_templates::runner::claim().expect("the slot was free");
        let body = json!({ "template": draft(), "account": "admin", "values": { "cycleName": "FY27" } }).to_string();
        let (status, out) = route(&on(), None, "POST", "/api-template-prove", &body, "1.0.0").await;
        assert_eq!(status, 409, "{out}");
        assert_eq!(out, "another API template is running - wait for it to finish");
        drop(held);
    }

    /// The slot is taken BEFORE the template, its flow or the database is
    /// looked at, so an environment switch cannot land between those checks
    /// and the run: with the slot held, even a draft that would fail its
    /// checks gets the busy sentence - and the slot is free again after a
    /// refusal that took it.
    #[tokio::test]
    async fn the_slot_is_taken_before_the_template_is_checked() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let _dir = root_with_recipe_and_account();
        let mut bad = draft();
        bad["outputs"] = json!(["neverCaptured"]);
        let body = json!({ "template": bad, "account": "admin", "values": { "cycleName": "FY27" } }).to_string();

        let held = v2_lib::api_templates::runner::claim().expect("the slot was free");
        let (status, out) = route(&on(), None, "POST", "/api-template-prove", &body, "1.0.0").await;
        assert_eq!((status, out.as_str()), (409, "another API template is running - wait for it to finish"));
        drop(held);

        let (status, out) = route(&on(), None, "POST", "/api-template-prove", &body, "1.0.0").await;
        assert_eq!(status, 400, "{out}");
        assert!(v2_lib::api_templates::runner::claim().is_some(), "a refusal gives the slot back");
    }

    /// The context's `Debug` shows the switch like the database one.
    #[test]
    fn the_switch_shows_in_the_contexts_debug() {
        let shown = format!("{:?}", on());
        assert!(shown.contains("api_writes: true"), "{shown}");
    }

    // ------------------------------------------------------------ flows

    const FLOW: &str = "pms-performance-cycle";
    const STAGES: [&str; 5] = ["setup", "rules", "competencies", "participants", "publish"];

    fn cycle_flow() -> v2_lib::api_templates::flow::Flow {
        serde_json::from_value(crate::common::cycle_flow_json()).unwrap()
    }

    /// A database answering every stage's check with `answer`, except the
    /// ones named in `but`.
    fn db_answering(answer: bool, but: &[(&str, Result<bool, String>)]) -> crate::common::FakeStageDb {
        let db = but.iter().fold(crate::common::FakeStageDb::new(), |db, (id, r)| db.answer(&format!("/*{id}*/"), r.clone()));
        STAGES.iter().fold(db, |db, id| db.answer(&format!("/*{id}*/"), Ok(answer)))
    }

    fn handed(
        db: &crate::common::FakeStageDb,
    ) -> impl FnOnce(&BridgeContext) -> Result<crate::common::FakeStageDb, (u16, String)> {
        let db = db.clone();
        move |_| Ok(db)
    }

    fn untouched(_: &BridgeContext) -> Result<crate::common::FakeStageDb, (u16, String)> {
        panic!("the database was asked for by a call that should have been refused first")
    }

    /// Through `route`, so `real_stage_db` is the database: with none
    /// chosen, a template on a flow is refused with db_ready's own status
    /// and the flow's sentence, before any browser.
    #[tokio::test]
    async fn no_database_chosen_refuses_a_flow_template() {
        use v2_lib::ai_bridge::real_stage_db;
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let dir = root_with_recipe_and_account();
        let c = on();
        v2_lib::api_templates::flow_store::save(dir.path(), &c.org, &c.project, &cycle_flow()).unwrap();
        let t = crate::common::saved_on_stage("pms-add-participants", "Add the participants", "participants");
        v2_lib::api_templates::store::save(dir.path(), &c.org, &c.project, &t).unwrap();
        let sentence = "this template belongs to a flow, and flow checks need a database: choose one on the AI Bridge tab";

        assert_eq!(real_stage_db(&c).err(), Some((409, sentence.to_string())));
        let body = json!({ "id": "pms-add-participants", "account": "admin", "values": { "cycleId": 274 } }).to_string();
        let (status, out) = route(&c, None, "POST", "/api-template-run", &body, "1.0.0").await;
        assert_eq!((status, out.as_str()), (409, sentence));
        assert!(v2_lib::api_templates::runner::claim().is_some(), "nothing was launched");
    }

    /// Flow checks are assistant-written reads of the company database, so
    /// with Database Read Access switched off none of them runs: saving a
    /// flow, asking its progress and a flow template's run are all refused
    /// with the switch named, before any database or browser.
    #[tokio::test]
    async fn flow_checks_are_refused_while_database_reading_is_off() {
        let _root = crate::serial::autorun();
        let _slot = crate::serial::api_template_run();
        let dir = root_with_recipe_and_account();
        let c = BridgeContext { disabled_tools: vec!["db_query".into()], ..on() };
        v2_lib::api_templates::flow_store::save(dir.path(), &c.org, &c.project, &cycle_flow()).unwrap();
        let t = crate::common::saved_on_stage("pms-add-participants", "Add the participants", "participants");
        v2_lib::api_templates::store::save(dir.path(), &c.org, &c.project, &t).unwrap();
        let sentence = "flow checks read the company database: switch on Database Read Access on the AI Bridge tab";

        assert_eq!(v2_lib::ai_bridge::real_stage_db(&c).err(), Some((409, sentence.to_string())));

        let progress = json!({ "flow": FLOW, "subject": 274 }).to_string();
        let (status, out) = route(&c, None, "POST", "/api-template-flow-progress", &progress, "1.0.0").await;
        assert_eq!((status, out.as_str()), (409, sentence));

        let save =
            json!({ "flow": crate::common::cycle_flow_json(), "sample": 274, "replace": true, "why": "again" }).to_string();
        let (status, out) = route(&c, None, "POST", "/api-template-flow-save", &save, "1.0.0").await;
        assert_eq!((status, out.as_str()), (409, sentence));

        let run = json!({ "id": "pms-add-participants", "account": "admin", "values": { "cycleId": 274 } }).to_string();
        let (status, out) = route(&c, None, "POST", "/api-template-run", &run, "1.0.0").await;
        assert_eq!((status, out.as_str()), (409, sentence));
        assert!(v2_lib::api_templates::runner::claim().is_some(), "nothing was launched");
    }

    #[tokio::test]
    async fn saving_a_flow_runs_every_check_on_the_sample() {
        use v2_lib::ai_bridge::api_template_flow_save;
        let _root = crate::serial::autorun();
        let _act = crate::serial::activity_log();
        let dir = root_with_recipe_and_account();
        let c = ctx();
        let db = db_answering(true, &[("competencies", Ok(false))]);

        let body = json!({ "flow": crate::common::cycle_flow_json(), "sample": 274 }).to_string();
        let (status, out) = api_template_flow_save(&c, &body, handed(&db)).await;
        assert_eq!(status, 200, "{out}");
        assert_eq!(db.calls().len(), 5, "one check per stage: {:?}", db.calls());
        assert!(db.calls().iter().all(|sql| sql.contains("274")), "{:?}", db.calls());

        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["saved"], FLOW, "{v}");
        let stages = v["stages"].as_array().unwrap();
        let listed: Vec<(&str, bool)> =
            stages.iter().map(|s| (s["id"].as_str().unwrap(), s["done"].as_bool().unwrap())).collect();
        assert_eq!(
            listed,
            [("setup", true), ("rules", true), ("competencies", false), ("participants", true), ("publish", true)]
        );
        assert_eq!(v["orphaned"], json!([]), "{v}");
        assert_eq!(v["not_on_a_flow"], json!([]), "nothing is left off a flow: {v}");

        let saved = v2_lib::api_templates::flow_store::load(dir.path(), &c.org, &c.project, FLOW).unwrap().expect("saved");
        let evidence = saved.saved.clone().expect("the app's saved block");
        assert_eq!(evidence.sample, json!(274));
        assert!(!evidence.at.is_empty());
        assert_eq!(v2_lib::api_templates::flow::Flow { saved: None, ..saved }, cycle_flow(), "saved as sent");
    }

    /// A flow is meant to place every template discovered for its record:
    /// the save answer lists the saved templates no flow places yet, so the
    /// assistant has the rest of the map in front of it.
    #[tokio::test]
    async fn saving_a_flow_lists_the_templates_on_no_flow_yet() {
        use v2_lib::ai_bridge::api_template_flow_save;
        let _root = crate::serial::autorun();
        let _act = crate::serial::activity_log();
        let dir = root_with_recipe_and_account();
        let c = ctx();
        let placed = crate::common::saved_on_stage("pms-set-eval-rules", "Set the evaluation rules", "rules");
        let loose = v2_lib::api_templates::ApiTemplate {
            stage: None,
            ..crate::common::saved_on_stage("pms-add-competency", "Add a competency", "rules")
        };
        for t in [&placed, &loose] {
            v2_lib::api_templates::store::save(dir.path(), &c.org, &c.project, t).unwrap();
        }
        let db = db_answering(true, &[]);

        let body = json!({ "flow": crate::common::cycle_flow_json(), "sample": 274 }).to_string();
        let (status, out) = api_template_flow_save(&c, &body, handed(&db)).await;
        assert_eq!(status, 200, "{out}");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["not_on_a_flow"],
            json!([{ "id": "pms-add-competency", "title": "Add a competency", "module": "PMS / Performance Cycle" }]),
            "only the template no flow places: {v}"
        );
        let message = v["message"].as_str().unwrap_or("");
        assert!(message.contains("pms-add-competency"), "{message}");
        assert!(message.contains("on no flow yet"), "{message}");
        assert!(!message.contains("pms-set-eval-rules"), "{message}");
    }

    #[tokio::test]
    async fn saving_a_flow_with_a_failing_check_is_refused() {
        use v2_lib::ai_bridge::api_template_flow_save;
        let _root = crate::serial::autorun();
        let _act = crate::serial::activity_log();
        let dir = root_with_recipe_and_account();
        let c = ctx();
        let db = db_answering(true, &[("rules", Err("Login timeout expired on SQLPROD01".to_string()))]);

        let body = json!({ "flow": crate::common::cycle_flow_json(), "sample": 274 }).to_string();
        let (status, out) = api_template_flow_save(&c, &body, handed(&db)).await;
        assert_eq!(status, 400, "{out}");
        assert!(out.contains("Evaluation rules"), "names the stage: {out}");
        assert!(!out.contains("SQLPROD01"), "the database error goes to the activity log only: {out}");
        assert_eq!(db.calls().len(), 5, "every check still ran: {:?}", db.calls());
        assert_eq!(v2_lib::api_templates::flow_store::load(dir.path(), &c.org, &c.project, FLOW).unwrap(), None);
    }

    /// A flow that fails its own checks, a sample of the wrong type or no
    /// sample: every problem at once, and no database is asked for.
    #[tokio::test]
    async fn a_bad_flow_or_sample_is_refused_before_the_database() {
        use v2_lib::ai_bridge::api_template_flow_save;
        let _root = crate::serial::autorun();
        let _dir = root_with_recipe_and_account();
        let c = ctx();

        let body = json!({ "flow": crate::common::cycle_flow_json(), "sample": "274" }).to_string();
        let (status, out) = api_template_flow_save(&c, &body, untouched).await;
        assert_eq!((status, out.as_str()), (400, "cycleId is a number subject: give a whole number, 0 or more"));

        let mut bad = crate::common::cycle_flow_json();
        bad["stages"][1]["requires"] = json!(["nowhere"]);
        let body = json!({ "flow": bad }).to_string();
        let (status, out) = api_template_flow_save(&c, &body, untouched).await;
        assert_eq!(status, 400, "{out}");
        assert_eq!(out.lines().count(), 2, "{out}");
        assert!(out.contains("'nowhere'"), "{out}");
        assert!(out.contains("\"sample\""), "{out}");

        let (status, out) = api_template_flow_save(&c, "not json", untouched).await;
        assert_eq!(status, 400, "{out}");
    }

    #[tokio::test]
    async fn replacing_a_flow_needs_replace_and_why_and_lists_orphans() {
        use v2_lib::ai_bridge::api_template_flow_save;
        let _log = crate::serial::log_tail();
        let _root = crate::serial::autorun();
        let _act = crate::serial::activity_log();
        let dir = root_with_recipe_and_account();
        let c = ctx();
        let t = crate::common::saved_on_stage("pms-set-eval-rules", "Set the evaluation rules", "rules");
        v2_lib::api_templates::store::save(dir.path(), &c.org, &c.project, &t).unwrap();
        let db = db_answering(true, &[]);

        let body = json!({ "flow": crate::common::cycle_flow_json(), "sample": 274 }).to_string();
        let (status, out) = api_template_flow_save(&c, &body, handed(&db)).await;
        assert_eq!(status, 200, "{out}");

        // Evaluation rules dropped: what required it now requires setup.
        let mut next = crate::common::cycle_flow_json();
        let stages = next["stages"].as_array_mut().unwrap();
        stages.remove(1);
        stages[1]["requires"] = json!(["setup"]);
        stages[2]["requires"] = json!(["setup"]);
        let refused = "a flow called \"pms-performance-cycle\" already exists - send replace: true and a why to change it";
        for extra in [json!({}), json!({ "replace": true }), json!({ "why": "rules moved" }), json!({ "replace": true, "why": "  " })] {
            let mut b = json!({ "flow": next, "sample": 274 });
            for (k, v) in extra.as_object().unwrap() {
                b[k] = v.clone();
            }
            let (status, out) = api_template_flow_save(&c, &b.to_string(), untouched).await;
            assert_eq!((status, out.as_str()), (400, refused), "{extra}");
        }

        let b = json!({ "flow": next, "sample": 274, "replace": true, "why": "evaluation rules moved into setup" });
        let (status, out) = api_template_flow_save(&c, &b.to_string(), handed(&db)).await;
        assert_eq!(status, 200, "{out}");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["orphaned"][0]["id"], "pms-set-eval-rules", "{v}");
        assert_eq!(v["orphaned"][0]["stage"], "rules", "{v}");
        assert!(v["message"].as_str().unwrap_or("").contains("pms-set-eval-rules"), "{v}");
        let saved = v2_lib::api_templates::flow_store::load(dir.path(), &c.org, &c.project, FLOW).unwrap().unwrap();
        assert!(saved.stages.iter().all(|s| s.id != "rules"), "the replacement was saved");
        let log = v2_lib::applog::recent(400);
        assert!(
            log.iter().any(|l| l.message == format!("api template flow {FLOW} replaced: evaluation rules moved into setup")),
            "the reason was not logged: {:?}",
            log.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn progress_answers_each_stage() {
        use v2_lib::ai_bridge::api_template_flow_progress;
        let _root = crate::serial::autorun();
        let _act = crate::serial::activity_log();
        let dir = root_with_recipe_and_account();
        let c = ctx();
        v2_lib::api_templates::flow_store::save(dir.path(), &c.org, &c.project, &cycle_flow()).unwrap();
        let t = crate::common::saved_on_stage("pms-add-participants", "Add the participants", "participants");
        v2_lib::api_templates::store::save(dir.path(), &c.org, &c.project, &t).unwrap();
        let db = db_answering(false, &[("setup", Ok(true)), ("rules", Ok(true))]);

        let body = json!({ "flow": FLOW, "subject": 274 }).to_string();
        let (status, out) = api_template_flow_progress(&c, &body, handed(&db)).await;
        assert_eq!(status, 200, "{out}");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["flow"], FLOW);
        assert_eq!(v["subject"], 274);
        let states: Vec<(&str, &str)> = v["stages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["id"].as_str().unwrap(), s["state"].as_str().unwrap()))
            .collect();
        assert_eq!(
            states,
            [("setup", "done"), ("rules", "done"), ("competencies", "skippable"), ("participants", "next"), ("publish", "blocked")]
        );
        assert_eq!(v["stages"][3]["templates"], json!(["pms-add-participants"]));

        // A subject of the wrong type, and a flow that is not saved: said
        // before any database is asked for.
        let body = json!({ "flow": FLOW, "subject": "274" }).to_string();
        let (status, out) = api_template_flow_progress(&c, &body, untouched).await;
        assert_eq!((status, out.as_str()), (400, "cycleId is a number subject: give a whole number, 0 or more"));
        let body = json!({ "flow": "never-saved", "subject": 274 }).to_string();
        let (status, out) = api_template_flow_progress(&c, &body, untouched).await;
        assert_eq!(status, 400, "{out}");
        assert!(out.contains("never-saved"), "{out}");
    }

    /// The list carries each template's stage and every flow with the
    /// templates on each stage - and a flow file that no longer parses
    /// hides nothing else.
    #[tokio::test]
    async fn the_list_carries_flows_and_stages() {
        let _root = crate::serial::autorun();
        let dir = root_with_recipe_and_account();
        let c = ctx();
        v2_lib::api_templates::flow_store::save(dir.path(), &c.org, &c.project, &cycle_flow()).unwrap();
        let t = crate::common::saved_on_stage("pms-set-eval-rules", "Set the evaluation rules", "rules");
        v2_lib::api_templates::store::save(dir.path(), &c.org, &c.project, &t).unwrap();
        let flows = v2_lib::api_templates::flow_store::flows_dir(dir.path(), &c.org, &c.project);
        std::fs::write(flows.join("broken.json"), "{ not a flow").unwrap();

        let (status, list) = route(&c, None, "GET", "/api-templates", "", "1.0.0").await;
        assert_eq!(status, 200, "{list}");
        let v: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(v["templates"][0]["stage"], json!({ "flow": FLOW, "id": "rules" }), "{v}");
        assert_eq!(v["flows"].as_array().unwrap().len(), 1, "{v}");
        let f = &v["flows"][0];
        assert_eq!(f["id"], FLOW);
        assert_eq!(f["title"], "Performance cycle wizard");
        assert_eq!(f["module"], "PMS / Performance Cycle");
        assert_eq!(f["subject"], json!({ "name": "cycleId", "type": "number" }));
        assert_eq!(f["stages"][1]["id"], "rules");
        assert_eq!(f["stages"][1]["requires"], json!(["setup"]));
        assert_eq!(f["stages"][1]["templates"], json!(["pms-set-eval-rules"]));
        assert_eq!(f["stages"][0]["creates"], true);
        assert_eq!(f["stages"][2]["optional"], true);
        assert_eq!(f["stages"][0]["templates"], json!([]));
        assert!(f["stages"][0].get("check").is_none(), "the list is a summary: {f}");
    }

    // ------------------------------------------- the list, filtered and paged

    /// A project of 300 templates, each as heavy as a real one: half in
    /// Leave, half in PMS, every one with params and outputs. Template
    /// 137 is the only one with a param called `uniqueParam137`. Plus the
    /// cycle flow with one template on its rules stage.
    fn root_with_300_templates() -> tempfile::TempDir {
        let dir = root_with_recipe_and_account();
        let c = ctx();
        for n in 0..300 {
            let mut v = draft();
            v["id"] = json!(format!("tpl-{n:03}"));
            v["title"] = json!(format!("Template number {n} that sets up a record for a long test"));
            v["module"] = json!(if n % 2 == 0 { "Leave / Apply Leave" } else { "PMS / Performance Cycle" });
            v["description"] = json!("A long description of what this template does, ".repeat(6));
            v["params"] = json!([
                { "name": if n == 137 { "uniqueParam137".to_string() } else { "cycleName".to_string() },
                  "type": "string", "required": true, "description": "The name the record is given in the list." },
                { "name": "startDate", "type": "string", "description": "The first day, as the screen shows it." },
            ]);
            v["outputs"] = json!(["cycleId", "cycleCode", "cycleStatus"]);
            let t: v2_lib::api_templates::ApiTemplate = serde_json::from_value(v).unwrap();
            v2_lib::api_templates::store::save(dir.path(), &c.org, &c.project, &t).unwrap();
        }
        v2_lib::api_templates::flow_store::save(dir.path(), &c.org, &c.project, &cycle_flow()).unwrap();
        let t = crate::common::saved_on_stage("pms-set-eval-rules", "Set the evaluation rules", "rules");
        v2_lib::api_templates::store::save(dir.path(), &c.org, &c.project, &t).unwrap();
        dir
    }

    async fn list(target: &str) -> (u16, serde_json::Value, String) {
        let (status, out) = route(&ctx(), None, "GET", target, "", "1.0.0").await;
        let v = serde_json::from_str(&out).unwrap_or(serde_json::Value::Null);
        (status, v, out)
    }

    /// Review Focus 4: no arguments on a project of 300 templates is a
    /// compact index of the first page, well under 20K characters, that
    /// says how to page and how to get the detail.
    #[tokio::test]
    async fn no_arguments_on_300_templates_is_a_compact_index_under_20k() {
        let _root = crate::serial::autorun();
        let _dir = root_with_300_templates();
        let (status, v, out) = list("/api-templates").await;
        assert_eq!(status, 200, "{out}");
        assert!(out.len() < 20_000, "{} characters", out.len());
        let rows = v["templates"].as_array().unwrap();
        assert_eq!(rows.len(), 25, "the default page");
        let row = &rows[0];
        for key in ["id", "title", "module", "effect", "proven", "stage"] {
            assert!(row.get(key).is_some(), "the index row carries {key}: {row}");
        }
        for key in ["params", "outputs", "last_run"] {
            assert!(row.get(key).is_none(), "the index row leaves out {key}: {row}");
        }
        assert!(v.get("test_files").is_none(), "{v}");
        assert_eq!(v["paging"], json!({ "total": 301, "offset": 0, "returned": 25, "next_offset": 25 }));
        let f = &v["flows"][0];
        assert_eq!((f["id"].as_str(), f["title"].as_str()), (Some(FLOW), Some("Performance cycle wizard")));
        assert_eq!(f["stage_count"], 5, "{f}");
        assert!(f.get("stages").is_none(), "{f}");
        // The fixture's templates are unproven, so the index says once - not
        // per row - what that means for relying on them.
        assert!(rows.iter().all(|r| r["proven"] == false) && rows.iter().all(|r| r.get("unproven").is_none()));
        assert_eq!(
            v["note"],
            format!(
                "This is the index of 301 templates. Narrow it with module, search or flow until at most 25 match, or pass id, for params, outputs, the newest run and the test files. Page with offset and limit (at most 100). A template whose proven is false was {}.",
                v2_lib::api_templates::share::UNPROVEN_FOR_ASSISTANT
            )
        );
    }

    /// The index of templates that are all proven says nothing about
    /// proving.
    #[tokio::test]
    async fn an_index_of_proven_templates_has_no_unproven_line() {
        let _root = crate::serial::autorun();
        let dir = root_with_recipe_and_account();
        let c = ctx();
        for n in 0..30 {
            let t = crate::common::saved_on_stage(&format!("proven-{n:02}"), &format!("Proven {n}"), "rules");
            v2_lib::api_templates::store::save(dir.path(), &c.org, &c.project, &t).unwrap();
        }
        let (_, v, out) = list("/api-templates").await;
        assert!(v["templates"].as_array().unwrap().iter().all(|r| r["proven"] == true), "{out}");
        assert!(!v["note"].as_str().unwrap().contains("proven"), "{out}");
    }

    /// A filtered answer in full lists the flows its templates are on, not
    /// every flow with every stage - the bulk the filters are there to cut.
    #[tokio::test]
    async fn a_filtered_answer_lists_only_the_flows_its_templates_are_on() {
        let _root = crate::serial::autorun();
        let dir = root_with_300_templates();
        let c = ctx();
        let mut other = crate::common::cycle_flow_json();
        other["id"] = json!("leave-request");
        other["title"] = json!("Leave request");
        let other: v2_lib::api_templates::flow::Flow = serde_json::from_value(other).unwrap();
        v2_lib::api_templates::flow_store::save(dir.path(), &c.org, &c.project, &other).unwrap();

        let (_, v, out) = list("/api-templates?search=eval-rules").await;
        assert_eq!(v["templates"][0]["id"], "pms-set-eval-rules", "{out}");
        let ids: Vec<&str> = v["flows"].as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec![FLOW], "{out}");
        assert_eq!(v["flows"][0]["stages"][1]["templates"], json!(["pms-set-eval-rules"]), "in full: {out}");

        // On no flow at all: no flows.
        let (_, v, out) = list("/api-templates?search=UNIQUEPARAM137").await;
        assert_eq!(v["flows"], json!([]), "{out}");
        let (_, v, out) = list("/api-templates?module=pms&search=tpl-13").await;
        assert_eq!(v["paging"]["total"], 5, "{out}");
        assert_eq!(v["flows"], json!([]), "{out}");

        // With no filter, every flow is still there.
        let (_, v, out) = list("/api-templates?limit=100&offset=250").await;
        assert_eq!(v["flows"].as_array().unwrap().len(), 2, "{out}");
    }

    #[tokio::test]
    async fn the_list_pages_with_offset_and_limit() {
        let _root = crate::serial::autorun();
        let _dir = root_with_300_templates();
        let (_, v, out) = list("/api-templates?offset=290&limit=25").await;
        assert_eq!(v["paging"], json!({ "total": 301, "offset": 290, "returned": 11 }), "{out}");
        assert_eq!(v["templates"].as_array().unwrap().len(), 11);
        // A limit over 100 is 100.
        let (_, hundred, _) = list("/api-templates?limit=500").await;
        assert_eq!(hundred["paging"], json!({ "total": 301, "offset": 0, "returned": 100, "next_offset": 100 }));
        let (_, v, _) = list("/api-templates?offset=25").await;
        assert_eq!(v["templates"][0]["id"], hundred["templates"][25]["id"], "the second page starts where the first ended");
        for bad in ["offset=-1", "offset=abc", "limit=0", "limit=two"] {
            let (status, _, out) = list(&format!("/api-templates?{bad}")).await;
            assert_eq!(status, 400, "{bad}: {out}");
            assert!(out.contains("a whole number"), "{bad}: {out}");
        }
    }

    /// `module` is a case-insensitive substring; `search` matches the id,
    /// the title, or a param or output name. A filtered result that fits
    /// in one page carries the full detail.
    #[tokio::test]
    async fn module_and_search_filter_and_a_small_result_is_in_full() {
        let _root = crate::serial::autorun();
        let _dir = root_with_300_templates();
        let (_, v, out) = list("/api-templates?module=leave%20%2F%20APPLY").await;
        assert_eq!(v["paging"]["total"], 150, "{out}");
        assert!(v["templates"].as_array().unwrap().iter().all(|r| r["module"] == "Leave / Apply Leave"));
        assert!(v["templates"][0].get("params").is_none(), "150 is the index: {out}");

        let (status, v, out) = list("/api-templates?search=UNIQUEPARAM137").await;
        assert_eq!(status, 200, "{out}");
        assert_eq!(v["paging"], json!({ "total": 1, "offset": 0, "returned": 1 }));
        let row = &v["templates"][0];
        assert_eq!(row["id"], "tpl-137");
        assert_eq!(row["params"][0]["name"], "uniqueParam137", "in full: {row}");
        assert_eq!(row["outputs"], json!(["cycleId", "cycleCode", "cycleStatus"]));
        assert!(row.get("last_run").is_some(), "{row}");
        assert!(v["test_files"].is_array(), "{v}");
        assert!(v.get("note").is_none(), "{v}");

        let (_, v, _) = list("/api-templates?search=cyclestatus&module=pms&limit=100").await;
        assert_eq!(v["paging"]["total"], 150, "an output name matches");
        let (_, v, _) = list("/api-templates?search=tpl-01").await;
        assert_eq!(v["paging"]["total"], 10, "an id matches: {v}");
        let (_, v, _) = list("/api-templates?search=number%20299%20that").await;
        assert_eq!(v["templates"][0]["id"], "tpl-299", "a title matches: {v}");
    }

    /// `flow` keeps that flow's templates and that flow alone, in full.
    #[tokio::test]
    async fn flow_restricts_templates_and_flows() {
        let _root = crate::serial::autorun();
        let dir = root_with_300_templates();
        let c = ctx();
        let mut other = crate::common::cycle_flow_json();
        other["id"] = json!("leave-request");
        other["title"] = json!("Leave request");
        let other: v2_lib::api_templates::flow::Flow = serde_json::from_value(other).unwrap();
        v2_lib::api_templates::flow_store::save(dir.path(), &c.org, &c.project, &other).unwrap();

        let (status, v, out) = list(&format!("/api-templates?flow={FLOW}")).await;
        assert_eq!(status, 200, "{out}");
        assert_eq!(v["paging"], json!({ "total": 1, "offset": 0, "returned": 1 }));
        assert_eq!(v["templates"][0]["id"], "pms-set-eval-rules");
        assert_eq!(v["flows"].as_array().unwrap().len(), 1, "{v}");
        assert_eq!(v["flows"][0]["stages"][1]["templates"], json!(["pms-set-eval-rules"]), "in full: {v}");

        let (status, _, out) = list("/api-templates?flow=never-saved").await;
        assert_eq!(status, 404, "{out}");
        assert_eq!(out, "no flow called \"never-saved\" is saved for this project - list_api_templates shows the ones that are");
    }

    /// `id` returns that one template in full, with the flow it is on.
    #[tokio::test]
    async fn id_returns_one_template_in_full() {
        let _root = crate::serial::autorun();
        let _dir = root_with_300_templates();
        let (status, v, out) = list("/api-templates?id=tpl-042").await;
        assert_eq!(status, 200, "{out}");
        assert_eq!(v["paging"], json!({ "total": 1, "offset": 0, "returned": 1 }));
        assert_eq!(v["templates"][0]["id"], "tpl-042");
        assert_eq!(v["templates"][0]["params"][1]["name"], "startDate");
        assert!(v["test_files"].is_array(), "{v}");

        let (_, v, _) = list("/api-templates?id=pms-set-eval-rules").await;
        assert_eq!(v["flows"].as_array().unwrap().len(), 1, "the flow it is on: {v}");
        assert_eq!(v["flows"][0]["id"], FLOW);

        let (status, _, out) = list("/api-templates?id=tpl-999").await;
        assert_eq!(status, 404, "{out}");
        assert_eq!(out, "no template called \"tpl-999\" is saved for this project - list_api_templates shows the ones that are");

        // With `id`, the filters and the paging are ignored.
        let (status, v, out) = list("/api-templates?id=tpl-042&module=nothing-like-it&search=zzz&offset=5&limit=abc").await;
        assert_eq!(status, 200, "{out}");
        assert_eq!(v["templates"][0]["id"], "tpl-042", "{out}");
        assert_eq!(v["paging"], json!({ "total": 1, "offset": 0, "returned": 1 }));
    }

    /// The path `list_api_templates` asks for, through the real MCP dispatch.
    fn tool_target(arguments: serde_json::Value) -> String {
        let asked = std::cell::RefCell::new(None);
        let call = |method: &str, path: &str, _body: &str| {
            if method == "GET" && path.starts_with("/api-templates") {
                *asked.borrow_mut() = Some(path.to_string());
            }
            Ok((200, r#"{"autorun": true, "disabled": []}"#.to_string()))
        };
        let req = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "list_api_templates", "arguments": arguments },
        });
        v2_lib::mcp::handle_message(&req.to_string(), "1.0.0", &call).unwrap();
        asked.into_inner().expect("the tool asked for no list")
    }

    /// An `offset` or `limit` that is not a whole number - a boolean, a
    /// list, a negative or fractional number - is refused by name, never
    /// ignored.
    #[tokio::test]
    async fn paging_that_is_not_a_whole_number_is_refused_by_name() {
        let _root = crate::serial::autorun();
        let _dir = root_with_recipe_and_account();
        let cases = [
            (json!({ "offset": true }), "\"offset\" is a whole number, 0 or more"),
            (json!({ "offset": [3] }), "\"offset\" is a whole number, 0 or more"),
            (json!({ "offset": -1 }), "\"offset\" is a whole number, 0 or more"),
            (json!({ "offset": 2.5 }), "\"offset\" is a whole number, 0 or more"),
            (json!({ "limit": false }), "\"limit\" is a whole number, 1 or more"),
            (json!({ "limit": { "n": 5 } }), "\"limit\" is a whole number, 1 or more"),
            (json!({ "limit": 0 }), "\"limit\" is a whole number, 1 or more"),
            (json!({ "limit": 7.5 }), "\"limit\" is a whole number, 1 or more"),
        ];
        for (arguments, want) in cases {
            let target = tool_target(arguments.clone());
            let (status, out) = route(&ctx(), None, "GET", &target, "", "1.0.0").await;
            assert_eq!((status, out.as_str()), (400, want), "{arguments} asked {target}");
        }
        let target = tool_target(json!({ "offset": 0, "limit": "10" }));
        let (status, out) = route(&ctx(), None, "GET", &target, "", "1.0.0").await;
        assert_eq!(status, 200, "{target}: {out}");
    }
}


// ============================================================ writing style
//
// The AI Bridge tab's Writing style card: switched on, the person's own
// Markdown replaces the guide's granularity and edge-case sections; off or
// missing, the guide is exactly the standard one. The guide reads the file
// on every call, so a save takes effect at once.

use v2_lib::writing_style::{self as style, WritingStyle};

/// Points the writing style at a temp dir for one test, and back at nothing
/// when it ends - pass or fail - so no other test reads its style.
struct StyleDir(tempfile::TempDir);

impl StyleDir {
    fn new() -> Self {
        let d = tempfile::tempdir().unwrap();
        style::set_dir(Some(d.path().to_path_buf()));
        StyleDir(d)
    }
    fn save(&self, enabled: bool, text: &str) {
        style::save(self.0.path(), &WritingStyle { enabled, text: text.into() }).unwrap();
    }
}

impl Drop for StyleDir {
    fn drop(&mut self) {
        style::set_dir(None);
    }
}

async fn guide_now() -> String {
    let (server, client) = ado_stub().await;
    Mock::given(wm_method("GET"))
        .and(wm_path("/acme/Web/_apis/wit/workitemtypes/Test%20Case/fields/Custom.Module"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "allowedValues": ["Login"] })))
        .mount(&server)
        .await;
    let (status, g) = route(&ctx(), Some(&client), "GET", "/guide", "", "1.23.2").await;
    assert_eq!(status, 200, "{g}");
    g
}

const CONFLICT_LINE: &str =
    "If anything here conflicts with the Format section or the import rules, the Format section and the import rules win.";

const STEP_1_5: &str = "1.5. If the writing style above adds steps (for example a scenario list to approve before drafting, or a summary or review at the end), follow them at the point it says.";

#[tokio::test]
async fn with_no_style_or_a_disabled_one_the_guide_is_exactly_the_standard_one() {
    let _lock = crate::serial::writing_style();
    style::set_dir(None);
    let standard = guide_now().await;
    assert!(standard.contains("## Granularity - quality over quantity"), "{standard}");
    assert!(standard.contains("## Edge cases worth writing"), "{standard}");
    assert!(!standard.contains("(set on this machine)"), "{standard}");
    assert!(!standard.contains("1.5."), "{standard}");

    // A dir with no file yet: still the standard guide.
    let dir = StyleDir::new();
    assert_eq!(guide_now().await, standard, "a missing file");

    // The starting style is off: still the standard guide.
    style::get_or_create(dir.0.path());
    assert_eq!(guide_now().await, standard, "the starting style");

    // Text saved but switched off: still the standard guide.
    dir.save(false, "## Mine\nWrite one case per screen.");
    assert_eq!(guide_now().await, standard, "a disabled style");
}

#[tokio::test]
async fn an_enabled_style_replaces_the_granularity_and_edge_case_sections() {
    let _lock = crate::serial::writing_style();
    let dir = StyleDir::new();
    dir.save(true, "## House rules\nWrite one case per screen, and tag each with its screen.\n");
    let g = guide_now().await;
    let flat = g.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(g.contains("## Writing style (set on this machine)\n## House rules\nWrite one case per screen"), "{g}");
    assert!(flat.contains(CONFLICT_LINE), "{g}");
    assert!(!g.contains("## Granularity - quality over quantity"), "{g}");
    assert!(!g.contains("## Edge cases worth writing"), "{g}");
    for kept in [
        "## Format",
        "## One branch per case",
        "## Allowed Module values (live)",
        "## Workflow",
        "## Writing style - sound like a tester, not a model",
    ] {
        assert!(g.contains(kept), "lost {kept:?}");
    }
    // The style sits where the granularity section was: after Format, before
    // the standard case-text style rules.
    let format = g.find("## Format").unwrap();
    let custom = g.find("## Writing style (set on this machine)").unwrap();
    let tester = g.find("## Writing style - sound like a tester").unwrap();
    assert!(format < custom && custom < tester);

    // The 1.5 line sits between reading the existing cases and drafting.
    assert!(flat.contains(STEP_1_5), "{g}");
    let step1 = flat.find("1. Call `get_test_cases`").unwrap();
    let step15 = flat.find(STEP_1_5).unwrap();
    let step2 = flat.find("2. Draft your cases.").unwrap();
    assert!(step1 < step15 && step15 < step2);
    // The workflow still ends at step 5.
    assert!(g.trim_end().ends_with("`transform_cases` instead of rewriting the file yourself."), "{g}");
}

#[tokio::test]
async fn a_save_takes_effect_on_the_next_guide_call() {
    let _lock = crate::serial::writing_style();
    let dir = StyleDir::new();
    dir.save(true, "First version of the rules.");
    assert!(guide_now().await.contains("First version of the rules."));

    dir.save(true, "Second version of the rules.");
    let g = guide_now().await;
    assert!(g.contains("Second version of the rules."), "{g}");
    assert!(!g.contains("First version of the rules."), "{g}");

    dir.save(false, "Second version of the rules.");
    let g = guide_now().await;
    assert!(!g.contains("Second version of the rules."), "{g}");
    assert!(g.contains("## Granularity - quality over quantity"), "{g}");
}

/// The starting style switched on carries the old trial's rules, and its
/// text has no em or en dashes.
#[tokio::test]
async fn the_starting_style_switched_on_carries_the_old_trial_rules() {
    let _lock = crate::serial::writing_style();
    let dir = StyleDir::new();
    dir.save(true, style::DEFAULT_RISK_TIERED);
    let g = guide_now().await;
    let flat = g.split_whitespace().collect::<Vec<_>>().join(" ");
    for needle in [
        "## Edge cases, the tiered way",
        "## Scenario list before drafting",
        "## Closing summary",
        "## Regression suite review (trial rules)",
        "T1 25, T2 12, T3 5",
        "`KEEP_REGRESSION`",
        "\"Would you like me to apply these Regression tag changes?\"",
    ] {
        assert!(flat.contains(needle), "missing {needle:?} in:\n{g}");
    }
    assert!(!style::DEFAULT_RISK_TIERED.contains('\u{2014}') && !style::DEFAULT_RISK_TIERED.contains('\u{2013}'));
}
