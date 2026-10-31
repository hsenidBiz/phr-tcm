//! Expired passwords and other sign-ins, seen before a sign-in (backlog 40,
//! part 1): the two per-environment queries, what they decide, and the marks
//! the assistant's proposed logins carry.

use v2_lib::autorun::active_login::{
    self, discovery_gate, mark_logins, signed_in_elsewhere, ActiveRow, SIGNED_ELSEWHERE_MARK,
};
use v2_lib::autorun::password_expiry::{self, card_words, judge, query_with_users, Expiry, ExpiryRow, DUE_SOON_DAYS};
use v2_lib::autorun::setup_find::RowSource;

const TODAY: &str = "2026-10-30";

fn row(user: &str, due: Option<&str>, prompt: bool) -> ExpiryRow {
    ExpiryRow { user: user.into(), reset_due: due.map(str::to_string), prompt_change: prompt }
}

#[test]
fn judge_expired_when_due_is_past() {
    let r = row("a", Some("2026-10-29 00:00:00.000"), false);
    assert_eq!(judge(Some(&r), TODAY), Expiry::Expired { due: "2026-10-29".into() });
}

#[test]
fn judge_due_soon_under_seven_days() {
    assert_eq!(DUE_SOON_DAYS, 7);
    let soon = row("a", Some("2026-11-05"), false);
    assert_eq!(judge(Some(&soon), TODAY), Expiry::DueSoon { due: "2026-11-05".into(), days_left: 6 });
    let ok = row("a", Some("2027-02-02"), false);
    assert_eq!(judge(Some(&ok), TODAY), Expiry::Ok { due: "2027-02-02".into(), days_left: 95 });
    // Seven days left is not yet "under 7".
    let seven = row("a", Some("2026-11-06"), false);
    assert!(matches!(judge(Some(&seven), TODAY), Expiry::Ok { days_left: 7, .. }));
    // Due today is not yet past.
    let today = row("a", Some("2026-10-30"), false);
    assert!(matches!(judge(Some(&today), TODAY), Expiry::DueSoon { days_left: 0, .. }));
}

#[test]
fn judge_forced_change_wins() {
    let r = row("a", Some("2027-02-02"), true);
    assert_eq!(judge(Some(&r), TODAY), Expiry::ForcedChange);
}

#[test]
fn judge_unknown_when_no_row_or_no_database() {
    assert!(matches!(judge(None, TODAY), Expiry::Unknown(_)));
    let blank = row("a", None, false);
    assert!(matches!(judge(Some(&blank), TODAY), Expiry::Unknown(_)));
    let junk = row("a", Some("not a date"), false);
    assert!(matches!(judge(Some(&junk), TODAY), Expiry::Unknown(_)));
    assert!(!matches!(judge(None, TODAY), Expiry::Expired { .. }));
}

#[test]
fn card_words_say_the_date_and_days() {
    let r = row("a", Some("2027-02-02"), false);
    assert_eq!(card_words(&judge(Some(&r), "2026-10-27")), "password due 2027-02-02, 98 days");
    let gone = row("a", Some("2026-10-01"), false);
    assert_eq!(card_words(&judge(Some(&gone), TODAY)), "password expired 2026-10-01");
}

#[test]
fn query_with_users_quotes_and_doubles_quotes() {
    let sql = "SELECT u AS user_id FROM t WHERE u IN ({users})";
    let out = query_with_users(sql, &["Automation51".to_string(), "o'neil".to_string()]).unwrap();
    assert_eq!(out, "SELECT u AS user_id FROM t WHERE u IN (N'Automation51', N'o''neil')");
    // A placeholder the caller wrote inside quotes is refused.
    let err = query_with_users("SELECT 1 FROM t WHERE u = '{users}'", &["a".to_string()]).unwrap_err();
    assert!(err.contains("outside quotes"), "{err}");
    // Another placeholder is not allowed, and the list is never empty.
    assert!(query_with_users("SELECT 1 FROM t WHERE u IN ({users}) AND p = {prefix}", &["a".to_string()]).is_err());
    assert!(query_with_users(sql, &[]).is_err());
    // The query has to use the list at all.
    assert!(query_with_users("SELECT 1 FROM t", &["a".to_string()]).is_err());
}

