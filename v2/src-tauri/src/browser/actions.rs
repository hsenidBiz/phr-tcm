//! The typed steps a script is made of, and the JavaScript each becomes.
//!
//! Every expression returns `{ ok, detail }`: `detail` is written for the
//! human watching, because in this runner the person - not the machine -
//! decides the verdict. An action that cannot tell what happened says so
//! rather than guessing.

/// How long between polls while waiting for an element.
const POLL_MS: u64 = 200;
/// How long the highlight stays on screen before the action fires, so a
/// watcher can see WHERE the click is about to go.
const HIGHLIGHT_MS: u64 = 350;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Navigate { url: String },
    Click { selector: String },
    Fill { selector: String, value: String },
    WaitFor { selector: String, timeout_ms: u32 },
    CheckText { value: String },
    CheckUrl { contains: String },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ActionOutcome {
    pub ok: bool,
    pub detail: String,
}

/// Anything that can run an expression in the page. `Cdp` is the real
/// one; tests supply a fake, which is why the executor's rules can be
/// pinned without starting a browser.
pub trait Evaluator {
    fn eval(
        &mut self,
        expression: &str,
    ) -> impl std::future::Future<Output = Result<serde_json::Value, String>>;
}

/// A JS string literal, escaped by the JSON encoder. Scripts are
/// authored by hand today and by an assistant later, so no value in
/// them is trusted to be quote-free. `/` is additionally escaped so a
/// value ending a `<script>` tag (e.g. `</script>`) cannot appear intact
/// in the generated source - serde_json's JSON encoding does not escape
/// the solidus on its own, since JSON itself does not require it.
fn lit(s: &str) -> String {
    let json = serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string());
    json.replace('/', "\\/")
}

/// Prepended to every expression rather than installed as a global: a
/// page navigation wipes globals, and the runner navigates.
fn find_helper() -> &'static str {
    r#"
const __find = (sel) => {
  if (sel.startsWith('text=')) {
    const want = sel.slice(5).trim().toLowerCase();
    const all = Array.from(document.querySelectorAll(
      'button,a,[role=button],label,input,textarea,select,td,th,li,summary,h1,h2,h3,span,div'));
    // Last match wins: the deepest element containing the words is the
    // control itself, not the panel it sits in.
    const hits = all.filter(e => ((e.innerText || e.value || '') + '').trim().toLowerCase().includes(want));
    return hits.length ? hits[hits.length - 1] : null;
  }
  return document.querySelector(sel);
};
"#
}

fn wrap(body: &str) -> String {
    format!("(() => {{{}{}}})()", find_helper(), body)
}

/// Outline the element the next action will touch.
pub fn highlight_js(selector: &str) -> String {
    let sel = lit(selector);
    wrap(&format!(
        r#"
  const el = __find({sel});
  if (!el) return {{ ok: false, detail: "not found: " + {sel} }};
  el.scrollIntoView({{ block: 'center', behavior: 'instant' }});
  const prev = el.style.outline;
  el.style.outline = '3px solid #7c5cff';
  setTimeout(() => {{ el.style.outline = prev; }}, 1200);
  return {{ ok: true, detail: "highlighted" }};
"#
    ))
}

/// The expression for one action.
pub fn js_for(action: &Action) -> String {
    match action {
        Action::Navigate { url } => {
            let u = lit(url);
            wrap(&format!(
                r#"
  location.href = {u};
  return {{ ok: true, detail: "navigating to " + {u} }};
"#
            ))
        }
        Action::Click { selector } => {
            let sel = lit(selector);
            wrap(&format!(
                r#"
  const el = __find({sel});
  if (!el) return {{ ok: false, detail: "not found: " + {sel} }};
  el.click();
  return {{ ok: true, detail: "clicked " + (el.innerText || el.value || el.tagName).toString().trim().slice(0, 60) }};
"#
            ))
        }
        Action::Fill { selector, value } => {
            let sel = lit(selector);
            let val = lit(value);
            wrap(&format!(
                r#"
  const el = __find({sel});
  if (!el) return {{ ok: false, detail: "not found: " + {sel} }};
  // A plain .value assignment is invisible to a controlled input - the
  // framework's own setter has to be the one called, then told.
  const desc = Object.getOwnPropertyDescriptor(el.constructor.prototype, 'value');
  if (desc && desc.set) desc.set.call(el, {val}); else el.value = {val};
  el.dispatchEvent(new Event('input', {{ bubbles: true }}));
  el.dispatchEvent(new Event('change', {{ bubbles: true }}));
  return {{ ok: true, detail: "filled " + {sel} }};
"#
            ))
        }
        Action::WaitFor { selector, .. } => {
            let sel = lit(selector);
            wrap(&format!(
                r#"
  const el = __find({sel});
  return el ? {{ ok: true, detail: "found: " + {sel} }} : {{ ok: false, detail: "not found: " + {sel} }};
"#
            ))
        }
        Action::CheckText { value } => {
            let v = lit(value);
            wrap(&format!(
                r#"
  const hay = (document.body ? document.body.innerText : '') || '';
  const found = hay.toLowerCase().includes({v}.toLowerCase());
  return {{ ok: found, detail: (found ? "page contains " : "page does NOT contain ") + {v} }};
"#
            ))
        }
        Action::CheckUrl { contains } => {
            let c = lit(contains);
            wrap(&format!(
                r#"
  const found = location.href.includes({c});
  return {{ ok: found, detail: "url is " + location.href }};
"#
            ))
        }
    }
}

/// Pull `{ ok, detail }` back out of a DevTools reply.
fn outcome_from(v: &serde_json::Value) -> ActionOutcome {
    let value = &v["result"]["value"];
    ActionOutcome {
        ok: value["ok"].as_bool().unwrap_or(false),
        detail: value["detail"].as_str().unwrap_or("no detail").to_string(),
    }
}

/// A harness failure, said plainly: the app under test did nothing
/// wrong, the browser connection did.
fn harness_error(e: String) -> ActionOutcome {
    ActionOutcome { ok: false, detail: format!("the browser did not answer: {e}") }
}

/// Run one action. Clicks and fills highlight first so the watcher can
/// see where they landed; waiting polls instead of guessing a sleep.
pub async fn execute<E: Evaluator>(ev: &mut E, action: &Action) -> ActionOutcome {
    if let Action::Click { selector } | Action::Fill { selector, .. } = action {
        match ev.eval(&highlight_js(selector)).await {
            Ok(v) => {
                let out = outcome_from(&v);
                if !out.ok {
                    return out; // the element is not there; do not click blind
                }
            }
            Err(e) => return harness_error(e),
        }
        tokio::time::sleep(std::time::Duration::from_millis(HIGHLIGHT_MS)).await;
    }

    if let Action::WaitFor { timeout_ms, selector } = action {
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(u64::from(*timeout_ms));
        loop {
            match ev.eval(&js_for(action)).await {
                Ok(v) => {
                    let out = outcome_from(&v);
                    if out.ok {
                        return out;
                    }
                }
                Err(e) => return harness_error(e),
            }
            if std::time::Instant::now() >= deadline {
                return ActionOutcome {
                    ok: false,
                    detail: format!("waited {timeout_ms}ms and never saw {selector}"),
                };
            }
            tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
        }
    }

    match ev.eval(&js_for(action)).await {
        Ok(v) => outcome_from(&v),
        Err(e) => harness_error(e),
    }
}
