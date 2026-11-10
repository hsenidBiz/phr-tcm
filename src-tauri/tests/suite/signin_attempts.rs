//! The cap on the assistant's sign-in attempts: three per account for one
//! sign-in problem, counted in memory.

use crate::serial;
use std::path::Path;
use v2_lib::autorun::signin_attempts::{attempts_used, close, forget_all, is_open, take, take_if_open, MOST_SIGN_IN_ATTEMPTS};

#[test]
fn three_attempts_then_refused_and_a_close_starts_over() {
    let _g = serial::sign_in_attempts();
    forget_all();
    let root = Path::new("data-a");
    assert_eq!(MOST_SIGN_IN_ATTEMPTS, 3);

    assert_eq!(take(root, "sup"), Ok(1));
    assert_eq!(take(root, "sup"), Ok(2));
    assert_eq!(take(root, "sup"), Ok(3));
    assert_eq!(take(root, "sup"), Err(attempts_used("sup")));
    // At the cap it stays at the cap.
    assert_eq!(take(root, "sup"), Err(attempts_used("sup")));
    assert!(attempts_used("sup").contains("\"sup\" has had 3 sign-in attempts"));

    // Another account has no open problem: a conditional take leaves it closed.
    assert_eq!(take_if_open(root, "other"), Ok(false));
    assert!(!is_open(root, "other"));
    // An open one is counted.
    assert!(is_open(root, "sup"));

    close(root, "sup");
    assert!(!is_open(root, "sup"));
    assert_eq!(take(root, "sup"), Ok(1));
    assert_eq!(take_if_open(root, "sup"), Ok(true));
    assert_eq!(take(root, "sup"), Ok(3));

    // Another data folder counts apart.
    assert_eq!(take(Path::new("data-b"), "sup"), Ok(1));
    forget_all();
}
