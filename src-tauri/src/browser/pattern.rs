//! The patterns a check can carry (`expect_text`'s and `expect_attribute`'s
//! `matches`). Rust's `regex` runs in linear time, so a pattern cannot hang
//! a run; what is refused here is a pattern that a Playwright export would
//! read differently (JavaScript has no `(?i)`, `\A`, `\p{..}` or
//! `[[:alpha:]]` in the same form), one that is too long to read, and one
//! that fits any text at all, which can never fail.
//!
//! The run follows what the export can do. JavaScript's `\w \d \b` (and
//! their negations) are ASCII-only, Rust's are Unicode, so a pattern is
//! compiled with them rewritten to their ASCII meaning (`ascii_classes`):
//! `^\w+ \w+$` does not match "Jose Silva" with an accent, in a run or in the
//! export. JavaScript's `\s` is its own set (the ASCII spaces, the no-break
//! and Unicode spaces, the line and paragraph separators, the byte order
//! mark) and its `.` leaves out `\n`, `\r` and the two separators, where
//! Rust's leaves out `\n` only: both are written out the same way. Literal
//! non-ASCII text in a pattern still matches itself.
//!
//! Matching is `Regex::is_match`: not anchored unless the writer writes `^`
//! or `$`, case-sensitive. Text is matched with its whitespace collapsed (so
//! no newline is left for a pattern to cross); an attribute is matched raw,
//! so it can hold any of the spaces and line ends above.

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

/// Exactly JavaScript's `\s`, as the body of a class: tab, line feed,
/// vertical tab, form feed, carriage return, space, no-break space, the
/// Ogham space mark, U+2000 to U+200A, the line and paragraph separators,
/// the narrow no-break space, the medium mathematical space, the ideographic
/// space and the byte order mark. (Rust's own `\s` has U+0085 besides, and
/// lacks U+FEFF.)
const JS_SPACE: &str =
    "\\t\\n\\x0B\\x0C\\r \\x{A0}\\x{1680}\\x{2000}-\\x{200A}\\x{2028}\\x{2029}\\x{202F}\\x{205F}\\x{3000}\\x{FEFF}";
/// JavaScript's `.` outside a class: anything but a line end (`\n`, `\r`,
/// U+2028, U+2029). Rust's leaves out `\n` only.
const JS_DOT: &str = "[^\\n\\r\\x{2028}\\x{2029}]";
const ASCII_WORD: &str = "0-9A-Za-z_";

/// `p` with `\d \w \b` and their negations given their ASCII meaning, and
/// `\s` and `.` given JavaScript's, as JavaScript has them: a class written
/// out, so a negation still matches any non-ASCII character, and `(?-u:\b)`
/// for a boundary. Every other escape is copied as written. Only run on a
/// pattern `unportable` let through.
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
                's' => (JS_SPACE.to_string(), false),
                'S' => (JS_SPACE.to_string(), true),
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
        i += 1;
        if c == '.' && !in_class {
            out.push_str(JS_DOT);
            class_start = false;
            continue;
        }
        out.push(c);
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

/// The most forms one word may take: each optional letter or group doubles them.
const MAX_WORD_FORMS: usize = 8;

fn flush_word(forms: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    if forms[0].chars().count() >= 3 {
        let mut word: Vec<String> = Vec::new();
        for f in forms.iter() {
            let f = f.to_lowercase();
            if !word.contains(&f) {
                word.push(f);
            }
        }
        if !out.contains(&word) {
            out.push(word);
        }
    }
    *forms = vec![String::new()];
}

/// The word being read with its last letter optional (`s?`, `s*`, `s{0,1}`): each form with and without it.
fn last_letter_optional(forms: &mut Vec<String>) {
    if forms[0].is_empty() || forms.len() * 2 > MAX_WORD_FORMS {
        return;
    }
    let mut next: Vec<String> = Vec::new();
    for f in forms.iter() {
        let mut shorter = f.clone();
        shorter.pop();
        for g in [shorter, f.clone()] {
            if !next.contains(&g) {
                next.push(g);
            }
        }
    }
    *forms = next;
}

