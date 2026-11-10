//! What the assistant may learn about a sign-in page without the page's
//! values: counts and shapes only, never an input's `.value`.

use super::recipe::PasswordResetPage;
use super::signin_recorder::{css_escape, field_css, FieldHints};
use crate::browser::cdp::Driver;
use crate::browser::expect::{expect, Check};
use crate::browser::locator::Target;
use crate::browser::timing::Timing;

/// An expression evaluating to the number of visible password fields on the
/// page: each has client rects, a computed `visibility` that is not hidden and
/// a `display` that is not none.
pub const PASSWORD_FIELDS_JS: &str = "(() => Array.from(document.querySelectorAll('input[type=password]')).filter(e => { const s = getComputedStyle(e); return e.getClientRects().length > 0 && s.visibility !== 'hidden' && s.display !== 'none'; }).length)()";

/// How many password fields are visible now; anything the browser answers
/// that is not a number is none.
pub async fn visible_password_fields<D: Driver>(d: &mut D) -> usize {
    crate::browser::page::eval_value(d, PASSWORD_FIELDS_JS)
        .await
        .ok()
        .and_then(|v| v.as_u64())
        .map_or(0, |n| n as usize)
}

/// How many controls and actions the assistant is shown; more are cut.
pub const MOST_CONTROLS: usize = 40;
pub const MOST_ACTIONS: usize = 40;

/// Said when a page shows two or more password fields.
pub const CHANGE_PASSWORD_HINT: &str = "this page looks like a change-password page (two or more password fields): describe it as the recipe's password_reset";

