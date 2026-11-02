//! `POSTBACK_FN` - the in-page JavaScript of a WebForms postback step - run
//! in a REAL browser against a fake WebForms site. Ignored by default: they
//! start (headless) Edge. Run them on purpose:
//!
//!   cargo test --test suite api_postback_live:: -- --ignored --test-threads=1
//!
//! One at a time, because each starts its own browser. The module filter
//! matters: without it `--ignored` also runs the other ignored tests in the
//! suite, and one of those needs the network.
//!
//! There is no JavaScript engine in the crate, and the runner tests answer
//! from a fake driver written from the same beliefs as the code, so this is
//! the only place the function is ever executed. Every assertion reads what
//! the SERVER received (the fake site records each POST it decodes) or what
//! the function handed back, never what Rust believes it sent. The site
//! refuses a post whose `__VIEWSTATE` is not the last one it issued, as a
//! real WebForms application does.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use v2_lib::api_templates::exec::FormFile;
use v2_lib::api_templates::postback::{
    page_key, read_reply, Answered, CarryMode, HtmlCapture, PostbackRequest, Refusal, Reply, CARRY_LIMIT_BYTES,
    POSTBACK_FN,
};
use v2_lib::api_templates::runner::ANSWER_LIMIT_CHARS;
use v2_lib::api_templates::RowCheck;
use v2_lib::browser::actions::{execute_with, Action};
use v2_lib::browser::cdp::Cdp;
use v2_lib::browser::launch::{launch_with, Browser, LaunchedBrowser};
use v2_lib::browser::page;
use v2_lib::browser::timing::Timing;

// ---------------------------------------------------------------- the browser

