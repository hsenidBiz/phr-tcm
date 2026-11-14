//! The Auto Run browser's captured requests, HAR files, test headers and cookies.

use super::*;

/// The assistant's capture tools (`autorun::capture_tools`), one per route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTool {
    List,
    Search,
    Request,
    Response,
    ResponseBody,
    GetFilter,
    SetFilter,
    Export,
    Import,
}

impl CaptureTool {
    /// Reads one set: the live capture, or an imported one named by `from`.
    pub(crate) fn reads_a_set(self) -> bool {
        matches!(self, Self::List | Self::Search | Self::Request | Self::Response | Self::ResponseBody)
    }
}

/// How long a capture call waits for the supervised browser's lock: a
/// step, a try or a discovery action holds it for its own job only.
pub const CAPTURE_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Said when the browser's lock does not come free for a capture call.
pub const CAPTURE_BUSY: &str =
    "The Auto Run browser is busy (a replay, a step or a discovery action is using it): ask again when it has finished.";

/// A capture tool's call. Nothing is redacted, matched or written while a
/// lock is held: the call takes what it needs out of its set (cloned) and
/// lets the lock go before it answers (`CapturePhase::finish`). An import
/// reads and parses its file before it changes the imported sets (one
/// import at a time, `commands::autorun::change_imported_captures`); a
/// read naming `from` is checked, and its set looked for, before anything
/// else, and never takes the browser's lock. The browser is slot `n`'s.
pub(crate) async fn autorun_capture(tool: CaptureTool, body: &str, n: SlotNo) -> (u16, String) {
    use crate::autorun::capture_tools as ct;
    let root = crate::autorun::store::configured_root();
    let args = match ct::args_of(body) {
        Ok(a) => a,
        Err(e) => return e,
    };
    if tool == CaptureTool::Import {
        let Some(root) = root.as_deref() else {
            return (503, crate::commands::autorun::NO_DATA_DIRECTORY.to_string());
        };
        return match ct::read_import(root, &args) {
            Ok((stem, entries)) => {
                crate::commands::autorun::change_imported_captures(|sets| ct::store_import(sets, stem, entries))
            }
            Err(e) => e,
        };
    }
    let from = if tool.reads_a_set() {
        match ct::from_of(&args) {
            Ok(f) => f,
            Err(e) => return e,
        }
    } else {
        None
    };
    if from.is_some() {
        let phase = {
            let mut imported = crate::commands::autorun::imported_captures();
            capture_tool_begin(&mut None::<crate::commands::autorun::Session>, &mut imported, root.as_deref(), tool, body)
        };
        return phase.finish();
    }
    if crate::autorun::replay_to::state(n).is_running() {
        return (409, CAPTURE_BUSY.to_string());
    }
    let phase = {
        let Ok(mut slot) = tokio::time::timeout(CAPTURE_LOCK_WAIT, crate::commands::autorun::session_slot(n).lock()).await
        else {
            return (409, CAPTURE_BUSY.to_string());
        };
        let mut imported = crate::commands::autorun::imported_captures();
        capture_tool_begin(&mut slot, &mut imported, root.as_deref(), tool, body)
    };
    phase.finish()
}

/// `answer` ending with what the assistant changed for testing in the
/// Auto Run browser (`overrides::with_note`): as `seen` under the session
/// lock the moment the call finished, or - for an answer given before the
/// browser was taken (a refusal) - as last published
/// (`commands::autorun::overrides_now_in`, slot `n`'s).
pub(crate) fn with_overrides_seen(
    answer: (u16, String),
    seen: Option<crate::autorun::overrides::Overrides>,
    n: SlotNo,
) -> (u16, String) {
    let changed = seen.unwrap_or_else(|| crate::commands::autorun::overrides_now_in(n));
    crate::autorun::overrides::with_note(answer, &changed)
}

/// The assistant's test header and cookie tools (`autorun::overrides`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrideTool {
    /// `set_autorun_headers`
    SetHeaders,
    /// `autorun_cookies`' list: a read, in any supervised browser.
    Cookies,
    /// `autorun_cookies`' set
    SetCookie,
    /// `autorun_cookies`' clear: a real sign-out.
    ClearCookies,
}

/// Said when the browser's lock does not come free for a header or cookie
/// call.
pub const OVERRIDES_BUSY: &str =
    "The Auto Run browser is busy (a step or a discovery action is using it): ask again when it has finished.";

