//! The setups file and the save rules (`autorun::setups`, and
//! `ai_bridge::setup_save_for`): a range of kept actions becomes a named
//! setup with inputs as templates, a find rule and the creates it made.
//!
//! Every test uses its own temp root, and the kept store is keyed by root, so
//! none needs a serial lock.

use crate::autorun_discovery::{root_with_recipe_and_account, ORG, PROJECT};
use serde_json::{json, Value};
use v2_lib::ai_bridge::{setup_save_for, setups_list_for};
use v2_lib::autorun::explore::{
    begin, record_change, Change, ChangeEntry, ExploreSession, SaveProof,
};
use v2_lib::autorun::kept::{self, KeptAction, KeptList, NOT_AN_EXPLORE};
use v2_lib::autorun::setup_find::{
    DbFind, FindRule, RowSource, NEEDS_A_FIND_RULE,
};
use v2_lib::autorun::setups::{
    build_setup, find_setup, fresh_value, list_answer, load_setups, no_such_setup, save, setups_path, InputAsked,
    ReturnsAsked, SaveAsked, SaveFrom, SetupCreate, NO_CREATE_IN_RANGE, ONLY_TEST_BUILDS,
};
use v2_lib::browser::actions::Action;
use v2_lib::browser::net_capture::{Body, Captured};

const GOOD_SQL: &str = "SELECT Id, Name FROM dbo.LeaveType WHERE Name LIKE {prefix} + N'%'";

fn act(v: Value) -> Action {
    serde_json::from_value(v).unwrap()
}

fn kept_at(n: u32, action: Value, started: u64, ended: u64) -> KeptAction {
    KeptAction {
        n,
        action: act(action),
        steps: None,
        area: Some("Leave".to_string()),
        started_ms: started,
        ended_ms: ended,
    }
}

/// The four actions of the plan's example: open the form, type the name,
/// save, check the row.
fn leave_list() -> KeptList {
    KeptList {
        session: "s".to_string(),
        started_area: Some("Leave".to_string()),
        actions: vec![
            kept_at(1, json!({"kind":"click","selector":"#new"}), 1000, 1100),
            kept_at(2, json!({"kind":"fill","selector":"#name","value":"AUTOTEST leave A"}), 1150, 1250),
            kept_at(3, json!({"kind":"click","selector":"#save"}), 1300, 1400),
            kept_at(
                4,
                json!({"kind":"expect_visible","selector":{"role":"row","name":"AUTOTEST leave A"}}),
                1500,
                1550,
            ),
        ],
        dropped: 0,
    }
}

fn entry(n: u32, kind: &str, id: &str, name: &str, saved_ms: u64, id_at: Option<&str>, verified: bool) -> ChangeEntry {
    ChangeEntry {
        n,
        at: "2026-10-27T00:00:00Z".to_string(),
        change: Change::Created { kind: kind.into(), id: id.into(), name: name.into(), verified },
        request: Some("POST /hr/leave/save".to_string()),
        undone: false,
        proof: if verified {
            Some(SaveProof { saved_ms, id_at: id_at.map(str::to_string) })
        } else {
            None
        },
    }
}

fn session_with(changes: Vec<ChangeEntry>) -> ExploreSession {
    ExploreSession {
        session: "s".to_string(),
        organization: ORG.to_string(),
        project: PROJECT.to_string(),
        environment: "env-00000000".to_string(),
        environment_name: "Test".to_string(),
        account: "admin".to_string(),
        started_at: "2026-10-27T00:00:00Z".to_string(),
        ended_at: None,
        changes,
        writes: vec![],
    }
}

fn leave_session() -> ExploreSession {
    session_with(vec![entry(1, "leave type", "274", "AUTOTEST leave A", 1350, Some("/data/Id"), true)])
}

fn db_rule() -> FindRule {
    FindRule {
        database: Some(DbFind { sql: GOOD_SQL.into(), id_column: "Id".into(), name_column: "Name".into() }),
        browser: None,
    }
}

fn name_input() -> InputAsked {
    InputAsked {
        action: 2,
        name: "leave_name".into(),
        description: "the leave type's name".into(),
        unique: true,
        names_the_record: true,
    }
}

