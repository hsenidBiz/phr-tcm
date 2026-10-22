//! The four role texts (writer, healer, case rewriter, auditor), which call
//! starts which role, and which cases a save repairs.

use serde_json::json;
use v2_lib::ai_tools::AUTORUN_TOOLS;
use v2_lib::autorun::failures::{CLASSIFY_FIRST, REPAIR_BOUNDS};
use v2_lib::autorun::roles::{cases_repaired_by, role_for_call, Role, ROLE_MAX_CHARS, ROLE_MAX_LINES};

/// A text with every run of whitespace read as one space, so a sentence
/// wrapped over lines still matches.
fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn holds(role: Role, needles: &[&str]) {
    let text = flat(&role.text());
    for n in needles {
        assert!(text.contains(&flat(n)), "the {} text lacks {n:?}", role.name());
    }
}

#[test]
fn the_four_roles_have_keys_names_and_texts() {
    let keys: Vec<_> = Role::ALL.iter().map(|r| r.key()).collect();
    assert_eq!(keys, ["writer", "healer", "case-rewriter", "auditor"]);
    let names: Vec<_> = Role::ALL.iter().map(|r| r.name()).collect();
    assert_eq!(names, ["writer", "healer", "case rewriter", "auditor"]);
    assert_eq!(Role::from_key(" Healer "), Some(Role::Healer));
    assert_eq!(Role::from_key("case rewriter"), Some(Role::CaseRewriter));
    assert_eq!(Role::from_key("case_rewriter"), Some(Role::CaseRewriter));
    assert_eq!(Role::from_key("planner"), Some(Role::Writer));
    assert_eq!(Role::from_key("generator"), Some(Role::Writer));
    assert_eq!(Role::from_key("refactorer"), Some(Role::CaseRewriter));
    assert_eq!(Role::from_key("doc-drift-auditor"), Some(Role::Auditor));
    assert_eq!(Role::from_key("tester"), None);
    for r in Role::ALL {
        let t = r.text();
        assert!(t.starts_with(&format!("## The {}'s rules", r.name())), "{}", r.name());
        assert!(!t.contains('\r'), "{}", r.name());
    }
}

#[test]
fn each_role_is_short() {
    for r in Role::ALL {
        let t = r.text();
        assert!(t.lines().count() <= ROLE_MAX_LINES, "{} has {} lines", r.name(), t.lines().count());
        assert!(t.chars().count() <= ROLE_MAX_CHARS, "{} has {} chars", r.name(), t.chars().count());
    }
}

#[test]
fn each_role_holds_whatever_the_tools() {
    for r in Role::ALL {
        assert!(flat(&r.text()).contains("whatever tools you have"), "{}", r.name());
    }
}

#[test]
fn the_writer_carries_the_owners_rules() {
    holds(
        Role::Writer,
        &[
            "source of truth",
            "never write, reword or extend",
            "settled before you explore",
            "One script per case",
            "from a clean start",
            "never assumed",
            "held back",
            "never scripted around",
            "Never invent an account, a menu path or a record",
            "a sub-agent cannot ask anyone",
            "Known quirks",
            "Carry out every step live before you write it",
            "Never guess a locator, an address or a menu path",
            "report the step word for word",
            "never fit a check to the page",
            "an icon too",
            "inside the row, panel or dialog the case names",
            "assumed (the case defines it indirectly)",
            "invented (you chose it where the case leaves it free)",
            "never swapped for another action without saying so",
            "No fixed wait, no page script and no retry",
            "test name prefix",
            "a replay of the whole case passes",
            "every assumed and invented value a save listed with its because",
        ],
    );
}