/// An expression answering `{ controls, actions }`: the visible fields and
/// the visible buttons and links, each by its attributes and caption only.
/// It never reads what a field holds; the one `getAttribute('value')` is a
/// submit or button input's own caption. Text is trimmed to 80 characters
/// and each list stops one past its cap so the reader can tell it was cut.
pub const INSPECT_JS: &str = r#"(() => {
  const shown = e => { const s = getComputedStyle(e); return e.getClientRects().length > 0 && s.visibility !== 'hidden' && s.display !== 'none'; };
  const clip = t => String(t || '').replace(/\s+/g, ' ').trim().slice(0, 80);
  const labelOf = e => {
    const parts = [];
    if (e.labels) { for (const l of e.labels) { parts.push(l.innerText); } }
    const aria = e.getAttribute('aria-label');
    if (aria) { parts.push(aria); }
    const by = e.getAttribute('aria-labelledby');
    if (by) { for (const id of by.split(/\s+/)) { const n = document.getElementById(id); if (n) { parts.push(n.innerText); } } }
    return clip(parts.join(' '));
  };
  const controls = Array.from(document.querySelectorAll('input, select, textarea'))
    .filter(e => !['hidden', 'submit', 'button', 'reset', 'image'].includes((e.type || '').toLowerCase()) && shown(e))
    .slice(0, 41)
    .map(e => ({ tag: e.tagName.toLowerCase(), type: clip(e.type), id: clip(e.id), name: clip(e.name), label: labelOf(e), placeholder: clip(e.placeholder), password: (e.type || '').toLowerCase() === 'password', role: clip(e.getAttribute('role')) }));
  const actions = Array.from(document.querySelectorAll('button, a[href], input[type=submit], input[type=button], input[type=reset]'))
    .filter(shown)
    .slice(0, 41)
    .map(e => {
      const input = e.tagName.toLowerCase() === 'input';
      return { kind: e.tagName.toLowerCase() === 'a' ? 'link' : 'button', text: clip(input ? e.getAttribute('value') : e.innerText), id: clip(e.id) };
    });
  return { controls, actions };
})()"#;

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct RawControl {
    pub tag: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub name: String,
    pub label: String,
    pub placeholder: String,
    pub password: bool,
    pub role: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct RawAction {
    /// `"button"` or `"link"`.
    pub kind: String,
    pub text: String,
    pub id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PageControl {
    pub role: String,
    pub input_type: String,
    pub id: String,
    pub name: String,
    pub label: String,
    pub placeholder: String,
    pub password: bool,
    pub locator: Target,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PageAction {
    pub kind: String,
    pub text: String,
    pub locator: Target,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SignInPageView {
    pub path: String,
    pub title: String,
    pub controls: Vec<PageControl>,
    pub actions: Vec<PageAction>,
    pub change_password_hint: Option<String>,
    /// Either list had more than the assistant is shown.
    pub cut: bool,
}

fn css_target(css: &str) -> Target {
    serde_json::from_value(serde_json::json!({ "css": css })).expect("a css locator")
}

/// How a recipe step would point at this control: its attributes as css
/// (`signin_recorder::field_css`), else the textbox named by its label.
pub fn control_locator(c: &RawControl) -> Target {
    let hints = FieldHints {
        tag: c.tag.clone(),
        password: c.password,
        id: c.id.clone(),
        name: c.name.clone(),
        placeholder: c.placeholder.clone(),
    };
    if let Some(css) = field_css(&hints) {
        return css_target(&css);
    }
    if !c.label.trim().is_empty() {
        return super::recorder::exact_role("textbox", &c.label);
    }
    css_target(if c.tag.trim().is_empty() { "input" } else { c.tag.trim() })
}

/// How a recipe step would point at this button or link: its id, else its
/// role and caption.
pub fn action_locator(a: &RawAction) -> Target {
    if !a.id.trim().is_empty() {
        return css_target(&format!("#{}", css_escape(&a.id)));
    }
    let kind = if a.kind.is_empty() { "button" } else { a.kind.as_str() };
    if a.text.trim().is_empty() {
        return css_target(if kind == "link" { "a" } else { "button" });
    }
    super::recorder::exact_role(kind, &a.text)
}

fn role_of(c: &RawControl) -> String {
    if !c.role.is_empty() {
        return c.role.clone();
    }
    if c.tag == "select" { "combobox" } else { "textbox" }.to_string()
}

async fn eval_string<D: Driver>(d: &mut D, expression: &str) -> String {
    crate::browser::page::eval_value(d, expression).await.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// What the page shows right now, for the assistant: its path (no host, no
/// query), its title and each visible control and action with a locator a
/// recipe step could use. Never a value.
pub async fn read_sign_in_page<D: Driver>(d: &mut D) -> Result<SignInPageView, String> {
    let read = crate::browser::page::eval_value(d, INSPECT_JS).await.map_err(|e| e.to_string())?;
    let controls: Vec<RawControl> = serde_json::from_value(read["controls"].clone()).unwrap_or_default();
    let actions: Vec<RawAction> = serde_json::from_value(read["actions"].clone()).unwrap_or_default();
    let href = eval_string(d, "location.href").await;
    let title = eval_string(d, "document.title").await;
    let title: String = title.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
    let cut = controls.len() > MOST_CONTROLS || actions.len() > MOST_ACTIONS;
    let passwords = controls.iter().filter(|c| c.password).count();
    Ok(SignInPageView {
        path: crate::browser::save_guard::path_of(&href),
        title,
        controls: controls
            .iter()
            .take(MOST_CONTROLS)
            .map(|c| PageControl {
                role: role_of(c),
                input_type: c.kind.clone(),
                id: c.id.clone(),
                name: c.name.clone(),
                label: c.label.clone(),
                placeholder: c.placeholder.clone(),
                password: c.password,
                locator: control_locator(c),
            })
            .collect(),
        actions: actions
            .iter()
            .take(MOST_ACTIONS)
            .map(|a| PageAction { kind: a.kind.clone(), text: a.text.clone(), locator: action_locator(a) })
            .collect(),
        change_password_hint: (passwords >= 2).then(|| CHANGE_PASSWORD_HINT.to_string()),
        cut,
    })
}

/// Is the browser on `page` with every field it needs showing: the path
/// holds `path_contains` and the recognising field, the new password field,
/// the confirmation and the submit button are each visible. The built-in
/// fields fill whatever the page names none of. A page that does not say
/// where it is, or misses any one, is false.
pub async fn reset_fields_showing<D: Driver>(d: &mut D, page: &PasswordResetPage, timing: &Timing) -> bool {
    let page = super::signin::with_builtin_fields(page.clone());
    let href = eval_string(d, "location.href").await;
    if href.is_empty() || !crate::browser::save_guard::path_of(&href).to_lowercase().contains(&page.path_contains.trim().to_lowercase()) {
        return false;
    }
    for t in [Some(&page.field), page.new.as_ref(), page.confirm.as_ref(), page.submit.as_ref()].into_iter().flatten() {
        if !expect(d, t, Check::Visible, timing.expect_ms, timing.poll_ms).await.ok {
            return false;
        }
    }
    true
}
