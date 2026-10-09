//! How long a run watches for the prompts a signed-in page may or may not
//! show (`after_sign_in`'s `when_visible` steps). They are watched
//! together, in one window, not each waited out in turn.

/// The window after a saved session is reused: the page was already
/// signed in, so whatever it shows is there as soon as the signed-in
/// marker is.
pub const PROMPT_WINDOW_MS: u64 = 1500;

/// The window after a fresh credential login. PeoplesHR's "another active
/// session" modal can arrive after the app shell (measured 187-288 ms
/// after it, live), so this one is longer.
pub const FRESH_LOGIN_WINDOW_MS: u64 = 5000;

/// The window after a trip home reloads the page (`nav::go_home`,
/// `nav::load_home`). The page was signed in before it loaded and the late
/// session modal only follows a fresh login, so what it shows is there
/// almost at once.
pub const HOME_PROMPT_WINDOW_MS: u64 = 500;

/// A trip to an area first tries the path's clicks from wherever the page
/// already is (`nav::go_to_module`). Each of those clicks, and the check
/// that the trip arrived, gets at most this long: a page the path cannot
/// start from gives up quickly and the trip goes home the old way.
pub const QUICK_TRY_MS: u64 = 3000;