#[test]
fn query_refuses_a_write_or_a_second_statement() {
    for bad in [
        "DELETE FROM t WHERE u IN ({users})",
        "UPDATE t SET p = 1 WHERE u IN ({users})",
        "SELECT 1 FROM t WHERE u IN ({users}); DELETE FROM t",
        "SELECT 1 FROM t WHERE u IN ({users}); SELECT 2",
        "EXEC sp_who {users}",
    ] {
        assert!(password_expiry::check_query(bad).is_err(), "{bad}");
        assert!(query_with_users(bad, &["a".to_string()]).is_err(), "{bad}");
    }
    assert!(password_expiry::check_query("SELECT 1 FROM t WHERE u IN ({users})").is_ok());
    assert!(password_expiry::check_query("   ").is_ok(), "an empty query is not checked, not refused");
}

fn active_row(since: &str, to: &str) -> ActiveRow {
    ActiveRow { since: since.into(), logged_to: to.into() }
}

#[test]
fn signed_in_elsewhere_ignores_our_own_session() {
    let rows = [active_row("2026-10-30 09:40:12.000", "ehrm")];
    let said = signed_in_elsewhere("Automation01", &rows, false).unwrap();
    assert_eq!(said, "Automation01 is signed in elsewhere since 09:40 (ehrm) - signing in will log that session out");
    assert_eq!(signed_in_elsewhere("Automation01", &rows, true), None);
    assert_eq!(signed_in_elsewhere("Automation01", &[], false), None);
}

#[test]
fn flag_zero_rows_never_count() {
    // The query itself filters on the flag; the documented text must say so.
    let guide = v2_lib::autorun::guide::autorun_guide();
    assert!(guide.contains("ACTLOG_ACT_FLG = 1"), "the guide's active session query must filter on ACTLOG_ACT_FLG = 1");
    assert!(active_login::check_query(
        "SELECT ACTLOG_DATETIME AS since, ACTLOG_LOGGED_TO AS logged_to FROM t WHERE ACTLOG_USER_ID = {user} AND ACTLOG_ACT_FLG = 1"
    )
    .is_ok());
    let sent = active_login::query_with_user("SELECT 1 FROM t WHERE u = {user}", "Automation01").unwrap();
    assert_eq!(sent, "SELECT 1 FROM t WHERE u = N'automation01'");
    assert!(active_login::query_with_user("DELETE FROM t WHERE u = {user}", "a").is_err());
}

struct Answers {
    expiry: Result<String, String>,
    active: Result<String, String>,
}

impl RowSource for Answers {
    async fn select(&self, sql: &str) -> Result<String, String> {
        if sql.contains("reset_due") { self.expiry.clone() } else { self.active.clone() }
    }
}

const EXPIRY_Q: &str = "SELECT u AS user_id, d AS reset_due, c AS prompt_change FROM t WHERE u IN ({users})";
const ACTIVE_Q: &str = "SELECT s AS since, l AS logged_to FROM a WHERE u = {user}";

fn expiry_answer(rows: &[&str]) -> Result<String, String> {
    Ok(format!(
        "user_id\treset_due\tprompt_change\n-------\t---------\t-------------\n{}\n\n({} rows affected)\n",
        rows.join("\n"),
        rows.len()
    ))
}

fn active_answer(rows: &[&str]) -> Result<String, String> {
    Ok(format!("since\tlogged_to\n-----\t---------\n{}\n\n({} rows affected)\n", rows.join("\n"), rows.len()))
}

#[tokio::test]
async fn a_signed_in_elsewhere_account_is_refused_for_a_discovery() {
    let db = Answers {
        expiry: expiry_answer(&["Automation01\t2027-02-02 00:00:00.000\t0"]),
        active: active_answer(&["2026-10-30 09:40:12.000\tehrm"]),
    };
    let gate = discovery_gate(&db, EXPIRY_Q, ACTIVE_Q, "Automation01", "Automation01", false, TODAY).await;
    let refused = gate.refused.expect("refused");
    assert!(refused.contains("is signed in elsewhere since 09:40 (ehrm)"), "{refused}");
    assert!(refused.contains("another account"), "{refused}");
    // Our own session does not count.
    let gate = discovery_gate(&db, EXPIRY_Q, ACTIVE_Q, "Automation01", "Automation01", true, TODAY).await;
    assert!(gate.refused.is_none());
}

#[tokio::test]
async fn a_flag_zero_account_is_not_refused() {
    let db = Answers { expiry: expiry_answer(&["a\t2027-02-02\t0"]), active: active_answer(&[]) };
    let gate = discovery_gate(&db, EXPIRY_Q, ACTIVE_Q, "a", "a", false, TODAY).await;
    assert!(gate.refused.is_none() && gate.warnings.is_empty(), "{:?}", gate.warnings);
}

