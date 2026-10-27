//! Case claims and one mapping run at a time: a case another assistant works
//! on is refused at once, and only one menu-mapping run goes at a time,
//! whoever runs it.

use crate::serial;
use v2_lib::ai_bridge::{claim_for, mapping_going, mapping_refusal, route_as, BridgeContext};
use v2_lib::autorun::agents::{self, AgentHeaders, AgentKey, Caller, Sweep};
use v2_lib::autorun::claims::{self, case_claimed, CLAIM_TIMEOUT_MS};
use v2_lib::autorun::slots::{SlotNo, SLOT_COUNT};
use v2_lib::commands::autorun_slots::{self as slots, release_slot, ReleaseWhy};

const ID_A: &str = "0123456789abcdef0123456789abcdef";
const ID_B: &str = "1123456789abcdef0123456789abcdef";
const T0: u64 = 1_000_000;

fn key(id: &str) -> AgentKey {
    AgentKey { id: id.to_string(), name: None }
}

fn caller(id: &str) -> Caller {
    Caller::Agent(key(id))
}

/// Register the agent with this id; the first registered is "Assistant 1".
fn register(id: &str) {
    let h = AgentHeaders { id: Some(id.to_string()), pid: Some(4242), client: Some("claude-code".into()), name: None };
    agents::note_call(&h.caller(), &h, T0).expect("registered");
}

fn fresh() {
    agents::clear();
    claims::clear();
    slots::clear();
}

fn ctx() -> BridgeContext {
    BridgeContext { org: "acme".into(), project: "Web".into(), ..Default::default() }
}

#[test]
fn a_claimed_case_is_refused_to_another_agent_naming_the_holder() {
    let _g = serial::agents();
    fresh();
    register(ID_A);
    register(ID_B);
    assert_eq!(claims::claim(&key(ID_A), 4521, T0), Ok(()));
    assert_eq!(claims::claim(&key(ID_B), 4521, T0 + 1), Err(case_claimed(4521, "Assistant 1")));
    // Another case is free.
    assert_eq!(claims::claim(&key(ID_B), 4522, T0 + 1), Ok(()));
    assert_eq!(claims::holder(4521, T0 + 1), Some(key(ID_A)));
    assert_eq!(
        case_claimed(4521, "Assistant 1"),
        "case 4521 is being worked on by Assistant 1 - pick another case, or ask the person to release Assistant 1 on the Auto Run screen"
    );
    fresh();
}

#[test]
fn the_same_agent_may_claim_again() {
    let _g = serial::agents();
    fresh();
    register(ID_A);
    assert_eq!(claims::claim(&key(ID_A), 4521, T0), Ok(()));
    assert_eq!(claims::claim(&key(ID_A), 4521, T0 + 10), Ok(()));
    // Claiming again renews: the time is counted from the last claim.
    assert_eq!(claims::holder(4521, T0 + 10 + CLAIM_TIMEOUT_MS - 1), Some(key(ID_A)));
    fresh();
}

#[test]
fn a_claim_times_out_after_20_minutes() {
    let _g = serial::agents();
    fresh();
    register(ID_A);
    register(ID_B);
    assert_eq!(CLAIM_TIMEOUT_MS, 20 * 60 * 1000);
    claims::claim(&key(ID_A), 4521, T0).unwrap();
    assert_eq!(claims::holder(4521, T0 + CLAIM_TIMEOUT_MS - 1), Some(key(ID_A)));
    assert_eq!(claims::holder(4521, T0 + CLAIM_TIMEOUT_MS), None);
    assert!(claims::claim(&key(ID_B), 4521, T0 + CLAIM_TIMEOUT_MS - 1).is_err());
    assert_eq!(claims::claim(&key(ID_B), 4521, T0 + CLAIM_TIMEOUT_MS), Ok(()));
    assert_eq!(claims::holder(4521, T0 + CLAIM_TIMEOUT_MS), Some(key(ID_B)));
    fresh();
}