fn asked(to: u32, inputs: Vec<InputAsked>, find: FindRule) -> SaveAsked {
    SaveAsked {
        name: "Leave type".into(),
        description: "makes one leave type".into(),
        from: 1,
        to,
        area: None,
        inputs,
        find,
        returns: None,
        why: None,
        remove: false,
    }
}

type Seen = dyn Fn(Option<&str>, &[Action]) -> Result<(), String>;

fn build(
    asked: &SaveAsked,
    kept: &KeptList,
    session: &ExploreSession,
    seen: &Seen,
) -> Result<v2_lib::autorun::setups::Built, Vec<String>> {
    build_with(asked, kept, session, seen, &[])
}

fn build_with(
    asked: &SaveAsked,
    kept: &KeptList,
    session: &ExploreSession,
    seen: &Seen,
    accounts: &[v2_lib::autorun::accounts::Account],
) -> Result<v2_lib::autorun::setups::Built, Vec<String>> {
    let allowed = |u: &str| u.starts_with("https://hr.example.internal/");
    let area_known = |a: &str| a == "Leave";
    build_setup(
        asked,
        &SaveFrom {
            kept,
            session,
            prefix: "AUTOTEST",
            allowed: &allowed,
            seen,
            area_known: &area_known,
            accounts,
            now: 1_760_000_000_000,
        },
    )
}

fn never_seen(_: Option<&str>, _: &[Action]) -> Result<(), String> {
    Err("not seen".into())
}

fn problems(r: Result<v2_lib::autorun::setups::Built, Vec<String>>) -> Vec<String> {
    r.expect_err("the save should be refused")
}

#[test]
fn a_setup_saves_its_range_with_inputs_as_templates() {
    let dir = root_with_recipe_and_account();
    let built = build(&asked(4, vec![name_input()], db_rule()), &leave_list(), &leave_session(), &never_seen)
        .expect("builds");
    assert_eq!(
        built.replaced,
        vec![
            v2_lib::autorun::setups::Replaced { input: "leave_name".into(), action: 2, field: "value".into() },
            v2_lib::autorun::setups::Replaced {
                input: "leave_name".into(),
                action: 4,
                field: "selector.name".into()
            },
        ]
    );
    assert!(!built.cut);
    let saved = save(dir.path(), ORG, PROJECT, built, None).expect("saves");
    assert_eq!((saved.saved.as_str(), saved.version, saved.changes), ("Leave type", 1, 0));

    assert!(setups_path(dir.path(), ORG, PROJECT).exists());
    let file = load_setups(dir.path(), ORG, PROJECT).unwrap();
    let s = find_setup(&file, "leave type").expect("found by name");
    assert_eq!(s.actions.len(), 4);
    assert_eq!(serde_json::to_value(&s.actions[1]).unwrap()["value"], "{{leave_name}}");
    assert_eq!(serde_json::to_value(&s.actions[3]).unwrap()["selector"]["name"], "{{leave_name}}");
    assert_eq!(
        s.creates,
        vec![SetupCreate {
            kind: "leave type".into(),
            name: "{{leave_name}}".into(),
            request: "POST /hr/leave/save".into(),
            id_at: "/data/Id".into(),
        }]
    );
    assert_eq!((s.version, s.changes, s.last_used_at, s.returns), (1, 0, 0, 0));
    assert_eq!(s.area.as_deref(), Some("Leave"));
    assert_eq!(s.inputs.len(), 1);
    assert!(s.inputs[0].unique && s.inputs[0].names_the_record);

    let components = v2_lib::autorun::components::components_path(dir.path(), ORG, PROJECT);
    assert!(!components.exists(), "the components file is never touched");
}

#[test]
fn every_problem_is_named_at_once() {
    let input = InputAsked { name: "leave_name".into(), ..name_input() };
    let mut list = leave_list();
    list.actions[1] = kept_at(2, json!({"kind":"fill","selector":"#name","value":"leave A"}), 1150, 1250);
    let said = problems(build(&asked(9, vec![input], FindRule::default()), &list, &leave_session(), &never_seen));
    assert_eq!(said.len(), 3, "{said:?}");
    let all = said.join(" | ");
    assert!(all.contains("test prefix AUTOTEST"), "{all}");
    assert!(all.contains("Action 9 was not kept"), "{all}");
    assert!(all.contains(NEEDS_A_FIND_RULE), "{all}");
}