/// Said when a test header or cookie is asked for in the person's browser:
/// only one the assistant opened is changed (spec decision 4).
pub fn persons_browser_not_changed() -> String {
    format!(
        "Test headers and cookies are changed only in an Auto Run browser you opened (a discovery or your replay): {}",
        crate::commands::autorun::PERSONS_BROWSER_OPEN
    )
}

/// Said by a browser that keeps no record of test changes (a test's fake).
pub const KEEPS_NO_OVERRIDES: &str = "this Auto Run browser keeps no record of test changes, so none is made in it";

/// `/autorun-headers`, `/autorun-cookies` and `/autorun-cookie`: the call
/// over the supervised browser (`override_tool_in`). Refused at once while
/// a replay runs (one going holds the browser); the browser's lock is
/// waited for 5 s at most. What is changed is published for the replay's
/// answer (`publish_overrides_in`). The sign-out is
/// `commands::autorun::clear_cookies_for_assistant`. The browser is slot `n`'s.
pub(crate) async fn autorun_override(tool: OverrideTool, body: &str, n: SlotNo) -> (u16, String) {
    if crate::autorun::replay_to::state(n).is_running() {
        return (409, replay_running_refusal(n));
    }
    let Ok(mut slot) = tokio::time::timeout(CAPTURE_LOCK_WAIT, crate::commands::autorun::session_slot(n).lock()).await else {
        return (409, OVERRIDES_BUSY.to_string());
    };
    // Only the sign-out has news for the panes, and it is answered by
    // `commands::autorun::clear_cookies_for_assistant`, not here.
    let (answer, _) = override_tool_in(n, &mut slot, tool, body).await;
    let answer = let_go_if_silent(&mut slot, answer).await;
    crate::commands::autorun::publish_overrides_in(n, &slot);
    if tool != OverrideTool::Cookies {
        // Names only - never a value.
        let now = slot.as_ref().and_then(|s| s.overrides.names()).unwrap_or_else(|| "nothing".to_string());
        crate::applog::info(format!("Auto Run: the assistant's {tool:?} answered {}; changed for testing now: {now}", answer.0));
    }
    answer
}

/// A header or cookie call over the browser `slot` holds (the app's
/// session, or a test's fake), with what the panes must hear (a sign-out).
/// `n` is the slot the browser is in: a replay going there refuses the call.
pub async fn override_tool_in<B: DiscoveryBrowser>(
    n: SlotNo,
    slot: &mut Option<B>,
    tool: OverrideTool,
    body: &str,
) -> ((u16, String), Option<crate::events::AutorunSessionChanged>) {
    let args = match body.trim() {
        "" => serde_json::json!({}),
        text => match serde_json::from_str::<serde_json::Value>(text) {
            Ok(v @ serde_json::Value::Object(_)) => v,
            _ => return ((400, "the arguments are a JSON object".to_string()), None),
        },
    };
    match tool {
        OverrideTool::SetHeaders => (set_headers_in(n, slot, &args).await, None),
        OverrideTool::Cookies => (cookies_in(slot, &args).await, None),
        OverrideTool::SetCookie => (set_cookie_in(n, slot, &args).await, None),
        OverrideTool::ClearCookies => crate::commands::autorun::clear_cookies_in_for_assistant(n, slot).await,
    }
}

/// Refused unless `slot` holds a live browser the assistant opened, with no
/// replay going and no guard of the person's on it - the checks the
/// assistant's reset makes (spec decision 4). A browser that has gone is
/// let go first. A replay going in slot `n`, where the browser is, refuses it.
pub(crate) async fn refuse_acting<B: DiscoveryBrowser>(n: SlotNo, slot: &mut Option<B>) -> Result<(), (u16, String)> {
    if crate::autorun::replay_to::state(n).is_running() {
        return Err((409, replay_running_refusal(n)));
    }
    if forget_gone_browser(slot).await {
        return Err((404, crate::commands::autorun::NO_BROWSER_OPEN.to_string()));
    }
    match slot.as_mut() {
        None => Err((404, crate::commands::autorun::NO_BROWSER_OPEN.to_string())),
        Some(b) => {
            let persons = !b.opened_by_assistant()
                || *b.parts().guard_owner == Some(crate::commands::autorun::GuardOwner::Person);
            if persons {
                Err((409, persons_browser_not_changed()))
            } else {
                Ok(())
            }
        }
    }
}