#[test]
fn the_healer_carries_the_bounds_word_for_word() {
    let text = flat(&Role::Healer.text());
    assert!(text.contains(&flat(CLASSIFY_FIRST)), "the classify-first sentence");
    let mut bullets = 0;
    for line in REPAIR_BOUNDS.lines().filter(|l| l.starts_with("- ")) {
        bullets += 1;
        assert!(text.contains(&flat(&line[2..])), "the bound {line:?}");
    }
    assert!(bullets >= 5, "{bullets} bounds");
    holds(
        Role::Healer,
        &[
            "a healer pointed at a missing precondition will keep rewriting assertions and locators until an empty or disabled page satisfies them, which produces a script that passes and proves nothing.",
            "the case stays failing",
            "never make it pass",
            "quirk candidate",
            "which bound stopped you",
            "every check given up",
            "nothing found that way goes into a script",
            "stronger model",
            "the case rewriter's rules",
            "until then its script is not checked",
            "the same failure came back after your last repair",
        ],
    );
}

#[test]
fn the_case_rewriter_carries_the_owners_rules() {
    holds(
        Role::CaseRewriter,
        &[
            "every step you do not declare must come back exactly as it was",
            "never changes the account",
            "change only the steps the rewrite touched",
            "a replay of the whole case as a separate step",
            "gives_up with why, and the person sees it",
            "never a check to give up",
            "Never let the page decide whether a failure counts",
            "A check never goes inside a guard",
            "is refused for a product bug",
            "never loosens page_errors",
            "No fixed wait, no page script and no retry",
            "never save a near copy",
            "must still work for every script that uses it",
            "Never edit the test case",
            "only when the person asks, never to make a run pass",
            "after 3 tries",
        ],
    );
}

#[test]
fn the_auditor_reports_and_never_edits() {
    holds(
        Role::Auditor,
        &[
            "You report; you never change a script, a component, a case or the run order",
            "FITTED",
            "MISSING",
            "WEAKER",
            "EXTRA",
            "PRECONDITION",
            "from_case, assumed, invented",
            "which changes no script",
            "pass with the app wrong",
            "fail with the app right",
            "Never mark a finding fixed",
        ],
    );
}

#[test]
fn every_backticked_name_in_a_role_is_a_tool() {
    let tools = v2_lib::mcp::all_tool_names();
    for r in Role::ALL {
        for (i, span) in r.text().split('`').enumerate() {
            if i % 2 == 1 && span.contains('_') {
                assert!(tools.iter().any(|t| t == span), "the {} text backticks {span:?}, which is no tool", r.name());
            }
        }
    }
}

#[test]
fn no_role_text_has_an_em_or_en_dash() {
    for r in Role::ALL {
        let t = r.text();
        assert!(!t.contains('\u{2014}') && !t.contains('\u{2013}'), "{}", r.name());
    }
}

#[test]
fn every_role_tool_is_an_auto_run_tool() {
    for t in ["start_autorun_discovery", "get_autorun_failures", "save_autorun_script", "save_autorun_component"] {
        assert!(AUTORUN_TOOLS.contains(&t), "{t}");
    }
}

#[test]
fn which_call_starts_which_role() {
    assert_eq!(role_for_call("start_autorun_discovery", &json!({})), Some(Role::Writer));
    assert_eq!(role_for_call("start_autorun_discovery", &json!({"mapping": true})), None);
    assert_eq!(role_for_call("start_autorun_discovery", &json!({"mapping": "true"})), None);
    // An explore session has no writer role: its rules come with the start's answer.
    assert_eq!(role_for_call("start_autorun_discovery", &json!({"explore": true})), None);
    assert_eq!(role_for_call("start_autorun_discovery", &json!({"explore": "true"})), None);
    assert_eq!(role_for_call("start_autorun_discovery", &json!({"explore": false})), Some(Role::Writer));
    assert_eq!(role_for_call("get_autorun_failures", &json!({})), Some(Role::Healer));
    let scripts = json!([{"case_id": 7, "steps": []}]);
    assert_eq!(role_for_call("save_autorun_script", &json!({"scripts": scripts})), None);
    assert_eq!(
        role_for_call(
            "save_autorun_script",
            &json!({"scripts": scripts, "edits": [{"case_id": 7, "steps": [2], "why": "x"}]})
        ),
        Some(Role::CaseRewriter)
    );
    assert_eq!(role_for_call("save_autorun_component", &json!({"name": "X"})), Some(Role::CaseRewriter));
    assert_eq!(role_for_call("get_test_cases", &json!({})), None);
}

