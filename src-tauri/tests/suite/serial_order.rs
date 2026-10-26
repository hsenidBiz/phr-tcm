//! The lock order written at the top of `serial.rs` - agents, then
//! log_tail, then autorun - is enforced here, not remembered. A test that
//! takes two of them in another order can deadlock with one that follows the
//! rule, and the whole suite then hangs with no failure message.

/// The three ordered locks, in the order a test must take them.
const ORDER: [&str; 3] = ["agents", "log_tail", "autorun"];

/// Every call of an ordered lock in `source`, grouped by the fn it is in:
/// `(fn name, the lock names in the order they are called)`. Comment lines
/// are skipped. Both `serial::x()` and `crate::serial::x()` are found,
/// because the first is a suffix of the second.
fn locks_by_fn(source: &str) -> Vec<(String, Vec<&'static str>)> {
    let needles: Vec<(String, &'static str)> =
        ORDER.iter().map(|name| (format!("serial::{name}()"), *name)).collect();
    let mut out: Vec<(String, Vec<&'static str>)> = Vec::new();
    for line in source.lines() {
        let code = line.trim_start();
        if code.starts_with("//") {
            continue;
        }
        if let Some(at) = code.find("fn ") {
            let before = &code[..at];
            let is_decl = before.split_whitespace().all(|w| w == "pub" || w == "async" || w.starts_with("pub("));
            if is_decl {
                let name: String =
                    code[at + 3..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                out.push((name, Vec::new()));
            }
        }
        let Some((_, calls)) = out.last_mut() else { continue };
        // Calls on one line are taken in the order they appear.
        let mut found: Vec<(usize, &'static str)> = Vec::new();
        for (needle, name) in &needles {
            found.extend(code.match_indices(needle.as_str()).map(|(i, _)| (i, *name)));
        }
        found.sort();
        calls.extend(found.into_iter().map(|(_, name)| name));
    }
    out
}

/// The fns in `source` that take two ordered locks against the order.
fn out_of_order(source: &str) -> Vec<String> {
    let rank = |name: &str| ORDER.iter().position(|n| *n == name).unwrap();
    locks_by_fn(source)
        .into_iter()
        .filter_map(|(name, calls)| {
            let bad = calls.windows(2).find(|w| w[0] != w[1] && rank(w[0]) > rank(w[1]))?;
            Some(format!("{name} takes {} before {}", bad[0], bad[1]))
        })
        .collect()
}

/// No test in the suite takes the ordered locks against the order.
#[test]
fn no_test_takes_the_serial_locks_out_of_order() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("suite");
    let mut found = Vec::new();
    let mut scanned = 0;
    for entry in std::fs::read_dir(&dir).expect("tests/suite") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a suite file");
        scanned += 1;
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        found.extend(out_of_order(&source).into_iter().map(|what| format!("{file}: {what}")));
    }
    assert!(scanned > 50, "the scan found only {scanned} suite files");
    assert!(
        found.is_empty(),
        "lock order is agents, log_tail, autorun (see serial.rs); out of order:\n{}",
        found.join("\n")
    );
}

/// A sample source: `<agents>`, `<log_tail>` and `<autorun>` stand for the
/// calls, spelled the short way and the long way alternately, so this file
/// holds no call of its own for the scan above to find.
fn sample(text: &str) -> String {
    let mut long = false;
    let mut out = text.to_string();
    for name in ORDER {
        let token = format!("<{name}>");
        while let Some(at) = out.find(&token) {
            let call = if long { format!("crate::serial::{name}()") } else { format!("serial::{name}()") };
            long = !long;
            out.replace_range(at..at + token.len(), &call);
        }
    }
    out
}

/// The scan itself: it sees an inversion however the lock is spelled, and
/// leaves a test that follows the order, or takes one lock, alone.
#[test]
fn the_scan_finds_an_inversion_and_only_that() {
    let inverted = sample("#[test]\nfn a() {\n    let _l = <log_tail>;\n    let _g = <agents>;\n}\n");
    assert_eq!(out_of_order(&inverted), vec!["a takes log_tail before agents".to_string()]);
    let late = sample("async fn b() {\n    let _a = <autorun>;\n    let _l = <log_tail>;\n}\n");
    assert_eq!(out_of_order(&late), vec!["b takes autorun before log_tail".to_string()]);
    let one_line = sample("fn c() { let _x = (<autorun>, <agents>); }\n");
    assert_eq!(out_of_order(&one_line), vec!["c takes autorun before agents".to_string()]);
    let fine = sample(
        "fn d() {\n    let _g = <agents>;\n    let _l = <log_tail>;\n    let _a = <autorun>;\n}\n\
         fn e() {\n    // <autorun> then <agents> in a comment\n    let _a = <autorun>;\n}\n\
         fn f() {\n    let _a = <autorun>;\n}\n",
    );
    assert!(out_of_order(&fine).is_empty());
    // Two fns never lend each other their calls.
    let apart = sample("fn g() { let _a = <autorun>; }\nfn h() { let _g = <agents>; }\n");
    assert!(out_of_order(&apart).is_empty());
}