/// Where the test changes stand, as the acting tools answer: `said`, what
/// is changed (`changed_for_testing`), or that nothing is.
pub(crate) fn overrides_answer(said: String, o: &crate::autorun::overrides::Overrides) -> (u16, String) {
    let mut answer = serde_json::json!({ "said": said });
    answer[crate::autorun::overrides::NOTE_KEY] = match o.changed_line() {
        Some(line) => serde_json::json!(line),
        None => serde_json::json!("Nothing is changed for testing now."),
    };
    (200, answer.to_string())
}

/// What a set of test headers or a cookie lasts for, said once it is set.
pub(crate) const LASTS_UNTIL: &str = "Every try, discovery and replay answer names it until you remove it, or reset_autorun_browser, autorun_cookies' clear or a release takes it away.";

/// `set_autorun_headers` over `slot`: the headers asked for merged into the
/// set in place (a `null` removes one), sent on every tab and on any tab
/// the page opens (`Driver::set_extra_headers`), never through `Fetch`.
/// A refused name (`overrides::REFUSED_HEADERS`) refuses the whole call
/// before the browser is touched.
pub async fn set_headers_in<B: DiscoveryBrowser>(n: SlotNo, slot: &mut Option<B>, args: &serde_json::Value) -> (u16, String) {
    use crate::autorun::overrides as ov;
    let changes = match ov::header_changes(args) {
        Ok(c) => c,
        Err(why) => return (400, why),
    };
    if let Err(refused) = refuse_acting(n, slot).await {
        return refused;
    }
    let Some(b) = slot.as_mut() else {
        return (404, crate::commands::autorun::NO_BROWSER_OPEN.to_string());
    };
    let p = b.parts();
    let Some(o) = p.overrides else {
        return (500, KEEPS_NO_OVERRIDES.to_string());
    };
    let next = match ov::merged(&o.headers, &changes) {
        Ok(n) => n,
        Err(why) => return (400, why),
    };
    if let Err(e) = crate::browser::cdp::Driver::set_extra_headers(p.driver, &next).await {
        // Some tabs may send the new set already: every name stays named
        // until a reset sends none.
        for (name, value) in next {
            o.headers.retain(|k, _| !k.eq_ignore_ascii_case(&name));
            o.headers.insert(name, value);
        }
        let (status, why) = ov::browser_failed(e);
        return (status, format!("{why}. Some tabs may send the new headers already; reset_autorun_browser removes every test header."));
    }
    o.headers = next;
    let said = if o.headers.is_empty() {
        "No test headers are sent now.".to_string()
    } else {
        format!(
            "Sent with every request of every tab, and of any tab the page opens: {}. {LASTS_UNTIL}",
            o.headers.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    };
    overrides_answer(said, o)
}

/// `autorun_cookies`' list over `slot`: every cookie the browser holds for
/// the app's site - the one TCM's sign-in confirmed (`overrides::confirmed_origin`),
/// never whatever page shows, which may be the identity provider's - whatever
/// its path, as `overrides::shown_cookie` shows it (the sign-in's values
/// hidden). `url` may name that site's host or one of its parent domains,
/// nothing else (400). A read: any supervised browser, the person's too.
pub async fn cookies_in<B: DiscoveryBrowser>(slot: &mut Option<B>, args: &serde_json::Value) -> (u16, String) {
    use crate::autorun::overrides as ov;
    let asked = match args.get("url") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(u)) => match ov::url_host(u) {
            Some(host) => Some(host),
            None => return (400, "url is an http or https address, such as https://app.example/".to_string()),
        },
        Some(_) => return (400, "url is an http or https address, such as https://app.example/".to_string()),
    };
    if forget_gone_browser(slot).await {
        return (404, crate::commands::autorun::NO_BROWSER_OPEN.to_string());
    }
    let Some(b) = slot.as_mut() else {
        return (404, crate::commands::autorun::NO_BROWSER_OPEN.to_string());
    };
    let p = b.parts();
    let site = match ov::confirmed_origin(&*p.driver) {
        Ok(origin) => ov::url_host(&origin).unwrap_or_default(),
        Err(refused) => return refused,
    };
    let host = match asked {
        Some(host) if !crate::browser::session::cookie_belongs(&host, &site) => {
            return (400, format!("url is outside the app's site ({site}): name that site or a parent domain of it"));
        }
        Some(host) => host,
        None => site,
    };
    let cookies = match ov::cookies_for_host(p.driver, &host).await {
        Ok(c) => c,
        Err(e) => return ov::browser_failed(e),
    };
    let set = p.overrides.as_deref().cloned().unwrap_or_default();
    let shown: Vec<serde_json::Value> = cookies
        .iter()
        .take(ov::MAX_LISTED)
        .map(|c| {
            let name = c["name"].as_str().unwrap_or("");
            ov::shown_cookie(c, set.cookies.get(name).is_some_and(|s| s.is(name, c)))
        })
        .collect();
    let answer = serde_json::json!({
        "site": host,
        "total": cookies.len(),
        "returned": shown.len(),
        "cookies": shown,
        "note": "The sign-in's cookies (HttpOnly, the sign-in and anti-forgery cookies, names like a secret) show their value as (hidden), and are never set by autorun_cookies.",
    });
    (200, answer.to_string())
}