#[test]
fn cases_repaired_by_reads_every_shape() {
    let edit = json!({"case_id": 7, "steps": [2], "why": "x"});
    assert_eq!(cases_repaired_by(&json!({"edits": [edit.clone()]})), vec![7]);
    assert_eq!(cases_repaired_by(&json!({"edits": edit.clone()})), vec![7]);
    assert_eq!(cases_repaired_by(&json!({"edits": json!([edit.clone()]).to_string()})), vec![7]);
    assert_eq!(
        cases_repaired_by(&json!({"scripts": {"scripts": [{"case_id": 7}], "edits": [edit.clone()]}})),
        vec![7]
    );
    assert_eq!(
        cases_repaired_by(&json!({"scripts": json!({"scripts": [], "edits": [edit.clone()]}).to_string()})),
        vec![7]
    );
    assert_eq!(
        cases_repaired_by(&json!({"scripts": [{"case_id": 9, "steps": [], "edits": [{"steps": [2], "why": "x"}]}]})),
        vec![9]
    );
    assert_eq!(cases_repaired_by(&json!({"edits": [{"case_id": "12", "steps": [2], "why": "x"}]})), vec![12]);
    assert_eq!(
        cases_repaired_by(&json!({"edits": [
            {"case_id": 9, "steps": [2], "why": "x"}, edit.clone(), {"case_id": 9, "steps": [3], "why": "y"}
        ]})),
        vec![7, 9]
    );
    assert_eq!(cases_repaired_by(&json!({"scripts": [{"case_id": 7, "steps": []}]})), Vec::<i32>::new());
}

#[test]
fn the_rules_of_the_moment_are_short_and_plain() {
    use v2_lib::autorun::roles::{
        with_rules, without_rules, RULES_DISCOVERY, RULES_EXPLORE, RULES_FAILURES, RULES_REPAIR_SAVE, RULES_REPLAY,
    };
    for rules in [RULES_DISCOVERY, RULES_REPAIR_SAVE, RULES_FAILURES, RULES_REPLAY, RULES_EXPLORE] {
        assert!(rules.starts_with("Rules now:\n- "), "{rules}");
        let lines: Vec<&str> = rules.lines().skip(1).collect();
        assert!((2..=3).contains(&lines.len()), "{rules}");
        for line in lines {
            assert!(line.starts_with("- "), "{line}");
            assert!(line.chars().count() < 220, "{line}");
        }
        assert!(!rules.contains('\u{2014}') && !rules.contains('\u{2013}'), "{rules}");
        assert!(!rules.contains('`'), "{rules}");
    }
    assert_eq!(
        with_rules((400, "x\n".into()), RULES_REPLAY),
        (400, format!("x\n\n{RULES_REPLAY}"))
    );
    assert_eq!(without_rules(&format!("x\n\n{RULES_REPLAY}"), RULES_REPLAY), "x");
    assert_eq!(without_rules("x\n\ny", RULES_REPLAY), "x\n\ny");
}

#[test]
fn the_pointer_holds_for_a_chat_that_never_saw_the_rules() {
    for r in Role::ALL {
        let p = v2_lib::autorun::roles::pointer(r);
        assert_eq!(
            p,
            format!(
                "(The {n}'s rules hold for this work whatever tools you use. If they did not come with an earlier answer in this chat, get_guide with topic autorun and role {k} gives them.)",
                n = r.name(),
                k = r.key()
            )
        );
        assert!(!p.contains("this session"), "{p}");
    }
}
