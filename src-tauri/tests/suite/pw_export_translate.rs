//! Playwright export: one Auto Run script as a raw spec.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use v2_lib::autorun::recipe::RecipeStep;
use v2_lib::autorun::CaseScript;
use v2_lib::browser::locator::Target;
use v2_lib::pw_export::translate::{check, raw_spec, RawSpecInput};

fn origins() -> Vec<String> {
    vec!["https://app.example".into(), "https://sso.example".into()]
}

fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn target(v: Value) -> Target {
    serde_json::from_value(v).unwrap()
}

fn after_sign_in() -> Vec<RecipeStep> {
    serde_json::from_value(json!([
        {
            "kind": "when_visible",
            "selector": { "css": ".bootbox.modal.show .modal-footer button" },
            "within_ms": 4000,
            "then": [{ "kind": "click", "selector": { "css": ".bootbox.modal.show .modal-footer button" } }]
        },
        {
            "kind": "when_visible",
            "selector": { "css": "#sidebar-toggle-menu:not(.active)" },
            "within_ms": 5000,
            "then": [{ "kind": "click", "selector": { "css": "#sidebar-toggle-menu" } }]
        }
    ]))
    .unwrap()
}

fn script(steps: Value) -> CaseScript {
    serde_json::from_value(json!({ "case_id": 1, "title": "A case", "steps": steps })).unwrap()
}

struct Fx {
    script: CaseScript,
    signin: Vec<RecipeStep>,
    area: Vec<Target>,
    by_name: BTreeMap<String, Vec<Target>>,
    texts: BTreeMap<i32, String>,
}

impl Fx {
    fn new(script: CaseScript) -> Fx {
        Fx {
            script,
            signin: vec![],
            area: vec![target(json!({ "css": "a.area" }))],
            by_name: BTreeMap::new(),
            texts: BTreeMap::new(),
        }
    }
    fn run(&self) -> Result<String, String> {
        raw_spec(&RawSpecInput {
            script: &self.script,
            md_path: "suites/sl/admin/m/f/test-cases/f.md".into(),
            feature_title: "F".into(),
            after_sign_in: &self.signin,
            area_clicks: &self.area,
            area_clicks_by_name: &self.by_name,
            step_texts: &self.texts,
            origins: &origins(),
            fixme: None,
        })
        .map_err(|e| e.0)
    }
}

fn one_step(actions: Value) -> Fx {
    Fx::new(script(json!([{ "step_number": 1, "actions": actions }])))
}

#[test]
fn a_sibling_script_becomes_the_golden_raw_spec() {
    let script: CaseScript = serde_json::from_str(include_str!("../fixtures/pw_export/script-135560.json")).unwrap();
    let texts: BTreeMap<String, String> =
        serde_json::from_value(json!({
            "1": "Open \"Performance Management\", go to \"Setup & Configuration\" and select \"Definition Wizard\".",
            "2": "If the landing page shows, make sure the \"Goals / KPIs\" and \"Competencies\" cards are both selected and click \"Continue to Configuration\".",
            "3": "Make sure at least one rating method is listed and click \"Save & Continue\".",
            "4": "On the \"Performance-based\" card, click \"Activate\", then click \"Activate\" in the \"Activate Template\" dialog.",
            "5": "Click \"Add Level\". Copy the two lines Senior and Expert from a text editor and paste them into the \"Level name\" field of the new row.",
            "6": "Click the \"Confirm edit\" check icon, then click \"Use Custom Levels\"."
        }))
        .unwrap();
    let texts: BTreeMap<i32, String> = texts.into_iter().map(|(k, v)| (k.parse().unwrap(), v)).collect();
    let signin = after_sign_in();
    let area = vec![target(json!({ "css": "a[href*='DefinitionWizard']" }))];
    let by_name = BTreeMap::new();
    let got = raw_spec(&RawSpecInput {
        script: &script,
        md_path: "suites/sl/admin/performance/definition-wizard/test-cases/definition-wizard.md".into(),
        feature_title: "Definition Wizard".into(),
        after_sign_in: &signin,
        area_clicks: &area,
        area_clicks_by_name: &by_name,
        step_texts: &texts,
        origins: &origins(),
        fixme: None,
    })
    .unwrap();
    golden("raw-135560.spec.ts.golden", &got);
}

#[test]
fn a_response_is_marked_before_the_click_and_taken_after() {
    let got = one_step(json!([
        { "kind": "click", "selector": "#save" },
        { "kind": "expect_response", "method": "post", "url_contains": "API/Save", "status": 201,
          "json": { "ok": true, "n": 2, "who": "it's", "tags": ["a", "b"] } }
    ]))
    .run()
    .unwrap();
    // Where the step's requests begin, marked as the step starts; the check takes from there.
    let mark = got.find("const sentFrom1 = sent.length;").expect("marked");
    let click = got.find("await cur.locator('#save').first().click();").unwrap();
    let wait = got
        .find("const r1_0 = await nextResponse(sentFrom1, taken1, (r: Req) => r.url().toLowerCase().includes('api/save') && r.method() === 'POST', 10000);")
        .expect("taken");
    assert!(mark < click && click < wait, "{got}");
    assert!(got.contains("const taken1 = new Set<object>();"), "{got}");
    assert!(got.contains("expect(r1_0.status()).toBe(201);"));
    assert!(got.contains(
        "expect(await r1_0.json()).toMatchObject({ 'ok': true, 'n': 2, 'who': 'it\\'s', 'tags': ['a', 'b'] });"
    ), "{got}");
    // The page's requests are logged in the order they are sent, once, before the first page load.
    assert_eq!(got.matches("const nextResponse = async").count(), 1, "{got}");
    assert!(got.find("page.context().on('request'").unwrap() < got.find("await page.goto('/')").unwrap(), "{got}");
    assert!(!got.contains("waitForResponse"), "{got}");
    // A spec with no such check carries none of it.
    let plain = one_step(json!([{ "kind": "click", "selector": "#a" }])).run().unwrap();
    assert!(!plain.contains("nextResponse") && !plain.contains("'requestfinished'"), "{plain}");
}

#[test]
fn two_responses_in_one_step_each_take_their_own() {
    let got = one_step(json!([
        { "kind": "click", "selector": "#filter" },
        { "kind": "expect_response", "method": "get", "url_contains": "api/List", "status": 200 },
        { "kind": "expect_response", "method": "get", "url_contains": "api/List", "status": 200 }
    ]))
    .run()
    .unwrap();
    let take = "(r: Req) => r.url().toLowerCase().includes('api/list') && r.method() === 'GET', 10000);";
    let mark = got.find("const sentFrom1 = sent.length;").expect("marked");
    let first = got.find(&format!("const r1_0 = await nextResponse(sentFrom1, taken1, {take}")).expect("the first check");
    let second = got.find(&format!("const r1_1 = await nextResponse(sentFrom1, taken1, {take}")).expect("the second check");
    let click = got.find("await cur.locator('#filter').first().click();").unwrap();
    assert!(mark < click && click < first && first < second, "{got}");
    // They share one set of the requests already taken, and one mark.
    assert_eq!(got.matches("const taken1 = new Set<object>();").count(), 1, "{got}");
    assert_eq!(got.matches("const sentFrom1 = sent.length;").count(), 1, "{got}");
}

