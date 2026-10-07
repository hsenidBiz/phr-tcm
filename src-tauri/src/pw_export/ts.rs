//! TypeScript text for the Playwright export: string literals and the
//! translation of an Auto Run selector into a Playwright locator expression.

use crate::browser::locator::{LocatorStep, Target};

const BS: char = '\\';

/// A single-quoted TypeScript string literal. Single quotes keep backticks
/// and `${` inert.
pub fn lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            '\'' => out.push_str(r"\'"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            '\u{2028}' => out.push_str(&format!("{BS}u2028")),
            '\u{2029}' => out.push_str(&format!("{BS}u2029")),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// A locator expression on the page variable `page`.
pub fn locator(page: &str, t: &Target) -> String {
    match t {
        Target::Legacy(css) => {
            let base = format!("{page}.locator({})", lit(css));
            if css.starts_with("text=") {
                format!("{base}.last()")
            } else {
                base
            }
        }
        Target::One(step) => step_expr(page, step),
        Target::Chain(steps) => {
            let mut expr = page.to_string();
            let mut after_frame = false;
            for step in steps {
                if after_frame {
                    expr.push_str(".contentFrame()");
                }
                expr = step_expr(&expr, step);
                after_frame = is_iframe(step);
            }
            expr
        }
    }
}

fn is_iframe(s: &LocatorStep) -> bool {
    s.css.as_deref().is_some_and(|c| c.starts_with("iframe") || c.starts_with("frame"))
        || s.role.as_deref().is_some_and(|r| r.eq_ignore_ascii_case("iframe"))
}

fn step_expr(base: &str, s: &LocatorStep) -> String {
    let mut e = base.to_string();
    if let Some(role) = &s.role {
        e.push_str(&format!(".getByRole({}", lit(role)));
        let mut opts = Vec::new();
        if let Some(name) = &s.name {
            opts.push(format!("name: {}", lit(name)));
            if s.exact {
                opts.push("exact: true".to_string());
            }
        }
        if !opts.is_empty() {
            e.push_str(&format!(", {{ {} }}", opts.join(", ")));
        }
        e.push(')');
    } else if let Some(text) = &s.text {
        e.push_str(&format!(".getByText({}", lit(text)));
        if s.exact {
            e.push_str(", { exact: true }");
        }
        e.push(')');
    } else if let Some(css) = &s.css {
        e.push_str(&format!(".locator({})", lit(css)));
    }
    if s.visible != Some(false) {
        e.push_str(".filter({ visible: true })");
    }
    if let Some(n) = s.nth {
        e.push_str(&format!(".nth({n})"));
    }
    e
}
