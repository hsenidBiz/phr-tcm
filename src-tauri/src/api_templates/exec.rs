//! Placeholder scanning and capture-path parsing shared by the template
//! checks (`api_templates::check`) and, once Task 3 lands, request
//! building itself - the same syntax has to agree in both places, so it
//! lives in exactly one of them.
//!
//! Only these two functions and `Seg` belong here for now; Task 3 adds
//! request building to this file.

/// One segment of a parsed capture path: `.name`, `[N]` or `[*]` (every
/// element of an array).
#[derive(Debug, Clone, PartialEq)]
pub enum Seg {
    Key(String),
    Index(usize),
    All,
}

/// Every `{{name}}` placeholder found in `s`, in order, names only (no
/// braces, surrounding whitespace trimmed). An unterminated `{{` is left
/// alone - a literal string, not a placeholder - the caller sees it as
/// plain text with no name to check.
pub fn placeholders(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim();
        if !name.is_empty() {
            out.push(name.to_string());
        }
        rest = &after[end + 2..];
    }
    out
}

/// Parses a capture path - `$.a`, `$.a[0]`, `$.a[*].id` - into segments.
/// Refused, each with a message naming the path: missing leading `$`, an
/// empty segment (so `$..a`, recursive descent, is refused rather than
/// silently accepted), an unclosed `[`, or an index that is neither `*`
/// nor all digits.
pub fn parse_capture_path(p: &str) -> Result<Vec<Seg>, String> {
    let mut chars = p.chars().peekable();
    if chars.next() != Some('$') {
        return Err(format!("capture path '{p}' must start with $"));
    }
    let mut segs = Vec::new();
    while let Some(&c) = chars.peek() {
        match c {
            '.' => {
                chars.next();
                let mut name = String::new();
                while let Some(&c2) = chars.peek() {
                    if c2 == '.' || c2 == '[' {
                        break;
                    }
                    name.push(c2);
                    chars.next();
                }
                if name.is_empty() {
                    return Err(format!("capture path '{p}' has an empty segment"));
                }
                segs.push(Seg::Key(name));
            }
            '[' => {
                chars.next();
                let mut inner = String::new();
                while let Some(&c2) = chars.peek() {
                    if c2 == ']' {
                        break;
                    }
                    inner.push(c2);
                    chars.next();
                }
                if chars.next() != Some(']') {
                    return Err(format!("capture path '{p}' has an unclosed ["));
                }
                if inner == "*" {
                    segs.push(Seg::All);
                } else if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                    segs.push(Seg::Index(inner.parse().expect("all-digit string parses as usize")));
                } else {
                    return Err(format!("capture path '{p}' has a bad index '[{inner}]'"));
                }
            }
            _ => return Err(format!("capture path '{p}' is malformed")),
        }
    }
    if segs.is_empty() {
        return Err(format!("capture path '{p}' names nothing after $"));
    }
    Ok(segs)
}