#[test]
fn a_range_with_no_verified_create_is_refused() {
    let none = session_with(vec![]);
    let said = problems(build(&asked(4, vec![name_input()], db_rule()), &leave_list(), &none, &never_seen));
    assert_eq!(said, vec![NO_CREATE_IN_RANGE.to_string()]);

    let unverified = session_with(vec![entry(1, "leave type", "274", "AUTOTEST leave A", 1350, None, false)]);
    let said = problems(build(&asked(4, vec![name_input()], db_rule()), &leave_list(), &unverified, &never_seen));
    assert_eq!(said, vec![NO_CREATE_IN_RANGE.to_string()]);

    let outside = session_with(vec![entry(1, "leave type", "274", "AUTOTEST leave A", 5000, Some("/Id"), true)]);
    let said = problems(build(&asked(4, vec![name_input()], db_rule()), &leave_list(), &outside, &never_seen));
    assert_eq!(said, vec![NO_CREATE_IN_RANGE.to_string()]);
}

#[test]
fn a_create_without_a_readable_place_is_refused() {
    let text_only = session_with(vec![entry(1, "leave type", "274", "AUTOTEST leave A", 1350, None, true)]);
    let said = problems(build(&asked(4, vec![name_input()], db_rule()), &leave_list(), &text_only, &never_seen));
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("cannot be built from") && said[0].contains("leave type 274"), "{said:?}");
}

#[test]
fn a_navigate_outside_the_sites_or_built_from_an_input_is_refused() {
    let mut list = leave_list();
    list.actions[0] = kept_at(1, json!({"kind":"navigate","url":"https://elsewhere.example/x"}), 1000, 1100);
    let said = problems(build(&asked(4, vec![name_input()], db_rule()), &list, &leave_session(), &never_seen));
    assert_eq!(
        said,
        vec!["Action 1 goes to an address outside the sign-in recipe's sites.".to_string()]
    );

    let mut list = leave_list();
    list.actions[0] = kept_at(
        1,
        json!({"kind":"navigate","url":"https://hr.example.internal/leave/AUTOTEST leave A"}),
        1000,
        1100,
    );
    let said = problems(build(&asked(4, vec![name_input()], db_rule()), &list, &leave_session(), &never_seen));
    assert_eq!(said, vec!["An address cannot come from an input: action 1.".to_string()]);
}

/// Review Focus 4.
#[test]
fn a_short_or_chance_input_value_is_refused_or_listed() {
    let mut list = leave_list();
    list.actions[1] = kept_at(2, json!({"kind":"fill","selector":"#name","value":"AU"}), 1150, 1250);
    let said = problems(build(&asked(4, vec![name_input()], db_rule()), &list, &leave_session(), &never_seen));
    assert!(said.join(" ").contains("too short to stand for an input: at least 3 characters"), "{said:?}");

    let mut list = leave_list();
    list.actions[1] = kept_at(2, json!({"kind":"fill","selector":"#name","value":"AUTOTEST1"}), 1150, 1250);
    list.actions[3] = kept_at(
        4,
        json!({"kind":"expect_contains_text","selector":"#msg","value":"Saved AUTOTEST1 ok"}),
        1500,
        1550,
    );
    let input = InputAsked { names_the_record: false, ..name_input() };
    let built = build(&asked(4, vec![input], db_rule()), &list, &leave_session(), &never_seen).expect("builds");
    assert_eq!(serde_json::to_value(&built.setup.actions[3]).unwrap()["value"], "Saved {{leave_name}} ok");
    let places: Vec<(u32, &str)> = built.replaced.iter().map(|r| (r.action, r.field.as_str())).collect();
    assert_eq!(places, vec![(2, "value"), (4, "value")]);
}

/// Review Focus 5.
#[test]
fn a_number_the_list_dropped_is_refused_by_name() {
    let mut list = leave_list();
    list.dropped = 5;
    for (i, a) in list.actions.iter_mut().enumerate() {
        a.n = 6 + u32::try_from(i).unwrap();
    }
    let mut a = asked(9, vec![], db_rule());
    a.from = 3;
    let said = problems(build(&a, &list, &leave_session(), &never_seen));
    assert!(
        said.contains(&"Action 3 is no longer kept: the list keeps the last 200, from action 6.".to_string()),
        "{said:?}"
    );
}

