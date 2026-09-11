//! The bridge's router, tested without sockets: routing, validation, and
//! the ADO-backed routes (Task 2) via wiremock. The TCP loop (Task 3) is
//! deliberately thin - everything interesting lives in `route`.

use v2_lib::ai_bridge::{new_token, q, route, BridgeContext};

fn ctx() -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        module_ref: Some("Custom.Module".into()),
        preconditions_ref: Some("Custom.Preconditions".into()),
        disabled_tools: vec![],
        working_dir: None,
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

    // The style rules are scoped to the case text - an assistant that read
    // them as a rule for its own replies would stop explaining itself.
    let style = body
        .split("## Writing style")
        .nth(1)
        .and_then(|rest| rest.split("## reviewer_notes").next())
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
}

/// The quote rule: a citation either carries the source's own words
/// verbatim, or states one of the fixed exemptions - never a paraphrase
/// dressed up as a quote. Pinned separately from the round-3 assertions
/// above because round-round-6 feedback found the old single sentence
/// ("one short quote only when the exact wording IS the requirement")
/// left "no quote" as a silent third option nobody was told to justify.
#[tokio::test]
async fn the_guide_teaches_the_quote_rule_and_its_exemptions() {
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
    assert!(body.contains("pbi"));
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
    let path = draft_on_disk(&dir);
    let target = format!(
        "/optimize?in_place=true&path={}",
        path.to_string_lossy().replace('\\', "%5C")
    );
    let (status, out) = route(&ctx(), None, "POST", &target, "", "1.0.0").await;
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
    let Parsed::Complete { method, target, token, body } = parse_http(ok) else {
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
async fn run_failures_returns_failed_cases_with_comment_and_bugs() {
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
        route(&ctx(), Some(&client), "GET", "/run-failures?pbi=42", "", "1.18.11").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["failed"], 1, "only the failed point counts - passed and never-run do not");
    assert_eq!(v["failures"].as_array().unwrap().len(), 1);
    let f = &v["failures"][0];
    assert_eq!(f["case_id"], 201);
    assert_eq!(f["title"], "Valid login");
    assert_eq!(f["comment"], "Redirect loops back to the sign-in page on the second attempt.");
    assert_eq!(f["bug_ids"][0], 777);
    assert_eq!(v["plan"]["name"], "Web - Auth Plan");
    assert_eq!(v["cases_in_suite"], 3);
}

/// Resolving a suite scans every test plan in the project (~60s on a
/// large org) - which is why the bridge remembers the answer: the second
/// call must reuse it and go straight to the points.
#[tokio::test]
async fn run_failures_resolves_the_suite_once_and_reuses_it() {
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
            route(&ctx(), Some(&client), "GET", "/run-failures?pbi=43", "", "1.18.11").await;
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
async fn run_failures_re_resolves_a_cached_suite_that_was_deleted() {
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
        route(&ctx(), Some(&client), "GET", "/run-failures?pbi=44", "", "1.18.11").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["cases_in_suite"], 1);

    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/run-failures?pbi=44", "", "1.18.11").await;
    assert_eq!(status, 200, "the 404 must trigger a re-resolve, not an error: {body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["cases_in_suite"], 2, "the answer must come from the NEW suite");
}

/// A PBI with no requirement suite is an ANSWER, not an error - and above
/// all not a reason to create one. The bridge never writes to Azure DevOps.
#[tokio::test]
async fn run_failures_on_a_pbi_with_no_suite_says_so_without_creating_one() {
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
        route(&ctx(), Some(&client), "GET", "/run-failures?pbi=42", "", "1.18.11").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["failures"].as_array().unwrap().len(), 0);
    assert!(v["note"].as_str().unwrap().contains("never had a test run"));
    // No POST reached the mock server - wiremock 404s any unmatched
    // request, and a create would have errored the route before this line.
}

#[tokio::test]
async fn run_failures_requires_a_pbi_and_a_signed_in_client() {
    let (status, _) = route(&ctx(), None, "GET", "/run-failures?pbi=42", "", "1.18.11").await;
    assert_eq!(status, 503, "no client means sign in first");

    let (_, client) = ado_stub().await;
    let (status, body) =
        route(&ctx(), Some(&client), "GET", "/run-failures", "", "1.18.11").await;
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
    // other test's way (the refcache is process-global).
    let c = BridgeContext {
        org: "cap-org".into(),
        project: "CapProj".into(),
        module_ref: None,
        preconditions_ref: None,
        disabled_tools: vec![],
        working_dir: None,
    };
    let key = v2_lib::refcache::tags_key("cap-org", "CapProj");
    let values: Vec<String> = (0..350).map(|i| format!("tag-{i:03}")).collect();
    v2_lib::refcache::put(&key, &values);

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