/// Windows holds a just-exited browser's profile files open for a moment.
fn remove_profile_dir(dir: &Path) {
    for _ in 0..20 {
        if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The registry of live browser trees (`serial::held_browsers`), held for
/// as long as a test's browser lives, so another module's exit path can
/// never end it mid-test (the pattern of `browser_live.rs`).
struct TreesHeld(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

struct Live {
    browser: LaunchedBrowser,
    cdp: Cdp,
    _trees: TreesHeld,
}

impl Drop for Live {
    fn drop(&mut self) {
        self.browser.end();
        remove_profile_dir(&self.browser.profile_dir);
    }
}

fn timing() -> Timing {
    Timing { action_ms: 4000, expect_ms: 3000, nav_ms: 15000, poll_ms: 100, highlight_ms: 0, lease_wait_ms: 300 }
}

/// A browser on the fake site's start page (so every address the function
/// fetches is same-origin), and the site's state to read back.
async fn open(port: u16) -> Live {
    let trees = TreesHeld(crate::serial::held_browsers());
    let mut browser = launch_with(Browser::Edge, &["--headless=new"]).expect("Edge did not start");
    let mut last = String::new();
    let mut connected = None;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        match Cdp::connect(browser.port).await {
            Ok(c) => {
                connected = Some(c);
                break;
            }
            Err(e) => last = e,
        }
    }
    let Some(cdp) = connected else {
        let p = browser.port;
        browser.end();
        let _ = std::fs::remove_dir_all(&browser.profile_dir);
        panic!("could not reach Edge on port {p}: {last}");
    };
    let mut live = Live { browser, cdp, _trees: trees };
    let go: Action = serde_json::from_value(json!({ "kind": "navigate", "url": format!("http://127.0.0.1:{port}/start") }))
        .expect("a bad action");
    let out = execute_with(&mut live.cdp, &go, &timing()).await;
    assert!(out.ok, "the start page did not load: {}", out.detail);
    live
}

fn req(url: &str, button: &str) -> PostbackRequest {
    PostbackRequest {
        url: url.to_string(),
        key: page_key(url),
        button: button.to_string(),
        argument: String::new(),
        fields: BTreeMap::new(),
        files: Vec::new(),
        shows: Vec::new(),
        hides: Vec::new(),
        rows: None,
        captures: BTreeMap::new(),
    }
}

fn with_fields(mut r: PostbackRequest, fields: &[(&str, &str)]) -> PostbackRequest {
    for (k, v) in fields {
        r.fields.insert(k.to_string(), v.to_string());
    }
    r
}

fn capture(selector: &str, attr: Option<&str>) -> HtmlCapture {
    HtmlCapture { selector: selector.to_string(), attr: attr.map(str::to_string) }
}

/// Calls the function in the page and reads its reply. No reply, from any
/// test, may carry a page-state value: the fake site marks every one it
/// issues with `SECRET`.
async fn send_with(
    live: &mut Live,
    r: &PostbackRequest,
    run: &str,
    carry: CarryMode,
    limit_ms: u64,
    carry_limit: u64,
) -> Reply {
    let doc = page::document(&mut live.cdp).await.expect("no document");
    let args = [
        serde_json::to_value(r).unwrap(),
        json!(limit_ms),
        json!(run),
        serde_json::to_value(carry).unwrap(),
        json!(carry_limit),
        json!(ANSWER_LIMIT_CHARS),
    ];
    let v = page::call_value_within(&mut live.cdp, &doc, POSTBACK_FN, &args, Duration::from_secs(40))
        .await
        .expect("the function threw");
    let raw = v.to_string();
    assert!(!raw.contains("SECRET"), "a page-state value left the page: {raw}");
    read_reply(&v).unwrap_or_else(|| panic!("not a reply the runner reads: {raw}"))
}

async fn send(live: &mut Live, r: &PostbackRequest, run: &str, carry: CarryMode) -> Reply {
    send_with(live, r, run, carry, 15_000, CARRY_LIMIT_BYTES).await
}

fn answered(r: Reply) -> Answered {
    match r {
        Reply::Answered(a) => a,
        other => panic!("expected an answer, got {other:?}"),
    }
}

fn refused(r: Reply) -> (Refusal, Option<u16>, Option<usize>) {
    match r {
        Reply::Refused { why, status, count } => (why, status, count),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// ------------------------------------------------------------------ the site

#[derive(Default)]
struct State {
    /// The number of the last view state issued by the evaluation page.
    issued: u64,
    saved_id: String,
    /// The grid's rows: `None` is a row just created, `Some` one saved.
    rows: Vec<Option<String>>,
    posts: Vec<Posted>,
    gets: Vec<String>,
    sign_out: bool,
    summary_shown: bool,
}

#[derive(Clone, Debug)]
struct Posted {
    path: String,
    content_type: String,
    fields: Vec<(String, String)>,
    raw: String,
}

impl Posted {
    fn get(&self, name: &str) -> Option<&str> {
        self.fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    fn all(&self, name: &str) -> Vec<&str> {
        self.fields.iter().filter(|(k, _)| k == name).map(|(_, v)| v.as_str()).collect()
    }
}

type Shared = Arc<Mutex<State>>;

struct Request {
    method: String,
    target: String,
    content_type: String,
    body: Vec<u8>,
}

struct Out {
    status: u16,
    location: Option<String>,
    body: String,
    delay_ms: u64,
}

fn ok(body: String) -> Out {
    Out { status: 200, location: None, body, delay_ms: 0 }
}

fn redirect(to: &str) -> Out {
    Out { status: 302, location: Some(to.to_string()), body: String::new(), delay_ms: 0 }
}

/// ASP.NET's yellow screen: a 500 with the "Server Error" heading.
fn yellow(title: &str, detail: &str) -> Out {
    Out {
        status: 500,
        location: None,
        body: format!(
            "<!DOCTYPE html><html><head><title>{title}</title></head><body><h1>Server Error in '/' Application.</h1><h2>{detail}</h2></body></html>"
        ),
        delay_ms: 0,
    }
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(n) => {
                    out.push(n);
                    i += 2;
                }
                Err(_) => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn pairs(body: &str) -> Vec<(String, String)> {
    body.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];
    let end = loop {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&buf[..n]);
        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&data[..end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_string();
    let target = first.next()?.to_string();
    let mut length = 0usize;
    let mut content_type = String::new();
    for l in lines {
        let lower = l.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0);
        } else if lower.starts_with("content-type:") {
            content_type = l["content-type:".len()..].trim().to_string();
        }
    }
    let mut body = data[end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    Some(Request { method, target, content_type, body })
}

fn write_out(stream: &mut TcpStream, out: &Out) {
    let reason = match out.status {
        200 => "OK",
        302 => "Found",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let location = out.location.as_ref().map(|l| format!("Location: {l}\r\n")).unwrap_or_default();
    let _ = write!(
        stream,
        "HTTP/1.1 {} {reason}\r\n{location}Content-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        out.status,
        out.body.len(),
        out.body
    );
}

/// A small WebForms page: view state, a text box and one submit button
/// posting to `action`.
fn simple_page(action: &str, padding: usize) -> String {
    format!(
        r#"<!doctype html><html><head><title>Page</title></head><body><form method="post" action="{action}">
<input type="hidden" name="__VIEWSTATE" id="__VIEWSTATE" value="VSTATE-S-SECRET" />
<input type="hidden" name="__EVENTVALIDATION" id="__EVENTVALIDATION" value="EVAL-S-SECRET" />
<input type="text" name="txtA" value="a" /><input type="submit" name="CmdGo" value="Go" />
</form><p>{}</p></body></html>"#,
        "x".repeat(padding)
    )
}

/// The evaluation page: every kind of control a WebForms page has, and a
/// view state that is new on each render.
fn eval_page(st: &mut State, query: &str) -> String {
    st.issued += 1;
    let n = st.issued;
    let id = pairs(query).into_iter().find(|(k, _)| k == "id").map(|(_, v)| v).unwrap_or_default();
    let q = if id.is_empty() { String::new() } else { format!("?id={id}") };
    let message = if id.is_empty() { "" } else { "Saved" };
    let summary = if st.summary_shown {
        r#"<div id="ValidationSummary1" style="color:red">Name is required</div>"#
    } else {
        r#"<div id="ValidationSummary1" style="display:none">Name is required</div>"#
    };
    let rows: String = st
        .rows
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let label = match v {
                Some(v) => format!("Item {}: {v}", i + 1),
                None => format!("Item {}", i + 1),
            };
            format!(
                r#"<tr><td>{label}</td><td><input type="text" name="txtItem${i}" value="{}" /></td></tr>"#,
                v.clone().unwrap_or_default()
            )
        })
        .collect();
    let saved_id = &st.saved_id;
    format!(
        r#"<!doctype html><html><head><title>Evaluation</title></head><body>
<form method="post" action="/Perf/Eval.aspx{q}" id="form1">
<input type="hidden" name="__EVENTTARGET" id="__EVENTTARGET" value="" />
<input type="hidden" name="__EVENTARGUMENT" id="__EVENTARGUMENT" value="" />
<input type="hidden" name="__VIEWSTATE" id="__VIEWSTATE" value="VSTATE-{n}-SECRET" />
<input type="hidden" name="__VIEWSTATEGENERATOR" id="__VIEWSTATEGENERATOR" value="GEN-SECRET" />
<input type="hidden" name="__EVENTVALIDATION" id="__EVENTVALIDATION" value="EVAL-{n}-SECRET" />
<span id="msg">{message}</span>{summary}
<input type="hidden" name="hdnEvalId" id="hdnEvalId" value="{id}" data-saved="{saved_id}" />
<input type="hidden" name="hdnEmpty" id="hdnEmpty" value="" />
<input type="text" name="txtName" id="txtName" value="old" />
<input type="text" name="txtLocked" value="locked" disabled="disabled" />
<input type="checkbox" name="chkGoal$0" checked="checked" />
<input type="checkbox" name="chkOld$0" checked="checked" />
<input type="checkbox" name="chkOff$0" />
<input type="radio" name="rbl" value="1" /><input type="radio" name="rbl" value="2" checked="checked" />
<select name="ddlType" id="ddlType"><option value="1">One</option><option value="2" selected="selected">Two</option></select>
<select name="ddlNone"><option value="x">X</option><option value="y">Y</option></select>
<select name="lstMulti" multiple="multiple"><option value="a">A</option><option value="b" selected="selected">B</option><option value="c" selected="selected">C</option></select>
<textarea name="txtNotes">note text</textarea>
<input type="submit" name="ctl00$Main$CmdSave" value="Save" />
<input type="submit" name="ctl00$Main$CmdSaveItem" value="Save Item" />
<input type="submit" name="ctl00$Main$CmdForbidden" value="Forbidden" />
<input type="submit" name="ctl00$Left$Dup" value="Dup" /><input type="submit" name="ctl00$Right$Dup" value="Dup" />
<a id="lnkCreate" href="javascript:__doPostBack('ctl00$Main$lnkCreate','')">Create</a>
<table id="gvItems"><tr><th>Name</th><th>Edit</th></tr>{rows}</table>
</form></body></html>"#
    )
}

fn eval_post(st: &mut State, query: &str) -> Out {
    if st.sign_out {
        return redirect("/Login.aspx");
    }
    let f = st.posts.last().expect("the post was just recorded").fields.clone();
    let get = |n: &str| f.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()).unwrap_or_default();
    if get("__VIEWSTATE") != format!("VSTATE-{}-SECRET", st.issued)
        || get("__EVENTVALIDATION") != format!("EVAL-{}-SECRET", st.issued)
    {
        return yellow("Validation of viewstate MAC failed", "The view state was not the last one issued.");
    }
    let mut action = get("__EVENTTARGET");
    if action.is_empty() {
        for name in ["ctl00$Main$CmdSave", "ctl00$Main$CmdSaveItem", "ctl00$Main$CmdForbidden"] {
            if f.iter().any(|(k, _)| k == name) {
                action = name.to_string();
            }
        }
    }
    match action.as_str() {
        "ctl00$Main$CmdSave" => {
            st.saved_id = "7".to_string();
            redirect("/Perf/Eval.aspx?id=7")
        }
        "ctl00$Main$lnkCreate" => {
            st.rows.push(None);
            ok(eval_page(st, query))
        }
        "ctl00$Main$CmdSaveItem" => {
            for i in 0..st.rows.len() {
                st.rows[i] = Some(get(&format!("txtItem${i}")));
            }
            ok(eval_page(st, query))
        }
        other => yellow(
            "Invalid postback or callback argument.  Event validation is enabled using pages enableEventValidation",
            &format!("Invalid postback or callback argument for {other}"),
        ),
    }
}

fn route(state: &Shared, rq: &Request) -> Out {
    let (path, query) = rq.target.split_once('?').unwrap_or((rq.target.as_str(), ""));
    let lower = path.to_ascii_lowercase();
    let p = lower.trim_end_matches('/');
    let post = rq.method == "POST";
    let mut st = state.lock().unwrap();
    if post {
        let urlencoded = rq.content_type.starts_with("application/x-www-form-urlencoded");
        let raw = String::from_utf8_lossy(&rq.body).into_owned();
        st.posts.push(Posted {
            path: p.to_string(),
            content_type: rq.content_type.clone(),
            fields: if urlencoded { pairs(&raw) } else { Vec::new() },
            raw,
        });
    } else {
        st.gets.push(p.to_string());
    }
    match p {
        "" | "/start" => ok("<!doctype html><html><body><p>start</p></body></html>".to_string()),
        "/plain.aspx" => ok(r#"<html><body><form><input name="a" /></form></body></html>"#.to_string()),
        "/login.aspx" => ok(r#"<html><body><form><input type="password" name="pwd" /></form></body></html>"#.to_string()),
        "/perf/eval.aspx" if post => eval_post(&mut st, query),
        "/perf/eval.aspx" => ok(eval_page(&mut st, query)),
        "/big.aspx" => ok(simple_page("/Big.aspx", 5000)),
        "/grow.aspx" if post => ok(simple_page("/Grow.aspx", 6000)),
        "/grow.aspx" => ok(simple_page("/Grow.aspx", 0)),
        "/delta.aspx" if post => ok("1|#||4|120|updatePanel|up1|<div>ok</div>|0|hiddenField|__VIEWSTATE|DELTA-SECRET|".to_string()),
        "/delta.aspx" => ok(simple_page("/Delta.aspx", 0)),
        "/boom.aspx" if post => Out {
            status: 500,
            location: None,
            body: "<html><head><title>Runtime Error</title></head><body>boom</body></html>".to_string(),
            delay_ms: 0,
        },
        "/boom.aspx" => ok(simple_page("/Boom.aspx", 0)),
        "/slow.aspx" if post => Out { status: 200, location: None, body: simple_page("/Slow.aspx", 0), delay_ms: 2500 },
        "/slow.aspx" => ok(simple_page("/Slow.aspx", 0)),
        "/files.aspx" if post => ok(simple_page("/Files.aspx", 0)),
        "/files.aspx" => ok(simple_page("/Files.aspx", 0)),
        "/redir.aspx" if post => redirect("/"),
        "/redir.aspx" => ok(simple_page("/Redir.aspx", 0)),
        _ => Out { status: 404, location: None, body: "not found".to_string(), delay_ms: 0 },
    }
}

/// The fake site, on a free port. Its threads end with the test process.
fn serve() -> (u16, Shared) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("no free port");
    let port = listener.local_addr().unwrap().port();
    let state: Shared = Arc::default();
    let shared = state.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let shared = shared.clone();
            std::thread::spawn(move || {
                let Some(rq) = read_request(&mut stream) else { return };
                let out = route(&shared, &rq);
                if out.delay_ms > 0 {
                    std::thread::sleep(Duration::from_millis(out.delay_ms));
                }
                write_out(&mut stream, &out);
            });
        }
    });
    (port, state)
}

fn last_post(state: &Shared) -> Posted {
    state.lock().unwrap().posts.last().cloned().expect("the site received no POST")
}

fn post_count(state: &Shared) -> usize {
    state.lock().unwrap().posts.len()
}

const EVAL: &str = "/Perf/Eval.aspx";

// --------------------------------------------------------------------- tests

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_postback_sends_what_the_browser_would_and_carries_the_answer() {
    let (port, state) = serve();
    let mut live = open(port).await;
    let mut r = with_fields(
        req(EVAL, "CmdSave"),
        &[("txtName", "Quarterly"), ("chkOld$0", ""), ("chkOff$0", "on"), ("txtGhost", "boo")],
    );
    r.captures.insert("evalId".into(), capture("#hdnEvalId", Some("value")));
    let a = answered(send(&mut live, &r, "run-1", CarryMode::Fresh).await);

    let p = last_post(&state);
    assert_eq!(p.path, "/perf/eval.aspx");
    assert!(p.content_type.starts_with("application/x-www-form-urlencoded"), "{}", p.content_type);
    assert_eq!(p.get("__VIEWSTATE"), Some("VSTATE-1-SECRET"));
    assert_eq!(p.get("__EVENTVALIDATION"), Some("EVAL-1-SECRET"));
    assert_eq!(p.get("__VIEWSTATEGENERATOR"), Some("GEN-SECRET"));
    assert_eq!(p.get("__EVENTTARGET"), Some(""), "a submit button posts an empty target");
    assert_eq!(p.get("__EVENTARGUMENT"), Some(""));
    assert_eq!(p.get("ctl00$Main$CmdSave"), Some("Save"), "the clicked submit posts name=value");
    assert_eq!(p.get("ctl00$Main$CmdSaveItem"), None, "another submit button is not sent");
    assert_eq!(p.get("txtName"), Some("Quarterly"), "the template replaces the page's value");
    assert_eq!(p.get("chkGoal$0"), Some("on"), "a checked box is sent");
    assert_eq!(p.get("chkOld$0"), None, "an empty template value unchecks");
    assert_eq!(p.get("chkOff$0"), Some("on"), "a template value checks");
    assert_eq!(p.get("txtLocked"), None, "a disabled control is not sent");
    assert_eq!(p.get("rbl"), Some("2"));
    assert_eq!(p.get("ddlType"), Some("2"), "the selected option");
    assert_eq!(p.get("ddlNone"), Some("x"), "a select with none selected sends its first option");
    assert_eq!(p.all("lstMulti"), vec!["b", "c"], "a multiple select sends each selected option");
    assert_eq!(p.get("txtNotes"), Some("note text"));
    assert_eq!(p.get("hdnEmpty"), Some(""), "an empty hidden input is sent empty");
    assert_eq!(p.get("txtGhost"), Some("boo"), "a field the page lacks is still sent");

    assert_eq!(a.status, 200);
    assert!(!a.carried, "the first postback loads its page");
    assert!(a.redirected);
    assert_eq!(a.final_key, "/perf/eval.aspx");
    assert_eq!(a.unknown_fields, vec!["txtGhost".to_string()]);
    assert_eq!(a.captures["evalId"].value.as_deref(), Some("7"), "read from the page the redirect landed on");
    assert!(a.state_names.contains(&"__VIEWSTATE".to_string()));
    assert!(a.state_names.contains(&"__EVENTVALIDATION".to_string()));
    assert!(a.page_names.contains(&"txtName".to_string()));
    assert!(!a.page_names.iter().any(|n| n.starts_with("__")));
    assert!(!a.state_names.contains(&"txtName".to_string()));
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_link_button_posts_its_dopostback_target() {
    let (port, state) = serve();
    let mut live = open(port).await;
    answered(send(&mut live, &req(EVAL, "CmdSave"), "run-link", CarryMode::Fresh).await);

    let mut create = req("/Perf/Eval.aspx?id=7", "lnkCreate");
    create.argument = "arg".into();
    let a = answered(send(&mut live, &create, "run-link", CarryMode::Must).await);
    let p = last_post(&state);
    assert_eq!(p.get("__EVENTTARGET"), Some("ctl00$Main$lnkCreate"), "the full target, not the template's short name");
    assert_eq!(p.get("__EVENTARGUMENT"), Some("arg"));
    assert_eq!(p.get("__VIEWSTATE"), Some("VSTATE-2-SECRET"), "the state the redirect's page carried");
    assert_eq!(p.get("ctl00$Main$CmdSave"), None, "no submit button is sent for a link");
    assert!(a.carried);
    assert_eq!(a.status, 200);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn three_round_trips_chain_their_state() {
    let (port, state) = serve();
    let mut live = open(port).await;
    answered(send(&mut live, &req(EVAL, "CmdSave"), "run-3", CarryMode::Fresh).await);
    answered(send(&mut live, &req("/Perf/Eval.aspx?id=7", "lnkCreate"), "run-3", CarryMode::Must).await);

    let mut item = with_fields(req("/Perf/Eval.aspx?id=7", "CmdSaveItem"), &[("txtItem$0", "alpha")]);
    item.rows = Some(RowCheck { table: "#gvItems".into(), has: "Item 1: alpha".into() });
    let a = answered(send(&mut live, &item, "run-3", CarryMode::Must).await);
    assert!(a.carried);
    assert_eq!(a.checks.len(), 1);
    assert!(a.checks[0].ok && a.checks[0].kind == "rows", "{:?}", a.checks);
    let p = last_post(&state);
    assert_eq!(p.get("txtItem$0"), Some("alpha"));
    assert_eq!(p.get("__VIEWSTATE"), Some("VSTATE-3-SECRET"), "each step posts the state the step before answered with");

    // A row the grid does not have, and a grid that is not one table.
    let mut missing = req("/Perf/Eval.aspx?id=7", "CmdSaveItem");
    missing.rows = Some(RowCheck { table: "#gvItems".into(), has: "Item 9".into() });
    let a = answered(send(&mut live, &missing, "run-3", CarryMode::Must).await);
    assert!(!a.checks[0].ok && a.checks[0].kind == "rows", "{:?}", a.checks);
    let mut none = req("/Perf/Eval.aspx?id=7", "CmdSaveItem");
    none.rows = Some(RowCheck { table: "#nope".into(), has: "x".into() });
    let a = answered(send(&mut live, &none, "run-3", CarryMode::Must).await);
    assert!(!a.checks[0].ok && a.checks[0].kind == "table" && a.checks[0].count == 0, "{:?}", a.checks);
    let mut many = req("/Perf/Eval.aspx?id=7", "CmdSaveItem");
    many.rows = Some(RowCheck { table: "tr".into(), has: "x".into() });
    let a = answered(send(&mut live, &many, "run-3", CarryMode::Must).await);
    assert!(!a.checks[0].ok && a.checks[0].kind == "table" && a.checks[0].count > 1, "{:?}", a.checks);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_hidden_validation_summary_passes_hides() {
    let (port, state) = serve();
    let mut live = open(port).await;
    let mut r = req(EVAL, "CmdSave");
    r.shows = vec!["Saved".into(), "#msg".into(), "Name is required".into(), "#ValidationSummary1".into()];
    r.hides = vec!["#ValidationSummary1".into(), "Name is required".into()];
    let a = answered(send(&mut live, &r, "run-v", CarryMode::Fresh).await);
    let by = |kind: &str, what: &str| a.checks.iter().find(|c| c.kind == kind && c.what == what).unwrap().ok;
    assert!(by("shows", "Saved"));
    assert!(by("shows", "#msg"));
    assert!(!by("shows", "Name is required"), "text in a hidden element is not shown");
    assert!(!by("shows", "#ValidationSummary1"), "a hidden element is not a visible match");
    assert!(by("hides", "#ValidationSummary1"), "an element with display:none passes hides");
    assert!(by("hides", "Name is required"));

    state.lock().unwrap().summary_shown = true;
    let a = answered(send(&mut live, &r, "run-v2", CarryMode::Fresh).await);
    let by = |kind: &str, what: &str| a.checks.iter().find(|c| c.kind == kind && c.what == what).unwrap().ok;
    assert!(!by("hides", "#ValidationSummary1"), "a shown summary fails hides");
    assert!(!by("hides", "Name is required"));
    assert!(by("shows", "Name is required"));
    assert!(by("shows", "#ValidationSummary1"));
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn page_state_never_comes_back() {
    let (port, _state) = serve();
    let mut live = open(port).await;
    let mut r = req(EVAL, "CmdSave");
    r.captures.insert("vs".into(), capture("#__VIEWSTATE", Some("value")));
    r.captures.insert("vsText".into(), capture("input[name=__EVENTVALIDATION]", None));
    r.captures.insert("gen".into(), capture("[name=__VIEWSTATEGENERATOR]", Some("value")));
    // `send` fails on any reply holding a `SECRET`; the excerpt shows the
    // answer with its state taken out and the rest of it intact.
    let a = answered(send(&mut live, &r, "run-s", CarryMode::Fresh).await);
    assert!(a.excerpt.contains(r#"name="__VIEWSTATE""#), "{}", a.excerpt);
    assert!(a.excerpt.contains(r#"value="(page state)""#));
    assert!(a.excerpt.contains(r#"value="old""#), "an ordinary input keeps its value: {}", a.excerpt);
    assert!(a.excerpt.contains("Saved"));
    for name in ["vs", "vsText", "gen"] {
        let c = &a.captures[name];
        assert!(c.state, "{name} is page state");
        assert_eq!(c.value, None, "{name} must carry no value");
        assert_eq!(c.count, 1);
    }
    assert_eq!(a.state_names.iter().filter(|n| n.as_str() == "__VIEWSTATE").count(), 1);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn captures_say_absent_and_empty_apart() {
    let (port, _state) = serve();
    let mut live = open(port).await;
    let mut r = req(EVAL, "CmdSave");
    for (name, sel, attr) in [
        ("evalId", "#hdnEvalId", Some("value")),
        ("empty", "#hdnEmpty", Some("value")),
        ("absent", "#hdnEvalId", Some("data-nope")),
        ("text", "#msg", None),
        ("emptyText", "#ValidationSummary1 + input", None),
        ("select", "#ddlType", Some("value")),
        ("two", "input[type=checkbox]", Some("name")),
        ("zero", "#nothing", None),
        ("bad", "[[", None),
    ] {
        r.captures.insert(name.into(), capture(sel, attr));
    }
    let a = answered(send(&mut live, &r, "run-c", CarryMode::Fresh).await);
    let c = &a.captures;
    assert_eq!((c["evalId"].count, c["evalId"].value.as_deref()), (1, Some("7")));
    assert_eq!((c["empty"].count, c["empty"].value.as_deref()), (1, Some("")), "an empty attribute is \"\"");
    assert_eq!((c["absent"].count, c["absent"].value.as_deref()), (1, None), "an absent attribute is null");
    assert_eq!(c["text"].value.as_deref(), Some("Saved"));
    assert_eq!(c["emptyText"].value.as_deref(), Some(""), "an element's empty text is \"\" and not null");
    assert_eq!(c["select"].value.as_deref(), Some("2"), "@value on a select is the selected option's value");
    assert_eq!(c["two"].count, 3);
    assert_eq!(c["two"].value, None, "no value comes back for more than one match");
    assert_eq!((c["zero"].count, c["zero"].value.clone()), (0, None));
    assert!(c["bad"].bad_selector);
    assert!(!c["evalId"].state && !c["evalId"].bad_selector);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_new_run_carries_nothing() {
    let (port, state) = serve();
    let mut live = open(port).await;
    answered(send(&mut live, &req(EVAL, "CmdSave"), "run-A", CarryMode::Fresh).await);
    let posts = post_count(&state);

    // The browser is still on the page with run A's map: run B carries none of it.
    let (why, _, _) = refused(send(&mut live, &req(EVAL, "lnkCreate"), "run-B", CarryMode::Must).await);
    assert_eq!(why, Refusal::Lost);
    assert_eq!(post_count(&state), posts, "nothing was sent");
    // ... and run B replaced run A's map, so run A cannot carry either.
    let (why, _, _) = refused(send(&mut live, &req(EVAL, "lnkCreate"), "run-A", CarryMode::Must).await);
    assert_eq!(why, Refusal::Lost);

    // A one-off call with nothing held loads the page; with a page held it carries on.
    let first = answered(send(&mut live, &req(EVAL, "CmdSave"), "one-off", CarryMode::IfThere).await);
    assert!(!first.carried, "nothing was held, so the page was loaded");
    let second = answered(send(&mut live, &req(EVAL, "lnkCreate"), "one-off", CarryMode::IfThere).await);
    assert!(second.carried, "the page the call before it answered with was held");
    let fresh = answered(send(&mut live, &req(EVAL, "CmdSave"), "one-off", CarryMode::Fresh).await);
    assert!(!fresh.carried, "fresh loads the page again");
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn the_carried_key_is_normalised_the_way_rust_does() {
    let (port, _state) = serve();
    let mut live = open(port).await;

    // Upper case and a trailing slash on the step's page, a different spelling on the wire.
    let first = req("/PERF/Eval.ASPX/", "CmdSave");
    assert_eq!(first.key, "/perf/eval.aspx");
    let a = answered(send(&mut live, &first, "run-k", CarryMode::Fresh).await);
    assert_eq!(a.final_key, page_key(&a.final_url), "the page's key is Rust's for the same address");
    assert_eq!(a.final_key, first.key);
    let next = req("/perf/eval.aspx/", "lnkCreate");
    assert_eq!(next.key, a.final_key);
    let b = answered(send(&mut live, &next, "run-k", CarryMode::Must).await);
    assert!(b.carried, "a step's key finds the page the answer before it was filed under");

    // An answer that lands on the site's root is filed under `/`.
    let r = answered(send(&mut live, &req("/Redir.aspx", "CmdGo"), "run-k", CarryMode::Fresh).await);
    assert_eq!(r.final_key, "/");
    assert_eq!(r.final_key, page_key(&r.final_url));
    assert!(r.redirected);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_page_that_is_not_webforms_sends_nothing() {
    let (port, state) = serve();
    let mut live = open(port).await;
    let (why, _, _) = refused(send(&mut live, &req("/Plain.aspx", "x"), "run-n", CarryMode::Fresh).await);
    assert_eq!(why, Refusal::NotWebforms);
    let (why, status, _) = refused(send(&mut live, &req("/Missing.aspx", "x"), "run-n", CarryMode::Fresh).await);
    assert_eq!((why, status), (Refusal::LoadFailed, Some(404)));
    assert_eq!(post_count(&state), 0, "nothing was posted");
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn an_unknown_button_sends_nothing() {
    let (port, state) = serve();
    let mut live = open(port).await;
    let (why, _, _) = refused(send(&mut live, &req(EVAL, "CmdNoSuch"), "run-b", CarryMode::Fresh).await);
    assert_eq!(why, Refusal::NoButton);
    let (why, _, count) = refused(send(&mut live, &req(EVAL, "Dup"), "run-b", CarryMode::Fresh).await);
    assert_eq!((why, count), (Refusal::ManyButtons, Some(2)));
    // Named in full, one of the pair is found.
    let a = answered(send(&mut live, &req(EVAL, "ctl00$Left$Dup"), "run-b", CarryMode::Fresh).await);
    assert_eq!(a.status, 500, "the site refuses a button it did not offer, but it was found and sent");
    assert_eq!(post_count(&state), 1, "only the named one was posted");
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn an_error_page_and_an_invalid_postback_are_named() {
    let (port, _state) = serve();
    let mut live = open(port).await;
    let a = answered(send(&mut live, &req(EVAL, "CmdForbidden"), "run-e", CarryMode::Fresh).await);
    assert_eq!(a.status, 500);
    assert!(a.invalid_postback);
    assert!(a.asp_error.is_some(), "{:?}", a.asp_error);
    assert_eq!(a.invalid_field.as_deref(), Some("ctl00$Main$CmdForbidden"));

    let b = answered(send(&mut live, &req("/Boom.aspx", "CmdGo"), "run-e", CarryMode::Fresh).await);
    assert_eq!(b.status, 500);
    assert_eq!(b.asp_error.as_deref(), Some("Runtime Error"));
    assert!(!b.invalid_postback);
    assert_eq!(b.invalid_field, None);

    let ok = answered(send(&mut live, &req(EVAL, "CmdSave"), "run-e", CarryMode::Fresh).await);
    assert_eq!(ok.asp_error, None);
    assert!(!ok.invalid_postback);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn the_sign_in_page_is_signed_out() {
    let (port, state) = serve();
    let mut live = open(port).await;
    state.lock().unwrap().sign_out = true;
    let a = answered(send(&mut live, &req(EVAL, "CmdSave"), "run-o", CarryMode::Fresh).await);
    assert!(a.signed_out, "the answer was the sign-in page");
    assert!(a.redirected);
    assert!(a.excerpt.is_empty());
    // Not carried: the sign-in page is not where the next step carries on from.
    let (why, _, _) = refused(send(&mut live, &req("/Login.aspx", "CmdSave"), "run-o", CarryMode::Must).await);
    assert_eq!(why, Refusal::Lost);

    // A page that is the sign-in page on the first load is refused before anything is sent.
    let posts = post_count(&state);
    let (why, _, _) = refused(send(&mut live, &req("/Login.aspx", "x"), "run-o", CarryMode::Fresh).await);
    assert_eq!(why, Refusal::SignedOut);
    assert_eq!(post_count(&state), posts);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_page_over_the_limit_is_not_carried() {
    let (port, state) = serve();
    let mut live = open(port).await;
    let (why, _, _) =
        refused(send_with(&mut live, &req("/Big.aspx", "CmdGo"), "run-l", CarryMode::Fresh, 15_000, 2000).await);
    assert_eq!(why, Refusal::TooLarge);
    assert_eq!(post_count(&state), 0);

    // A small page whose answer is large: sent, reported, and not kept.
    let a = answered(send_with(&mut live, &req("/Grow.aspx", "CmdGo"), "run-l", CarryMode::Fresh, 15_000, 3000).await);
    assert!(a.too_large);
    assert!(a.excerpt.is_empty());
    assert!(a.captures.is_empty() && a.checks.is_empty());
    let (why, _, _) =
        refused(send_with(&mut live, &req("/Grow.aspx", "CmdGo"), "run-l", CarryMode::Must, 15_000, 3000).await);
    assert_eq!(why, Refusal::Lost);
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn an_update_panel_answer_is_partial_and_its_state_stays_out() {
    let (port, _state) = serve();
    let mut live = open(port).await;
    // `send` fails the test if the delta's `__VIEWSTATE` value comes back anywhere.
    let a = answered(send(&mut live, &req("/Delta.aspx", "CmdGo"), "run-d", CarryMode::Fresh).await);
    assert!(a.partial);
    assert!(a.excerpt.is_empty(), "a partial answer's excerpt is left empty");
    assert!(a.checks.is_empty() && a.captures.is_empty());
    let (why, _, _) = refused(send(&mut live, &req("/Delta.aspx", "CmdGo"), "run-d", CarryMode::Must).await);
    assert_eq!(why, Refusal::Lost, "a delta is not a page to carry on from");
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_postback_with_files_posts_multipart() {
    let (port, state) = serve();
    let mut live = open(port).await;
    let mut r = with_fields(req("/Files.aspx", "CmdGo"), &[("txtA", "typed")]);
    r.files.push(FormFile {
        field: "upload".into(),
        name: "a.txt".into(),
        content_type: "text/plain".into(),
        size: 5,
        base64: "aGVsbG8=".into(),
    });
    let a = answered(send(&mut live, &r, "run-f", CarryMode::Fresh).await);
    assert_eq!(a.status, 200);
    let p = last_post(&state);
    assert!(p.content_type.starts_with("multipart/form-data"), "{}", p.content_type);
    assert!(p.raw.contains(r#"name="upload"; filename="a.txt""#), "{}", p.raw);
    assert!(p.raw.contains("hello"));
    assert!(p.raw.contains(r#"name="txtA""#) && p.raw.contains("typed"));
    assert!(p.raw.contains(r#"name="__VIEWSTATE""#));
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_postback_that_takes_too_long_is_a_timeout() {
    let (port, _state) = serve();
    let mut live = open(port).await;
    let r = send_with(&mut live, &req("/Slow.aspx", "CmdGo"), "run-t", CarryMode::Fresh, 600, CARRY_LIMIT_BYTES).await;
    assert!(matches!(r, Reply::Timeout), "{r:?}");
}

#[tokio::test]
#[ignore = "starts a real headless Edge"]
async fn a_page_that_was_just_loaded_is_the_one_posted_from() {
    // Two loads in a row: the second `fresh` call must post the NEW view
    // state, never the one an earlier load issued (the site refuses it).
    let (port, state) = serve();
    let mut live = open(port).await;
    answered(send(&mut live, &req(EVAL, "CmdSave"), "run-x", CarryMode::Fresh).await);
    let a = answered(send(&mut live, &req(EVAL, "CmdSave"), "run-x", CarryMode::Fresh).await);
    assert_eq!(a.status, 200);
    assert!(!a.carried);
    assert_eq!(state.lock().unwrap().gets.iter().filter(|g| *g == "/perf/eval.aspx").count(), 4);
}
