# Several assistants at once: a browser each

Date: 2026-10-21. Status: design.

## Intent

Avin (2026-10-20): "is it possible to launch multiple browser windows set to multiple agents to
write scripts? ... cookies are shared between browsers even in Incognito mode but is there a way
we can get around this?", then "Yes, write the design".

Cookies are not the obstacle. Every browser TCM launches already has its own throwaway profile
(`browser/launch.rs`, `fresh_profile`: a new `--user-data-dir` and DevTools port each time), and
unattended runs already give each case its own context (`Target.createBrowserContext`,
`autorun/one_browser.rs`). Two TCM browsers never share a cookie. What limits Auto Run to ONE
assistant is that everything around the browser is single:

- one supervised-browser slot, `static SESSION` in `src-tauri/src/commands/autorun.rs`, that
  discovery, tries, replays, resets, the network/cookie/element tools and area repair all lock
  (15 functions in `ai_bridge.rs`, 20 in `commands/autorun.rs`, 2 in `commands/environments.rs`);
- process-wide replay state in `autorun/replay_to.rs` (`RUNNING`/`OneReplay`, `CANCEL`,
  `ON_REQUEST`, `PROGRESS`, `CLEANUP_AT`, `ACTIONS`) and in `ai_bridge.rs` (`ASSISTANT_RUN`,
  `BATCH_STOP`), plus `OVERRIDES_SHOWN`, `SESSION_USES`, `ANSWERING`, `DISCOVERY_ACTIVE` in
  `commands/autorun.rs`;
- the bridge cannot tell callers apart: every Claude session starts its own `v2.exe --mcp`
  (`mcp::run_stdio_proxy`), all of them read the same handshake file (port + token) and send
  identical requests;
- one lease per account (`autorun/lease.rs`): PeoplesHR allows one session per user;
- shared per-project files written by several callers;
- the Auto Run screen shows one browser.

This design lets up to three assistants (separate Claude sessions) each discover, try, replay and
save in a browser of its own, signed in as an account of its own, while one assistant alone keeps
working exactly as it does today.

## Decisions (owner, 2026-10-20)

- Several assistants may write scripts at the same time, each in its own browser window.
- Write the design (this document) before anything is built.
- (2026-10-21) **3 browsers in all**: the main one and two more, fixed.
- (2026-10-21) **Subagents of one session get their own browsers from the start**: the optional
  `agent` name on every Auto Run tool (sent as `x-tcm-agent-name`, making process id + name the
  agent) and a proxy that answers calls concurrently ship in phase 3, not phase 5.
- (2026-10-21) **Idle release**: an extra browser with no call for 30 minutes is released; the main
  one never is.
- (2026-10-21) **Unattended runs keep today's rule**: a run waits until every assistant's browser is
  released.

## What exists today (the facts the parts build on)

- **Proxy.** `mcp::run_stdio_proxy` is one process per MCP client session, a blocking
  line-by-line loop over stdin: one call at a time per process. Every bridge request goes through
  `mcp::send_to_bridge`, which adds `x-bridge-token`, `x-tcm-proxy-version`
  (`PROXY_VERSION_HEADER`) and, during a tool call, `x-tcm-tool` (`TOOL_HEADER`, from the
  thread-local `CallingTool`). Subagents of one Claude session share that session's MCP process.
- **Bridge.** `ai_bridge::start_listener` parses each request with `parse_http` (which already reads the
  two `x-tcm-*` headers into `Parsed::Complete`), checks the token, then calls
  `route(ctx, client, method, target, body, version)`. `tool_call_line` logs one line per call.
- **Slot helpers are already generic.** Most browser work is written against
  `&mut Option<B: DiscoveryBrowser>` (`release_in_for_assistant`, `reset_in_for_assistant`,
  `clear_cookies_in_for_assistant`, `repair_area_in`, `end_discovery_for_replay`,
  `keep_answering(slot, running, ...)`): handing them a different slot is a call-site change.
- **Per-browser already.** `Session` holds the browser, `Cdp` (and so the capture,
  `browser/net_capture.rs`), `lease: lease::Held`, `guarded_case` + `guard_owner`
  (`GuardOwner`), `discovery: Option<DiscoveryState>` (area, account, tried components, mapping
  run, case, no_save), `opened_by` (`OpenedBy::Person` / `Assistant`), `replay_stand`,
  `stepping`, `noted`, `overrides`, `downloads_root`.