#[test]
fn a_taken_name_needs_why_and_becomes_the_next_version() {
    let dir = root_with_recipe_and_account();
    let make = |name: &str| {
        let mut a = asked(4, vec![name_input()], db_rule());
        a.name = name.to_string();
        build(&a, &leave_list(), &leave_session(), &never_seen).unwrap()
    };
    save(dir.path(), ORG, PROJECT, make("Leave type"), None).unwrap();

    let refused = save(dir.path(), ORG, PROJECT, make("Leave  Type"), None).unwrap_err();
    assert!(refused.contains("is already saved") && refused.contains("\"why\""), "{refused}");

    let again = save(dir.path(), ORG, PROJECT, make("leave type"), Some("the form moved")).unwrap();
    assert_eq!((again.version, again.changes), (2, 1));
    let file = load_setups(dir.path(), ORG, PROJECT).unwrap();
    assert_eq!(file.setups.len(), 1);
}

/// Review Focus 1.
#[test]
fn a_damaged_setups_file_is_never_overwritten() {
    let dir = root_with_recipe_and_account();
    let path = setups_path(dir.path(), ORG, PROJECT);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ x").unwrap();

    let built = build(&asked(4, vec![name_input()], db_rule()), &leave_list(), &leave_session(), &never_seen)
        .unwrap();
    let refused = save(dir.path(), ORG, PROJECT, built, None).unwrap_err();
    assert!(refused.contains("projects/") && refused.contains("-setups.json could not be read"), "{refused}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ x");
    assert_eq!(load_setups(dir.path(), ORG, PROJECT).unwrap_err(), refused);
    assert!(!refused.contains(dir.path().to_str().unwrap()), "no full path: {refused}");
}

#[test]
fn a_missing_setups_file_is_an_empty_one() {
    let dir = root_with_recipe_and_account();
    assert!(load_setups(dir.path(), ORG, PROJECT).unwrap().setups.is_empty());
    assert_eq!(no_such_setup("x y"), "there is no setup called \"x y\" - list_setups lists them");
}

#[test]
fn a_browser_find_locator_must_have_been_used_or_seen() {
    let mut list = leave_list();
    list.actions[3] =
        kept_at(4, json!({"kind":"expect_visible","selector":{"role":"cell","name":"AUTOTEST"}}), 1500, 1550);
    let rule = |role: &str| FindRule {
        database: None,
        browser: Some(vec![
            act(json!({"kind":"click","selector":"#new"})),
            act(json!({"kind":"expect_visible","selector":{"role":role,"name":"{prefix}"}})),
        ]),
    };
    let input = InputAsked { names_the_record: false, ..name_input() };
    build(&asked(4, vec![input.clone()], rule("cell")), &list, &leave_session(), &never_seen)
        .expect("a locator the session used passes with the map unread");

    let said = problems(build(&asked(4, vec![input.clone()], rule("gridcell")), &list, &leave_session(), &never_seen));
    assert!(said.join(" ").contains("uses a locator this session never used and the map never saw"), "{said:?}");

    let seen_ok = |_: Option<&str>, _: &[Action]| Ok(());
    build(&asked(4, vec![input], rule("gridcell")), &list, &leave_session(), &seen_ok)
        .expect("a locator the map saw passes");
}

#[test]
fn a_use_component_is_saved_as_the_steps_it_ran() {
    let mut list = leave_list();
    list.actions[0] = KeptAction {
        n: 1,
        action: act(json!({"kind":"use_component","component":"Open form","inputs":{}})),
        steps: Some(vec![
            act(json!({"kind":"click","selector":"#menu"})),
            act(json!({"kind":"click","selector":"#new"})),
        ]),
        area: Some("Leave".into()),
        started_ms: 1000,
        ended_ms: 1100,
    };
    let built = build(&asked(4, vec![name_input()], db_rule()), &list, &leave_session(), &never_seen).unwrap();
    assert_eq!(built.setup.actions.len(), 5);
    assert!(built.setup.actions.iter().all(|a| !matches!(a, Action::UseComponent { .. })));
    assert_eq!(serde_json::to_value(&built.setup.actions[1]).unwrap()["selector"], "#new");
}

