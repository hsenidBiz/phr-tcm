//! The one HTML escaper every page and HTML fragment this app writes goes
//! through. Five copies of it once lived beside their callers.

/// Text for HTML element content or a `"`-quoted attribute value.
pub fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `esc`, plus the single quote - for a value going into a `'`-quoted
/// attribute (`esc` alone leaves a raw `'` that would close the attribute
/// early and let the rest of the value run as markup).
pub fn esc_attr(text: &str) -> String {
    esc(text).replace('\'', "&#39;")
}
