//! The patterns a check can carry (`expect_text`'s and `expect_attribute`'s
//! `matches`). Rust's `regex` runs in linear time, so a pattern cannot hang
//! a run; what is refused here is a pattern that a Playwright export would
//! read differently (JavaScript has no `(?i)`, `\A`, `\p{..}` or
//! `[[:alpha:]]` in the same form), one that is too long to read, and one
//! that fits any text at all, which can never fail.
//!
//! The run follows what the export can do. JavaScript's `\w \d \s \b` (and
//! their negations) are ASCII-only, Rust's are Unicode, so a pattern is
//! compiled with them rewritten to their ASCII meaning (`ascii_classes`):
//! `^\w+ \w+$` does not match "Jose Silva" with an accent, in a run or in the
//! export. Literal non-ASCII text in a pattern still matches itself.
//!
//! Matching is `Regex::is_match`: not anchored unless the writer writes `^`
//! or `$`, case-sensitive, on the text with its whitespace collapsed (so no
//! newline is left for a pattern to cross).

/// The longest `matches` value saved.
pub const MAX_PATTERN: usize = 500;

/// The first thing in `p` a Playwright export cannot carry, said as it goes
/// after "uses". An escape-aware scan: the character after a backslash is
/// skipped (after it is judged), and inside `[...]` only `[`, `&&`, `--` and
/// `~~` count.
fn unportable(p: &str) -> Option<&'static str> {
    let cs: Vec<char> = p.chars().collect();
    let mut i = 0;
    let mut in_class = false;
    // Just after the `[` (or `[^`) that opened the class, where `]` is a
    // plain character in Rust and ends an empty class in JavaScript.
    let mut class_start = false;
    while i < cs.len() {
        let c = cs[i];
        let next = cs.get(i + 1).copied();
        match c {
            '\\' => {
                match next {
                    Some('A' | 'z' | 'Z') => return Some("\\A, \\z or \\Z"),
                    Some('p' | 'P') => return Some("\\p{...} or \\P{...}"),
                    Some('x' | 'u') if cs.get(i + 2) == Some(&'{') => return Some("\\x{...} or \\u{...}"),
                    Some('U') => return Some("\\U..."),
                    Some('b' | 'B') if cs.get(i + 2) == Some(&'{') => return Some("\\b{...}"),
                    Some('<' | '>') => return Some("\\< or \\>"),
                    Some('a') => return Some("\\a"),
                    _ => {}
                }
                i += 2;
                class_start = false;
                continue;
            }
            '[' if in_class => {
                return Some(if next == Some(':') { "a [[:class:]]" } else { "a class inside a class" });
            }
            '[' => {
                in_class = true;
                class_start = true;
                i += 1;
                if cs.get(i) == Some(&'^') {
                    i += 1;
                }
                if cs.get(i) == Some(&']') {
                    return Some("a class starting with ]");
                }
                continue;
            }
            ']' if in_class && !class_start => in_class = false,
            '&' | '-' | '~' if in_class && next == Some(c) => return Some("a class set operation (&&, -- or ~~)"),
            '(' if !in_class && next == Some('?') && cs.get(i + 2) != Some(&':') => {
                return Some("a (?...) flag or group other than (?:...)");
            }
            _ => {}
        }
        class_start = false;
        i += 1;
    }
    None
}

/// The ASCII whitespace JavaScript's `\s` and Rust's agree on for a pattern
/// run on collapsed text, as the body of a class.
const ASCII_SPACE: &str = "\\t\\n\\x0B\\x0C\\r ";
const ASCII_WORD: &str = "0-9A-Za-z_";

/// `p` with `\d \w \s \b` and their negations given their ASCII meaning,
/// as JavaScript has them: a class written out, so a negation still
/// matches any non-ASCII character, and `(?-u:\b)` for a boundary. Every
/// other escape is copied as written. Only run on a pattern `unportable`
/// let through.
fn ascii_classes(p: &str) -> String {
    let cs: Vec<char> = p.chars().collect();
    let mut out = String::with_capacity(p.len() + 16);
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = false;
    while i < cs.len() {
        let c = cs[i];
        if c == '\\' {
            let Some(&n) = cs.get(i + 1) else {
                out.push(c);
                break;
            };
            i += 2;
            class_start = false;
            let (body, negated) = match n {
                'd' => ("0-9".to_string(), false),
                'D' => ("0-9".to_string(), true),
                'w' => (ASCII_WORD.to_string(), false),
                'W' => (ASCII_WORD.to_string(), true),
                's' => (ASCII_SPACE.to_string(), false),
                'S' => (ASCII_SPACE.to_string(), true),
                'b' if !in_class => {
                    out.push_str("(?-u:\\b)");
                    continue;
                }
                'B' if !in_class => {
                    out.push_str("(?-u:\\B)");
                    continue;
                }
                _ => {
                    out.push(c);
                    out.push(n);
                    continue;
                }
            };
            match (in_class, negated) {
                (true, false) => out.push_str(&body),
                (_, true) => out.push_str(&format!("[^{body}]")),
                (false, false) => out.push_str(&format!("[{body}]")),
            }
            continue;
        }
        out.push(c);
        i += 1;
        if c == '[' && !in_class {
            in_class = true;
            class_start = true;
            if cs.get(i) == Some(&'^') {
                out.push('^');
                i += 1;
            }
            continue;
        }
        if c == ']' && in_class && !class_start {
            in_class = false;
        }
        class_start = false;
    }
    out
}

/// Compiled with a size limit and the ASCII classes (`ascii_classes`). Err
/// is the regex error's last non-empty line, without a leading "error: ".
pub fn compiled(p: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(&ascii_classes(p)).size_limit(1 << 20).build().map_err(|e| {
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