#[test]
fn returns_names_a_create_and_defaults_to_the_first() {
    let session = session_with(vec![
        entry(1, "leave type", "274", "AUTOTEST leave A", 1350, Some("/data/Id"), true),
        entry(2, "policy", "9", "AUTOTEST policy", 1360, Some("/Id"), true),
    ]);
    let input = InputAsked { names_the_record: false, ..name_input() };
    let built = build(&asked(4, vec![input.clone()], db_rule()), &leave_list(), &session, &never_seen).unwrap();
    assert_eq!((built.setup.creates.len(), built.setup.returns), (2, 0));

    let mut a = asked(4, vec![input.clone()], db_rule());
    a.returns = Some(ReturnsAsked { entry: 2 });
    assert_eq!(build(&a, &leave_list(), &session, &never_seen).unwrap().setup.returns, 1);

    a.returns = Some(ReturnsAsked { entry: 7 });
    let said = problems(build(&a, &leave_list(), &session, &never_seen));
    assert_eq!(said, vec!["Entry 7 is not a create these actions made.".to_string()]);
}

#[test]
fn fresh_value_is_prefix_name_and_stamp() {
    assert_eq!(fresh_value("AUTOTEST", "Leave type", 1_760_000_000_000), "AUTOTESTLeave-type-gj6k3cw");
    assert_eq!(fresh_value("AUTOTEST", "Leave type", 1_760_000_000_001), "AUTOTESTLeave-type-gj6k3cx");
}

/// C1: a setup builds only records named with the test prefix.
#[test]
fn a_setup_must_build_only_records_named_with_the_prefix() {
    // Recorded without the prefix, no input: refused.
    let off = session_with(vec![entry(1, "leave type", "274", "Annual Leave Test", 1350, Some("/data/Id"), true)]);
    let said = problems(build(&asked(4, vec![], db_rule()), &leave_list(), &off, &never_seen));
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with(ONLY_TEST_BUILDS), "{said:?}");

    // A unique input that is not what named the record does not help.
    let said = problems(build(&asked(4, vec![name_input()], db_rule()), &leave_list(), &off, &never_seen));
    assert!(said.iter().any(|s| s.starts_with(ONLY_TEST_BUILDS)), "{said:?}");

    // An input that is neither unique nor names the record cannot vouch for a name.
    let loose = InputAsked { unique: false, names_the_record: false, ..name_input() };
    let by_input = session_with(vec![entry(1, "leave type", "274", "AUTOTEST leave A", 1350, Some("/data/Id"), true)]);
    let said = problems(build(&asked(4, vec![loose], db_rule()), &leave_list(), &by_input, &never_seen));
    assert!(said.iter().any(|s| s.starts_with(ONLY_TEST_BUILDS)), "{said:?}");

    // Every create in the range, not only the one returned.
    let two = session_with(vec![
        entry(1, "leave type", "274", "AUTOTEST leave A", 1350, Some("/data/Id"), true),
        entry(2, "policy", "9", "Plain policy", 1360, Some("/Id"), true),
    ]);
    let mut a = asked(4, vec![], db_rule());
    a.returns = Some(ReturnsAsked { entry: 1 });
    let said = problems(build(&a, &leave_list(), &two, &never_seen));
    assert!(said.iter().any(|s| s.starts_with(ONLY_TEST_BUILDS)), "{said:?}");

    // A literal prefixed name, or a unique input that names it, passes.
    build(&asked(4, vec![], db_rule()), &leave_list(), &leave_session(), &never_seen).expect("a prefixed name");
    build(&asked(4, vec![name_input()], db_rule()), &leave_list(), &leave_session(), &never_seen)
        .expect("a unique input names it");
}