/// The export takes requests in the order they were SENT, as the app does (`pick_in_order`), not the
/// order their answers arrive: the helper the spec carries is run here in Node against the same
/// timelines the app's rule is asked about, and the two agree on which request each check takes.
#[test]
fn the_export_takes_requests_in_the_order_the_app_does() {
    use std::process::Command;
    use v2_lib::autorun::api_checks::{pick_in_order, Pick};
    use v2_lib::browser::net_record::{NetEntry, NetState};
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node is not installed: the generated helper was not run");
        return;
    }
    let got = lf(&one_step(json!([
        { "kind": "click", "selector": "#filter" },
        { "kind": "expect_response", "url_contains": "api/List", "status": 200 }
    ]))
    .run()
    .unwrap());
    let from = got.find("    type Req = ").expect("the log");
    let at = got.find("    const nextResponse = ").expect("the helper");
    let end = at + got[at..].find("\n    };\n").expect("its end") + "\n    };\n".len();
    let helper = &got[from..end];

    // Each timeline: JS that sends, answers and cancels requests A and B, then what each of two checks
    // takes - judged by the export here, and by `pick_in_order` over the same states below.
    let scenarios: [(&str, &str, [&str; 2]); 3] = [
        // The first is slow: B answers first, and A after the first check has been made.
        ("slow_first", "send(A); send(B); finish(B); setTimeout(() => finish(A), 80);", ["B", "A"]),
        // Both answered, B first, before the checks are made: the oldest sent comes first.
        ("answered_out_of_order", "send(A); send(B); finish(B); finish(A);", ["A", "B"]),
        // A debounced search: the first is cancelled, the second answered.
        ("debounced", "send(A); fail(A, 'net::ERR_ABORTED'); send(B); finish(B);", ["B", "B"]),
    ];
    let mut ran = String::new();
    for (name, timeline, _) in &scenarios {
        ran.push_str(&format!(
            "  {{ const out = []; reset(); {timeline}\n    for (let i = 0; i < 2; i++) {{ try {{ out.push((await nextResponse(0, taken, () => true, 400)).name); }} catch (e) {{ out.push('error'); }} }}\n    results['{name}'] = out; }}\n"
        ));
    }
    let script = format!(
        "const handlers: Record<string, ((r: any) => void)[]> = {{}};\nconst context = {{ on: (ev: string, f: (r: any) => void) => {{ (handlers[ev] ||= []).push(f); }} }};\nconst page: any = {{ context: () => context, waitForTimeout: (ms: number) => new Promise(r => setTimeout(r, ms)) }};\nlet cur = page;\n{helper}\nlet taken = new Set<object>();\nconst made: Record<string, any> = {{}};\nconst reset = () => {{ sent.length = 0; taken = new Set<object>(); for (const k of Object.keys(made)) delete made[k]; }};\nconst req = (name: string) => made[name] ||= {{ name, method: () => 'GET', url: () => 'https://hr.example/hr/api/List?x=' + name, redirectedFrom: () => null, frame: () => ({{ page: () => page }}), failure: () => ({{ errorText: '' }}), response: async () => ({{ name }}) }};\nconst send = (n: string) => handlers.request.forEach(f => f(req(n)));\nconst finish = (n: string) => handlers.requestfinished.forEach(f => f(req(n)));\nconst fail = (n: string, text: string) => {{ const r = req(n); r.failure = () => ({{ errorText: text }}); handlers.requestfailed.forEach(f => f(r)); }};\nconst A = 'A', B = 'B';\nconst results: Record<string, string[]> = {{}};\n(async () => {{\n{ran}  console.log(JSON.stringify(results));\n}})();\n"
    );
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("helper.ts");
    std::fs::write(&file, script).unwrap();
    let out = Command::new("node").arg(&file).output().unwrap();
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
    let said: serde_json::Value = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();

    // The app's rule over the same timelines.
    let entry = |name: &str, seq: u64, state: NetState| NetEntry {
        seq,
        id: name.to_string(),
        method: "GET".into(),
        path_query: format!("/hr/api/List?x={name}"),
        status: matches!(state, NetState::Finished).then_some(200),
        mime: None,
        state,
        redirect: None,
        kind: Some("XHR".into()),
        same_site: true,
        polling: false,
    };
    let took = |entries: &[NetEntry], taken: &[String]| match pick_in_order(entries, None, "api/list", taken) {
        Pick::Finished(e) | Pick::Failed(e) | Pick::PendingOnly(e) => e.id,
        Pick::None { .. } => "none".to_string(),
    };
    let fin = NetState::Finished;
    let app: Vec<(&str, [String; 2])> = vec![
        ("slow_first", {
            let one = took(&[entry("A", 0, NetState::Pending), entry("B", 1, fin.clone())], &[]);
            let two = took(&[entry("A", 0, fin.clone()), entry("B", 1, fin.clone())], &[one.clone()]);
            [one, two]
        }),
        ("answered_out_of_order", {
            let one = took(&[entry("A", 0, fin.clone()), entry("B", 1, fin.clone())], &[]);
            let two = took(&[entry("A", 0, fin.clone()), entry("B", 1, fin.clone())], &[one.clone()]);
            [one, two]
        }),
        // Both checks are asked about the same, cancelled-then-answered, record: B is taken, then nothing is left.
        ("debounced", {
            let record = [entry("A", 0, NetState::Failed("net::ERR_ABORTED".into())), entry("B", 1, fin.clone())];
            let one = took(&record, &[]);
            [one.clone(), one]
        }),
    ];
    for ((name, _, expected), (app_name, app_took)) in scenarios.iter().zip(&app) {
        assert_eq!(name, app_name);
        assert_eq!(app_took.as_slice(), expected.map(String::from).as_slice(), "the app, {name}");
        let export: Vec<String> = said[name].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
        // The export's second check, when nothing is left for it, says so with an error; the app's
        // `debounced` has taken the one request there is at both checks above, so only the first is compared.
        let n = if *name == "debounced" { 1 } else { 2 };
        assert_eq!(export[..n], app_took[..n], "the export, {name}: {export:?}");
    }
}

#[test]
fn a_list_of_objects_in_a_json_check_is_refused() {
    let why = one_step(json!([
        { "kind": "expect_response", "url_contains": "x", "json": { "rows": [{ "a": 1 }] } }
    ]))
    .run()
    .unwrap_err();
    assert!(why.contains("list of objects"), "{why}");
    let why = one_step(json!([
        { "kind": "api_request", "path": "/x", "expect": { "json": [{ "a": 1 }] } }
    ]))
    .run()
    .unwrap_err();
    assert!(why.contains("list of objects"), "{why}");
}