/// `autorun_cookies`' set over `slot`: one cookie on the app's own site - the
/// one TCM's sign-in confirmed (`overrides::confirmed_origin`), never
/// whatever page shows - on its host only, `path` `/` unless asked;
/// recorded exactly (`SetCookie`) so a reset deletes it only while it is
/// still that, and left out of the sign-in's print
/// (`Held::ignore_in_print`), so it never makes the sign-in look changed.
/// Refused for the sign-in's cookies (`overrides::protected_cookie`), an
/// HttpOnly cookie of that name (in any case) on the site included.
pub async fn set_cookie_in<B: DiscoveryBrowser>(n: SlotNo, slot: &mut Option<B>, args: &serde_json::Value) -> (u16, String) {
    use crate::autorun::overrides as ov;
    let ask = match ov::cookie_ask(args) {
        Ok(a) => a,
        Err(why) => return (400, why),
    };
    if ov::protected_cookie(&ask.name, false) {
        return (403, ov::protected_cookie_sentence(&ask.name));
    }
    if let Err(refused) = refuse_acting(n, slot).await {
        return refused;
    }
    let Some(b) = slot.as_mut() else {
        return (404, crate::commands::autorun::NO_BROWSER_OPEN.to_string());
    };
    let p = b.parts();
    let Some(o) = p.overrides else {
        return (500, KEEPS_NO_OVERRIDES.to_string());
    };
    let origin = match ov::confirmed_origin(&*p.driver) {
        Ok(origin) => origin,
        Err(refused) => return refused,
    };
    let host = ov::url_host(&origin).unwrap_or_default();
    let existing = match ov::cookies_for_host(p.driver, &host).await {
        Ok(c) => c,
        Err(e) => return ov::browser_failed(e),
    };
    let http_only_named = |c: &serde_json::Value| {
        c["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(&ask.name)) && c["httpOnly"].as_bool() == Some(true)
    };
    if existing.iter().any(http_only_named) {
        return (403, ov::protected_cookie_sentence(&ask.name));
    }
    match crate::browser::cdp::Driver::call(p.driver, "Network.setCookie", ov::set_cookie_params(&ask, &origin)).await {
        Ok(reply) if reply["success"].as_bool() == Some(false) => (400, ov::COOKIE_REFUSED.to_string()),
        Ok(_) => {
            let set = ov::SetCookie { domain: host.clone(), path: ask.path.clone(), value: ask.value.clone() };
            o.cookies.insert(ask.name.clone(), set);
            p.lease.ignore_in_print(&ask.name);
            let said = format!("Set cookie {} on {host} (path {}). {LASTS_UNTIL}", ask.name, ask.path);
            overrides_answer(said, o)
        }
        Err(crate::browser::cdp::CdpError::Protocol { message, .. }) => {
            (400, format!("{} The browser said: {message}", ov::COOKIE_REFUSED))
        }
        Err(e) => ov::browser_failed(e),
    }
}


/// A capture call part way: answered already, or holding what it took out
/// of its set, to be redacted and answered with no lock held.
pub enum CapturePhase {
    Answered((u16, String)),
    Read(crate::autorun::capture_tools::Read, crate::autorun::capture_tools::Taken, serde_json::Value),
    Export(std::path::PathBuf, crate::autorun::capture_tools::TakenExport),
}

impl CapturePhase {
    /// The answer: the redaction, the matching and the export's write
    /// happen here.
    pub fn finish(self) -> (u16, String) {
        use crate::autorun::capture_tools as ct;
        match self {
            CapturePhase::Answered(a) => a,
            // The browser's masked kept values never show in an answer.
            CapturePhase::Read(read, taken, args) => {
                let (status, text) = ct::answer(&taken, read, &args);
                (status, crate::api_templates::exec::hide_text(&text, taken.hide()))
            }
            CapturePhase::Export(root, taken) => ct::write_export(&root, taken),
        }
    }
}

/// A capture tool's call over the browser `slot` holds (the app's session,
/// or a test's fake) and the imported sets: what `/autorun-requests` and
/// its siblings answer. A read naming `from` reads that imported set, with
/// no browser needed; every other read, the filter and the export need the
/// browser (404 "No browser is open." without one). An import needs only
/// the Auto Run folder (`root`). `capture_tool_begin`, then its `finish`.
pub fn capture_tool_in<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    imported: &mut crate::autorun::capture_tools::ImportedSets,
    root: Option<&std::path::Path>,
    tool: CaptureTool,
    body: &str,
) -> (u16, String) {
    capture_tool_begin(slot, imported, root, tool, body).finish()
}