#[test]
fn the_default_caller_is_refused_only_while_another_agent_is_live() {
    let _g = serial::agents();
    fresh();
    // Nobody is connected: nothing to be refused by.
    assert_eq!(claim_for(&Caller::Default, 4521), Ok(()));
    register(ID_A);
    let now = v2_lib::autorun::sessions::now_ms();
    claims::claim(&key(ID_A), 4521, now).unwrap();
    assert_eq!(claim_for(&Caller::Default, 4521), Err((409, case_claimed(4521, "Assistant 1"))));
    // Another case is free, and the default caller never claims.
    assert_eq!(claim_for(&Caller::Default, 4522), Ok(()));
    assert_eq!(claims::holder(4522, now), None);
    // The session ended: the claim no longer refuses anyone.
    agents::note_bye(ID_A, now);
    assert_eq!(claim_for(&Caller::Default, 4521), Ok(()));
    // Live again: refused again.
    agents::note_call(&caller(ID_A), &AgentHeaders { id: Some(ID_A.into()), ..Default::default() }, now);
    assert_eq!(claim_for(&Caller::Default, 4521), Err((409, case_claimed(4521, "Assistant 1"))));
    fresh();
}

#[test]
fn an_agent_is_refused_by_a_live_holder_and_not_by_a_gone_one() {
    let _g = serial::agents();
    fresh();
    register(ID_A);
    register(ID_B);
    let now = v2_lib::autorun::sessions::now_ms();
    claims::claim(&key(ID_A), 4521, now).unwrap();
    assert_eq!(claim_for(&caller(ID_B), 4521), Err((409, case_claimed(4521, "Assistant 1"))));
    assert_eq!(claim_for(&caller(ID_A), 4521), Ok(()));
    agents::note_bye(ID_A, now);
    assert_eq!(claim_for(&caller(ID_B), 4521), Ok(()));
    assert_eq!(claims::holder(4521, now), Some(key(ID_B)));
    fresh();
}

#[test]
fn ending_an_agent_frees_its_claims() {
    let _g = serial::agents();
    fresh();
    register(ID_A);
    register(ID_B);
    claims::claim(&key(ID_A), 4521, T0).unwrap();
    claims::claim(&key(ID_A), 4522, T0).unwrap();
    claims::claim(&key(ID_B), 4523, T0).unwrap();
    claims::end_for(&key(ID_A));
    assert_eq!(claims::holder(4521, T0), None);
    assert_eq!(claims::holder(4522, T0), None);
    assert_eq!(claims::holder(4523, T0), Some(key(ID_B)));
    assert_eq!(claims::claim(&key(ID_B), 4521, T0), Ok(()));
    claims::clear();
    assert_eq!(claims::holder(4523, T0), None);
    fresh();
}

#[test]
fn a_second_mapping_run_is_refused_naming_whose_browser() {
    let _g = serial::agents();
    fresh();
    register(ID_A);
    register(ID_B);
    slots::bind(SlotNo(1), &key(ID_B));
    let going = [false, true, false];
    assert_eq!(mapping_refusal(SlotNo::MAIN, going), Some(mapping_going("Assistant 2's browser")));
    assert_eq!(
        mapping_going("Assistant 2's browser"),
        "a mapping run is going in Assistant 2's browser - one mapping run at a time; wait for it to end"
    );
    // The run that is going is not refused by itself; nothing going refuses nothing.
    assert_eq!(mapping_refusal(SlotNo(1), going), None);
    assert_eq!(mapping_refusal(SlotNo::MAIN, [false; SLOT_COUNT]), None);
    // Another extra browser is refused too.
    assert_eq!(mapping_refusal(SlotNo(2), going), Some(mapping_going("Assistant 2's browser")));
    fresh();
}

#[test]
fn a_mapping_run_in_a_browser_nobody_holds_names_the_browser_without_a_label() {
    let _g = serial::agents();
    fresh();
    // The person's main browser, or an assistant that has gone.
    assert_eq!(
        mapping_refusal(SlotNo(1), [true, false, false]),
        Some(mapping_going("the main Auto Run browser"))
    );
    register(ID_A);
    slots::bind(SlotNo::MAIN, &key(ID_A));
    assert_eq!(
        mapping_refusal(SlotNo(1), [true, false, false]),
        Some(mapping_going("Assistant 1's browser"))
    );
    agents::note_bye(ID_A, T0);
    assert_eq!(
        mapping_refusal(SlotNo(1), [true, false, false]),
        Some(mapping_going("the main Auto Run browser"))
    );
    slots::unbind(SlotNo::MAIN);
    assert_eq!(
        mapping_refusal(SlotNo::MAIN, [false, false, true]),
        Some(mapping_going("an Auto Run browser"))
    );
    let said = mapping_going("Assistant 1's browser");
    assert!(!said.contains(ID_A) && !said.contains("4242"), "{said}");
    fresh();
}

