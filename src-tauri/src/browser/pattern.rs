//! The patterns a check can carry (`expect_text`'s and `expect_attribute`'s
//! `matches`). Rust's `regex` runs in linear time, so a pattern cannot hang
//! a run; what is refused here is a pattern that a Playwright export would
//! read differently (JavaScript has no `(?i)`, `\A`, `\p{..}` or
//! `[[:alpha:]]` in the same form), one that is too long to read, and one
//! that fits any text at all, which can never fail.
//!
//! Matching is `Regex::is_match`: not anchored unless the writer writes `^`
//! or `$`, case-sensitive, on the text with its whitespace collapsed (so no
//! newline is left for a pattern to cross).

/// The longest `matches` value saved.
pub const MAX_PATTERN: usize = 500;

/// The first thing in `p` a Playwright export cannot carry, said as it goes
/// after "uses". An escape-aware scan: the character after a backslash is
/// skipped, and inside `[...]` only `[:` counts.
fn unportable(p: &str) -> Option<&'static str> {
    let cs: Vec<char> = p.chars().collect();
    let mut i = 0;
    let mut in_class = false;
    // Just after the `[` (or `[^`) that opened the class, where `]` is a
    // plain character.
    let mut class_start = false;
    while i < cs.len() {
        let c = cs[i];
        match c {
            '\\' => {
                match cs.get(i + 1) {
                    Some('A' | 'z' | 'Z') => return Some("\\A, \\z or \\Z"),
                    Some('p' | 'P') => return Some("\\p{...} or \\P{...}"),
                    _ => {}
                }
                i += 2;
                class_start = false;
                continue;
            }
            '[' if in_class => {
                if cs.get(i + 1) == Some(&':') {
                    return Some("a [[:class:]]");
                }
            }
            '[' => {
                in_class = true;
                class_start = true;
                i += 1;
                if cs.get(i) == Some(&'^') {
                    i += 1;
                }
                continue;
            }
            ']' if in_class && !class_start => in_class = false,
            '(' if !in_class && cs.get(i + 1) == Some(&'?') && cs.get(i + 2) != Some(&':') => {
                return Some("a (?...) flag or group other than (?:...)");
            }
            _ => {}
        }
        class_start = false;
        i += 1;
    }
    None
}

/// Compiled with a size limit. Err is the regex error's last non-empty line,
/// without a leading "error: ".
pub fn compiled(p: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(p).size_limit(1 << 20).build().map_err(|e| {
        let text = e.to_string();
        let line = text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("");
        line.strip_prefix("error: ").unwrap_or(line).to_string()
    })
}

/// `/p/`, how sentences show a pattern.
pub fn shown(p: &str) -> String {
    format!("/{p}/")
}

/// The save-time check of a `matches` value. `kind` is the action's word.
pub fn check_pattern(kind: &str, p: &str) -> Result<(), String> {
    if p.trim().is_empty() {
        return Err(format!("{kind} has an empty \"matches\" - it would match any text"));
    }
    if p.chars().count() > MAX_PATTERN {
        return Err(format!("{kind}: \"matches\" is longer than {MAX_PATTERN} characters"));
    }
    if let Some(what) = unportable(p) {
        return Err(format!(
            "{kind}: \"matches\" uses {what}, which a Playwright export cannot carry - write the pattern without it"
        ));
    }
    let re = compiled(p).map_err(|why| format!("{kind}: \"matches\" is not a valid pattern: {why}"))?;
    if re.is_match("") && re.is_match("x0 Y") {
        return Err(format!("{kind}: \"matches\" fits any text, even an empty one - write what the text must hold"));
    }
    Ok(())
}