/// I2: a browser find rule's actions meet the checks the build's actions do.
#[test]
fn a_browser_find_rules_actions_get_the_build_checks() {
    let seen_ok = |_: Option<&str>, _: &[Action]| Ok(());
    let accounts = vec![v2_lib::autorun::accounts::Account {
        key: "admin".into(),
        label: "Admin".into(),
        username: "hr.admin".into(),
        password: "Sup3rSecret".into(),
    }];
    let input = InputAsked { names_the_record: false, ..name_input() };
    let rule = |actions: Vec<Value>| FindRule { database: None, browser: Some(actions.into_iter().map(act).collect()) };
    let check = json!({"kind":"expect_visible","selector":{"role":"row","name":"{prefix}"}});
    let said_for = |actions: Vec<Value>| {
        problems(build_with(
            &asked(4, vec![input.clone()], rule(actions)),
            &leave_list(),
            &leave_session(),
            &seen_ok,
            &accounts,
        ))
        .join(" | ")
    };

    let all = said_for(vec![json!({"kind":"navigate","url":"https://elsewhere.example/x"}), check.clone()]);
    assert!(all.contains("The browser find rule's action 1 goes to an address outside the sign-in recipe's sites"), "{all}");
    assert!(!all.contains("elsewhere.example"), "a host in the refusal: {all}");

    let all = said_for(vec![json!({"kind":"navigate","url":"file:///C:/x.html"}), check.clone()]);
    assert!(all.contains("action 1 goes to a local file"), "{all}");

    let all = said_for(vec![json!({"kind":"fill","selector":"#name","value":"Sup3rSecret"}), check.clone()]);
    assert!(all.contains("action 1 types a login"), "{all}");

    let all = said_for(vec![
        json!({"kind":"when_visible","selector":"#x","then":[{"kind":"sign_in","account":"admin"}]}),
        check.clone(),
    ]);
    assert!(all.contains("action 1 signs in"), "{all}");

    let all = said_for(vec![json!({"kind":"navigate","url":"https://hr.example.internal/leave?x=1"}), check.clone()]);
    assert!(all.contains("action 1 goes to an address with a query string"), "{all}");

    // And a build action that types a login.
    let mut list = leave_list();
    list.actions[1] = kept_at(2, json!({"kind":"fill","selector":"#name","value":"Sup3rSecret"}), 1150, 1250);
    let said = problems(build_with(&asked(4, vec![], db_rule()), &list, &leave_session(), &never_seen, &accounts));
    assert!(said.join(" ").contains("Action 2 types a login"), "{said:?}");
}

/// I3: nothing with a query string is ever kept.
#[test]
fn an_address_or_request_with_a_query_is_refused_at_save() {
    let mut list = leave_list();
    list.actions[0] = kept_at(
        1,
        json!({"kind":"navigate","url":"https://hr.example.internal/leave/edit?id=12&returnUrl=%2Fx&token=abc"}),
        1000,
        1100,
    );
    let said = problems(build(&asked(4, vec![name_input()], db_rule()), &list, &leave_session(), &never_seen));
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("Action 1 goes to an address with a query string"), "{said:?}");
    assert!(said[0].contains("clicking instead"), "{said:?}");
    assert!(!said[0].contains("token") && !said[0].contains("hr.example.internal"), "{said:?}");

    let mut list = leave_list();
    list.actions[0] = kept_at(1, json!({"kind":"open_tab","name":"t","url":"https://hr.example.internal/r?id=1"}), 1000, 1100);
    let said = problems(build(&asked(4, vec![name_input()], db_rule()), &list, &leave_session(), &never_seen));
    assert!(said.join(" ").contains("query string"), "{said:?}");

    let mut list = leave_list();
    list.actions[0] = kept_at(
        1,
        json!({"kind":"api_request","path":"/hr/leave/types","query":{"page":"2"},"expect":{"status":200}}),
        1000,
        1100,
    );
    let said = problems(build(&asked(4, vec![name_input()], db_rule()), &list, &leave_session(), &never_seen));
    assert!(said.join(" ").contains("Action 1 sends a request with a query"), "{said:?}");

    // The same address without a query is kept.
    let mut list = leave_list();
    list.actions[0] = kept_at(1, json!({"kind":"navigate","url":"https://hr.example.internal/leave/new"}), 1000, 1100);
    build(&asked(4, vec![name_input()], db_rule()), &list, &leave_session(), &never_seen).expect("no query");
}

// ---- The route: setup_save_for ----

struct Fake(Result<String, String>);

impl RowSource for Fake {
    async fn select(&self, _sql: &str) -> Result<String, String> {
        self.0.clone()
    }
}