/// A `(` was read and `start` is the index after it. The group's own letters join the word being
/// read, as an extra form when the group is optional (`Record(s)?`); any other group is a break,
/// and an optional one that is not plain letters is skipped whole, its words being optional.
/// Returns the index to go on from.
fn read_group(chars: &[char], start: usize, forms: &mut Vec<String>, out: &mut Vec<Vec<String>>) -> usize {
    let mut depth = 1usize;
    let mut j = start;
    while j < chars.len() {
        match chars[j] {
            '\\' => j += 1,
            '[' => {
                j += 1;
                while j < chars.len() && chars[j] != ']' {
                    if chars[j] == '\\' {
                        j += 1;
                    }
                    j += 1;
                }
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        j += 1;
    }
    if j >= chars.len() {
        flush_word(forms, out);
        return start;
    }
    let inner: String = chars[start..j].iter().collect();
    let inner = inner.strip_prefix("?:").unwrap_or(&inner);
    let plain = !inner.is_empty() && inner.chars().all(char::is_alphanumeric);
    let after = j + 1;
    match chars.get(after) {
        Some('?' | '*') => {
            let mut end = after + 1;
            if chars.get(end) == Some(&'?') {
                end += 1;
            }
            if plain && !forms[0].is_empty() && forms.len() * 2 <= MAX_WORD_FORMS {
                let longer: Vec<String> = forms.iter().map(|f| format!("{f}{inner}")).collect();
                forms.extend(longer);
            } else {
                flush_word(forms, out);
            }
            end
        }
        // A repeat count: whatever the group holds may come any number of times, or none.
        Some('{') => {
            flush_word(forms, out);
            after
        }
        _ if plain => {
            for f in forms.iter_mut() {
                f.push_str(inner);
            }
            after
        }
        _ => {
            flush_word(forms, out);
            start
        }
    }
}

/// The words written in a pattern's literal text, lowercased, each with every form an optional part
/// allows: runs of 3 or more letters or digits outside `[...]` classes and `{..}` counts, broken by any
/// metacharacter or escape (`\d`, `\.`, `\b` ...). A letter or a group of letters that `?`, `*` or `{0,..}`
/// makes optional gives the word both ways: `participants?` is "participant" and "participants",
/// `Records? saved` is "record"/"records" and "saved". The first form of a word has none of its optional
/// parts and must be 3 or more characters. In order, each once.
pub fn literal_word_forms(p: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = p.chars().collect();
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut forms: Vec<String> = vec![String::new()];
    // The character before was a quantifier: a `?` now is its lazy mark, not a new quantifier.
    let mut lazy_ok = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        let after_quantifier = std::mem::take(&mut lazy_ok);
        match c {
            // An escape is a class or a punctuation mark, never a word.
            '\\' => {
                i += 1;
                flush_word(&mut forms, &mut out);
            }
            '[' => {
                flush_word(&mut forms, &mut out);
                while i < chars.len() {
                    let k = chars[i];
                    i += 1;
                    match k {
                        '\\' => i += 1,
                        ']' => break,
                        _ => {}
                    }
                }
            }
            '(' => i = read_group(&chars, i, &mut forms, &mut out),
            '?' | '*' | '+' => {
                if c == '?' && after_quantifier {
                    continue;
                }
                lazy_ok = true;
                if c != '+' {
                    last_letter_optional(&mut forms);
                }
            }
            '{' => {
                let end = chars[i..].iter().position(|k| *k == '}').map_or(chars.len(), |n| i + n);
                let body: String = chars[i..end].iter().collect();
                i = (end + 1).min(chars.len());
                lazy_ok = true;
                if body.starts_with('0') {
                    last_letter_optional(&mut forms);
                } else if body != "1" && !body.starts_with("1,") {
                    // A letter repeated a set number of times: only what comes before it is certain.
                    forms.iter_mut().for_each(|f| {
                        f.pop();
                    });
                    flush_word(&mut forms, &mut out);
                }
            }
            c if c.is_alphanumeric() => forms.iter_mut().for_each(|f| f.push(c)),
            _ => flush_word(&mut forms, &mut out),
        }
    }
    flush_word(&mut forms, &mut out);
    out
}

/// [`literal_word_forms`] with each word's required form only: the words every match writes.
pub fn literal_words(p: &str) -> Vec<String> {
    literal_word_forms(p).into_iter().map(|mut w| w.swap_remove(0)).collect()
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