#[test]
fn an_api_request_sends_its_query_and_checks_the_answer() {
    let got = one_step(json!([
        { "kind": "api_request", "path": "/api/x", "query": { "q": "1" }, "expect": { "status": 200, "json": { "a": "b" } } }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("const a1_0 = await cur.request.get('/api/x', { params: { 'q': '1' }, maxRedirects: 0 });"), "{got}");
    assert!(got.contains("expect(a1_0.status()).toBe(200);"));
    assert!(got.contains("expect(await a1_0.json()).toMatchObject({ 'a': 'b' });"));
}

#[test]
fn when_visible_waits_then_acts_inside_an_if() {
    let got = one_step(json!([
        { "kind": "when_visible", "selector": "#banner", "then": [{ "kind": "click", "selector": "#banner .x" }] },
        { "kind": "when_visible", "selector": "text=Hello", "within_ms": 500,
          "then": [{ "kind": "click", "selector": "text=Hello" }] }
    ]))
    .run()
    .unwrap();
    assert!(
        got.contains(
            "    if (await cur.locator('#banner').first().waitFor({ timeout: 2000 }).then(() => true, () => false)) {\n      await cur.locator('#banner .x').first().click();\n    }\n"
        ),
        "{got}"
    );
    // A Legacy text= selector already ends in .last(); no .first() after it.
    assert!(got.contains("await cur.locator('text=Hello').last().waitFor({ timeout: 500 })"), "{got}");
    assert!(!got.contains(".last().first()"));
}

#[test]
fn first_goes_on_single_element_locators_but_not_on_nth_or_counts() {
    let got = one_step(json!([
        { "kind": "click", "selector": { "role": "button", "name": "Save", "nth": 1 } },
        { "kind": "expect_count", "selector": "li", "equals": 3, "timeout_ms": 900 },
        { "kind": "expect_attribute", "selector": [{ "css": "#a" }, { "role": "textbox" }], "name": "title", "equals": "x" },
        { "kind": "expect_hidden", "selector": "#gone", "timeout_ms": 100 },
        { "kind": "expect_focused", "selector": "#f" }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("getByRole('button', { name: 'Save' }).filter({ visible: true }).nth(1).click();"), "{got}");
    assert!(got.contains("await expect(cur.locator('li')).toHaveCount(3, { timeout: 900 });"), "{got}");
    assert!(got.contains(".getByRole('textbox').filter({ visible: true }).first()).toHaveAttribute('title', 'x', { timeout: 10000 });"), "{got}");
    assert!(got.contains("await expect(cur.locator('#gone').first()).toBeHidden({ timeout: 100 });"), "{got}");
    assert!(got.contains("await expect(cur.locator('#f').first().and(cur.locator(':focus-within'))).toHaveCount(1, { timeout: 10000 });"), "{got}");
}

/// The enabled and disabled checks become Playwright's own, which read the
/// same three things; an absent attribute is a negated `toHaveAttribute`.
#[test]
fn enabled_disabled_and_an_absent_attribute_translate() {
    let got = one_step(json!([
        { "kind": "expect_enabled", "selector": "#save" },
        { "kind": "expect_disabled", "selector": "#save", "timeout_ms": 300 },
        { "kind": "expect_attribute", "selector": "#t", "name": "aria-pressed", "absent": true }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("await expect(cur.locator('#save').first()).toBeEnabled({ timeout: 10000 });"), "{got}");
    assert!(got.contains("await expect(cur.locator('#save').first()).toBeDisabled({ timeout: 300 });"), "{got}");
    assert!(got.contains("await expect(cur.locator('#t').first()).not.toHaveAttribute('aria-pressed', { timeout: 10000 });"), "{got}");
}

/// `expect_checked` becomes Playwright's `toBeChecked`, negated for a box
/// that must be clear; both read the live state, as Auto Run does.
#[test]
fn checked_translates_to_to_be_checked() {
    let got = one_step(json!([
        { "kind": "expect_checked", "selector": "#agree" },
        { "kind": "expect_checked", "selector": "#news", "checked": false, "timeout_ms": 300 }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("await expect(cur.locator('#agree').first()).toBeChecked({ timeout: 10000 });"), "{got}");
    assert!(got.contains("await expect(cur.locator('#news').first()).not.toBeChecked({ timeout: 300 });"), "{got}");
    // A clear box is not a partly ticked one: an aria widget saying mixed fails.
    assert!(got.contains("await expect(cur.locator('#news').first()).not.toHaveAttribute('aria-checked', 'mixed', { timeout: 300 });"), "{got}");
    assert!(!got.contains("#agree').first()).not.toHaveAttribute"), "{got}");
}

/// `expect_no_change` becomes one block: the control waited for and checked
/// not to be under another element, the pointer put on it, a forced click,
/// the same wait, and the same comparisons the app makes (the address, the
/// open dialogs and any browser dialog, what the click sent, the messages the
/// page shows, the region's text and states), so a case that passes here
/// fails there only for a reason. The helpers go out once, and their bodies
/// are the app's own (`no_change::STATE_JS`, `DIALOGS_JS`, `MESSAGES_JS`,
/// `COVERED_JS`).
#[test]
fn expect_no_change_exports_a_forced_click_and_the_same_comparisons() {
    use v2_lib::browser::no_change::{COVERED_JS, DIALOGS_JS, MESSAGES_JS, STATE_JS};
    let got = one_step(json!([
        { "kind": "expect_no_change", "selector": "#save" },
        { "kind": "expect_no_change", "selector": "#card", "region": { "css": "#cards", "nth": 0 }, "within_ms": 2000 }
    ]))
    .run()
    .unwrap();
    let got = lf(&got);
    let block = |name: &str, target: &str, region: &str, wait: u32| {
        format!(
            "    {{\n      const target = {target};\n      const region = {region};\n      await expect(target).toBeVisible({{ timeout: 10000 }});\n      const covered = await target.evaluate(coveredBy);\n      if (covered) {{ throw new Error('{name} is covered by ' + covered); }}\n      await target.hover({{ force: true }});\n      const before = {{ url: cur.url(), dialogs: await cur.evaluate(openDialogs), messages: await cur.evaluate(shownMessages), state: await region.evaluate(noChangeState) }};\n      let browserDialog = false;\n      const onDialog = (d: {{ dismiss(): Promise<void> }}) => {{ browserDialog = true; d.dismiss().catch(() => {{}}); }};\n      const writes: string[] = [];\n      const onRequest = (r: Awaited<ReturnType<typeof cur.waitForRequest>>) => {{ if (/^(POST|PUT|PATCH|DELETE)$/.test(r.method())) {{ writes.push(`${{r.method()}} ${{new URL(r.url()).pathname}}`); }} }};\n      cur.on('dialog', onDialog);\n      cur.on('request', onRequest);\n      await target.click({{ force: true }});\n      await cur.waitForTimeout({wait});\n      cur.off('dialog', onDialog);\n      cur.off('request', onRequest);\n      expect(writes, 'what the click sent').toEqual([]);\n      expect(cur.url(), 'the address').toBe(before.url);\n      expect(browserDialog, 'a browser dialog').toBe(false);\n      expect(await cur.evaluate(openDialogs), 'open dialogs').toBe(before.dialogs);\n      expect(newTexts(before.messages, await cur.evaluate(shownMessages)), 'new messages').toEqual([]);\n      expect(await region.evaluate(noChangeState), 'the region').toEqual(before.state);\n    }}\n"
        )
    };
    // A plain-string selector is never narrowed to its first match: several is a failure, as in the app.
    assert!(got.contains(&block("#save", "cur.locator('#save')", "cur.locator('#save')", 1000)), "{got}");
    assert!(got.contains(&block("#card", "cur.locator('#card')", "cur.locator('#cards').filter({ visible: true }).nth(0)", 2000)), "{got}");
    // Once, before the first use, with the app's own bodies word for word.
    assert_eq!(got.matches("const noChangeState = (e: Element) =>").count(), 1, "{got}");
    assert_eq!(got.matches("const openDialogs = () =>").count(), 1, "{got}");
    assert_eq!(got.matches("const shownMessages = () =>").count(), 1, "{got}");
    assert_eq!(got.matches("const coveredBy = (e: Element) =>").count(), 1, "{got}");
    assert_eq!(got.matches("const newTexts = ").count(), 1, "{got}");
    assert!(got.find("const noChangeState").unwrap() < got.find("const before").unwrap(), "{got}");
    assert!(got.find("const coveredBy").unwrap() < got.find("const before").unwrap(), "{got}");
    // Every line between the function's first and last (those are wrapped).
    for body in [STATE_JS, DIALOGS_JS, MESSAGES_JS, COVERED_JS] {
        let lines: Vec<&str> = body.lines().collect();
        for line in &lines[1..lines.len() - 1] {
            assert!(got.contains(&format!("    {line}\n")), "the helper lost {line:?}: {got}");
        }
    }
    assert!(got.contains("    }).call(e as HTMLElement);\n") && got.contains("    })();\n"), "{got}");
    assert!(got.contains("function (this: HTMLElement) {"), "{got}");
    // A spec with no such action carries neither helper.
    let plain = lf(&one_step(json!([{ "kind": "click", "selector": "#a" }])).run().unwrap());
    assert!(!plain.contains("noChangeState") && !plain.contains("openDialogs"), "{plain}");
}

/// The check that the control is not under another element is the app's
/// (`COVERED_JS`), run before the pointer goes on it and before the page is
/// read: a covered control fails with the app's own words and is never clicked.
#[test]
fn expect_no_change_checks_the_control_is_ready_and_not_covered_before_it_clicks() {
    let got = lf(&one_step(json!([{ "kind": "expect_no_change", "selector": "#save" }])).run().unwrap());
    let at = |needle: &str| got.find(needle).unwrap_or_else(|| panic!("{needle} missing: {got}"));
    assert!(at("toBeVisible") < at("evaluate(coveredBy)"), "{got}");
    assert!(at("evaluate(coveredBy)") < at("target.hover({ force: true })"), "{got}");
    assert!(at("target.hover({ force: true })") < at("const before ="), "{got}");
    assert!(at("const before =") < at("target.click({ force: true })"), "{got}");
    assert!(got.contains("throw new Error('#save is covered by ' + covered)"), "{got}");
    // The wording is the app's: `<target> is covered by <what is there>`.
    assert!(v2_lib::browser::input::COVERED_BY.starts_with("is covered by"));
    // A selector with a form of its own (an object) keeps the first-match rule it always had.
    let object = lf(&one_step(json!([{ "kind": "expect_no_change", "selector": { "css": "#save" } }])).run().unwrap());
    assert!(object.contains("const target = cur.locator('#save').filter({ visible: true }).first();") || object.contains("const target = cur.locator('#save').first();"), "{object}");
}

#[test]
fn tab_switching_moves_cur() {
    let got = one_step(json!([
        { "kind": "click", "selector": "#open" },
        { "kind": "expect_tab", "name": "help-tab", "url_contains": "a.b?c" },
        { "kind": "switch_tab", "name": "help-tab" },
        { "kind": "click", "selector": "#inside" },
        { "kind": "close_tab", "name": "help-tab" },
        { "kind": "open_tab", "name": "t2", "url": "/path?x=1" },
        { "kind": "switch_tab", "name": "main" },
        { "kind": "expect_tab_closed", "name": "t2", "within_ms": 3000 }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("    let tab_help_tab: typeof page;\n"), "{got}");
    assert!(got.contains("const mark1 = opened.length;"), "{got}");
    assert!(got.contains("tab_help_tab = await nextTab(mark1, 10000);"), "{got}");
    assert!(got.contains("claimed.add(tab_t2);"), "{got}");
    assert!(got.contains("await expect(tab_help_tab).toHaveURL(new RegExp('a\\\\.b\\\\?c'));"), "{got}");
    assert!(got.contains("cur = tab_help_tab;\n    await cur.locator('#inside').first().click();"), "{got}");
    assert!(got.contains("await tab_help_tab.close();\n    if (cur === tab_help_tab) { cur = page; }"), "{got}");
    assert!(got.contains("tab_t2 = await cur.context().newPage();\n    await tab_t2.goto('/path?x=1');\n    claimed.add(tab_t2);\n    cur = tab_t2;"), "{got}");
    assert!(got.contains("cur = page;\n    await tab_t2.waitForEvent('close', { timeout: 3000 });"), "{got}");
}

#[test]
fn return_to_area_replays_the_named_area() {
    let mut fx = one_step(json!([
        { "kind": "return_to_area" },
        { "kind": "return_to_area", "area": "Other" }
    ]));
    fx.signin = after_sign_in();
    fx.by_name.insert("Other".into(), vec![target(json!({ "css": "a.other" }))]);
    let got = fx.run().unwrap();
    assert_eq!(got.matches("await cur.goto('/');").count(), 2, "{got}");
    let own = got.find("    // 1.").unwrap();
    let tail = &got[own..];
    assert!(tail.find("a.area").unwrap() < tail.find("a.other").unwrap(), "{got}");
    assert!(tail.contains("#sidebar-toggle-menu"), "the recipe's after_sign_in runs again");

    let mut fx = one_step(json!([{ "kind": "return_to_area", "area": "Nowhere" }]));
    fx.signin = vec![];
    let why = fx.run().unwrap_err();
    assert!(why.contains("Nowhere"), "{why}");
}

#[test]
fn press_key_times_three_presses_three_times() {
    let got = one_step(json!([
        { "kind": "press_key", "key": "Shift+Tab", "times": 3 },
        { "kind": "press_key", "key": "Ctrl+ArrowUp" },
        { "kind": "press_key", "key": "Space" }
    ]))
    .run()
    .unwrap();
    assert_eq!(got.matches("await cur.keyboard.press('Shift+Tab');").count(), 3, "{got}");
    assert!(got.contains("await cur.keyboard.press('Control+ArrowUp');"));
    assert!(got.contains("await cur.keyboard.press('Space');"));
}

#[test]
fn text_url_download_and_session_actions() {
    let got = one_step(json!([
        { "kind": "check_text", "value": "it's `x` ${y}" },
        { "kind": "check_url", "contains": "/a.b?c=(1)" },
        { "kind": "reload" },
        { "kind": "expire_session" },
        { "kind": "click", "selector": "#dl" },
        { "kind": "expect_download", "name": "Report_*.xlsx" }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("await expect(cur.locator('body')).toContainText('it\\'s `x` ${y}', { ignoreCase: true, useInnerText: true });"), "{got}");
    assert!(got.contains("await expect(cur).toHaveURL(new RegExp('\\\\/a\\\\.b\\\\?c=\\\\(1\\\\)'), { timeout: 10000 });"), "{got}");
    assert!(got.contains("await cur.reload();"));
    assert!(got.contains("await cur.context().clearCookies();"));
    let arm = got.find("const dl1_0 = cur.waitForEvent('download', { timeout: 30000 });").unwrap();
    assert!(arm < got.find("#dl").unwrap());
    assert!(got.contains("expect(d1_0.suggestedFilename()).toMatch(new RegExp('^Report_.*\\\\.xlsx$', 'i'));"), "{got}");
}

#[test]
fn drag_onto_translates_and_before_after_do_not() {
    let got = one_step(json!([{ "kind": "drag", "from": "#a", "to": "#b", "position": "onto" }])).run().unwrap();
    assert!(got.contains("await cur.locator('#a').first().dragTo(cur.locator('#b').first());"), "{got}");
    for pos in ["before", "after"] {
        let why = one_step(json!([{ "kind": "drag", "from": "#a", "to": "#b", "position": pos }])).run().unwrap_err();
        assert!(why.contains("drag"), "{why}");
    }
}

#[test]
fn the_header_comes_first_and_a_missing_text_leaves_the_number_alone() {
    let mut fx = Fx::new(script(json!([
        { "step_number": 7, "actions": [], "unchecked": "line one\nline two" },
        { "step_number": 8, "actions": [] }
    ])));
    fx.texts.insert(7, "first\nsecond".into());
    let got = fx.run().unwrap();
    assert!(got.starts_with(
        "// spec: suites/sl/admin/m/f/test-cases/f.md\n// seed: suites/_generated/seed.spec.ts\n\nimport { test, expect } from '@playwright/test';\n\ntest.describe('F', () => {\n  test('A case', async ({ page }) => {\n    let cur = page;\n    await page.goto('/');\n"
    ), "{got}");
    assert!(got.contains("    // 7. first second\n    // TODO unchecked: the step's expected result - line one line two\n"), "{got}");
    assert!(got.contains("\n    // 8.\n"), "{got}");
    assert!(got.ends_with("  });\n});\n"));
    assert!(!got.contains("fixme") && !got.contains("skip"));
}

#[test]
fn each_unexportable_kind_says_why() {
    let table: Vec<(&str, Value)> = vec![
        ("upload", json!({ "kind": "upload", "selector": "#f", "file": "a.xlsx" })),
        ("sign_in", json!({ "kind": "sign_in", "account": "x" })),
        ("expect_dialog", json!({ "kind": "expect_dialog", "answer": "accept" })),
        ("expect_download", json!({ "kind": "expect_download", "name": "a.xlsx", "sheet": "S" })),
        ("expect_download", json!({ "kind": "expect_download", "name": "a.xlsx", "contains_text": ["x"] })),
        ("expect_row_count", json!({ "kind": "expect_row_count", "table": "#t", "at_least": 1, "at_most": 3 })),
    ];
    for (kind, action) in table {
        let s = script(json!([{ "step_number": 1, "actions": [action.clone()] }]));
        let why = check(&s, &origins()).unwrap_err().0;
        assert!(why.contains(kind), "{kind}: {why}");
        let why = one_step(json!([action])).run().unwrap_err();
        assert!(why.contains(kind), "{kind}: {why}");
    }
    // Inside a when_visible too.
    let s = script(json!([{ "step_number": 1, "actions": [
        { "kind": "when_visible", "selector": "#a", "then": [{ "kind": "upload", "selector": "#f", "file": "a" }] }
    ]}]));
    assert!(check(&s, &origins()).unwrap_err().0.contains("upload"));
}

#[test]
fn a_tab_opened_by_the_previous_step_is_claimed_by_the_next() {
    let fx = Fx::new(script(json!([
        { "step_number": 4, "actions": [{ "kind": "click", "selector": "#open" }] },
        { "step_number": 5, "actions": [{ "kind": "expect_tab", "name": "a" }] }
    ])));
    let got = fx.run().unwrap();
    assert!(got.contains("page.context().on('page', p => { opened.push(p); });"), "{got}");
    assert!(got.contains("const mark4 = opened.length;"), "{got}");
    assert!(got.contains("const mark5 = opened.length;"), "{got}");
    assert!(got.contains("tab_a = await nextTab(mark4, 10000);"), "counts from the previous step: {got}");
    assert!(got.contains("claimed.add(p);"));
    assert!(!got.contains("waitForEvent('page'"));
}

#[test]
fn two_expect_tabs_in_one_step_claim_distinct_pages_in_the_helper() {
    let got = one_step(json!([
        { "kind": "expect_tab", "name": "a" },
        { "kind": "expect_tab", "name": "b", "within_ms": 2000 }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("tab_a = await nextTab(mark1, 10000);\n    tab_b = await nextTab(mark1, 2000);"), "{got}");
    // The helper marks what it hands out as claimed and skips claimed pages.
    assert!(got.contains("filter(p => !claimed.has(p))"), "{got}");
}

#[test]
fn when_visible_on_a_hidden_selector_looks_for_a_visible_match_with_the_floor() {
    let got = one_step(json!([
        { "kind": "when_visible", "selector": { "css": "#x", "visible": false }, "within_ms": 100,
          "then": [{ "kind": "click", "selector": "#x" }] },
        { "kind": "wait_for", "selector": { "css": "#x", "visible": false }, "timeout_ms": 900 }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("if (await cur.locator('#x').filter({ visible: true }).first().waitFor({ timeout: 500 })"), "{got}");
    assert!(got.contains("cur.locator('#x').first().waitFor({ state: 'attached', timeout: 900 });"), "{got}");
}

#[test]
fn an_api_request_does_not_follow_redirects() {
    let got = one_step(json!([
        { "kind": "api_request", "path": "/a" },
        { "kind": "api_request", "path": "/b", "query": { "q": "1" } }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("cur.request.get('/a', { maxRedirects: 0 })"), "{got}");
    assert!(got.contains("{ params: { 'q': '1' }, maxRedirects: 0 }"), "{got}");
}

#[test]
fn keys_are_read_the_way_the_app_reads_them() {
    let got = one_step(json!([
        { "kind": "press_key", "key": "ctrl+Enter" },
        { "kind": "press_key", "key": "Shift + Tab" },
        { "kind": "press_key", "key": "shift+ctrl+End" }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("press('Control+Enter')"), "{got}");
    assert!(got.contains("press('Shift+Tab')"), "{got}");
    assert!(got.contains("press('Control+Shift+End')"), "{got}");
    let why = one_step(json!([{ "kind": "press_key", "key": "Hyper+Q" }])).run().unwrap_err();
    assert!(why.contains("press_key"), "{why}");
}

#[test]
fn a_blank_area_name_replays_the_own_area_and_response_text_is_trimmed() {
    let got = one_step(json!([
        { "kind": "return_to_area", "area": "  " },
        { "kind": "expect_response", "method": " post ", "url_contains": " Save " }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("await cur.locator('a.area')"), "{got}");
    assert!(got.contains("includes('save') && r.method() === 'POST'"), "{got}");
}

#[test]
fn text_and_focus_checks_follow_the_app() {
    let got = one_step(json!([
        { "kind": "check_text", "value": "x" },
        { "kind": "expect_focused", "selector": "#f", "timeout_ms": 50 }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("{ ignoreCase: true, useInnerText: true }"), "{got}");
    assert!(
        got.contains("await expect(cur.locator('#f').first().and(cur.locator(':focus-within'))).toHaveCount(1, { timeout: 50 });"),
        "{got}"
    );
}

#[test]
fn same_site_addresses_export_as_paths() {
    let got = one_step(json!([
        { "kind": "navigate", "url": " /hr/x?y=1 " },
        { "kind": "navigate", "url": "https://app.example/hr/a?b=2#c" },
        { "kind": "navigate", "url": "HTTPS://App.Example" },
        { "kind": "navigate", "url": "https://app.example?q=1" },
        { "kind": "navigate", "url": "https://sso.example/login?r=1" },
        { "kind": "open_tab", "name": "t", "url": "https://app.example/p?x=1" }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("await cur.goto('/hr/x?y=1');"), "{got}");
    assert!(got.contains("await cur.goto('/hr/a?b=2#c');"), "{got}");
    assert!(got.contains("await cur.goto('/');"), "{got}");
    assert!(got.contains("await cur.goto('/?q=1');"), "{got}");
    assert!(got.contains("await cur.goto('https://sso.example/login?r=1');"), "another allowed origin stays whole: {got}");
    assert!(got.contains("await tab_t.goto('/p?x=1');"), "{got}");
}

#[test]
fn other_addresses_are_refused_by_check_and_by_the_spec() {
    for url in ["https://other.example/x", "file:///c:/x.html", "//host/x", "x/y", "ftp://app.example/x"] {
        let why = one_step(json!([{ "kind": "navigate", "url": url }])).run().unwrap_err();
        assert!(why.contains("navigate"), "{url}: {why}");
        let why = one_step(json!([{ "kind": "open_tab", "name": "t", "url": url }])).run().unwrap_err();
        assert!(why.contains("open_tab"), "{url}: {why}");
        let s = script(json!([{ "step_number": 1, "actions": [{ "kind": "navigate", "url": url }] }]));
        assert!(check(&s, &origins()).is_err(), "{url}");
    }
}

#[test]
fn text_checks_read_the_element_the_way_auto_run_does() {
    let got = one_step(json!([
        { "kind": "expect_text", "selector": "#name", "equals": "  Senior \n  Expert ", "timeout_ms": 900 },
        { "kind": "expect_contains_text", "selector": "#list", "value": "Step\t2" }
    ]))
    .run()
    .unwrap();
    // Whitespace collapsed on the wanted side too, compared whole or as a part, case kept.
    assert!(
        got.contains("await expect.poll(async () => asCase(await cur.locator('#name').first().evaluate(readText), await cur.locator('#name').first().evaluate(readWritten), 'Senior Expert', true), { timeout: 900 }).toBe('Senior Expert');"),
        "{got}"
    );
    assert!(got.contains("await expect.poll(async () => asCase(await cur.locator('#list').first().evaluate(readText), await cur.locator('#list').first().evaluate(readWritten), 'Step 2', false), { timeout: 10000 }).toContain('Step 2');"), "{got}");
    assert!(!got.contains("toHaveText") && !got.contains("toContainText"), "{got}");
    // The reader is declared once, before the steps.
    assert_eq!(got.matches("const readText = ").count(), 1, "{got}");
    assert!(got.find("const readText = ").unwrap() < got.find("// 1.").unwrap(), "{got}");
}

#[test]
fn the_exported_text_reader_is_the_apps_own() {
    use v2_lib::browser::expect::READ_TEXT_JS;
    use v2_lib::pw_export::translate::read_text_ts;
    let ts = read_text_ts(0).join("\n");
    // Undo the two TypeScript additions and the constant comes back word for word.
    let back = ts
        .replacen("const readText = (e: Element) => (function (this: HTMLElement) {", "function() {", 1)
        .replacen("}).call(e as HTMLElement);", "}", 1);
    assert_eq!(back, READ_TEXT_JS.replace("\r\n", "\n"));
    // A spec with no text check carries no reader.
    let got = one_step(json!([{ "kind": "click", "selector": "#a" }])).run().unwrap();
    assert!(!got.contains("readText"), "{got}");
}

#[test]
fn a_text_check_in_the_recipe_also_declares_the_reader() {
    let mut fx = one_step(json!([{ "kind": "click", "selector": "#a" }]));
    fx.signin = serde_json::from_value(json!([{ "kind": "expect_text", "selector": "#who", "equals": "Me" }])).unwrap();
    let got = fx.run().unwrap();
    assert!(got.contains("const readText = "), "{got}");
}

#[test]
fn open_tab_alone_still_declares_what_it_marks_claimed() {
    let got = one_step(json!([{ "kind": "open_tab", "name": "t2", "url": "/x" }])).run().unwrap();
    assert!(got.contains("claimed.add(tab_t2);"), "{got}");
    assert!(got.contains("const claimed = new Set<typeof page>();"), "{got}");
    assert!(got.find("const claimed").unwrap() < got.find("claimed.add(tab_t2)").unwrap(), "{got}");
}

#[test]
fn tabs_named_in_the_recipe_are_declared_and_collected() {
    let mut fx = one_step(json!([{ "kind": "switch_tab", "name": "main" }]));
    fx.signin = serde_json::from_value(json!([
        { "kind": "click", "selector": "#help" },
        { "kind": "expect_tab", "name": "help" },
        { "kind": "close_tab", "name": "help" }
    ]))
    .unwrap();
    let got = fx.run().unwrap();
    assert!(got.contains("    let tab_help: typeof page;\n"), "{got}");
    assert!(got.contains("const opened: (typeof page)[] = [];"), "{got}");
    assert!(got.find("let tab_help").unwrap() < got.find("tab_help = await nextTab(").unwrap(), "{got}");
}

#[test]
fn a_single_value_json_expectation_is_refused_with_its_own_sentence() {
    for v in [json!(5), json!("ok"), json!(true)] {
        let s = script(json!([{ "step_number": 1, "actions": [{ "kind": "expect_response", "url_contains": "x", "json": v }] }]));
        let e = check(&s, &origins()).unwrap_err().0;
        assert!(e.contains("single value") && e.contains("toMatchObject"), "{e}");
        let s = script(json!([{ "step_number": 1, "actions": [{ "kind": "api_request", "path": "/api/x", "expect": { "status": 200, "json": v } }] }]));
        assert!(check(&s, &origins()).unwrap_err().0.starts_with("api_request json that is a single value"));
    }
    // An object or a list at the top still exports.
    let got = one_step(json!([{ "kind": "expect_response", "url_contains": "x", "json": [1, 2] }])).run().unwrap();
    assert!(got.contains("toMatchObject([1, 2])"), "{got}");
}

/// Compares `got` with a golden file under `tests/fixtures/pw_export/`, or
/// rewrites the file when PW_EXPORT_BLESS is set (then read the diff).
/// `src/lib/pwExportGolden.test.ts` type-checks every golden, so each one
/// must stay what the translator writes today.
fn golden(name: &str, got: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pw_export").join(name);
    if std::env::var_os("PW_EXPORT_BLESS").is_some() {
        std::fs::write(&path, got).unwrap();
    }
    let want = lf(&std::fs::read_to_string(&path).unwrap_or_default());
    assert_eq!(got, want, "{name} is stale - rerun with PW_EXPORT_BLESS=1 and review the diff");
}

#[test]
fn every_exportable_kind_becomes_the_kitchen_sink_golden() {
    let s: CaseScript = serde_json::from_value(json!({
        "case_id": 900,
        "title": "Kitchen sink - every exportable action",
        "steps": [
            { "step_number": 1, "actions": [
                { "kind": "navigate", "url": "https://app.example/hr/x?y=1#z" },
                { "kind": "navigate", "url": "https://sso.example/login" },
                { "kind": "click", "selector": { "role": "button", "name": "Save", "exact": true } },
                { "kind": "fill", "selector": { "css": "input[name='q']" }, "value": "it's `x` ${y}\nline 2" },
                { "kind": "wait_for", "selector": { "css": "#spinner", "visible": false }, "timeout_ms": 3000 },
                { "kind": "wait_for", "selector": [{ "css": "iframe#body" }, { "text": "Ready", "exact": true }], "timeout_ms": 4000 }
            ] },
            { "step_number": 2, "unchecked": "A person checks the colour.", "actions": [
                { "kind": "check_text", "value": "Welcome" },
                { "kind": "check_url", "contains": "/hr/x?y=1" },
                { "kind": "expect_visible", "selector": "#a", "timeout_ms": 1000 },
                { "kind": "expect_hidden", "selector": { "css": ".toast" } },
                { "kind": "expect_text", "selector": { "css": "select#level" }, "equals": "Senior   Expert", "timeout_ms": 2000 },
                { "kind": "expect_contains_text", "selector": { "css": "textarea#notes" }, "value": "draft" },
                { "kind": "expect_count", "selector": { "css": "tr.row" }, "equals": 3 },
                { "kind": "expect_attribute", "selector": { "css": "#save", "nth": 1 }, "name": "aria-disabled", "equals": "false" },
                { "kind": "expect_focused", "selector": "#name", "timeout_ms": 500 }
            ] },
            { "step_number": 3, "actions": [
                { "kind": "when_visible", "selector": { "role": "dialog", "name": "Confirm" }, "within_ms": 1500,
                  "then": [{ "kind": "click", "selector": { "role": "button", "name": "OK" } }] },
                { "kind": "expect_response", "method": "post", "url_contains": "/api/Save", "status": 200, "json": { "ok": true, "ids": [1, 2] }, "timeout_ms": 8000 },
                { "kind": "expect_response", "url_contains": "/api/list" },
                { "kind": "api_request", "path": "/api/items", "query": { "page": "1" }, "expect": { "status": 200, "json": { "count": 2 } } },
                { "kind": "api_request", "path": "/api/ping" }
            ] },
            { "step_number": 4, "actions": [
                { "kind": "reload" },
                { "kind": "expire_session" },
                { "kind": "return_to_area" },
                { "kind": "return_to_area", "area": "Goal Setting" },
                { "kind": "press_key", "key": "shift+Tab" },
                { "kind": "press_key", "key": "Tab", "times": 3 },
                { "kind": "drag", "from": "#card-1", "to": "#lane-2" },
                { "kind": "drag", "from": "#card-2", "to": "#lane-3", "position": "onto" }
            ] },
            { "step_number": 5, "actions": [
                { "kind": "click", "selector": { "text": "Export" } },
                { "kind": "expect_download", "name": "report*.xlsx" },
                { "kind": "when_visible", "selector": "#export-pdf",
                  "then": [{ "kind": "click", "selector": "#export-pdf" }, { "kind": "expect_download", "name": "summary.pdf", "within_ms": 9000 }] }
            ] },
            { "step_number": 6, "actions": [
                { "kind": "click", "selector": "#help" },
                { "kind": "expect_tab", "name": "help-tab", "url_contains": "/help", "within_ms": 5000 },
                { "kind": "switch_tab", "name": "help-tab" },
                { "kind": "click", "selector": "text=Contents" },
                { "kind": "close_tab", "name": "help-tab" },
                { "kind": "open_tab", "name": "second", "url": "/hr/second" },
                { "kind": "switch_tab", "name": "main" },
                { "kind": "click", "selector": "#close-second" },
                { "kind": "expect_tab_closed", "name": "second", "within_ms": 3000 }
            ] }
        ]
    }))
    .unwrap();
    let mut fx = Fx::new(s);
    fx.signin = after_sign_in();
    fx.by_name.insert("Goal Setting".into(), vec![target(json!({ "css": "a.goals" })), target(json!({ "role": "link", "name": "Goal Setting" }))]);
    fx.texts = [(1, "Open the page."), (2, "Check what it shows."), (4, "Move the cards.")]
        .into_iter()
        .map(|(n, t)| (n, t.to_string()))
        .collect();
    golden("kitchen-sink.spec.ts.golden", &fx.run().unwrap());
}

#[test]
fn open_tab_with_no_expect_tab_becomes_its_golden() {
    let mut fx = one_step(json!([
        { "kind": "open_tab", "name": "report", "url": "/hr/report" },
        { "kind": "expect_visible", "selector": "#chart" },
        { "kind": "close_tab", "name": "report" }
    ]));
    fx.signin = after_sign_in();
    golden("open-tab-only.spec.ts.golden", &fx.run().unwrap());
}

#[test]
fn an_armed_wait_never_rejects_unhandled_when_its_guard_is_skipped() {
    let got = one_step(json!([
        { "kind": "when_visible", "selector": "#export", "then": [
            { "kind": "click", "selector": "#export" },
            { "kind": "expect_download", "name": "a.xlsx" },
            { "kind": "expect_response", "url_contains": "/api/x" }
        ] }
    ]))
    .run()
    .unwrap();
    // Handled at once, beside the arming, before the guard...
    let dl = got.find("const dl1_0 = ").unwrap();
    let dl_settled = got.find("dl1_0.catch(() => {});").unwrap();
    let guard = got.find("if (await cur.locator('#export')").unwrap();
    assert!(dl < dl_settled && dl_settled < guard, "{got}");
    // ...while the check inside still awaits the wait itself, which rejects.
    assert!(got.contains("      const d1_0 = await dl1_0;\n"), "{got}");
    // A response check arms no promise at all: it takes from the requests logged since the step began, so a
    // guard that is skipped leaves nothing behind to reject.
    assert!(!got.contains("resp1_0"), "{got}");
    let mark = got.find("const sentFrom1 = sent.length;").unwrap();
    assert!(mark < guard, "{got}");
    assert!(got.contains("      const r1_0 = await nextResponse(sentFrom1, taken1, (r: Req) => r.url().toLowerCase().includes('/api/x'), 10000);\n"), "{got}");
}

#[test]
fn an_expectation_with_no_timeout_waits_as_long_as_auto_run_does() {
    let ms = v2_lib::browser::timing::Timing::default().expect_ms;
    let got = one_step(json!([
        { "kind": "expect_visible", "selector": "#a" },
        { "kind": "expect_hidden", "selector": "#b" },
        { "kind": "expect_text", "selector": "#c", "equals": "x" },
        { "kind": "expect_contains_text", "selector": "#d", "value": "y" },
        { "kind": "expect_count", "selector": "li", "equals": 2 },
        { "kind": "expect_attribute", "selector": "#e", "name": "n", "equals": "v" },
        { "kind": "expect_focused", "selector": "#f" },
        { "kind": "expect_response", "url_contains": "/api" },
        { "kind": "expect_visible", "selector": "#own", "timeout_ms": 1234 }
    ]))
    .run()
    .unwrap();
    let default = format!("{{ timeout: {ms} }}");
    // Six expect lines and the text polls: all the app's default.
    assert_eq!(got.matches(&default).count(), 7, "{got}");
    // And the response check, which takes its wait as a number of milliseconds.
    assert!(got.contains(&format!("includes('/api'), {ms});")), "{got}");
    assert!(got.contains("toBeVisible({ timeout: 1234 })"), "{got}");
    // Nothing falls back to Playwright's own 5 s.
    for l in got.lines().filter(|l| l.contains("await expect(") || l.contains("expect.poll(") || l.contains("= await nextResponse(")) {
        if l.contains("toHaveURL(") {
            continue;
        }
        assert!(l.contains("timeout: ") || l.contains(&format!(", {ms});")), "no timeout on: {l}");
    }
}

/// `expect_style` becomes Playwright's `toHaveCSS`: a colour as the rgb()
/// the browser reports, a keyword ignoring case, `contains` as a pattern,
/// and `not_equals` negated.
#[test]
fn a_style_check_translates_to_to_have_css() {
    let got = one_step(json!([
        { "kind": "expect_style", "selector": "#s", "property": "color", "equals": "red" },
        { "kind": "expect_style", "selector": "#s", "property": "box-shadow", "contains": "2px", "timeout_ms": 300 },
        { "kind": "expect_style", "selector": "#s", "property": "display", "not_equals": "none" },
        { "kind": "expect_style", "selector": "#s", "property": "content", "equals": "\"Hi\"" }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("await expect(cur.locator('#s').first()).toHaveCSS('color', 'rgb(255, 0, 0)', { timeout: 10000 });"), "{got}");
    assert!(got.contains("await expect(cur.locator('#s').first()).toHaveCSS('box-shadow', new RegExp('2px', 'i'), { timeout: 300 });"), "{got}");
    assert!(got.contains("await expect(cur.locator('#s').first()).not.toHaveCSS('display', new RegExp('^none$', 'i'), { timeout: 10000 });"), "{got}");
    assert!(got.contains("await expect(cur.locator('#s').first()).toHaveCSS('content', '\"Hi\"', { timeout: 10000 });"), "{got}");
}

/// Auto Run makes runs of spaces one and writes a comma as `, `: the
/// exported pattern takes any run of spaces, and any spaces around a comma.
#[test]
fn a_style_pattern_tolerates_runs_of_spaces() {
    let got = one_step(json!([
        { "kind": "expect_style", "selector": "#s", "property": "transform", "equals": "matrix(1,0,0,1,0,0)" },
        { "kind": "expect_style", "selector": "#s", "property": "box-shadow", "contains": "0px   1px" }
    ]))
    .run()
    .unwrap();
    assert!(
        got.contains(r#"toHaveCSS('transform', new RegExp('^matrix\\(1\\s*,\\s*0\\s*,\\s*0\\s*,\\s*1\\s*,\\s*0\\s*,\\s*0\\)$', 'i'), { timeout: 10000 });"#),
        "{got}"
    );
    assert!(got.contains(r#"toHaveCSS('box-shadow', new RegExp('0px\\s+1px', 'i'), { timeout: 10000 });"#), "{got}");
}

/// #46: a clean-up step runs whether the case's steps passed or failed, so
/// the spec plays it in a `finally` around them, and says what it is.
#[test]
fn a_clean_up_step_is_a_finally_around_the_case_steps() {
    let mut fx = Fx::new(script(json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#save" }] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#delete" }],
          "cleanup": true, "unchecked": "removes what step 1 made" }
    ])));
    fx.texts.insert(1, "Save the record".into());
    let got = fx.run().unwrap();
    let want = "    try {\n      // 1. Save the record\n      await cur.locator('#save').first().click();\n    } finally {\n      // Clean-up: removes what step 1 made\n      try {\n        await cur.locator('#delete').first().click();\n      } catch (e) {\n        test.info().annotations.push({ type: 'cleanup-failed', description: String(e) });\n      }\n    }\n";
    assert!(lf(&got).contains(want), "{got}");
    assert!(!got.contains("// 2."), "the clean-up is no step of the case: {got}");
}

#[test]
fn a_script_with_no_clean_up_has_no_try() {
    let got = one_step(json!([{ "kind": "click", "selector": "#save" }])).run().unwrap();
    assert!(!got.contains("try {") && !got.contains("finally"), "{got}");
}

/// #46 review: an error in the clean-up never replaces the steps' own, and
/// never fails a spec whose steps passed: it is caught and noted.
#[test]
fn a_clean_up_error_is_caught_and_noted_never_thrown() {
    let fx = Fx::new(script(json!([
        { "step_number": 1, "actions": [{ "kind": "click", "selector": "#save" }] },
        { "step_number": 2, "actions": [{ "kind": "click", "selector": "#delete" }],
          "cleanup": true, "unchecked": "removes what step 1 made" }
    ])));
    let got = lf(&fx.run().unwrap());
    let want = "    } finally {\n      // Clean-up: removes what step 1 made\n      try {\n        await cur.locator('#delete').first().click();\n      } catch (e) {\n        test.info().annotations.push({ type: 'cleanup-failed', description: String(e) });\n      }\n    }\n";
    assert!(got.contains(want), "{got}");
}

#[test]
fn pattern_and_bound_checks_translate() {
    let got = one_step(json!([
        { "kind": "expect_text", "selector": { "css": "#n" }, "matches": "^Showing \\d+$" },
        { "kind": "expect_attribute", "selector": { "css": "#n" }, "name": "aria-label", "matches": "^View " },
        { "kind": "expect_count", "selector": { "css": "#n" }, "at_least": 2 },
        { "kind": "expect_count", "selector": "li", "at_most": 4, "timeout_ms": 3000 }
    ]))
    .run()
    .unwrap();
    assert!(got.contains("await expect.poll(() => cur.locator('#n').filter({ visible: true }).first().evaluate(readText), { timeout: 10000 }).toMatch(new RegExp('^Showing \\\\d+$'));"), "{got}");
    assert!(got.contains("await expect(cur.locator('#n').filter({ visible: true }).first()).toHaveAttribute('aria-label', new RegExp('^View '), { timeout: 10000 });"), "{got}");
    assert!(got.contains("await expect.poll(() => cur.locator('#n').filter({ visible: true }).count(), { timeout: 10000 }).toBeGreaterThanOrEqual(2);"), "{got}");
    // `at_most` passes on too few, so it settles as the app does.
    assert!(got.contains("expect(await stillCount(cur.locator('li'), 4, 3000)).toBeLessThanOrEqual(4);"), "{got}");
    assert!(got.contains("const stillCount = async (loc: ReturnType<typeof page.locator>, most: number, timeout: number): Promise<number> => {"), "{got}");
    assert!(got.contains("if (Date.now() - since >= 750) return n;"), "{got}");
    assert!(got.contains("setTimeout(r, Math.min(250, Math.max(0, end - Date.now())))"), "{got}");
    // Only a script that asks for one carries the helper.
    let plain = one_step(json!([{ "kind": "expect_count", "selector": "li", "at_least": 1 }])).run().unwrap();
    assert!(!plain.contains("stillCount"), "{plain}");
    let url = one_step(json!([{ "kind": "check_url", "contains": "/x", "timeout_ms": 5000 }])).run().unwrap();
    assert!(url.contains("toHaveURL(new RegExp('\\\\/x'), { timeout: 5000 });"), "{url}");
}

/// `expect_each` is the app's own loop (`expect::keep_looking`): the list
/// and every item judged, passing only once the same pass has held for
/// 750 ms - written as `settle`, with the item count as what must hold.
#[test]
fn an_each_check_translates_to_a_settled_look_over_every_item() {
    let got = one_step(json!([
        { "kind": "expect_each", "selector": { "css": "div.card" }, "child": { "css": "h3" }, "matches": "^AUTOTEST " },
        { "kind": "expect_each", "selector": { "css": "div.card" }, "contains": "AUTO   TEST", "min": 3, "timeout_ms": 4000 }
    ]))
    .run()
    .unwrap();
    for want in [
        "expect(await settle(async () => {",
        "const items = cur.locator('div.card').filter({ visible: true });",
        "const n = await items.count();",
        "if (n > 200) return `matched ${n} items, more than the 200 expect_each reads`;",
        "if (n < 1) return `expected at least 1, counted ${n}`;",
        "if (n < 3) return `expected at least 3, counted ${n}`;",
        "const item = items.nth(i);",
        "const el = item.locator('h3').filter({ visible: true });",
        "const found = await el.count();",
        "if (found !== 1) return `item ${i + 1} of ${n}: found ${found} of ` + 'h3';",
        "const el = item;",
        "const got = await el.evaluate(readText, undefined, { timeout: left() });",
        "if (got === null || !new RegExp('^AUTOTEST ').test(got)) return `item ${i + 1} of ${n}: saw ${await saw(el, got)}`;",
        // The text is collapsed, as the app collapses both sides.
        "if (got === null || !got.includes('AUTO TEST')) return",
        "if ((await items.count()) !== n) return 'the list changed while it was read';",
        "return { read: String(n) };",
        "}, 10000)).toBeNull();",
        "}, 4000)).toBeNull();",
    ] {
        assert!(got.contains(want), "missing {want}\n{got}");
    }
    // Each helper once, however many checks.
    for once in ["const readText = ", "const settle = async (", "const saw = async ("] {
        assert_eq!(got.matches(once).count(), 1, "{once}\n{got}");
    }
    assert!(!got.contains("const placed") && !got.contains("const readBox"), "{got}");
}

#[test]
fn an_each_check_reads_what_the_run_reads() {
    let got = one_step(json!([
        { "kind": "expect_each", "selector": { "css": "a.row" }, "attribute": "href", "equals": "/x" },
        { "kind": "expect_each", "selector": { "css": "a.row" }, "attribute": "href", "not_empty": true },
        { "kind": "expect_each", "selector": { "css": "a.row" }, "attribute": "title", "contains": "Open" },
        { "kind": "expect_each", "selector": { "css": "li" }, "equals": "  A   B " },
        { "kind": "expect_each", "selector": { "css": "li" }, "not_empty": true },
        { "kind": "expect_each", "selector": { "css": "li" }, "child": { "css": "b" }, "visible": true },
        { "kind": "expect_each", "selector": { "css": "li" }, "child": { "css": "i", "visible": false }, "visible": true }
    ]))
    .run()
    .unwrap();
    for want in [
        // An attribute is read raw, and missing is not the empty string.
        "const got = await el.getAttribute('href', { timeout: left() });",
        "if (got !== '/x') return `item ${i + 1} of ${n}: saw ${JSON.stringify(got)}`;",
        "if (!got || !got.trim()) return `item ${i + 1} of ${n}: saw ${JSON.stringify(got)}`;",
        "if (got === null || !got.includes('Open')) return `item ${i + 1} of ${n}: saw ${JSON.stringify(got)}`;",
        // Text is read as the app reads it, and both sides are collapsed.
        "if (got !== 'A B') return `item ${i + 1} of ${n}: saw ${await saw(el, got)}`;",
        "if (!got || !got.trim()) return `item ${i + 1} of ${n}: saw ${await saw(el, got)}`;",
        // "Visible" stops at the child; a child written to match hidden ones is asked.
        "const el = item.locator('b').filter({ visible: true });",
        "const el = item.locator('i');",
        "if (!(await el.isVisible())) return `item ${i + 1} of ${n}: ` + ",
    ] {
        assert!(got.contains(want), "missing {want}\n{got}");
    }
    // A form control's value is never in a message.
    assert!(got.contains("? '(a field)' : JSON.stringify(got)"), "{got}");
}

#[test]
fn the_settling_helper_holds_a_pass_as_the_app_does() {
    let got = one_step(json!([
        { "kind": "expect_each", "selector": { "css": "li" }, "not_empty": true }
    ]))
    .run()
    .unwrap();
    // A pass counts once it has held 750 ms, polled every 250 ms; a pass
    // still held at the deadline passes; a look that failed starts over.
    for want in [
        "if (Date.now() - since >= 750) return null;",
        "if (Date.now() >= end) return held === null ? last : null;",
        "setTimeout(r, Math.min(250, Math.max(0, end - Date.now())))",
        "Math.abs(v - boxes[i]) <= 1",
    ] {
        assert!(got.contains(want), "missing {want}\n{got}");
    }
    // A look that throws (an item re-rendered under it) is looked at again.
    assert!(got.contains(".catch("), "{got}");
}

/// Every call a look makes is held to the time the check has left, so a page
/// that has stopped answering cannot keep a look of n items going for n times
/// two seconds past the check's own timeout.
#[test]
fn every_call_of_a_list_look_is_held_to_the_time_the_check_has_left() {
    let got = one_step(json!([
        { "kind": "expect_each", "selector": { "css": "li" }, "attribute": "title", "not_empty": true },
        { "kind": "expect_each", "selector": { "css": "li" }, "not_empty": true },
        { "kind": "expect_order", "selector": { "css": "li" }, "texts": ["a", "b"] },
        { "kind": "expect_position", "selector": { "css": "#a" }, "relative_to": { "css": "#bar" }, "relation": "right_of" }
    ]))
    .run()
    .unwrap();
    assert!(!got.contains("timeout: 2000"), "a fixed two seconds per call is left:\n{got}");
    for want in [
        "let lookEnd = 0;",
        "const left = () => Math.max(1, Math.min(2000, lookEnd - Date.now()));",
        "el.getAttribute('title', { timeout: left() })",
        "el.evaluate(readText, undefined, { timeout: left() })",
        "items.nth(i).evaluate(readText, undefined, { timeout: left() })",
        "e instanceof HTMLInputElement || e instanceof HTMLTextAreaElement, undefined, { timeout: left() })",
        "loc.evaluate(readBox, undefined, { timeout: left() })",
    ] {
        assert!(got.contains(want), "missing {want}\n{got}");
    }
}

/// The settling helper as the spec declares it, run in node: a look the
/// deadline cuts off leaves a held pass standing (as the app's own loop does),
/// nothing held reports why, and a call's time is what the check has left.
#[test]
fn the_exported_settle_keeps_a_held_pass_when_the_deadline_cuts_a_look_off() {
    let got = lf(&one_step(json!([{ "kind": "expect_order", "selector": { "css": "li" }, "texts": ["a", "b"] }])).run().unwrap());
    let from = got.find("    let lookEnd = 0;").expect("the helper declares lookEnd");
    let settle_at = got.find("    const settle = async (").unwrap();
    let end = settle_at + got[settle_at..].find("\n    };\n").unwrap() + "\n    };\n".len();
    let helper = &got[from..end];
    let source = format!(
        "{helper}\n{}",
        r#"const fs = require('node:fs');
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const main = async () => {
  const out: Record<string, unknown> = {};
  // Two passes, then a look that outlasts the deadline: the pass held stands.
  {
    let calls = 0;
    out.cutAfterPass = await settle(async () => {
      calls++;
      if (calls <= 2) return { read: '2' };
      await sleep(left() + 50);
      throw new Error('Timeout exceeded.\nCall log');
    }, 1500);
  }
  // The same look with nothing held: it is why.
  out.cutWithNothing = await settle(async () => {
    await sleep(left() + 50);
    throw new Error('Timeout exceeded.\nCall log');
  }, 400);
  // A look that throws well before the deadline starts the hold over.
  {
    let calls = 0;
    out.throwsEarly = await settle(async () => {
      calls++;
      if (calls === 2) throw new Error('detached');
      return { read: '2' };
    }, 3000);
  }
  // Each call's time is what is left, never more than two seconds, never nothing.
  {
    const budgets: number[] = [];
    await settle(async () => {
      budgets.push(left());
      return 'not yet';
    }, 700);
    out.budgets = budgets;
  }
  fs.writeFileSync(process.argv[2], JSON.stringify(out));
};
main().catch((e) => { console.error(e); process.exit(1); });
"#
    );
    let dir = tempfile::tempdir().unwrap();
    let (script, output) = (dir.path().join("settle.ts"), dir.path().join("out.json"));
    std::fs::write(&script, source).unwrap();
    let run = std::process::Command::new("node").arg("--no-warnings").arg(&script).arg(&output).output().expect("node is needed for this test");
    assert!(run.status.success(), "node failed: {}", String::from_utf8_lossy(&run.stderr));
    let out: Value = serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(out["cutAfterPass"], Value::Null, "{out}");
    assert_eq!(out["cutWithNothing"], json!("Timeout exceeded."), "{out}");
    assert_eq!(out["throwsEarly"], Value::Null, "{out}");
    let budgets = out["budgets"].as_array().unwrap();
    assert!(budgets.len() >= 2, "{out}");
    assert!(budgets.iter().all(|b| (1..=700).contains(&b.as_i64().unwrap())), "{out}");
    assert!(budgets[1].as_i64().unwrap() < budgets[0].as_i64().unwrap(), "{out}");
}

#[test]
fn an_order_check_translates_to_a_settled_read_of_every_item() {
    let got = one_step(json!([
        { "kind": "expect_order", "selector": { "role": "option" }, "texts": ["All", " Draft  ", "Published"] }
    ]))
    .run()
    .unwrap();
    for want in [
        "expect(await settle(async () => {",
        "const items = cur.getByRole('option').filter({ visible: true });",
        "if (n === 0) return 'is not on the page';",
        "if (n > 200) return `matched ${n} items, more than the 200 expect_order reads`;",
        "texts.push(await items.nth(i).evaluate(readText, undefined, { timeout: left() }));",
        "const order = ['All', 'Draft', 'Published'];",
        "const at = texts.findIndex((t, i) => i >= from && t.includes(want));",
        "from = at + 1;",
        // The texts read are what must hold still.
        "return { read: texts.join('\\u001f') };",
        "}, 10000)).toBeNull();",
    ] {
        assert!(got.contains(want), "missing {want}\n{got}");
    }
    // A failure names what was wanted, never what a field holds.
    assert!(!got.contains("JSON.stringify(texts"), "{got}");
    assert_eq!(got.matches("const settle = async (").count(), 1, "{got}");
    assert!(!got.contains("const saw = ") && !got.contains("const placed"), "{got}");
}

#[test]
fn a_position_check_translates_to_the_apps_own_boxes_and_rule() {
    let got = one_step(json!([
        { "kind": "expect_position", "selector": { "css": "#a" }, "relative_to": { "css": "#bar" }, "relation": "right_of" },
        { "kind": "expect_position", "selector": { "css": "#a" }, "relative_to": { "css": "#bar" }, "relation": "in_top_right", "timeout_ms": 3000 }
    ]))
    .run()
    .unwrap();
    for want in [
        "expect(await settle(() => placed(cur.locator('#a').filter({ visible: true }), cur.locator('#bar').filter({ visible: true }), 'right_of'), 10000)).toBeNull();",
        "'in_top_right'), 3000)).toBeNull();",
    ] {
        assert!(got.contains(want), "missing {want}\n{got}");
    }
    for once in ["const placed = async (", "const readBox = ", "const settle = async ("] {
        assert_eq!(got.matches(once).count(), 1, "{once}\n{got}");
    }
    // The box is the app's own measurement, frame offsets included.
    assert!(got.contains("const f = frame.getBoundingClientRect();"), "{got}");
    assert!(got.contains("(frame.clientLeft || 0) + (parseFloat(cs.paddingLeft) || 0)"), "{got}");
    assert!(!got.contains("const readText") && !got.contains("const saw"), "{got}");
}

#[test]
fn the_exported_position_rule_names_every_relation() {
    use v2_lib::browser::position::Relation;
    use v2_lib::pw_export::translate::PLACED_TS;
    assert!(PLACED_TS.contains("const t = 1;"), "{PLACED_TS}");
    for r in [
        Relation::LeftOf,
        Relation::RightOf,
        Relation::Above,
        Relation::Below,
        Relation::Inside,
        Relation::InTopLeft,
        Relation::InTopRight,
        Relation::InBottomLeft,
        Relation::InBottomRight,
    ] {
        let name = serde_json::to_value(r).unwrap();
        let want = format!("case '{}':", name.as_str().unwrap());
        assert!(PLACED_TS.contains(&want), "{want} missing from\n{PLACED_TS}");
    }
    // A box of no size is nowhere, and an element is exactly one.
    assert!(PLACED_TS.contains("box.w <= 0 || box.h <= 0"), "{PLACED_TS}");
    assert!(PLACED_TS.contains("n > 1"), "{PLACED_TS}");
}

#[test]
fn list_order_and_position_checks_are_no_longer_refused() {
    let s = script(json!([{ "step_number": 1, "actions": [
        { "kind": "expect_each", "selector": { "css": "li" }, "not_empty": true },
        { "kind": "expect_order", "selector": { "css": "li" }, "texts": ["a", "b"] },
        { "kind": "expect_position", "selector": { "css": "#a" }, "relative_to": { "css": "#b" }, "relation": "above" }
    ] }]));
    check(&s, &origins()).unwrap();
    // The checks that need a target still refuse an empty chain.
    let bad = script(json!([{ "step_number": 1, "actions": [
        { "kind": "expect_order", "selector": [], "texts": ["a", "b"] }
    ] }]));
    assert!(check(&bad, &origins()).is_err());
}

#[test]
fn a_case_that_needs_a_ruling_exports_as_fixme_with_its_reason() {
    let reason = "needs a ruling: step 2: the case says \"exactly 4 levels\" but the app shows \"6 levels\" (data) - a person rules on it in Auto Run";
    let fx = one_step(json!([{ "kind": "click", "selector": "#save" }]));
    let with = raw_spec(&RawSpecInput {
        script: &fx.script,
        md_path: "suites/sl/admin/m/f/test-cases/f.md".into(),
        feature_title: "F".into(),
        after_sign_in: &fx.signin,
        area_clicks: &fx.area,
        area_clicks_by_name: &fx.by_name,
        step_texts: &fx.texts,
        origins: &origins(),
        fixme: Some(reason.to_string()),
    })
    .unwrap();
    let lines: Vec<&str> = with.lines().collect();
    let at = lines.iter().position(|l| l.starts_with("  test(")).expect("the test line");
    assert_eq!(lines[at + 1], format!("    test.fixme(true, '{reason}');"), "{with}");
    assert!(with.contains("#save"), "the body is still written: {with}");
    let without = fx.run().unwrap();
    assert!(!without.contains("test.fixme"), "{without}");
}

/// A text check passes on the words as written in the page when only the
/// letter case differs from the drawn ones (`expect::text_as_case`); the
/// export carries the same rule, once, beside `readText`.
#[test]
fn expect_text_exports_the_as_written_rule() {
    use v2_lib::pw_export::translate::AS_CASE_TS;
    let got = one_step(json!([{ "kind": "expect_text", "selector": "#d", "equals": "Duration" }])).run().unwrap();
    assert!(got.contains("const readWritten = ("), "{got}");
    assert!(got.contains(AS_CASE_TS), "{got}");
    assert!(got.contains("asCase(await"), "{got}");
    assert!(got.contains(".toBe('Duration')"), "{got}");
    assert_eq!(got.matches("const readWritten = ").count(), 1, "{got}");
    assert_eq!(got.matches("const asCase = ").count(), 1, "{got}");
    assert!(got.find("const asCase = ").unwrap() < got.find("// 1.").unwrap(), "{got}");
    // A text check by pattern has no as-written rule.
    let got = one_step(json!([{ "kind": "expect_text", "selector": "#d", "matches": "^Dur" }])).run().unwrap();
    assert!(!got.contains("readWritten") && !got.contains("asCase"), "{got}");
    // Nor does a spec with no text check.
    let got = one_step(json!([{ "kind": "click", "selector": "#a" }])).run().unwrap();
    assert!(!got.contains("readWritten") && !got.contains("asCase"), "{got}");
}

#[test]
fn the_exported_written_reader_is_the_apps_own() {
    use v2_lib::browser::expect::WRITTEN_TEXT_JS;
    use v2_lib::pw_export::translate::read_written_ts;
    let ts = read_written_ts(0).join("\n");
    let back = ts
        .replacen("const readWritten = (e: Element) => (function (this: HTMLElement) {", "function() {", 1)
        .replacen("}).call(e as HTMLElement);", "}", 1);
    assert_eq!(back, WRITTEN_TEXT_JS.replace("\r\n", "\n"));
}

#[test]
fn the_address_checks_export_with_the_same_rules() {
    let got = one_step(json!([
        { "kind": "check_url", "equals": "/hr/cycles" },
        { "kind": "check_url", "equals": "https://app.example/hr/cycles?id=4#top", "timeout_ms": 4000 },
        { "kind": "remember_url", "as": "before" },
        { "kind": "check_url", "same_as": "before" }
    ]))
    .run()
    .unwrap();
    // A path compares the path and query; a full address keeps its origin
    // and is never turned into a path.
    assert!(got.contains("return u.pathname + u.search; }, { timeout: 10000 }).toBe('/hr/cycles');"), "{got}");
    assert!(got.contains("return u.origin + u.pathname + u.search + u.hash; }, { timeout: 4000 }).toBe('https://app.example/hr/cycles?id=4#top');"), "{got}");
    assert_eq!(got.matches("const notedUrls: Record<string, string> = {};").count(), 1, "{got}");
    assert!(got.contains("notedUrls['before'] = cur.url();"), "{got}");
    assert!(got.contains("await expect.poll(() => cur.url(), { timeout: 10000 }).toBe(notedUrls['before']);"), "{got}");
    // Only a script that notes an address carries the notebook.
    let plain = one_step(json!([{ "kind": "check_url", "contains": "/x" }])).run().unwrap();
    assert!(!plain.contains("notedUrls"), "{plain}");
}

/// The export reduces `cur.url()` the way its emitted script does (origin if
/// the rule names one, path, query, and the fragment if the rule names one)
/// and compares it with `UrlRule::expected`: that must say what the app's
/// `url_equals` says.
#[test]
fn the_exported_address_check_agrees_with_the_apps() {
    use v2_lib::browser::actions::{url_equals, url_rule};
    let exported = |href: &str, want: &str| {
        let (rule, seen) = (url_rule(want), url_rule(href));
        let reduced = format!(
            "{}{}{}",
            if rule.origin.is_some() { seen.origin.clone().unwrap_or_default() } else { String::new() },
            seen.path_query,
            if rule.fragment.is_some() { seen.fragment.clone().unwrap_or_default() } else { String::new() },
        );
        reduced == rule.expected()
    };
    for (href, want, agree) in [
        ("https://app.example/", "https://app.example", true),
        ("https://APP.example:443/hr", "https://app.example/hr", true),
        ("https://app.example/hr?id=4#frag", "  /hr?id=4  ", true),
        ("https://app.example/hr", "https://other.example/hr", false),
        ("http://app.example/hr", "https://app.example/hr", false),
        ("https://app.example/hr?id=5", "/hr?id=4", false),
        ("https://app.example/hr#/a", "/hr#/b", false),
    ] {
        assert_eq!(url_equals(href, want), agree, "app: {href} vs {want:?}");
        assert_eq!(exported(href, want), agree, "export: {href} vs {want:?}");
    }
    // The export keeps a full address as a full address.
    let got = one_step(json!([{ "kind": "check_url", "equals": "https://app.example" }])).run().unwrap();
    assert!(got.contains("return u.origin + u.pathname + u.search; }") && got.contains(".toBe('https://app.example/');"), "{got}");
    let got = one_step(json!([{ "kind": "check_url", "equals": " /hr/cycles " }])).run().unwrap();
    assert!(got.contains(".toBe('/hr/cycles');"), "{got}");
}

#[test]
fn an_exact_address_off_the_projects_origins_cannot_be_exported() {
    let err = one_step(json!([{ "kind": "check_url", "equals": "https://elsewhere.example/x" }])).run().unwrap_err();
    assert!(err.contains("check_url"), "{err}");
}


#[test]
fn page_until_exports_the_same_walk() {
    let got = one_step(json!([
        { "kind": "page_until", "next": { "role": "button", "name": "Next" },
          "until_all": [{ "text": "Published" }, [{ "css": ".card" }, { "text": "Unpublished" }]], "max_pages": 40 }
    ]))
    .run()
    .unwrap();
    for want in [
        "const until = [",
        "const names = [",
        "for (let p = 1; ; p++) {",
        "if (p >= 40) throw new Error('walked '",
        "(await l.count()) > 0",
        "isEnabled()",
        "isVisible()",
        "classList.contains('disabled')",
        "const before = await signature();",
        "await next.click();",
        "await expect.poll(signature, { timeout: 15000 }).not.toBe(before)",
        "Next did not turn the page (page ' + p + ').",
    ] {
        assert!(got.contains(want), "missing {want}\n{got}");
    }
    assert!(!got.contains("waitForTimeout"), "a fixed wait is left:\n{got}");
    assert!(!got.contains("networkidle"), "the page turn waits on networkidle, which an in-page click has already reached:\n{got}");
    // One `until` is one target; a chain is a list of locator objects.
    let one = one_step(json!([
        { "kind": "page_until", "next": "#n", "until": [{ "css": ".card" }, { "text": "Unpublished" }], "max_pages": 3 }
    ]))
    .run()
    .unwrap();
    assert!(one.contains("const until = [cur.locator('.card').filter({ visible: true }).getByText('Unpublished').filter({ visible: true })];"), "{one}");
}

#[test]
fn each_unchecked_expectation_is_a_todo_line() {
    let mut fx = Fx::new(script(json!([
        { "step_number": 1, "actions": [], "unchecked": [
            { "item": "no lock icon", "why": "no check reads an icon" },
            { "item": "no access-denied message", "why": "no check reads words the app never shows (backlog 37)", "stand_in": "expect_hidden" }
        ] },
        { "step_number": 2, "actions": [] }
    ])));
    fx.texts.insert(1, "first".into());
    let got = fx.run().unwrap();
    assert!(
        got.contains(
            "    // 1. first\n    // TODO unchecked: no lock icon - no check reads an icon\n    // TODO unchecked: no access-denied message - no check reads words the app never shows (backlog 37) (stand-in: expect_hidden)\n"
        ),
        "{got}"
    );
    assert!(!got.contains("Not checked"), "{got}");
}

#[test]
fn the_todo_lines_are_the_cards_expectations() {
    use v2_lib::autorun::unchecked::{expectations, todo_line};
    #[derive(serde::Deserialize)]
    struct Entry {
        name: String,
        script: v2_lib::autorun::CaseScript,
    }
    let entries: Vec<Entry> = serde_json::from_str(include_str!("../fixtures/unchecked_expectations.json")).unwrap();
    for e in entries {
        let want: Vec<String> = expectations(&e.script).iter().map(todo_line).collect();
        let spec = Fx::new(e.script.clone()).run().unwrap();
        let got: Vec<String> = spec
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("// TODO unchecked:"))
            .map(str::to_string)
            .collect();
        assert_eq!(got, want, "{}", e.name);
    }
}