/// A root with an explore session whose change list holds one verified
/// create saved at 1350, and the assistant `"a"`'s kept actions for it.
fn explored() -> (crate::autorun_discovery::TempDir, String) {
    let dir = root_with_recipe_and_account();
    let id = begin(dir.path(), ORG, PROJECT, "admin", 1_760_000_000_000).unwrap();
    let post = Captured {
        method: "POST".into(),
        url_path_query: "/hr/leave/save?x=1".into(),
        started_ms: 1350,
        status: Some(200),
        resp_body: Some(Body::Text(r#"{"data":{"Id":274,"Name":"AUTOTEST leave A"}}"#.to_string())),
        ..Default::default()
    };
    let change = Change::Created {
        kind: "leave type".into(),
        id: "274".into(),
        name: "AUTOTEST leave A".into(),
        verified: false,
    };
    record_change(dir.path(), &id, change, None, &[&post], 1000).unwrap();
    kept::started(dir.path(), "a", Some(&id), None);
    for k in leave_list().actions {
        kept::keep(dir.path(), &id, &k.action, None, None, k.started_ms, k.ended_ms);
    }
    (dir, id)
}

fn body() -> String {
    json!({
        "name": "Leave type",
        "description": "makes one leave type",
        "from": 1,
        "to": 4,
        "inputs": [{"action": 2, "name": "leave_name", "unique": true, "names_the_record": true}],
        "find": {"database": {"sql": GOOD_SQL, "id_column": "Id", "name_column": "Name"}},
    })
    .to_string()
}

#[tokio::test]
async fn saving_outside_an_explore_session_is_refused() {
    let dir = root_with_recipe_and_account();
    let (status, said) =
        setup_save_for(dir.path(), ORG, PROJECT, "nobody", &body(), None::<&Fake>, 5).await;
    assert_eq!((status, said.as_str()), (409, NOT_AN_EXPLORE));

    let (status, said) = setup_save_for(dir.path(), ORG, PROJECT, "nobody", "{ nope", None::<&Fake>, 5).await;
    assert_eq!(status, 400, "{said}");
}

/// Two saved setups: "Leave type" (both find rules, never used) and
/// "Policy" (browser rule only, used at 1_760_000_000_000).
fn two_setups(dir: &std::path::Path) {
    let browser_rule = vec![act(json!({"kind":"expect_visible","selector":{"role":"row","name":"{prefix}"}}))];
    let mut built = build(&asked(4, vec![name_input()], db_rule()), &leave_list(), &leave_session(), &never_seen)
        .unwrap();
    built.setup.find.browser = Some(browser_rule.clone());
    let mut policy = built.setup.clone();
    save(dir, ORG, PROJECT, built, None).unwrap();
    policy.name = "Policy".into();
    policy.find = FindRule { database: None, browser: Some(browser_rule) };
    policy.last_used_at = 1_760_000_000_000;
    let mut file = load_setups(dir, ORG, PROJECT).unwrap();
    file.setups.push(policy);
    std::fs::write(setups_path(dir, ORG, PROJECT), serde_json::to_string_pretty(&file).unwrap()).unwrap();
}

#[test]
fn list_setups_names_each_setup_without_values() {
    let dir = root_with_recipe_and_account();
    two_setups(dir.path());
    let said = list_answer(&load_setups(dir.path(), ORG, PROJECT).unwrap());
    let list = said["setups"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["name"], "Leave type");
    assert_eq!(list[0]["find"], json!(["database", "browser"]));
    assert_eq!(list[0]["last_used"], Value::Null);
    assert_eq!(list[0]["inputs"][0]["name"], "leave_name");
    assert_eq!(list[0]["inputs"][0]["unique"], true);
    assert_eq!(list[0]["inputs"][0]["names_the_record"], true);
    assert_eq!(list[0]["version"], 1);
    assert_eq!(list[1]["name"], "Policy");
    assert_eq!(list[1]["find"], json!(["browser"]));
    assert_eq!(list[1]["last_used"], "2025-10-09T08:53:20.000Z");
    let text = said.to_string();
    assert!(!text.contains("{{") && !text.contains("SELECT") && !text.contains("AUTOTEST leave A"), "{text}");
}

#[tokio::test]
async fn removing_a_setup_leaves_the_others() {
    let dir = root_with_recipe_and_account();
    two_setups(dir.path());
    // An owner with no kept list: removing needs no explore session and no why.
    let ask = json!({"name": "leave type", "remove": true}).to_string();
    let (status, said) = setup_save_for(dir.path(), ORG, PROJECT, "nobody", &ask, None::<&Fake>, 5).await;
    assert_eq!((status, said.as_str()), (200, r#"{"removed":"Leave type"}"#));
    let names: Vec<String> =
        load_setups(dir.path(), ORG, PROJECT).unwrap().setups.into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["Policy".to_string()]);

    let (status, said) = setup_save_for(dir.path(), ORG, PROJECT, "nobody", &ask, None::<&Fake>, 6).await;
    assert_eq!((status, said), (404, no_such_setup("leave type")));
}

#[tokio::test]
async fn remove_and_list_refuse_a_damaged_file() {
    let dir = root_with_recipe_and_account();
    let path = setups_path(dir.path(), ORG, PROJECT);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ x").unwrap();
    let ask = json!({"name": "leave type", "remove": true}).to_string();
    let (status, said) = setup_save_for(dir.path(), ORG, PROJECT, "nobody", &ask, None::<&Fake>, 5).await;
    assert_eq!(status, 409, "{said}");
    assert!(said.contains("-setups.json could not be read"), "{said}");
    let (status, said) = setups_list_for(dir.path(), ORG, PROJECT);
    assert_eq!(status, 409, "{said}");
    assert!(said.contains("-setups.json could not be read"), "{said}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ x");
}

#[tokio::test]
async fn the_database_rule_is_run_once_at_save_when_it_can() {
    let (dir, _) = explored();
    let failing = Fake(Err("x".into()));
    let (status, said) = setup_save_for(dir.path(), ORG, PROJECT, "a", &body(), Some(&failing), 5).await;
    assert_eq!(status, 400, "{said}");
    assert!(said.starts_with("The database find rule did not run"), "{said}");
    assert!(!setups_path(dir.path(), ORG, PROJECT).exists(), "nothing saved");

    let (status, said) = setup_save_for(dir.path(), ORG, PROJECT, "a", &body(), None::<&Fake>, 5).await;
    assert_eq!(status, 200, "{said}");
    let v: Value = serde_json::from_str(&said).unwrap();
    assert_eq!(v["database_checked"], "not run: Database Read Access is off");
    assert_eq!(v["saved"], "Leave type");
    assert_eq!(v["actions"], 4);
    assert_eq!(v["returns"], "leave type");
    assert_eq!(v["inputs"], json!(["leave_name"]));
}

#[tokio::test]
async fn a_found_or_missing_record_is_said_at_save() {
    let (dir, _) = explored();
    let found = Fake(Ok("Id\tName\n--\t----\n9\tAUTOTEST leave A\n\n(1 rows affected)\n".into()));
    let (status, said) = setup_save_for(dir.path(), ORG, PROJECT, "a", &body(), Some(&found), 5).await;
    assert_eq!(status, 200, "{said}");
    assert_eq!(serde_json::from_str::<Value>(&said).unwrap()["database_checked"], "found");

    let missing = Fake(Ok("Id\tName\n--\t----\n\n(0 rows affected)\n".into()));
    let (status, said) = setup_save_for(dir.path(), ORG, PROJECT, "a", &body(), Some(&missing), 6).await;
    // The same name, saved again without why: refused, but only after the rule ran.
    assert_eq!(status, 409, "{said}");
}

#[tokio::test]
async fn the_save_answer_has_no_login_or_address() {
    let (dir, _) = explored();
    let (status, said) = setup_save_for(dir.path(), ORG, PROJECT, "a", &body(), None::<&Fake>, 5).await;
    assert_eq!(status, 200, "{said}");
    assert!(!said.contains("kim") && !said.contains("https://") && !said.contains('?'), "{said}");
    let file = std::fs::read_to_string(setups_path(dir.path(), ORG, PROJECT)).unwrap();
    assert!(!file.contains("kim") && !file.contains("https://") && !file.contains('?'), "{file}");
}