/// The part of a capture call that needs its set: what it takes out of the
/// live capture or an imported set, cloned and unredacted, or an answer
/// already (a refusal, the filter, an import).
pub fn capture_tool_begin<B: DiscoveryBrowser>(
    slot: &mut Option<B>,
    imported: &mut crate::autorun::capture_tools::ImportedSets,
    root: Option<&std::path::Path>,
    tool: CaptureTool,
    body: &str,
) -> CapturePhase {
    use crate::autorun::capture_tools as ct;
    use crate::browser::cdp::Driver;
    use CapturePhase::Answered;
    let args = match ct::args_of(body) {
        Ok(a) => a,
        Err(e) => return Answered(e),
    };
    let no_root = || Answered((503, crate::commands::autorun::NO_DATA_DIRECTORY.to_string()));
    let read = match tool {
        CaptureTool::List => Some(ct::Read::List),
        CaptureTool::Search => Some(ct::Read::Search),
        CaptureTool::Request => Some(ct::Read::Request),
        CaptureTool::Response => Some(ct::Read::Response),
        CaptureTool::ResponseBody => Some(ct::Read::ResponseBody),
        _ => None,
    };
    let taken = |set: &ct::Set, read: ct::Read, args: serde_json::Value| match ct::take(set, read, &args) {
        Ok(t) => CapturePhase::Read(read, t, args),
        Err(e) => Answered(e),
    };
    if tool == CaptureTool::Import {
        return match root {
            Some(root) => Answered(ct::import(root, imported, &args)),
            None => no_root(),
        };
    }
    if let Some(read) = read {
        match ct::from_of(&args) {
            Err(e) => return Answered(e),
            Ok(Some(name)) => {
                return match ct::imported_set(imported, &name) {
                    Ok(set) => taken(&set, read, args),
                    Err(e) => Answered(e),
                };
            }
            Ok(None) => {}
        }
    }
    let Some(b) = slot.as_mut() else {
        return Answered((404, crate::commands::autorun::NO_BROWSER_OPEN.to_string()));
    };
    let driver = b.parts().driver;
    if tool == CaptureTool::SetFilter {
        return Answered(match Driver::capture_mut(driver) {
            Some(cap) => ct::set_filter(cap, imported, &args),
            None => (404, ct::NO_CAPTURE.to_string()),
        });
    }
    let Some(cap) = Driver::capture(&*driver) else {
        return Answered((404, ct::NO_CAPTURE.to_string()));
    };
    match (tool, read) {
        (_, Some(read)) => taken(&ct::Set::live(cap), read, args),
        (CaptureTool::GetFilter, _) => Answered((200, ct::filter_answer(cap, imported).to_string())),
        (CaptureTool::Export, _) => match root {
            Some(root) => match ct::take_export(cap, &args) {
                Ok(t) => CapturePhase::Export(root.to_path_buf(), t),
                Err(e) => Answered(e),
            },
            None => no_root(),
        },
        _ => Answered((404, ct::NO_CAPTURE.to_string())),
    }
}