#[tokio::test]
async fn a_discovery_warns_for_an_expired_or_due_soon_password() {
    let db = Answers { expiry: expiry_answer(&["a\t2026-10-01\t0"]), active: active_answer(&[]) };
    let gate = discovery_gate(&db, EXPIRY_Q, ACTIVE_Q, "a", "a", false, TODAY).await;
    assert!(gate.refused.is_none());
    assert_eq!(gate.warnings.len(), 1);
    assert!(gate.warnings[0].contains("password expired 2026-10-01"), "{:?}", gate.warnings);
    let db = Answers { expiry: expiry_answer(&["a\t2026-11-02\t0"]), active: active_answer(&[]) };
    let gate = discovery_gate(&db, EXPIRY_Q, ACTIVE_Q, "a", "a", false, TODAY).await;
    assert!(gate.warnings[0].contains("password due 2026-11-02, 3 days"), "{:?}", gate.warnings);
}

#[tokio::test]
async fn a_database_that_does_not_answer_never_blocks_and_is_said_once() {
    let db = Answers { expiry: Err("could not connect to srv-secret".into()), active: Err("could not connect to srv-secret".into()) };
    let gate = discovery_gate(&db, EXPIRY_Q, ACTIVE_Q, "a", "a", false, TODAY).await;
    assert!(gate.refused.is_none());
    assert_eq!(gate.warnings.len(), 1, "{:?}", gate.warnings);
    assert!(gate.warnings[0].contains("not checked"), "{:?}", gate.warnings);
    assert!(!gate.warnings[0].contains("srv-secret"), "no transport words: {:?}", gate.warnings);
}

#[tokio::test]
async fn queries_left_empty_check_nothing() {
    let db = Answers { expiry: Err("never asked".into()), active: Err("never asked".into()) };
    let gate = discovery_gate(&db, "", "  ", "a", "a", false, TODAY).await;
    assert!(gate.refused.is_none() && gate.warnings.is_empty());
}

/// `busy` is signed in elsewhere; every other login is not.
struct PerUser(Answers);

impl RowSource for PerUser {
    async fn select(&self, sql: &str) -> Result<String, String> {
        if sql.contains("reset_due") {
            self.0.select(sql).await
        } else if sql.contains("N'busy'") {
            active_answer(&["2026-10-30 08:15:00.000\tehrm"])
        } else {
            active_answer(&[])
        }
    }
}

fn logins() -> Vec<(String, String)> {
    ["old", "soon", "fine", "busy"].iter().map(|k| (k.to_string(), k.to_string())).collect()
}

fn per_user() -> PerUser {
    PerUser(Answers {
        expiry: expiry_answer(&["old\t2026-09-01\t0", "soon\t2026-11-01\t0", "fine\t2027-03-01\t0", "busy\t2027-03-01\t0"]),
        active: Err("unused".into()),
    })
}

#[tokio::test]
async fn proposed_logins_are_marked_and_the_usable_come_first() {
    let marks = mark_logins(&per_user(), EXPIRY_Q, ACTIVE_Q, &logins(), &|_| false, TODAY).await;
    let by_key = |k: &str| marks.marks_of(k);
    assert_eq!(by_key("old"), vec!["expired".to_string()]);
    assert_eq!(by_key("soon"), vec!["due soon".to_string()]);
    assert_eq!(by_key("busy"), vec![SIGNED_ELSEWHERE_MARK.to_string()]);
    assert!(by_key("fine").is_empty());
    let keys: Vec<String> = logins().into_iter().map(|l| l.0).collect();
    let order = marks.usable_first(&keys);
    assert_eq!(order, vec!["fine", "soon", "busy", "old"].into_iter().map(String::from).collect::<Vec<_>>());
    assert!(marks.note.is_none());
}

#[tokio::test]
async fn a_login_whose_session_is_ours_is_not_marked_signed_in_elsewhere() {
    let marks = mark_logins(&per_user(), EXPIRY_Q, ACTIVE_Q, &logins(), &|key| key == "busy", TODAY).await;
    assert!(marks.marks_of("busy").is_empty());
}

#[tokio::test]
async fn proposed_logins_without_a_database_are_unmarked_and_said_once() {
    let db = Answers { expiry: Err("down".into()), active: Err("down".into()) };
    let marks = mark_logins(&db, EXPIRY_Q, ACTIVE_Q, &logins(), &|_| false, TODAY).await;
    assert!(marks.marks.iter().all(|(_, m)| m.is_empty()));
    let note = marks.note.expect("said once");
    assert!(note.contains("not checked"), "{note}");
    // Nothing set: nothing asked, nothing said.
    let marks = mark_logins(&db, "", "", &logins(), &|_| false, TODAY).await;
    assert!(marks.note.is_none());
}