/// A mapping run held in a slot is a flag per slot, published with the
/// discovery, and the start reads it before it opens anything.
#[test]
fn the_mapping_flag_is_published_with_the_discovery_and_read_by_the_start() {
    let autorun = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
    assert!(autorun.contains("pub fn mapping_in(n: SlotNo) -> bool"));
    let publish = &autorun[autorun.find("pub(crate) fn publish_discovery_in(").unwrap()..];
    let publish = &publish[..publish.find("\n}\n").unwrap()];
    assert!(publish.contains("MAPPING_ACTIVE[n.index()].store(") && publish.contains(".mapping.is_some()"), "{publish}");
    let bridge = include_str!("../../src/ai_bridge.rs").replace("\r\n", "\n");
    let start = &bridge[bridge.find("async fn autorun_discover_start(").unwrap()..];
    let start = &start[..start.find("\n}\n").unwrap()];
    let refuse = start.find("mapping_refusal(n, ").expect("a second mapping run is refused");
    let busy = start.find("refuse_discovery_while_busy(n)").unwrap();
    assert!(refuse < busy, "the mapping refusal comes before the browser is touched");
    assert!(start.contains("(409,"), "{start}");
    for slot in SlotNo::all() {
        assert!(!v2_lib::commands::autorun::mapping_in(slot), "{slot:?}");
    }
}

#[tokio::test]
async fn on_sweep_ends_a_claim_past_grace_and_not_a_quiet_one() {
    let _g = serial::agents();
    let _a = serial::autorun();
    fresh();
    register(ID_A);
    register(ID_B);
    claims::claim(&key(ID_A), 4521, T0).unwrap();
    claims::claim(&key(ID_B), 4522, T0).unwrap();
    // Gone is not yet past its grace, and a quiet entry's session was not
    // seen to end.
    slots::on_sweep(&Sweep { newly_gone: vec![key(ID_A)], quiet: vec![key(ID_A), key(ID_B)], ..Sweep::default() }).await;
    assert_eq!(claims::holder(4521, T0), Some(key(ID_A)));
    assert_eq!(claims::holder(4522, T0), Some(key(ID_B)));
    slots::on_sweep(&Sweep { past_grace: vec![key(ID_A)], ..Sweep::default() }).await;
    assert_eq!(claims::holder(4521, T0), None);
    assert_eq!(claims::holder(4522, T0), Some(key(ID_B)));
    fresh();
}

#[tokio::test]
async fn the_persons_release_ends_the_claims_of_the_assistant_whose_browser_it_was() {
    let _g = serial::agents();
    let _a = serial::autorun();
    fresh();
    register(ID_A);
    register(ID_B);
    slots::bind(SlotNo(1), &key(ID_B));
    claims::claim(&key(ID_A), 4521, T0).unwrap();
    claims::claim(&key(ID_B), 4522, T0).unwrap();
    claims::claim(&key(ID_B), 4523, T0).unwrap();
    release_slot(SlotNo(1), ReleaseWhy::Person).await.unwrap();
    assert_eq!(slots::bound(SlotNo(1)), None);
    assert_eq!(claims::holder(4522, T0), None);
    assert_eq!(claims::holder(4523, T0), None);
    assert_eq!(claims::holder(4521, T0), Some(key(ID_A)), "another assistant's claim stays");
    // A release of a browser nobody holds ends nothing.
    release_slot(SlotNo(2), ReleaseWhy::Person).await.unwrap();
    assert_eq!(claims::holder(4521, T0), Some(key(ID_A)));
    fresh();
}

