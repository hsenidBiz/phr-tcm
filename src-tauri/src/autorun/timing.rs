//! How long a run watches for the prompts a signed-in page may or may not
//! show (`after_sign_in`'s `when_visible` steps). They are watched
//! together, in one window, not each waited out in turn.

/// The window after a saved session is reused or the home page is loaded
/// afresh: the page was already signed in, so whatever it shows is there
/// as soon as the signed-in marker is.
pub const PROMPT_WINDOW_MS: u64 = 1500;

/// The window after a fresh credential login. PeoplesHR's "another active
/// session" modal can arrive after the app shell (measured 187-288 ms
/// after it, live), so this one is longer.
pub const FRESH_LOGIN_WINDOW_MS: u64 = 5000;