- **Files with a write lock already** (process-wide, every read-change-write holds it):
  the map (`discovery_map::update`, `write_lock`), components (`components::write_lock`,
  `with_components_locked`), quirks (`quirks::write_lock`), scripts (`store::scripts_lock`),
  test-made (`test_made::lock`). **Without one:** areas/nav (`nav::put_path`, `remove_path`,
  `set_save_words`, `set_protected`, `set_direct_urls`, `area_repair::save_repair`,
  `put_old_path_back` each load and save with nothing held), the order file
  (`store::save_order_merged`), the mapping summary (`mapping_summary`, one file per project).
- **Exclusions that stay.** An unattended run and a supervised browser are never open at once
  (F7: `autorun_replay::replay_is_running`, `supervised_session_is_open`). The recorder
  (`autorun_record::CURRENT`), Auto Run setup browsers and the API template runner
  (`api_templates::runner::claim`, which a case's setup also takes) are one at a time.

## Part 1: who is calling

**The agent id.** Each proxy makes an id once, at start (`ai_bridge::new_token`'s generator, 128
random bits as hex), and sends it on every bridge request beside the token:

| Header | Value |
|---|---|
| `x-tcm-agent` | the id (`[0-9a-f]{32}`) |
| `x-tcm-agent-pid` | the proxy's own process id |
| `x-tcm-agent-client` | `clientInfo.name` from the MCP `initialize`, cut to 32 of `[A-Za-z0-9 ._-]` (e.g. `claude-code`); absent before `initialize` |

`parse_http` reads them into `Parsed::Complete` as it reads `tool`. A header that is missing or
not of that shape is ignored, never a refusal. `route` gains a `caller: &Caller` argument:

- `Caller::Default` - no (valid) id: an older proxy, a test, a hand-made request. **The default
  agent behaves exactly as every call behaves today**: it always uses the main browser (Part 2)
  and is never refused by another assistant's claims.
- `Caller::Agent(AgentId)` - a known id.

The id is a name, not an authority: the bridge token stays the only thing that admits a request,
and any caller can already release the browser today. Nothing about it is secret, but it never
reaches the webview either (the screen names a slot number and a label).

**The registry** (`autorun/agents.rs`, new): per id, its label, pid, client name, first and last
call time, and its case claims (Part 6). Coordination state, not a cache - same footing as
`IMPORTING` in `commands/autorun.rs` - held in a `std::sync::Mutex` never held across an await.
Labels are "Assistant 1", "Assistant 2", ... - the smallest number no live agent has, so a
person sees 1-3, not 17. `tool_call_line` adds the label: `AI tool try_autorun_action
(Assistant 2): ok`.

## Part 2: one slot per browser

`static SESSION` becomes a **slot table** (`commands/autorun/slots.rs`, or beside `SESSION`):

```
Slots { main: Arc<Slot>, extra: [Option<Arc<Slot>>; EXTRA_CAP] }
Slot  { number, agent: Mutex<Option<AgentId>>, session: tokio::Mutex<Option<Session>>,
        replay: ReplayState, assistant_run: Mutex<Option<AssistantRun>>, batch_stop: AtomicBool,
        uses: AtomicU64, overrides_shown: Mutex<Overrides>, answering: AtomicBool,
        downloads_dir: PathBuf }
ReplayState { running: AtomicU8, cancel: AtomicBool, on_request: AtomicBool,
              progress: AtomicU64, cleanup_at: AtomicI32, actions: ... }
```

Everything listed under Intent as process-wide moves into the slot it describes. What each slot
holds, then: the browser and its CDP connection, its capture, its lease, its save guard and
`GuardOwner`, its overrides, its discovery state (tried components, mapping run), its replay and
its stop, its parked replay answer, its batch stop, its "used since" counter, and its own
downloads folder.

- **Main slot** (number 0) is today's `SESSION`: the person's Open browser, watched steps, the
  person's Replay to step, the person's sign-in, setup making way - and the default agent.
- **Extra slots** (1..=`EXTRA_CAP`) exist only while an assistant's browser is in them.
- **Downloads.** Main keeps `downloads/supervised`; extra slot n uses `downloads/supervised-<n>`.
  Today `keep_supervised_downloads` empties the folder when a browser opens - an extra browser
  sharing it would wipe the main browser's downloads.

**Cap: three browsers in all** (main + `EXTRA_CAP = 2`), a constant. Why: this machine has 32 GB
and 12 cores, with about 7 GB free on a working day (measured 2026-10-21, with the dev app, Claude
sessions and the person's own browser open); each Claude session and each visible Edge on the
PeoplesHR SPA cost hundreds of MB, and WebView2/Edge render WebGL on WARP here (software), so every
visible animated page costs CPU. Three is the most that leaves the person's own work usable; the
hand checks measure it (Constraints) and may lower it, never raise it without a measurement.

**Lock rules.** Never hold two slots' session locks at once, except Release all and the
environment switch, which take them in slot order (main, then 1, 2). Opening a browser in any slot
holds an `OPENING: tokio::sync::RwLock<()>` for read; the environment switch/save
(`commands/environments.rs`, `save_with`, `switch_with`) and switching Auto Run off
(`commands/modules.rs`, `auto_run_browser_open`) take it for write and check every slot, where
today they lock `SESSION` and check it.

## Part 3: how a tool finds its browser

One function, `slot_for(caller, need) -> Result<Arc<Slot>, (u16, String)>`, called at the top of
every Auto Run route that touches a browser (the 15 `supervised()` callers in `ai_bridge.rs`, plus
`/autorun-replay`, `-replay-stop`, `-release`, `-reset`, `-cookies-clear`, `-discover-end`).
`need` is `Open` for the calls that may open a browser (`start_autorun_discovery`,
`replay_autorun_to_step`) and `Use` for the rest.

1. `Caller::Default` -> main. Always.
2. An agent bound to a slot -> that slot.
3. An agent bound to none, and main is not bound to another live agent -> **main**, as today -
   including today's rules about the person's browser in it (reads and tries borrow it; the
   acting tools refuse in it; a discovery takes over a browser the assistant left). A call that
   opens or acts in main binds the agent to it; a read does not.
4. Main bound to another agent:
   - `Open` -> a free extra slot, bound to this agent; with none free, 409:
     "3 Auto Run browsers are open already, the most at once - wait for another assistant to
     finish, or ask the person to release one on the Auto Run screen".
   - `Use` -> 409: "you have no Auto Run browser - start_autorun_discovery or
     replay_autorun_to_step opens one (another assistant is using the main one)".

So one assistant alone always lands in main and nothing it sees changes; only a second live
assistant gets a browser of its own. A binding ends when the slot's browser closes or goes, when
the person takes the main browser (`Session::make_persons`, Open browser), on the agent's own
release, and when the agent goes (Part 8). An extra slot whose browser closes is emptied.

The default agent in a main slot bound to an agent acts in that agent's browser - what every call
does today. Only an old proxy is a default agent, and it already gets the version warning.

**Global, not per agent:** the person's own browser (main), unattended runs' browsers and their
exclusion (F7 now means: an unattended run is refused while any slot holds a browser, and every
slot's open is refused while one runs), the recorder and setup browsers, the API template runner
and template browsers (`api_templates::held`), the account leases table, the imported capture sets
(read-only, `cache::keys::imported_captures`), and every project file.

## Part 4: replays and stops, per slot

- `OneReplay::claim_in(&slot.replay, owner)`; `replay_to::stop(&slot.replay)`; the runner already
  takes the cancel flag by reference (`stopped_before_opening(&CANCEL)`, step waits), so it is
  handed the slot's.
- `replay_autorun_to_step {stop}` (`autorun_replay_stop_with`) stops **the caller's** replay only;
  with none going in its slot it says so, never stops another's. The 202 "still running"
  progress, the parked answer (`AssistantRun`, `still_fresh`) and its `session_uses` check read
  the caller's slot.
- `discover_autorun_actions {stop}` and every `stop_batch()` caller: the slot's `batch_stop`.
- The person's Stop and Close browser: main's replay and batch. Release all: every slot's.
- `replay_supervised(app, slot, org, project, req, by)` and the `ReplayHost` take the slot.
- The person's panes keep hearing **main only**: `AutorunReplayProgress`, `AutorunSessionChanged`,
  `AutorunDiscoveryChanged` (`publish_discovery`, `DISCOVERY_ACTIVE`, `tell_panes*`) are emitted
  for main and keep their shape, so `RunPane.tsx`, `ReplayPane.tsx` and `lib/discoveryActive.ts`
  do not change. Extra slots report through Part 7's event.

## Part 5: accounts

PeoplesHR ends an account's session wherever else it is signed in, so **each assistant needs its
own account**. The lease table stays as it is - one lease per (environment, account key) - and a
browser holder is still refused at once, never queued.

- `lease::Holder::Browser` gains the slot it is in. To the person and to unattended cases it still
  reads "the Auto Run browser" (keeping `is_in_use`/`HOLDER_WORDS` exact, so a refused case is
  still Blocked, not Failed). To an assistant, its own answer adds who: "the account hr_admin was
  in use by the Auto Run browser - try again when it is free. Assistant 1 is signed in as it:
  work on cases that sign in as another account, or wait until Assistant 1 is done." Never a
  login, only the key.
- `get_accounts` (`accounts_read`) adds `in_use_by` per account: "your browser", "Assistant 2's
  browser", "the person's browser", "an unattended run", "an API template run", "Auto Run setup",
  or absent when free.
- `propose_accounts` is unchanged; the guide says to use it when every account a needed role has
  is in use, so the person can add one (passwords stay where `propose_accounts` keeps them today).
- Scripts name their account; this design never runs a case as an account other than its script's
  (Anti-goals). Two assistants whose cases need the same account work one after the other.

## Part 6: shared files and cases

**Case claims.** Two assistants editing one case would overwrite each other's script. An agent
claims a case when it starts a discovery for it (`case_id`), replays it, tries with its `case_id`,
or saves its script. Another agent's discovery-for, replay, try-with-case or save of a claimed case
is refused (409): "case 4521 is being worked on by Assistant 1 - pick another case, or ask the
person to release Assistant 1 on the Auto Run screen". Claims end when the agent goes, when its
browser is released, or 20 minutes after its last call on that case. The person is never refused
by a claim (their editor save, their replay); the default agent neither claims nor is refused while
no other agent is live, so one assistant alone sees nothing new.

**A short lock around every read-change-write**, per file kind, process-wide as the existing ones:

| File | Today | Change |
|---|---|---|
| map (`discovery_map.rs`) | `update` under `write_lock` | none; updates merge, so several discoveries add to it safely |
| components | `write_lock` | none |
| quirks | `write_lock` | none |
| scripts (`store.rs`) | `scripts_lock` around the write | see "re-read" below |
| areas (`nav.rs`, `area_repair.rs`) | **no lock** | `nav::with_nav_locked` around every load-change-save: `put_path`, `remove_path`, `set_save_words`, `set_protected`, `set_direct_urls`, `save_repair`, `put_old_path_back` |
| order (`store::save_order_merged`) | none | the same kind of lock around load-merge-save |
| mapping summary | one file per project | **one mapping run at a time** across slots: a second `start_autorun_discovery {mapping}` is refused naming who maps |

**Paths that read before a browser trip (or an Azure DevOps await) and write after** must
re-read under the lock and refuse when what they read has changed:

- **Area save, new or re-record** (`ai_bridge::autorun_discover_area`, before `nav::put_path`): `existing` is
  read before the clicks are replayed; `put_path` re-reads but replaces a same-named area
  whatever it became. Under the nav lock it compares as `save_repair` does (clicks, arrived,
  module) and refuses "area X changed while it was being saved - nothing was saved; look at it
  again" - and refuses a new area whose name another save took meanwhile.
- **Area repair**: `save_repair` already re-reads and compares; it gains the lock so the compare
  and the write are one step.
- **Script save** (`save_autorun_scripts`): each existing script is read for the declared-changes
  and seen gates, then Azure DevOps is read (an await), then `save_scripts_atomically` writes.
  Under `scripts_lock`, just before writing, each script's `saved_at` is compared with what the
  gates read; any differ -> nothing is saved: "case 4521 changed while this save was checked
  (saved at <time>) - read it again and save again".
- **Component save**: checks its own slot's `DiscoveryState::tried`, then writes under the lock -
  nothing more needed.
- `record_validation_rule`, quirks, defects: write through locked updates - nothing more needed.

HAR exports share TCM's `captures` folder; an export to an existing name already says it replaced
it - kept.

## Part 7: the person's view

An **Assistants** card on the Auto Run screen (`src/screens/AutoRun/`, a new `AssistantsCard.tsx`
shown above the tabs, only while at least one agent is known). One row per live agent:

- label and client ("Assistant 2 - Claude Code"), which browser ("main Auto Run browser" /
  "its own browser"), Edge or Chrome, the account key it is signed in as;
- what it is doing: idle (and for how long since its last call), discovering an area, mapping
  menus, replaying case N (step k of m), trying; "changed for testing" when overrides are set,
  "saves blocked" while guarded;
- **Stop** (its replay or batch), **End discovery**, **Release** (ends its discovery keeping the
  map, closes its browser if an assistant opened it, frees its account and its claims). Icons
  from `src/lib/actionIcons.ts`; colours from tokens.

Rust: `auto_run_agents() -> Vec<AgentView>`, `auto_run_stop_agent(slot)`,
`auto_run_end_agent_discovery(slot)`, `auto_run_release_agent(slot)`, and the event
`AutorunAgentsChanged { agents }` emitted whenever a slot, a binding or a row's activity changes
(throttled to once a second for progress). `AgentView` carries the slot number, label, client,
browser kind, account key, activity, flags - no id, no pid, no login, no address.
**Release Auto Run browser** on the Setup card keeps its meaning (`release_autorun_browsers`):
every slot, every app browser. Close browser and End discovery on the Setup card stay main's.

## Part 8: an assistant that goes away

- On stdin EOF the proxy sends `POST /agent-bye` (best effort) before it exits.
- Every 30 s the app looks at each agent's pid (`windows-sys` `OpenProcess` +
  `GetExitCodeProcess`, beside `browser::tree`'s process code). Gone, or a bye, then 60 s grace
  (a reconnect makes a new id anyway): its claims end; an **extra** slot is released as the
  card's Release does (a replay going is stopped first through its slot's cancel); a **main**
  binding is only cleared - the browser stays as today, for the person or the next assistant.
- **Idle**: an extra slot whose agent is alive but has made no call for 30 minutes, with no
  replay or batch going, is released the same way, and that agent's next call hears "your Auto
  Run browser was released after 30 minutes without a call - start a discovery or replay again".
  Main is never released for idleness (unchanged from today).
- Every release is logged with the label: `Auto Run: released Assistant 2's browser (its
  session ended)`.

## Part 9: save guard, overrides and capture per agent

They already live in `Session`; the slot gives each agent its own. What changes is the statics
read outside the lock: `discovery_no_save()`, `running_discovery_area()`, `overrides_now()`,
`publish_overrides`, `with_changes_on_a_refusal` and `assistant_browser_holds(key)` (the template
refusal hint) read the caller's slot. `GuardOwner::Person` only ever occurs in main. The acting
network/cookie tools keep their rules per slot: refused in a browser `OpenedBy::Person` or while
its slot's watched run goes. Capture budgets (32 MB bodies, 1000 requests) are per browser, so
three browsers stay within 96 MB.

## Part 10: what an assistant is told

Guide (`autorun/guide.rs`) and tool descriptions (`mcp.rs`), and the tcm skills' text in
`ai_tools.rs` (discover, heal, map-menus):

- The Quick rules line "Replays, discovery and resets share one browser and its sign-in"
  (guide.rs ~156 and ~1598) becomes "Your replays, discovery and resets share your browser and
  its sign-in. Other assistants may be working at the same time, each in a browser of its own."
- A new section **Working beside other assistants**: each assistant needs its own account -
  read `get_accounts` (`in_use_by`) and pick a free one, or `propose_accounts`; split the cases
  so no two assistants share a case or an account; a refused account or case is not an error to
  retry in a loop - take other work or report; at most three browsers; one mapping run at a time;
  the map, areas, components and quirks are shared, so re-read an area before repairing it; an
  unattended run waits for every assistant's browser; your browser is released when your session
  ends or after 30 minutes without a call. Subagents of one session share one browser: for parallel
  script writing, use separate sessions (unless Question 2 is answered yes).
- Descriptions: `get_autorun_page`, `probe`, `inspect`: "your Auto Run browser, or the person's when
  you have none"; `replay_autorun_to_step` `stop`: "stops your own replay"; `reset_autorun_browser`
  `release`: "your own browser only"; `start_autorun_discovery`: "opens a browser of your own when
  another assistant is using the main one".

## Performance

- The request pacer (`ado/throttle.rs`) is one process-wide budget for Azure DevOps and stays one:
  only the script save, order and template routes read ADO, and several assistants share it -
  correct, since ADO's limit is per user.
- Each assistant signs in once per account in its own browser; the replay's reuse of a browser
  already signed in (`replay_to::carry_from`, "Signed in already") works per slot.
- Case setups and API template runs stay one at a time (`api_templates::runner::claim`): a second
  assistant's replay of a case with a setup is refused with that sentence while another's runs.
  Waiting for it is a later change.
- The bridge already serves requests concurrently (one task per connection); the session lock
  was the serialising point, and now is per slot.

## Build order (smallest safe first)

1. **Agent id, nothing visible.** Proxy makes the id, sends the three headers; `parse_http`
   reads them; `route` takes `Caller`; the registry and labels; `tool_call_line` names the label;
   `slot_for` exists and always answers main. Tests: headers parsed, missing/malformed =
   default, label numbering, log line. No answer changes.
2. **The slot table with one slot.** Move every per-browser static into `Slot`; thread `&Slot`
   through `replay_to`, `replay_supervised`, the bridge routes and the person's commands; per-slot
   downloads folder. Same behaviour: the existing suite passes unchanged. Also the file fixes that
   are right on their own: `nav::with_nav_locked`, the area save's re-check, the script save's
   `saved_at` re-check, the order lock.
3. **Extra slots** (needs the owner's answers): `slot_for`'s rules 2-4, the cap, `OPENING`, F7 and
   environment/module checks across slots, per-slot stop/release/reset, lease holder by slot and
   the agent-facing sentences, `get_accounts.in_use_by`, case claims, one mapping run at a time,
   gone-process release, guide and descriptions; **subagent names** (`agent` argument,
   `x-tcm-agent-name`) and the concurrent proxy (owner, 2026-10-21: from the start).
4. **The Assistants card** and its commands/event; docs-site shots for it. Phases 3 and 4 go out
   in the same beta - extra browsers are never shipped without the person's way to see and
   release them.
5. **Idle release** of extra slots (30 minutes).

## Decisions for the builder (my calls)

1. `caller` is an explicit `route` argument, not a task-local: the assistant's replay runs in a
   `tokio::spawn`ed task (`autorun_replay_with`), which does not inherit task-locals.
2. Labels are kept for a live agent's whole life, never reused while it lives.
3. The registry forgets an agent 10 minutes after it is gone and released.
4. Claim time-out: 20 minutes after the agent's last call on that case.
5. Extra browsers open at an offset window position (`--window-position`) so they do not stack
   exactly on the main one.
6. `AgentView` activity is computed from the slot under `try_lock` only: a busy slot reports its
   last known activity, never waits.

## Questions for the owner

All four were answered on 2026-10-21 (see Decisions): 3 browsers; subagents get their own browsers from the start; idle release after 30 minutes; unattended runs keep today's rule.

## Anti-goals

- Any change for one assistant working alone, or for an old proxy (the default agent).
- Two assistants sharing one browser through separate contexts: one crash takes all, a visible
  context still opens its own window, and profiles already isolate cookies.
- Sharing or queueing one account between assistants, or running a case as an account its script
  does not name.
- An agent stopping, resetting or releasing another agent's browser (only the person, through
  the card or Release all, may).
- A login, password, address, agent id or pid in an event, the webview, a log line or an answer
  (account keys and labels only).
- Any DELETE, any new cache (the registry is coordination state), any request TCM itself sends to
  the application.

## Constraints

- Rust tests in `src-tauri/tests/suite/` (a new `agents.rs` with its `mod` line in
  `suite/main.rs`); anything touching the registry, the slot table or the lease table takes its
  lock from `suite/serial.rs`.
- `src/bindings.ts` regenerated (`cargo test --test bindings`), never hand-edited; the token
  check in `tests/bindings.rs` must still pass with `AgentView`.
- `src/ui-consistency.test.ts` passes untouched; colours from tokens; icons from
  `actionIcons.ts`; user-facing sentences name no URL.
- Tool-list and description tests updated where descriptions change (`mcp.rs`, `ai_tools.rs`,
  `src/lib/mcpTools.ts`).
- The card is on a documented screen: docs shots and `docs:build` per CLAUDE.md, committed together.
- Changelog lines are for the end user: "Several assistants can now write scripts at the same
  time, each in its own browser" - no file paths.
- `docs/backlog.html` gets this item and its hand checks.
- Hand checks owed against a live org: two sessions with two accounts discovering at once; a
  second session asking for an account the first holds (refused, named); the same case from two
  sessions (refused); a session closed mid-replay (its browser goes within about 90 s, its account
  free); the person's Open browser and Release all while two work; Task Manager memory and CPU with
  three browsers on PeoplesHR, recorded in the backlog.