/// The controller's ruling: an assistant's own release of its browser does
/// not end its claims; only the person's release and a session that ended do.
#[test]
fn an_assistants_own_release_does_not_end_its_claims() {
    let autorun = include_str!("../../src/commands/autorun.rs").replace("\r\n", "\n");
    let own = &autorun[autorun.find("pub async fn release_for_assistant(").unwrap()..];
    let own = &own[..own.find("\n}\n").unwrap()];
    assert!(!own.contains("claims::"), "{own}");
    let slots_src = include_str!("../../src/commands/autorun_slots.rs").replace("\r\n", "\n");
    let release = &slots_src[slots_src.find("pub async fn release_slot_within(").unwrap()..];
    let release = &release[..release.find("\n}\n").unwrap()];
    let read = release.find("let held = bound(n)").expect("the bound key is read first");
    let unbound = release.find("unbind(n);").unwrap();
    let ended = release.find("claims::end_for(").expect("the claims end");
    assert!(read < unbound && ended > read, "{release}");
}

/// The four routes refuse a claimed case before anything else, and a refused
/// start gives back the extra browser it reserved.
#[tokio::test]
async fn the_routes_refuse_a_case_another_assistant_works_on() {
    let _g = serial::agents();
    let _a = serial::autorun();
    fresh();
    register(ID_A);
    register(ID_B);
    let now = v2_lib::autorun::sessions::now_ms();
    claims::claim(&key(ID_A), 4521, now).unwrap();
    let refusal = case_claimed(4521, "Assistant 1");
    // A holds main, so B opening gets slot 1 reserved before the refusal.
    slots::bind(SlotNo::MAIN, &key(ID_A));
    let try_body = serde_json::json!({ "action": { "type": "wait", "ms": 1 }, "case_id": 4521 }).to_string();
    let start = serde_json::json!({ "account": "hr_admin", "case_id": 4521 }).to_string();
    let replay = serde_json::json!({ "case_id": 4521, "step": 1 }).to_string();
    let script = serde_json::json!({ "scripts": [{ "case_id": 4520 }, { "case_id": 4521 }] }).to_string();
    let bare = serde_json::json!([{ "case_id": 4521 }]).to_string();
    for (path, body) in [
        ("/autorun-discover-start", start.as_str()),
        ("/autorun-replay", replay.as_str()),
        ("/autorun-script", script.as_str()),
        ("/autorun-script", bare.as_str()),
    ] {
        let (status, said) = route_as(&caller(ID_B), &ctx(), None, "POST", path, body, "1.0.0").await;
        assert_eq!((status, said.as_str()), (409, refusal.as_str()), "{path}");
        assert_eq!(slots::bound(SlotNo(1)), None, "{path}: the refused start kept its extra browser");
        assert_eq!(slots::slot_of(&key(ID_B)), None, "{path}");
    }
    // A try needs a browser of its own: A's case is refused to the default
    // caller too (the older proxy's calls land in main).
    let (status, said) = route_as(&Caller::Default, &ctx(), None, "POST", "/autorun-try", &try_body, "1.0.0").await;
    assert_eq!((status, said.as_str()), (409, refusal.as_str()));
    // A's own claim does not refuse A.
    assert_eq!(claim_for(&caller(ID_A), 4521), Ok(()));
    // B's claim of 4520 landed before the refusal of 4521 in the bundle.
    assert_eq!(claims::holder(4520, now), Some(key(ID_B)));
    // A bundle that names no case claims nothing, and a try with no case
    // is the existing 400, not a claim.
    let (status, _) = route_as(&caller(ID_B), &ctx(), None, "POST", "/autorun-script", "[]", "1.0.0").await;
    assert_eq!(status, 400);
    fresh();
}

/// No identifier of an agent in what a refusal says.
#[test]
fn a_refusal_names_the_label_only() {
    let _g = serial::agents();
    fresh();
    register(ID_A);
    claims::claim(&key(ID_A), 7, T0).unwrap();
    let said = claims::claim(&key(ID_B), 7, T0).unwrap_err();
    assert!(!said.contains(ID_A) && !said.contains(ID_B) && !said.contains("4242"), "{said}");
    fresh();
}
