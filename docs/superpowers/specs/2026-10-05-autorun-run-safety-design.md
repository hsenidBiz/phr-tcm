# Auto Run run safety (part 2 of 5)

Design agreed with the owner on 2026-10-05, from the backlog in `docs/autorun/backlog-2026-10.md`, items 2, 3 and 11. The owner's decisions:

- A "save" is a POST, PUT, PATCH or DELETE request whose path contains a save word: save, update, delete, submit, approve, publish or assign. A project can add its own patterns.
- Each part ships as its own beta.

Auto Run and API Templates stay hidden. Nothing in this part is named in the changelog or the help.

## 1. No-save scripts (#2)

`CaseScript` gains `no_save: bool`, serialised only when true. It is set:
- in the script editor, with a checkbox `Must not save`;
- by the assistant's `save_autorun_script`;
- through import.

A repair cannot turn the flag off. Only a person saving from the editor can.

### While a no-save script runs (unattended, supervised, the assistant's try route, and part 4's replay)

The runner enables Chrome's request interception (`Fetch.enable`) for the case's browser. Every request the page starts is checked:
- A request whose method is POST, PUT, PATCH or DELETE AND whose path, lowercased and with the query left out, contains a save word or a project pattern is **failed inside the browser** (`Fetch.failRequest`, reason `BlockedByClient`), so it never reaches the server.
- Every other request continues unchanged.

The case then fails at once, in the step that was running, with the sentence:

`this script must not save, but the page tried to send <METHOD> <path> - it was stopped before it reached the server`

The path carries no host and no query. The failure has its own class, `ErrorClass::SaveBlocked` (key `no-save`). It is not transient and never retried. The case's proposal is Failed.

If interception cannot be enabled, the case is Blocked before step 1 with `the no-save guard could not be set up: <why>`. A no-save script never runs unguarded.

### Project patterns

Patterns are extra save words for one project. They are matched as case-insensitive substrings of the path.
- They are kept in the project's Auto Run settings file, beside the recipe.
- The Setup tab gets a row `Save words`, showing the built-in words plus the project's own. An `Edit` dialog adds or removes the project's own words; the built-in words cannot be removed.
- The guide explains the flag and the words, and tells the assistant to set `no_save` on any script that works on a shared draft and must not change it.

## 2. Preconditions checked before step 1 (#3)

`CaseScript` gains `preconditions: [{ "flow": <flow id>, "stage": <stage id>, "value": <the subject value>, "why"?: <one sentence> }]`. The subject value is the flow's subject, for example the cycle's name or id, as the flow's checks take it.

Before sign-in, the runner (unattended and supervised) runs each precondition's stage check through the flow machinery (`api_templates::gate::stage_state`) against the active environment's database. The app runs these checks itself: they do not need the assistant's Database Read Access switch, but they do need a database chosen.

| Outcome | Result |
|---|---|
| Every check reports the stage as done | The case proceeds. |
| A check reports not done | Blocked: `precondition not met: <stage title> for <value> (<flow title>)`, followed by ` - <why>` when the precondition gives one. |
| A check could not run | Blocked: `precondition could not be checked: <reason>`. The reason uses the gate's existing wording, with no SQL or connection detail. |
| No database is chosen | Blocked: `preconditions need a database chosen on the AI Bridge tab`. |

Saving validates every precondition: the flow and stage must exist, and the value must be present. The refusal sentences name what is missing.

The guide documents preconditions. It tells the assistant to add one whenever a script relies on a record built beforehand, and to find the flow and stage with `list_api_templates`.

## 3. One sign-in per account at a time (#11)

PeoplesHR allows one session per user. A new module, `autorun/lease.rs`, holds a process-wide lease per (environment id, account key).

| Holder | Holds the lease |
|---|---|
| An unattended case | From its sign-in to the case's end |
| A supervised Auto Run browser | From the moment it signs in as an account until it is closed or signs in as another |
| An API template run or prove | For its whole run |

The assistant's try route and part 4's replay use the supervised browser's lease.

### Waiting and refusals

A request for a held lease waits up to 60 seconds, polling, then gives up:
- **Unattended case:** Blocked, with `the account <key> was in use by <holder> - try again when it is free`. Holder descriptions include `the Auto Run browser`, `an API template run` and `another case in this run`.
- **API template run:** refused with the same sentence.
- **Supervised browser asked to sign in as a held account:** refused at once, with no wait, using the same sentence.

A lease is always released on drop. An end, an error, a stop and a panic all release it. A test proves each path.

The account key is a key, never a login, and the lease never logs a password.

## 4. Out of scope

- Fixtures, script setup and cleanup (part 5).
- Replay to step N (part 4). Part 4 uses the no-save guard and the lease from this part.

## 5. Testing

- Rust tests live in `tests/suite` only.
- **No-save guard:** live tests on a local fixture page that posts to `/api/Save` and `/api/Search`. Under the guard the save is blocked (the fixture server never sees it) and the case fails with the sentence. The search goes through, and a project pattern blocks its own path.
- **Preconditions:** tested with the fake `StageDb`, for each outcome above.
- **Lease:** waiting, timeout, release on every path, and supervised refusal.
- **Webview:** the editor checkbox and the Setup row and dialog, covered with vitest.
