# Cleanup Pass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the reviewer-rated minor defects left in mentions and swimlanes (1.25.28), Auto Run, the 1.25.23 review and the Execution order modal, each with a test that pins it.

**Architecture:** Eight independent tasks, grouped by area. Rust changes stay in their modules (`commands/`, `autorun/`, `import_parser/`, `steps_xml.rs`, `auth.rs`) with tests under `src-tauri/tests/`. Frontend changes stay in the hook, lib module or component that owns the behaviour. Four tasks change a Rust type or command that crosses IPC and regenerate `src/bindings.ts`.

**Tech Stack:** Rust (Tauri 2, tauri-specta, serde_json, quick-xml, wiremock, tokio), React 19 + TypeScript, TanStack Query v5, Tailwind 4 theme tokens, vitest + Testing Library, plain ES5 page scripts in `src-tauri/web/`.

**Spec:** there is no separate spec. The item list below, with its rulings, is the spec.

## The items, and the rulings this plan makes

Every item was checked against the code on `fix/cleanup-pass` (HEAD `72e9301`) before it was planned.

### A. Mentions and swimlanes

1. **The mention check polls while the app is hidden.** Confirmed: `src/hooks/useMentions.ts:25` sets `refetchIntervalInBackground: true`. **Ruling:** `refetchIntervalInBackground: false` and `refetchOnWindowFocus: true` on this one query. In TanStack Query v5, "window focus" is the page's `visibilitychange`, so coming back from minimised checks once at once. The app-wide default (`main.tsx`) stays `refetchOnWindowFocus: false`. **Consequence to confirm (owner):** this reverses the 1.25.28 plan decision "the mentions query polls in the background". A minimised app no longer checks, so a mention no longer arrives as a Windows notification while minimised. It arrives as a toast when the window is shown again. A window that is visible but behind other windows still polls and still gets the OS notification.
2. **The PR mention scan is O(n²).** Confirmed: `src/hooks/usePrAttention.ts:106-117` rescans every PR's threads whenever any one thread query settles, so one poll cycle over N PRs scans N × (all threads). `prMentions` itself is linear. **Ruling:** keep a per-hook map of `repo:id` to the `dataUpdatedAt` last scanned, reset when the organisation or the signed-in id changes, and scan only PRs whose own threads changed. `noteMentions` is still called on every pass (with an empty list when nothing changed), because its first call is what sets the organisation's first-run baseline.
3. **The seen set is capped at 500 and eviction can re-raise a mention.** Confirmed: `src/lib/notifications.ts:88-95` prepends only new ids and slices to 500, so an id re-reported every check still ages out. **Ruling:** every id a source reports (in `raise` and `markSeen`) moves to the front of the seen set, and one call that reports more than the cap keeps them all. Eviction then drops only what nothing has reported for longest. A mention that a check still returns is refreshed by that check and cannot be evicted by fewer than 500 other ids arriving between two checks. **Accepted residual:** a mention whose item leaves the check's top 20 for long enough to be evicted, then comes back, can raise again. A time floor was rejected: it would also hide real mentions written on items that were outside the top 20 at the last check.
4. **The drawer has no state dropdown for a lane parent that is not a card.** Confirmed: `src/screens/WorkBoard.tsx:883-887` looks the type up in `board.data.items`, so a parent that is not a card gets `[]`. There is no command that reads one type's states. **Ruling:** a new read-only command `work_item_type_states(organization, project, work_item_type)` wraps the existing `AdoClient::get_work_item_states`. `WorkItemDrawer` reads its own item type's states when it is given none. This also gives a state dropdown to an item a notification opened from outside the loaded board.
5. **The wipes spell key prefixes literally.** Confirmed: `forgetAllNotifications` (`notifications.ts:262`) and `forgetMentionBaselines` (`mentions.ts:45`). **Ruling:** each prefix is the key helper called with an empty organisation (`listKey("")`, `knownKey("")`, `baselineKey("")`). A new test writes through the real writers, then wipes, and checks every written key is gone. It passes before and after the change on purpose: it is the tripwire for a future rename.

### B. Auto Run

6. **Open browsers survive the app closing.** Confirmed: `src-tauri/src/lib.rs:383` calls `.run(...)` with no run-event handler. **Ruling:** build the app and run it with a handler. On `RunEvent::Exit`, block for at most 3 seconds on a new `commands::autorun::close_autorun_browsers()`. That function ends a recording, Start, check or Try the way Cancel does (`auto_run_record_cancel`), then closes the supervised browser. `close_browser` now waits for the killed process before it removes the throwaway profile. **Accepted residual:** an unattended run's per-case browser is not covered (not in the item), and Edge helper processes can still hold a profile file for a moment, in which case that one folder stays in `%TEMP%`.
7. **A cancelled Try says the recording sentence.** Confirmed: `unless_cancelled` always returns `recorder::CANCELLED` ("the recording was cancelled - nothing was saved"), and a Try goes through it. **Ruling:** `unless_cancelled` takes the sentence to return. A Try passes the new `TRY_CANCELLED` ("the check was cancelled - the saved path was not changed"). `ModuleTryResult` gains `cancelled: bool`. The dialog shows any cancelled Try as "Stopped trying X." and never as the path failing, including a Try that was cancelled from another dialog.
8. **Scripts numbered without Shared Steps rows.** Confirmed: `autorun/floor.rs` has no notion of a Shared Steps row, and `autorun/publish.rs` marks step `i + 1` against step id `i` with no check. `PublishCase` carries only step ids, and a Shared Steps row's id is `""` (the same as a step with no id), so publish cannot tell a Shared Steps row apart today. **Ruling:** this is one task. `Expected` gains `shared`. Floor rule 5 refuses a script step on a Shared Steps row, with a sentence that says how to renumber. `PublishCase` gains `shared_steps` (1-based rows, sent by the review screen, which has the case's steps). Before publish writes a case's step marks, it runs the same check (`floor::steps_on_shared_rows`) over the run's own step numbers. **Owner decision (the ruling is this plan's default):** when the check fails, publish sends that case's verdict and comment but **no step-by-step marks**, and lists the reason under problems. The alternative is to skip the whole case. Legacy scripts are **not** renumbered automatically, because a script that happens to be right would be moved one row off.

### C. From the 1.25.23 review

9. **Same-titled drafts pair differently in the app and in Rust.** Confirmed three ways. (a) `fileSync.fileOwnedKeys` pairs exact rows first, but `apply_draft_edits` (`import_parser/export.rs:333-377`) claims the Nth same-titled file entry in queue order, and compares titles with `eq_ignore_ascii_case` while the app lowercases fully. After a re-sort, two same-titled rows write onto each other's entries, and the entries' extra keys swap. (b) `uploadHold.rowsNamedBy` marks the first same-titled create row, not the row that was sent. (c) `QueueSection.removeRow` removes by index, so a second click before the re-render removes the next row too. **Ruling:** pairing follows identity as far as the app has one. (a) Each `DraftEdit` carries `occurrence`, meaning which of the file's id-less same-titled entries the app paired the row with. Rust claims those first, falls back to the old rule, and compares titles with `to_lowercase`. (b) A hold records each held row's content signature and marks exact rows first, then by title, the same order the file sync uses. A hold stored before this change still marks by title. (c) `removeRow` removes by object identity and ignores a second remove of the same row.
10. **Shared Steps rows show blank in `CasePreview` and `BugDialog`.** Confirmed: `RunPanel/CasePreview.tsx:130-135` and `BugDialog.tsx:36`. **Ruling:** `CasePreview` renders `SharedStepLabel` across both columns. `BugDialog` writes "Shared steps #N", plus " - title" when the title is already in the query cache. The words come from one helper in `src/lib/sharedSteps.ts`.
11. **`ref="0"` refusal skips the steps while the save reports success.** **Dropped**, see below.
12. **Merge always closes `</steps>` and drops the prolog and comments.** Confirmed: `steps_xml.rs:372-374`. **Ruling:** `merge_into` returns `None` (so the caller builds) unless the root element is `steps`. Everything before the root's opening tag and from its closing tag on is copied from the original. Comments *between* steps are still dropped. Azure DevOps does not write any there.
13. **An all-no-op submit shows "Processing" during the pre-flight.** Confirmed: `submitStarted` runs before the pre-flight fetch (`QueueSection.tsx:637`), and every progress label reads "Processing". **Ruling:** `SubmitPhase` gains `stage: "checking" | "uploading"`. During the pre-flight the bar reads "Checking what changed" and the buttons read "Checking". An all-no-op submit ends in that stage and never shows "Processing".
14. **Review page.** Confirmed: `web/cases-page.js:359-391` polls every 4 s with no backoff, and `web/cases-notes.js:57-61` has no timeout, so a `/note` that never settles leaves `busy()` above 0 and pauses live swaps forever. **Ruling:** the poll becomes a `setTimeout` chain. Each failed ask doubles the wait, up to 60 s, and the first answer resets it to 4 s. A save gives up after 10 s (`AbortController` plus a settle guard), frees the box and says "Not saved - the app did not answer".
15. **Sign-in.** Confirmed: `auth.rs:354-358` sets a per-read timeout, so a connection that drips bytes restarts it with every byte and can outlive the sign-in window. `sign_in_network_error` reuses the Azure DevOps sentences. **Ruling:** the request line is read against one budget, the per-connection timeout or what is left of the window, whichever is shorter. Three sign-in sentences in `auth.rs` name Microsoft sign-in in the `ado/transport.rs` style: no URL, and a pointer to Settings → Logs.
16. **`draft_write_allowed` accepts any bare JSON array, and a second containment helper exists.** Confirmed: `commands/queue.rs:293`, and `ai_bridge::bridge_may_write` (`ai_bridge.rs:121-124`) does its own `starts_with`. **Ruling:** a bare array counts as a draft only when it is non-empty, every entry is an object, and at least one entry has a non-empty title under an importer title key (`import_parser::TITLE_KEYS`). `bridge_may_write` uses `workspace::is_inside`.
17. **The slug drops caseless scripts.** Confirmed: `web/cases-specs.js:155-157` keeps only cased letters and digits. **Ruling:** any non-ASCII character counts as a word character, except the punctuation, symbol, space and surrogate blocks. ES5 only, with no `\p{}` and no `u` flag. Citations do not resolve through ids: `jumpTo` uses `matchHeading`, whose `wordSet` already keeps `À-￿`. A test pins that a CJK citation still resolves.

### D. Run order

18. **The Execution order modal has no accessible name.** Confirmed: `ExecutionOrderModal.tsx:204-205`. The shared `Modal` (`components/ui/modal.tsx`) supports neither `aria-labelledby` nor `aria-label`. **Ruling:** `Modal` gains optional `labelledBy` and `label` props. `ExecutionOrderModal` names itself with its heading through `useId`.

### Dropped (why)

- **11. `ref="0"` refusal in `steps_patch`.** It cannot be reached from the app. `TestCase::is_valid` (`model.rs:145-149`) refuses any Shared Steps reference `<= 0`, and both production callers run it before any request. Those callers are `commands::cases::update_test_case` (`cases.rs:86`) and `queue_item_request` (`queue.rs:1099`). The save therefore already fails with "Step N has an invalid Shared Steps reference." and sends nothing. `steps_patch` only reaches its refusal when `update_test_case_from_model` is called directly, which only the tests in `tests/ado.rs` do. Not planned: the separate over-refusal, where a title-only edit of a case with an unreadable reference is refused whole, is a different behaviour from the one item 11 describes.

### Out of scope (from the brief)

Auto Run phase 6 housekeeping. The runner following a view switch. A background refresh dropping the modal's unsaved edits. Run-order minors about the removed Run next.

## Global Constraints

- **Branch:** `fix/cleanup-pass`. Stay on it. No release, no version bump, no `changelog.ts` entry, no README change. Do not mention any optional-extras unlock anywhere.
- **Rust tests live only in `src-tauri/tests/`.** Never a `#[cfg(test)]` module inside `src/`.
- **One build or test command at a time** (shared machine). Before ANY cargo command, run the dev-app check with the PowerShell tool: `Get-Process v2, cargo -ErrorAction SilentlyContinue | Select-Object Name, Path`. If a `cargo` is listed, wait for it. Never kill a `v2` process. If one's `Path` is under `target\gate`, ask the controller.
- **Rust commands** run from `src-tauri/` as `CARGO_TARGET_DIR=target/gate cargo test --test <name>` (Bash tool). **Frontend:** `npx vitest run --exclude "**/.claude/**" <files>` and `npx tsc --noEmit` from the repo root. Use the Grep and Read tools for searching (bash `grep` hangs here).
- **`src/bindings.ts` is generated** by `CARGO_TARGET_DIR=target/gate cargo test --test bindings`. Never hand-edit it. If it shows as modified but `git diff --ignore-all-space --ignore-cr-at-eol -- src/bindings.ts` is empty, run `git checkout -- src/bindings.ts` and do not commit it.
- **No HTTP DELETE to Azure DevOps.** The one new request (Task 2) is a GET.
- **User-facing errors name no URL.** Transport failures go through `network_error` (`ado/transport.rs`) or, for sign-in, the new `auth.rs` sentences. Raw errors go to `applog`.
- **One cache implementation per side.** Nothing new is cached. The new TanStack queries are in-memory (`staleTime`), like the drawer's existing `activities` query. `src/lib/cache.test.ts` and `tests/cache.rs` stay green.
- **Theme tokens only.** No hex colours. `src/ui-consistency.test.ts` and `src/a11y.test.tsx` are never weakened.
- **No new crates, no new npm packages.**
- **No em dashes** in any text a user reads that this plan adds.
- **Commits:** Bash heredoc, `git commit -q -F - <<'EOF' ... EOF`, confirm with `git log -1`. End the message with a `Co-Authored-By:` trailer naming the model that writes the commit. The plan shows `Claude Opus 5.5`; write your own model's name if it differs. Keep each edited file's existing line endings (they are CRLF).
- **Big files** (`QueueSection.tsx`, `App.tsx`): Grep for the region, then do a bounded Read.

## Review Focus

1. **A swimlane parent whose type's states cannot be read** (no permission, or the request fails). Expected: the drawer still opens and offers the item's own current state, with no error thrown. Test: Task 2, `a lane parent whose type's states cannot be read still opens, offering its own state`.
2. **An upload hold stored before this change** (titles only, no signatures) is still in someone's localStorage. Expected: it loads and still marks its rows by title, as before. Test: Task 5, `a hold stored before signatures existed still marks its rows by title`.
3. **A draft file changed on disk since the snapshot**, so the entry the app named (`occurrence`) is gone. Expected: the row falls back to the first unclaimed same-titled entry, and nothing is appended twice. Test: Task 5, `an_occurrence_the_file_no_longer_has_falls_back_to_the_first_unclaimed_entry`.
4. **A review-page save the app answers slowly, but inside the new 10 s limit.** Expected: it is saved and says "Saved ✓". The timeout never cuts off a slow but live app. Test: Task 7, `a save the app answers slowly, inside the limit, is still saved`.
5. **The app exits while a Try (not a recording) is running.** Expected: the Try ends as cancelled with the Try's own sentence, and its browser goes. Test: Task 3, in `a_recording_waits_for_a_run_and_a_run_waits_for_a_recording`.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `src/hooks/useMentions.ts` (modify) + test | No background polling; one check when the page becomes visible |
| `src/hooks/usePrAttention.ts` (modify) + test | Scan only PRs whose threads changed |
| `src/lib/notifications.ts` (modify) + test | Seen-set refresh on report; `KNOWN_CAP` exported; wipe prefixes from key helpers |
| `src/lib/mentions.ts` (modify) + test | Baseline wipe prefix from `baselineKey` |
| `src-tauri/src/commands/board.rs`, `src-tauri/src/lib.rs` (modify) | `work_item_type_states` command, registered |
| `src/components/WorkItemDrawer.tsx`, `src/screens/WorkBoard.tsx` (modify) + test | Drawer reads its own type's states when given none |
| `src-tauri/src/commands/autorun.rs` (modify) | `close_autorun_browsers`; `close_browser` waits before removing the profile |
| `src-tauri/src/commands/autorun_record.rs` (modify) | `TRY_CANCELLED`; `unless_cancelled` takes its sentence; `ModuleTryResult.cancelled` |
| `src-tauri/src/lib.rs` (modify) | `RunEvent::Exit` closes Auto Run's browsers |
| `src/screens/AutoRun/ModulePathsDialog.tsx` (modify) + test | Any cancelled Try reads as stopped |
| `src-tauri/src/autorun/floor.rs` (modify) | `Expected.shared`; `steps_on_shared_rows`; `on_shared_row`; rule 5 |
| `src-tauri/src/autorun/publish.rs` (modify) | `PublishCase.shared_steps`; `step_marks_checked`; used in `publish_run` |
| `src/screens/AutoRun/index.tsx`, `RunReview.tsx` (modify) + test | Send each case's Shared Steps rows |
| `src-tauri/src/model.rs`, `src-tauri/src/import_parser/export.rs` (modify) | `DraftEdit.occurrence`; claim by occurrence first; full-lowercase titles |
| `src/lib/fileSync.ts`, `src/lib/queueStamp.ts`, `src/lib/uploadHold.ts` (modify) + tests | `fileOwners`; occurrences in write-backs; signature-first holds; `narrowHold` |
| `src/components/QueueSection.tsx` (modify) + test | Send occurrences, `narrowHold`, remove by identity, "Checking" stage |
| `src-tauri/src/steps_xml.rs` (modify) | Merge keeps the prolog and trailer; builds unless the root is `steps` |
| `src-tauri/src/commands/queue.rs`, `src-tauri/src/ai_bridge.rs` (modify) | Tighter draft shape; one containment helper |
| `src-tauri/web/cases-page.js`, `cases-notes.js`, `cases-specs.js` (modify) + tests | Poll backoff; save timeout; slug keeps every script |
| `src-tauri/src/auth.rs` (modify) | Bounded request-line read; sign-in sentences |
| `src/lib/sharedSteps.ts` (create), `src/components/SharedStepLabel.tsx`, `BugDialog.tsx`, `src/screens/RunPanel/CasePreview.tsx` (modify) + tests | Shared Steps rows by name |
| `src/lib/submitRun.ts` (modify) + test | `stage`, `submitUploading`, `submitLabel` |
| `src/components/ui/modal.tsx`, `src/screens/RunPanel/ExecutionOrderModal.tsx` (modify) + tests | Accessible name |
| `src/bindings.ts` (generated) | Regenerated in Tasks 2, 3, 4 and 5 |

## Tasks

1. Mentions and the bell (items 1, 2, 3, 5)
2. The drawer reads a lane parent's own states (item 4)
3. Auto Run's browsers close with the app; a Try cancels in its own words (items 6, 7)
4. Scripts numbered without Shared Steps rows (item 8)
5. Same-titled drafts pair by identity (item 9)
6. Steps XML merge and draft-file writes (items 12, 16)
7. Review page and sign-in (items 14, 15, 17)
8. Shared Steps rows by name, the pre-flight label, and the Execution order modal's name (items 10, 13, 18)

---

### Task 1: Mentions and the bell

**Files:**
- Modify: `src/hooks/useMentions.ts` (whole file, 50 lines)
- Modify: `src/hooks/usePrAttention.ts:11-12` (imports), `:94-117` (the scan)
- Modify: `src/lib/notifications.ts:44-46`, `:88-95`, `:113-141`, `:259-273`
- Modify: `src/lib/mentions.ts:34-50`
- Test: `src/hooks/useMentions.test.tsx`, `src/hooks/usePrAttention.test.tsx`, `src/lib/notifications.test.ts`, `src/lib/mentions.test.ts` (append)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `export const KNOWN_CAP = 500` in `notifications.ts`. `markSeen` and `raise` keep their signatures. Their seen-set behaviour changes as described in item 3.

- [ ] **Step 1: Write the failing tests.**

In `src/hooks/useMentions.test.tsx`, change the Testing Library import to `import { act, render, waitFor } from "@testing-library/react";` and the hook import to `import { MENTIONS_POLL_MS, useMentions } from "./useMentions";`. Then append:

```tsx
/// A minimised app is not checked for mentions - nobody is there to see the
/// toast. Coming back into view checks once at once, then the 5-minute
/// check resumes. The app's own client turns window-focus refetches off for
/// everything (main.tsx); this query has to turn them back on for itself.
test("no check while the app is hidden, and one as soon as it is back in view", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] });
  let visibility: DocumentVisibilityState = "visible";
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });
  try {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "recent_mentions") {
        calls += 1;
        return [];
      }
    });
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } } });
    render(
      <QueryClientProvider client={qc}>
        <Probe />
      </QueryClientProvider>,
    );
    const tick = async (ms: number) => {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(ms);
      });
    };
    const show = async (v: DocumentVisibilityState) => {
      visibility = v;
      await act(async () => {
        document.dispatchEvent(new Event("visibilitychange", { bubbles: true }));
      });
    };

    await tick(50);
    expect(calls).toBe(1);

    await show("hidden");
    await tick(3 * MENTIONS_POLL_MS);
    expect(calls).toBe(1);

    await show("visible");
    await tick(50);
    expect(calls).toBe(2);

    await tick(MENTIONS_POLL_MS + 1_000);
    expect(calls).toBe(3);
  } finally {
    delete (document as unknown as { visibilityState?: unknown }).visibilityState;
    vi.useRealTimers();
  }
});
```

In `src/hooks/usePrAttention.test.tsx`, add below the existing `vi.mock("../lib/toast", ...)` line:

```tsx
// The real scan, counted: the tests below need to know HOW MANY PRs were
// scanned, not just what the scan raised.
vi.mock("../lib/mentions", async (importOriginal) => {
  const real = await importOriginal<typeof import("../lib/mentions")>();
  return { ...real, prMentions: vi.fn(real.prMentions) };
});
```

Add `import { prMentions } from "../lib/mentions";` to the imports. Then append:

```tsx
/// A thread query settling re-renders the hook with EVERY PR's threads.
/// Scanning them all each time made one poll cycle PRs x threads; a refresh
/// of one PR's threads scans that PR only.
test("a thread refresh rescans that PR's threads only, not every PR's", async () => {
  vi.mocked(prMentions).mockClear();
  mockIPC((cmd) => {
    if (cmd === "connected_user") return { id: "me-guid", display_name: "Avin" };
    if (cmd === "pr_overview") return { mine: [pr(1), pr(2), pr(3)], awaiting: [] };
    if (cmd === "pr_threads") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Probe org="acme" project="Web" />
    </QueryClientProvider>,
  );
  const scanned = () => vi.mocked(prMentions).mock.calls.map((c) => c[0].id);
  await waitFor(() => expect([...scanned()].sort()).toEqual([1, 2, 3]));

  vi.mocked(prMentions).mockClear();
  await act(async () => {
    await qc.refetchQueries({ queryKey: ["pr-threads", "acme", "Web", "web", 2] });
  });
  expect(scanned()).toEqual([2]);
});
```

In `src/lib/notifications.test.ts`, add `KNOWN_CAP` to the import list from `./notifications`. Then append:

```ts
/// A source re-reports its whole state on every check. An id it still
/// reports must stay "seen", however many other ids arrive over time - or a
/// mention dismissed weeks ago comes back once 500 newer ids push it out.
test("an id every check still reports never falls out of the seen set", () => {
  const mention = { id: "mention:wi:41:7", kind: "mention" as const, title: "Sam mentioned you", body: "" };
  expect(raise(ORG, [mention])).toHaveLength(1);
  dismiss(ORG, mention.id);
  for (let check = 0; check < 5; check++) {
    // Between two checks: fewer new ids than the cap, but many in all.
    raise(
      ORG,
      Array.from({ length: 300 }, (_, i) => ({
        id: `pr-comments:web:${check}:${i}`,
        kind: "pr-comments" as const,
        title: "t",
        body: "",
      })),
    );
    // The next check reports the mention again: still seen, not raised.
    expect(raise(ORG, [mention])).toEqual([]);
  }
});

test("one report larger than the cap keeps every id it reported", () => {
  const many = Array.from({ length: KNOWN_CAP + 100 }, (_, i) => ({
    id: `pr-review:web:${i}`,
    kind: "pr-review" as const,
    title: "t",
    body: "",
  }));
  raise(ORG, many);
  expect(raise(ORG, [many[KNOWN_CAP + 99]])).toEqual([]);
});
```

In `src/lib/mentions.test.ts`, add `import { forgetAllNotifications, markSeen, raise } from "./notifications";` (merge it with the existing `./notifications` import). Then append:

```ts
/// A different account signing in wipes the bell and the first-run
/// baselines. Every key the real writers use must go, whatever it is
/// called, so a renamed key cannot slip past the wipe.
test("the account wipe removes every key the bell and the mention baseline wrote", () => {
  const before = new Set(Object.keys(localStorage));
  raise("acme", [{ id: "assigned:1", kind: "assigned", title: "t", body: "" }]);
  markSeen("acme", ["mention:wi:9:9"]);
  noteMentions("acme", [
    {
      notification: { id: "mention:wi:1:2", kind: "mention", title: "t", body: "" },
      created: new Date().toISOString(),
    },
  ]);
  const written = Object.keys(localStorage).filter((k) => !before.has(k));
  // The list, the seen set and the baseline, at least.
  expect(written.length).toBeGreaterThanOrEqual(3);

  forgetAllNotifications();
  forgetMentionBaselines();
  expect(Object.keys(localStorage).filter((k) => written.includes(k))).toEqual([]);
});
```

- [ ] **Step 2: Run to see them fail:** `npx vitest run --exclude "**/.claude/**" src/hooks/useMentions.test.tsx src/hooks/usePrAttention.test.tsx src/lib/notifications.test.ts src/lib/mentions.test.ts`.
  Expected:
  - The visibility test fails: `calls` is 4, not 1, after the hidden stretch.
  - The rescan test fails: after the refresh, `[1, 2, 3]` is scanned, not `[2]`.
  - `an id every check still reports...` fails at the second check with the mention re-raised.
  - `one report larger...` fails with the last id re-raised.
  - The wipe test **passes**. It is a tripwire that pins today's behaviour so the refactor in Step 3 cannot break it.

- [ ] **Step 3: Implement.**

Replace the whole of `src/hooks/useMentions.ts` with:

```ts
/**
 * Work-item @mentions of you, checked when the app starts, every five
 * minutes while it is on screen, and once more whenever it comes back into
 * view. Into the bell (and a toast or OS notification, like a new
 * assignment). Mounted beside usePrAttention, which does the same for
 * mentions in PR threads. A failed check is logged and simply tried again
 * at the next one - no toast.
 */
import { useQuery } from "@tanstack/react-query";
import { useEffect } from "react";
import { commands } from "../bindings";
import { unwrap } from "../lib/ipc";
import { announceMentions, noteMentions, workItemNotification } from "../lib/mentions";
import { logUi } from "../lib/uiLog";

export const MENTIONS_POLL_MS = 5 * 60_000;

export function useMentions(org: string, project: string): void {
  const mentions = useQuery({
    queryKey: ["recent-mentions", org, project],
    queryFn: async () => (await unwrap(commands.recentMentions(org, project))) ?? [],
    enabled: Boolean(org && project),
    refetchInterval: MENTIONS_POLL_MS,
    // Not while the app is minimised or hidden: nothing is on screen to
    // show what a check finds.
    refetchIntervalInBackground: false,
    // Coming back into view checks once, at once, so a mention made while
    // the app was hidden does not wait for the next five-minute tick. In
    // TanStack Query "window focus" is the page becoming visible. The app
    // turns this off for every other query (main.tsx).
    refetchOnWindowFocus: true,
    retry: false,
  });

  useEffect(() => {
    if (!mentions.data) return;
    // The tour guard lives in noteMentions itself (the write chokepoint),
    // not here - see mentions.ts.
    announceMentions(
      noteMentions(
        org,
        mentions.data.map((m) => ({
          notification: workItemNotification(org, project, m),
          created: m.created_date,
        })),
      ),
    );
  }, [mentions.data, org, project]);

  useEffect(() => {
    if (mentions.error) {
      logUi(`mentions: work-item check failed, trying again at the next check: ${mentions.error.message}`);
    }
  }, [mentions.error, mentions.errorUpdatedAt]);
}
```

In `src/hooks/usePrAttention.ts`, change line 12 to `import { useEffect, useRef } from "react";`. Then replace lines 94-117 (from `// Mentions of you in these same threads` through the end of that `useEffect`) with:

```ts
  // Mentions of you in these same threads, on every thread refresh - no
  // request of their own. The store dedupes, so a rescan raises only
  // what is new. The tour guard lives in noteMentions itself (the write
  // chokepoint), not here - see mentions.ts.
  //
  // Skipped while the identity query is unsettled: `reSignIn` invalidates
  // `connected-user`, `pr-overview` and `pr-threads` together, and a
  // thread result that lands before identity does would otherwise scan
  // with the PREVIOUS account's id. `isFetching` alone is not enough - a
  // failed refetch keeps the old `data` and would resume scanning with the
  // stale id on the very next thread refresh - so `isError` gates it too.
  // The existing retry interval on `me` (above) recovers from that state.
  //
  // Only the PRs whose own threads changed are scanned. A thread query
  // settling re-renders this hook with EVERY PR's threads, and scanning
  // them all each time made one poll cycle PRs x threads. `scanned` holds
  // the `dataUpdatedAt` each PR was last scanned at, for one organisation
  // and one signed-in id; either changing scans everything afresh.
  const scanned = useRef<{ who: string; at: Map<string, number> }>({ who: "", at: new Map() });
  const threadStamp = threads.map((t) => t.dataUpdatedAt).join("|");
  useEffect(() => {
    if (!myId || me.isFetching || me.isError) return;
    const who = `${org}|${myId}`;
    if (scanned.current.who !== who) scanned.current = { who, at: new Map() };
    const at = scanned.current.at;
    const found = prs.flatMap((pr, i) => {
      const t = threads[i];
      if (!t?.data) return [];
      const key = `${pr.repo}:${pr.id}`;
      if (at.get(key) === t.dataUpdatedAt) return [];
      at.set(key, t.dataUpdatedAt);
      return prMentions(pr, t.data, myId).map((m) => ({
        notification: prNotification(org, project, m),
        created: m.createdDate,
      }));
    });
    // Called even with nothing found: the organisation's first check is
    // what sets its first-run baseline (mentions.ts).
    announceMentions(noteMentions(org, found));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [threadStamp, myId, me.isFetching, me.isError, me.dataUpdatedAt, org, project]);
```

In `src/lib/notifications.ts`:

1. Replace lines 44-46 with:

```ts
/** Newest `LIST_CAP` kept in the list; `KNOWN_CAP` ids remembered as seen. */
export const LIST_CAP = 50;
export const KNOWN_CAP = 500;
```

2. Replace `remember` (lines 88-95) with:

```ts
/** Remember `ids` as seen, most recently reported first. An id already
 * remembered moves to the front instead of being added twice, so a source
 * that re-reports its state on every check keeps its ids fresh. The cap
 * then evicts only what nothing has reported for longest, and a mention the
 * current check still returns cannot fall out and be raised again. A single
 * report larger than the cap keeps all of it. Writes nothing when the
 * order would not change. */
function remember(org: string, ids: string[]): void {
  const front = [...new Set(ids)];
  if (front.length === 0) return;
  const cur = known(org);
  const inFront = new Set(front);
  const next = [...front, ...cur.filter((id) => !inFront.has(id))].slice(0, Math.max(KNOWN_CAP, front.length));
  if (next.length === cur.length && next.every((id, i) => id === cur[i])) return;
  try {
    localStorage.setItem(knownKey(org), JSON.stringify(next));
  } catch {
    // session-only
  }
}
```

3. Replace `raise` and `markSeen` (lines 113-141) with:

```ts
/** Add what is new. Ids already raised - listed or since dismissed - are
 * skipped, so callers can report the whole current state every time. Every
 * reported id, new or not, is refreshed in the seen set (see remember). */
export function raise(
  org: string,
  items: Array<Omit<AppNotification, "at" | "read">>,
): AppNotification[] {
  if (!org || items.length === 0) return [];
  const seen = new Set([...known(org), ...load(org).map((n) => n.id)]);
  const fresh = items.filter((i) => !seen.has(i.id));
  remember(
    org,
    items.map((i) => i.id),
  );
  if (fresh.length === 0) return [];
  const at = new Date().toISOString();
  const added = fresh.map((i) => ({ ...i, at, read: false }));
  save(org, [...added, ...load(org)].slice(0, LIST_CAP));
  return added;
}

/** Record ids as seen without listing them - a source's backlog on its
 * first run. raise() skips them from then on, exactly like a dismissed
 * notification. Reporting them again keeps them fresh (see remember). */
export function markSeen(org: string, ids: string[]): void {
  if (!org || ids.length === 0) return;
  remember(org, ids);
}
```

4. In `forgetAllNotifications` (lines 259-273), replace the `try { ... }` block with:

```ts
  // The prefixes come from the key helpers themselves (an empty org), so a
  // key renamed there cannot slip past this wipe.
  const prefixes = [listKey(""), knownKey("")];
  try {
    for (const k of Object.keys(localStorage)) {
      if (prefixes.some((p) => k.startsWith(p))) localStorage.removeItem(k);
    }
  } catch {
    // storage unavailable - nothing was stored to leak
  }
```

In `src/lib/mentions.ts`, replace the body of `forgetMentionBaselines` (lines 42-50) with:

```ts
export function forgetMentionBaselines(): void {
  // From the key helper itself, so a rename cannot slip past the wipe.
  const prefix = baselineKey("");
  try {
    for (const k of Object.keys(localStorage)) {
      if (k.startsWith(prefix)) localStorage.removeItem(k);
    }
  } catch {
    // storage unavailable - nothing was stored to leak
  }
}
```

- [ ] **Step 4: Run** (one at a time): `npx vitest run --exclude "**/.claude/**" src/hooks/useMentions.test.tsx src/hooks/usePrAttention.test.tsx src/lib/notifications.test.ts src/lib/mentions.test.ts src/components/NotificationBell.test.tsx src/lib/cache.test.ts`, then `npx tsc --noEmit`. All green, including every test that was there before.

- [ ] **Step 5: Commit**

```bash
git add src/hooks/useMentions.ts src/hooks/useMentions.test.tsx src/hooks/usePrAttention.ts src/hooks/usePrAttention.test.tsx src/lib/notifications.ts src/lib/notifications.test.ts src/lib/mentions.ts src/lib/mentions.test.ts
git commit -q -F - <<'EOF'
fix(v2): mentions rest while the app is hidden, scan only changed PR threads, and stay seen

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 2: The drawer reads a lane parent's own states

**Files:**
- Modify: `src-tauri/src/commands/board.rs` (append a command after `activity_values`, ~l.180)
- Modify: `src-tauri/src/lib.rs:135` (register it)
- Modify: `src/components/WorkItemDrawer.tsx:129-134` (a query after `activities`), `:405-408` (the options)
- Modify: `src/screens/WorkBoard.tsx:337-340` (comment only)
- Test: `src-tauri/tests/work_board.rs` (append), `src/components/WorkItemDrawer.test.tsx`
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: `AdoClient::get_work_item_states(&self, org, project, wi_type) -> Result<Vec<StateInfo>, AdoError>` (exists).
- Produces: `#[tauri::command] pub async fn work_item_type_states(app, organization: String, project: String, work_item_type: String) -> Result<Vec<work_board::StateInfo>, ado::AdoError>`. In TS this is `commands.workItemTypeStates(organization, project, workItemType)`.

- [ ] **Step 1: Write the failing tests.**

Append to `src-tauri/tests/work_board.rs`:

```rust
/// A swimlane's parent is often a type with no card on the board (a
/// Feature above PBIs). Its drawer reads that type's own states through
/// `work_item_type_states`, which is this read.
#[tokio::test]
async fn a_type_with_no_card_on_the_board_has_its_states_read() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/org/proj/_apis/wit/workitemtypes/Feature/states"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "value": [
                { "name": "New", "color": "b2b2b2", "category": "Proposed" },
                { "name": "In Progress", "color": "007acc", "category": "InProgress" },
                { "name": "Done", "color": "339933", "category": "Completed" }
            ]
        })))
        .mount(&server)
        .await;
    let states = AdoClient::with_base_urls("tok".into(), server.uri(), server.uri())
        .get_work_item_states("org", "proj", "Feature")
        .await
        .unwrap();
    let names: Vec<&str> = states.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["New", "In Progress", "Done"]);
}
```

In `src/components/WorkItemDrawer.test.tsx`, change `renderDrawer` to take the states:

```tsx
function renderDrawer(states: string[] = ["To Do", "In Progress", "Done"]) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <WorkItemDrawer
        org="acme"
        project="Web"
        itemId={2003}
        states={states}
        onClose={vi.fn()}
        onSaved={vi.fn()}
      />
    </QueryClientProvider>,
  );
}
```

Then append:

```tsx
const optionsOf = () =>
  [...(screen.getByLabelText("State") as HTMLSelectElement).options].map((o) => o.value);

/// A swimlane's parent that is not itself a card (or an item a notification
/// opened from outside the loaded board) arrives with no states. The drawer
/// reads its own type's instead of offering nothing.
test("an item that arrives with no states reads its own type's", async () => {
  const asked: unknown[] = [];
  mockIPC((cmd, args) => {
    switch (cmd) {
      case "work_item_detail":
        return detail("Parent feature");
      case "list_team_members":
        return [];
      case "activity_values":
        return [];
      case "work_item_comments":
        return [];
      case "work_item_type_states":
        asked.push(args);
        return [
          { name: "To Do", color: "b2b2b2", category: "Proposed" },
          { name: "Doing", color: "007acc", category: "InProgress" },
          { name: "Done", color: "339933", category: "Completed" },
        ];
    }
  });
  renderDrawer([]);
  await waitFor(() => expect(optionsOf()).toEqual(["To Do", "Doing", "Done"]));
  expect(asked).toEqual([{ organization: "acme", project: "Web", workItemType: "Bug" }]);
});

/// Review Focus 1.
test("a lane parent whose type's states cannot be read still opens, offering its own state", async () => {
  mockIPC((cmd) => {
    switch (cmd) {
      case "work_item_detail":
        return detail("Parent feature");
      case "list_team_members":
        return [];
      case "activity_values":
        return [];
      case "work_item_comments":
        return [];
      case "work_item_type_states":
        throw { kind: "Network", detail: "Can't reach Azure DevOps." };
    }
  });
  renderDrawer([]);
  await waitFor(() => expect(optionsOf()).toEqual(["To Do"]));
});
```

- [ ] **Step 2: Run to see them fail.** Do the dev-app check first, then from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test work_board a_type_with_no_card`. Expected: PASS. The client read exists already; this test guards the read the new command wraps. Then `npx vitest run --exclude "**/.claude/**" src/components/WorkItemDrawer.test.tsx`. Expected: `an item that arrives with no states...` FAILS: the select holds only `To Do`, and nothing asked for `work_item_type_states`.

- [ ] **Step 3: Implement.**

Append to `src-tauri/src/commands/board.rs`, right after `activity_values`:

```rust
/// The states a work item type has on this project's process. For an item
/// the board did not load as a card - a swimlane's parent, or one a
/// notification opened - whose drawer would otherwise offer no state to
/// pick. Read only.
#[tauri::command]
#[specta::specta]
pub async fn work_item_type_states(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    work_item_type: String,
) -> Result<Vec<work_board::StateInfo>, ado::AdoError> {
    let token = get_fresh_token(&app).await?;
    ado::AdoClient::new(token)
        .get_work_item_states(&organization, &project, &work_item_type)
        .await
}
```

In `src-tauri/src/lib.rs`, after `board::activity_values,` (line 135) add `board::work_item_type_states,`.

Regenerate the bindings (dev-app check first): from `src-tauri/`, `CARGO_TARGET_DIR=target/gate cargo test --test bindings`. `src/bindings.ts` now has `workItemTypeStates`.

In `src/components/WorkItemDrawer.tsx`, right after the `activities` query (after line 134) add:

```tsx
  // A swimlane's parent that is not itself a card, or an item a
  // notification opened from outside the loaded board, arrives with no
  // states. Read its own type's, the way the board reads them for its
  // cards. In memory only, like `activities`.
  const ownStates = useQuery({
    queryKey: ["work-item-type-states", org, project, detail.data?.work_item_type],
    queryFn: () => unwrap(commands.workItemTypeStates(org, project, detail.data!.work_item_type)),
    enabled: states.length === 0 && Boolean(detail.data?.work_item_type),
    staleTime: Infinity,
    retry: false,
  });
  const stateNames = states.length > 0 ? states : (ownStates.data ?? []).map((s) => s.name);
```

and replace the two lines that build the options (lines 405-408) with:

```tsx
                    {!stateNames.includes(draft.state) && <option>{draft.state}</option>}
                    {stateNames.map((s) => (
                      <option key={s}>{s}</option>
                    ))}
```

In `src/screens/WorkBoard.tsx`, replace the comment on lines 337-340 with:

```ts
  // A notification handed us an item: open its drawer. It waits for the
  // board's own load because the drawer takes a card's states from it; an
  // item outside the loaded board still opens, and the drawer reads its
  // type's states itself. Handled once: the handoff is cleared by the caller.
```

- [ ] **Step 4: Run** (one at a time): `npx vitest run --exclude "**/.claude/**" src/components/WorkItemDrawer.test.tsx src/screens/WorkBoard.test.tsx src/ui-consistency.test.ts`, then `npx tsc --noEmit`. After the dev-app check, run from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test work_board`. All green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/board.rs src-tauri/src/lib.rs src-tauri/tests/work_board.rs src/bindings.ts src/components/WorkItemDrawer.tsx src/components/WorkItemDrawer.test.tsx src/screens/WorkBoard.tsx
git commit -q -F - <<'EOF'
fix(v2): the work item drawer reads its own type's states when the board has none for it

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 3: Auto Run's browsers close with the app; a Try cancels in its own words

**Files:**
- Modify: `src-tauri/src/commands/autorun_record.rs:191-195` (`ModuleTryResult`), `:269-324` (`unless_cancelled`, `check_in_fresh_browser`), `:397` (Stop's check), `:443-465` (Try)
- Modify: `src-tauri/src/commands/autorun.rs:118-139` (`close_browser`, plus `close_autorun_browsers`)
- Modify: `src-tauri/src/lib.rs:283-385` (the exit hook)
- Modify: `src/screens/AutoRun/ModulePathsDialog.tsx:231-259` (`tryPath`)
- Test: `src-tauri/tests/autorun_recorder.rs`, `src/screens/AutoRun/ModulePathsDialog.test.tsx`
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: `auto_run_record_cancel()`, `RecorderClaim`, `recorder::CANCELLED` (exist).
- Produces:
  - `pub const TRY_CANCELLED: &str = "the check was cancelled - the saved path was not changed"` in `commands::autorun_record`
  - `pub async fn unless_cancelled<T>(work: impl Future<Output = Result<T, String>>, cancelled: &'static str) -> Result<T, String>`
  - `ModuleTryResult { ok: bool, detail: String, cancelled: bool }`. In TS, `cancelled: boolean`.
  - `pub async fn close_autorun_browsers()` in `commands::autorun`

- [ ] **Step 1: Write the failing tests.**

In `src-tauri/tests/autorun_recorder.rs`:

1. Replace the `use v2_lib::commands::autorun_record::{...}` block with:

```rust
use v2_lib::commands::autorun::close_autorun_browsers;
use v2_lib::commands::autorun_record::{
    auto_run_record_cancel, auto_run_recording_is_open, listen, open_the_recording, prepare_to_record,
    recording_is_going, recording_is_open, refuse_to_record_now, refuse_while_recording, unless_cancelled,
    RecorderClaim, RecordingFor, ALREADY_RECORDING, RECORDING_BUSY, TRY_CANCELLED,
};
```

2. In `a_recording_waits_for_a_run_and_a_run_waits_for_a_recording`, replace lines 393-409 (from `let rec = RecorderClaim::claim().expect("free again");` above `let closed = Arc::new(...)` through `assert!(!recording_is_going());`) with:

```rust
    let rec = RecorderClaim::claim().expect("free again");
    let closed = Arc::new(AtomicBool::new(false));
    let browser = ClosesOnDrop(closed.clone());
    let check = tokio::spawn(unless_cancelled(
        async move {
            let _browser = browser;
            tokio::time::sleep(Duration::from_secs(600)).await;
            Ok::<String, String>("/hr/leave".into())
        },
        CANCELLED,
    ));
    tokio::time::sleep(Duration::from_millis(20)).await;
    auto_run_record_cancel().await.unwrap();
    let out = tokio::time::timeout(Duration::from_secs(5), check).await.expect("a cancelled check ends at once").unwrap();
    assert_eq!(out, Err(CANCELLED.to_string()));
    assert!(closed.load(Ordering::SeqCst), "the check's browser is closed");
    // The Cancel is used up: the next check runs to its end.
    assert_eq!(unless_cancelled(async { Ok::<_, String>("/hr/leave") }, CANCELLED).await, Ok("/hr/leave"));

    // Item 7: a cancelled Try says so in its own words, not the recording's.
    let tried = tokio::spawn(unless_cancelled(
        async {
            tokio::time::sleep(Duration::from_secs(600)).await;
            Ok::<String, String>("/hr/leave".into())
        },
        TRY_CANCELLED,
    ));
    tokio::time::sleep(Duration::from_millis(20)).await;
    auto_run_record_cancel().await.unwrap();
    let out = tokio::time::timeout(Duration::from_secs(5), tried).await.expect("a cancelled Try ends at once").unwrap();
    assert_eq!(out, Err(TRY_CANCELLED.to_string()));
    assert!(!TRY_CANCELLED.contains("recording"), "{TRY_CANCELLED}");
    drop(rec);
    assert!(!recording_is_going());
```

3. At the end of the same test (after `assert!(!auto_run_recording_is_open().await);`, before the closing `}`), add:

```rust
    // Item 6: the app exiting ends an open recording the way Cancel does -
    // its browser is closed and the recorder is free.
    let rec = RecorderClaim::claim().expect("free again");
    let (closed, spawned) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    open_the_recording(rec, about(), fake_recording(ClosesOnDrop(closed.clone()), spawned.clone()))
        .await
        .expect("nothing cancelled this one");
    tokio::time::timeout(Duration::from_secs(5), close_autorun_browsers()).await.expect("closing on exit is bounded");
    assert!(closed.load(Ordering::SeqCst), "the recording browser is closed");
    assert!(!recording_is_going());
    assert!(!recording_is_open().await);

    // Review Focus 5: exiting while a Try runs (it holds the recorder, with
    // no recording to end) stops the Try too, in the Try's own words.
    let rec = RecorderClaim::claim().expect("free again");
    let tried = tokio::spawn(unless_cancelled(
        async {
            tokio::time::sleep(Duration::from_secs(600)).await;
            Ok::<String, String>("/hr/leave".into())
        },
        TRY_CANCELLED,
    ));
    tokio::time::sleep(Duration::from_millis(20)).await;
    tokio::time::timeout(Duration::from_secs(5), close_autorun_browsers()).await.expect("bounded");
    let out = tokio::time::timeout(Duration::from_secs(5), tried).await.expect("the Try ends").unwrap();
    assert_eq!(out, Err(TRY_CANCELLED.to_string()));
    drop(rec);

    // With nothing open there is nothing to do, and it returns at once.
    tokio::time::timeout(Duration::from_secs(1), close_autorun_browsers()).await.expect("nothing to close");
```

In `src/screens/AutoRun/ModulePathsDialog.test.tsx`, in the existing test `a Try can be cancelled, and a cancelled Try is not shown as the path failing`, replace `resolveTry?.({ ok: false, detail: "the recording was cancelled - nothing was saved" });` with `resolveTry?.({ ok: false, cancelled: true, detail: "the check was cancelled - the saved path was not changed" });` and the matching `queryByText(...)` argument with `"the check was cancelled - the saved path was not changed"`. Then append:

```tsx
/// A Try cancelled from somewhere else - another dialog's "Cancel that
/// recording" - comes back cancelled though THIS dialog never asked. It is
/// still not the path failing.
test("a Try cancelled from elsewhere reads as stopped, not as the path failing", async () => {
  mount((cmd) => {
    if (cmd === "auto_run_load_nav") return { direct_urls: true, modules: [LEAVE] };
    if (cmd === "auto_run_try_module_path") {
      return { ok: false, cancelled: true, detail: "the check was cancelled - the saved path was not changed" };
    }
  });
  const tryIt = await screen.findByRole("button", { name: "Try Leave" });
  await waitFor(() => expect(tryIt).toBeEnabled());
  fireEvent.click(tryIt);
  await waitFor(() => expect(toast.info).toHaveBeenCalledWith("Stopped trying Leave."));
  expect(screen.queryByText("the check was cancelled - the saved path was not changed")).not.toBeInTheDocument();
});
```

- [ ] **Step 2: Run to see them fail.** Do the dev-app check first, then from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test autorun_recorder`. Expected: compile errors, because `close_autorun_browsers` and `TRY_CANCELLED` do not exist and `unless_cancelled` takes one argument. Then `npx vitest run --exclude "**/.claude/**" src/screens/AutoRun/ModulePathsDialog.test.tsx`. Expected: the new test FAILS, with no "Stopped trying Leave." toast and the detail shown as a failure.

- [ ] **Step 3: Implement.**

In `src-tauri/src/commands/autorun_record.rs`:

1. Replace `ModuleTryResult` (lines 191-195) with:

```rust
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ModuleTryResult {
    pub ok: bool,
    pub detail: String,
    /// The Try was cancelled - by this dialog or any other - so it says
    /// nothing about the path. The dialog shows it as stopped, not failed.
    pub cancelled: bool,
}

/// Said by a Try that was cancelled. A Try only checks a saved path, so
/// nothing was saved or lost - unlike a recording's `recorder::CANCELLED`.
pub const TRY_CANCELLED: &str = "the check was cancelled - the saved path was not changed";
```

2. Replace `unless_cancelled` (lines 273-295, doc comment included) with:

```rust
/// Run `work` (a check, which holds the recorder's claim) until it ends or
/// a Cancel arrives, whichever is first. On a Cancel `work` is dropped where
/// it stands, and with it anything it borrowed a browser through - every
/// step of a check is bounded, but together they can take minutes, and the
/// person asked to stop now. The Cancel is used up, so it cannot also end
/// the next recording. `cancelled` is what a cancel says: a recording's
/// check and a Try say different things.
pub async fn unless_cancelled<T>(
    work: impl std::future::Future<Output = Result<T, String>>,
    cancelled: &'static str,
) -> Result<T, String> {
    let cancel_asked = async {
        loop {
            if CANCEL_PENDING.swap(false, Ordering::SeqCst) {
                return;
            }
            tokio::time::sleep(CANCEL_POLL).await;
        }
    };
    tokio::select! {
        out = work => out,
        () = cancel_asked => {
            crate::applog::info("Auto-run module path check cancelled");
            Err(cancelled.to_string())
        }
    }
}
```

3. In `check_in_fresh_browser`, add a last parameter `cancelled: &'static str` after `path: &ModulePath`, and pass it as the second argument of its `unless_cancelled(nav::check_path(...), cancelled)` call.

4. In `auto_run_record_stop`, line 397 becomes `match check_in_fresh_browser(&root, &organization, &project, &account, which, &path, recorder::CANCELLED).await {`.

5. Replace the body's final `Ok(...)` of `auto_run_try_module_path` (lines 459-464) with:

```rust
    let which = Browser::from_name(&browser_name);
    Ok(match check_in_fresh_browser(&root, &organization, &project, &account, which, &path, TRY_CANCELLED).await {
        Ok(arrived) => ModuleTryResult { ok: true, cancelled: false, detail: format!("reached {arrived}") },
        Err(detail) => ModuleTryResult { ok: false, cancelled: detail == TRY_CANCELLED, detail },
    })
```

In `src-tauri/src/commands/autorun.rs`, replace `close_browser` and `close_session` (lines 118-129) with:

```rust
/// Kill the process and drop its throwaway profile. Shared with
/// `autorun_replay`, whose `RealBrowsers` closes one of these after every
/// case (and on the way out of a failed open) so a background browser can
/// never outlive the run that started it. It waits for the process to be
/// gone first: a browser still shutting down holds files in its profile,
/// and removing the folder under it fails.
pub(crate) fn close_browser(mut browser: LaunchedBrowser) {
    let _ = browser.child.kill();
    let _ = browser.child.wait();
    let _ = std::fs::remove_dir_all(&browser.profile_dir);
}

fn close_session(s: Session) {
    close_browser(s.browser);
}

/// Auto Run's browsers go with the app. A recording - or a Start, a check or
/// a Try still going - is ended the way Cancel ends it, which closes the
/// recording browser; then the supervised browser is closed. Each takes its
/// throwaway profile with it (`close_browser`). Called from the app's exit
/// hook in lib.rs, which bounds it.
pub async fn close_autorun_browsers() {
    let _ = crate::commands::autorun_record::auto_run_record_cancel().await;
    if let Some(s) = SESSION.lock().await.take() {
        close_session(s);
        crate::applog::info("Auto-run browser closed as the app exits");
    }
}
```

In `src-tauri/src/lib.rs`, add this function directly above `#[cfg_attr(mobile, tauri::mobile_entry_point)]`:

```rust
/// Auto Run's browsers go with the app (`close_autorun_browsers`). Bounded,
/// so a browser that will not die cannot hold the app's exit.
fn close_autorun_on_exit() {
    tauri::async_runtime::block_on(async {
        let closing = commands::autorun::close_autorun_browsers();
        if tokio::time::timeout(std::time::Duration::from_secs(3), closing).await.is_err() {
            applog::warn("Auto Run's browsers were still closing when the app exited");
        }
    });
}
```

and replace the last three lines of `run()` (lines 383-384, `.run(tauri::generate_context!())` and `.expect(...)`) with:

```rust
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                close_autorun_on_exit();
            }
        });
```

Regenerate the bindings (dev-app check first): from `src-tauri/`, `CARGO_TARGET_DIR=target/gate cargo test --test bindings`. `ModuleTryResult` gains `cancelled: boolean`.

In `src/screens/AutoRun/ModulePathsDialog.tsx`, replace the `show` helper inside `tryPath` (lines 237-250) with:

```tsx
    const show = (result: { ok: boolean; detail: string; cancelled?: boolean }) => {
      // A cancelled Try says nothing about the path: drop any old answer
      // rather than show the cancel as the path failing. Cancelled by this
      // dialog (asked), or by anything else (the backend says so).
      if (result.cancelled || (cancelAsked.current && !result.ok)) {
        setTried((t) => {
          const next = { ...t };
          delete next[module];
          return next;
        });
        toast.info(`Stopped trying ${module}.`);
        return;
      }
      setTried((t) => ({ ...t, [module]: { ok: result.ok, detail: result.detail } }));
    };
```

- [ ] **Step 4: Run** (one at a time, dev-app check before each cargo command), from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test autorun_recorder`, then `--test autorun_replay`, then `--test autorun_commands`. Then `npx vitest run --exclude "**/.claude/**" src/screens/AutoRun/ModulePathsDialog.test.tsx src/screens/AutoRun/index.test.tsx`, then `npx tsc --noEmit`. All green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/autorun_record.rs src-tauri/src/commands/autorun.rs src-tauri/src/lib.rs src-tauri/tests/autorun_recorder.rs src/bindings.ts src/screens/AutoRun/ModulePathsDialog.tsx src/screens/AutoRun/ModulePathsDialog.test.tsx
git commit -q -F - <<'EOF'
fix(v2): Auto Run's browsers close with the app, and a cancelled Try says so in its own words

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 4: Scripts numbered without Shared Steps rows

**Files:**
- Modify: `src-tauri/src/autorun/floor.rs` (whole file, 80 lines)
- Modify: `src-tauri/src/autorun/publish.rs:14-20` (`PublishCase`), add `step_marks_checked` after `mark_for_step` (~l.120), `:321-361` (use it)
- Modify: `src/screens/AutoRun/index.tsx:605`, `src/screens/AutoRun/RunReview.tsx:70-73`, `:238`
- Test: `src-tauri/tests/autorun_floor.rs`, `src-tauri/tests/autorun_publish.rs`, `src/screens/AutoRun/RunReview.test.tsx`
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: `steps_xml::Step.shared: Option<i32>`, `CaseRecord.steps[].step_number` (exist).
- Produces:
  - `Expected { step_number: i32, expected: String, shared: bool }`
  - `pub fn steps_on_shared_rows(script_steps: &[i32], shared_rows: &[i32]) -> Vec<i32>` (sorted, each once)
  - `pub fn on_shared_row(n: i32) -> String`
  - `PublishCase { case_id: i32, step_ids: Vec<String>, #[serde(default)] shared_steps: Vec<i32> }`. In TS, `shared_steps?: number[]`.
  - `pub fn step_marks_checked(case: &CaseRecord, publish: Option<&PublishCase>) -> Result<(Vec<String>, Vec<Option<String>>), String>`
  - `RunReview` prop `sharedSteps?: Record<number, number[]>`

- [ ] **Step 1: Write the failing tests.**

In `src-tauri/tests/autorun_floor.rs`, change the import to `use v2_lib::autorun::floor::{check_floor, expected_of, steps_on_shared_rows, Expected};`, and change the `case` helper's map to `.map(|(i, e)| Expected { step_number: i as i32 + 1, expected: e.to_string(), shared: false })`. Then append:

```rust
/// Scripts saved before 1.25.23 were numbered as if the case had no Shared
/// Steps rows. On a case with one, such a script puts a step on the Shared
/// Steps row itself. That is refused, with the way to fix it.
#[test]
fn a_script_step_on_a_shared_steps_row_is_refused() {
    let steps = vec![
        Step { action: "Open the page".into(), expected: "The page shows".into(), shared: None },
        Step { shared: Some(812), ..Default::default() },
        Step { action: "Save".into(), expected: "Saved".into(), shared: None },
    ];
    let expected = expected_of(&steps);
    assert!(expected[1].shared && !expected[0].shared && !expected[2].shared);

    // Numbered without the Shared Steps row: "Save" sits on step 2.
    let old = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "page" }] },
        { "step_number": 2, "actions": [{ "kind": "check_text", "value": "Saved" }] }
    ]));
    let out = check_floor(&old, &expected);
    let on_shared = out.iter().find(|s| s.starts_with("step 2 is a Shared Steps entry")).expect("rule 5");
    assert!(on_shared.contains("from 2 on"), "{on_shared}");

    // Numbered with it: holds.
    let fixed = script(serde_json::json!([
        { "step_number": 1, "actions": [{ "kind": "check_text", "value": "page" }] },
        { "step_number": 3, "actions": [{ "kind": "check_text", "value": "Saved" }] }
    ]));
    assert!(check_floor(&fixed, &expected).is_empty(), "{:?}", check_floor(&fixed, &expected));
}

#[test]
fn steps_on_shared_rows_are_sorted_and_named_once() {
    assert_eq!(steps_on_shared_rows(&[4, 2, 2, 1, 0, -1], &[2, 4]), vec![2, 4]);
    assert!(steps_on_shared_rows(&[1, 3], &[2]).is_empty());
    assert!(steps_on_shared_rows(&[1, 2], &[]).is_empty());
}
```

In `src-tauri/tests/autorun_publish.rs`, add `step_marks_checked` to the `use v2_lib::autorun::publish::{...}` list. Change the two literals in `publish_cases()` to add `shared_steps: vec![]`. Then append:

```rust
/// A run whose script puts a step on one of the case's Shared Steps rows
/// was numbered before those rows were counted: its marks would land a row
/// off, so it sends none and says why. With no Shared Steps it is exactly
/// step_marks.
#[test]
fn a_run_numbered_without_the_shared_steps_row_sends_no_step_marks() {
    let dir = tempfile::tempdir().unwrap();
    let run = reviewed_run(dir.path());
    let ids = vec!["101".to_string(), String::new(), "103".to_string()];

    let plain = PublishCase { case_id: 7, step_ids: ids.clone(), shared_steps: vec![] };
    let (sent_ids, marks) = step_marks_checked(&run.cases[0], Some(&plain)).unwrap();
    assert_eq!(sent_ids, ids);
    assert_eq!(marks, step_marks(&run.cases[0], &ids));

    // Case 7's run has steps 0 to 3; row 2 of the case is a Shared Steps row.
    let shared = PublishCase { case_id: 7, step_ids: ids, shared_steps: vec![2] };
    let why = step_marks_checked(&run.cases[0], Some(&shared)).unwrap_err();
    assert!(why.contains("step 2") && why.contains("Shared Steps"), "{why}");
    assert!(!why.contains("http"), "{why}");

    // No PublishCase at all (the screen did not send the case): no ids, no marks.
    assert_eq!(step_marks_checked(&run.cases[0], None).unwrap(), (vec![], vec![]));
}
```

In `src/screens/AutoRun/RunReview.test.tsx`:
1. Add `sharedSteps?: Record<number, number[]>;` to `renderReview`'s overrides type, and `sharedSteps={overrides.sharedSteps}` to the `<RunReview ... />` it renders.
2. In `sending says what it will do and sends only after the person agrees`, change the expected `cases` to `[{ case_id: 1, step_ids: ["2", "3", "4"], shared_steps: [] }, { case_id: 2, step_ids: ["2"], shared_steps: [] }]`.
3. Append:

```tsx
test("each case's Shared Steps rows travel with the send", async () => {
  const publishCalls: unknown[] = [];
  renderReview(SEND_RUN, {
    pbiTitle: "Leave module",
    runId: "run-9",
    stepIds: SEND_STEP_IDS,
    sharedSteps: { 1: [2] },
    extra: (cmd, args) => {
      if (cmd === "auto_run_publish") {
        publishCalls.push(args);
        return {
          status: "sent",
          run_id: 9,
          web_url: "https://dev.azure.com/acme/_testManagement/runs/9",
          sent: [1, 2],
          skipped: [],
          problems: [],
        };
      }
      return null;
    },
  });
  await screen.findByText(/proposed: passed/i);
  fireEvent.click(screen.getByRole("button", { name: "Send to Azure DevOps" }));
  fireEvent.click(await screen.findByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(publishCalls).toHaveLength(1));
  expect((publishCalls[0] as { cases: unknown[] }).cases).toEqual([
    { case_id: 1, step_ids: ["2", "3", "4"], shared_steps: [2] },
    { case_id: 2, step_ids: ["2"], shared_steps: [] },
  ]);
});
```

- [ ] **Step 2: Run to see them fail.** Do the dev-app check first, then from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test autorun_floor`. Expected: compile error (`Expected` has no field `shared`; no `steps_on_shared_rows`). Then `npx vitest run --exclude "**/.claude/**" src/screens/AutoRun/RunReview.test.tsx`. Expected: both send tests FAIL, because the payload has no `shared_steps`.

- [ ] **Step 3: Implement.**

Replace `src-tauri/src/autorun/floor.rs` with:

```rust
//! The expected-result floor: every way an Auto Run script falls short of
//! its test case's expected results. Pure - no browser, no filesystem.

use super::CaseScript;
use crate::steps_xml::Step;

/// One expected result of the test case, by step position (1-based).
pub struct Expected {
    pub step_number: i32,
    pub expected: String,
    /// The case step is a Shared Steps reference: its steps live in another
    /// work item, and a script writes no step for it (guide.rs).
    pub shared: bool,
}

/// The case's expected results, by position. Empty ones are kept: they are
/// the "no expectation" case, which the floor treats differently from a
/// missing step.
pub fn expected_of(steps: &[Step]) -> Vec<Expected> {
    steps
        .iter()
        .enumerate()
        .map(|(i, s)| Expected {
            step_number: i as i32 + 1,
            expected: s.expected.trim().to_string(),
            shared: s.shared.is_some(),
        })
        .collect()
}

fn truncated(s: &str) -> String {
    s.chars().take(60).collect()
}

/// The script step numbers that land on a Shared Steps row of the case,
/// sorted, each once. A script writes no step for such a row; one that does
/// was numbered as if the case had no Shared Steps - how every script saved
/// before 1.25.23 was numbered - so its later steps, and their results, sit
/// a row off. Shared by the save (rule 5 below) and the send (publish.rs).
pub fn steps_on_shared_rows(script_steps: &[i32], shared_rows: &[i32]) -> Vec<i32> {
    let mut hit: Vec<i32> = script_steps.iter().copied().filter(|n| shared_rows.contains(n)).collect();
    hit.sort_unstable();
    hit.dedup();
    hit
}

/// What rule 5 says about script step `n`.
pub fn on_shared_row(n: i32) -> String {
    format!(
        "step {n} is a Shared Steps entry in the case, which a script writes no step for - if this script was numbered without its Shared Steps, add one to every step number from {n} on"
    )
}

/// Every way this script falls short of its case. Empty means it holds.
/// Rules 1-2 are driven by the CASE (only speak about a step position it
/// has an opinion on). Rules 3-4 are driven by the SCRIPT: a step with
/// `unchecked` set is judged wherever it sits, including step 0 and any
/// step past the case's count, where "no case step there" is rule 4. Rule 5
/// refuses a script step on a Shared Steps row.
pub fn check_floor(script: &CaseScript, expected: &[Expected]) -> Vec<String> {
    let mut out: Vec<(i32, String)> = Vec::new();

    for e in expected {
        let want = e.expected.trim();
        let n = e.step_number;
        if want.is_empty() {
            continue;
        }
        match script.steps.iter().find(|s| s.step_number == n) {
            None => {
                out.push((n, format!("step {n} expects \"{}\" but the script has no step {n}", truncated(want))));
            }
            Some(step) => {
                let has_check = step.actions.iter().any(|a| a.is_check());
                if step.unchecked.is_none() && !has_check {
                    out.push((
                        n,
                        format!(
                            "step {n} expects \"{}\" but the script checks nothing there - add an expect_ action, or say why in \"unchecked\"",
                            truncated(want)
                        ),
                    ));
                }
            }
        }
    }

    for step in &script.steps {
        if step.unchecked.is_none() {
            continue;
        }
        let n = step.step_number;
        let has_check = step.actions.iter().any(|a| a.is_check());
        if has_check {
            out.push((n, format!("step {n} says it is unchecked but has a check - drop one or the other")));
            continue;
        }
        let want_here = expected.iter().find(|e| e.step_number == n).is_some_and(|e| !e.expected.trim().is_empty());
        if !want_here {
            out.push((n, format!("step {n} says it is unchecked but the case expects nothing there")));
        }
    }

    let shared_rows: Vec<i32> = expected.iter().filter(|e| e.shared).map(|e| e.step_number).collect();
    let numbers: Vec<i32> = script.steps.iter().map(|s| s.step_number).collect();
    for n in steps_on_shared_rows(&numbers, &shared_rows) {
        out.push((n, on_shared_row(n)));
    }

    out.sort();
    out.dedup();
    out.into_iter().map(|(_, s)| s).collect()
}
```

In `src-tauri/src/autorun/publish.rs`:

1. Replace `PublishCase` (lines 14-20) with:

```rust
/// What the screen knows about a case that the run file does not: the
/// case's real Azure DevOps step ids, in document order, and which of its
/// rows are Shared Steps references.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct PublishCase {
    pub case_id: i32,
    pub step_ids: Vec<String>,
    /// The case's Shared Steps rows, by position (1-based). A Shared Steps
    /// row's step id is "", the same as a step with no id, so the ids alone
    /// cannot tell them apart. Absent: no such rows are known.
    #[serde(default)]
    pub shared_steps: Vec<i32>,
}
```

2. Add after `mark_for_step` (after line 120):

```rust
/// The step ids and marks one case's result carries - or, when the run put
/// a step on one of the case's Shared Steps rows, why it carries none. Such
/// a run came from a script numbered without those rows (every script saved
/// before 1.25.23), so each mark from that row on would land on the wrong
/// step. The verdict and the comment still go; the marks do not.
pub fn step_marks_checked(
    case: &CaseRecord,
    publish: Option<&PublishCase>,
) -> Result<(Vec<String>, Vec<Option<String>>), String> {
    let Some(p) = publish else {
        return Ok((Vec::new(), Vec::new()));
    };
    let ran: Vec<i32> = case.steps.iter().map(|s| s.step_number).collect();
    if let Some(&n) = super::floor::steps_on_shared_rows(&ran, &p.shared_steps).first() {
        return Err(format!(
            "its script puts step {n} on a Shared Steps row, so its step-by-step marks were not sent - add one to every script step number from {n} on, then run it again"
        ));
    }
    let marks = step_marks(case, &p.step_ids);
    Ok((p.step_ids.clone(), marks))
}
```

3. In `publish_run`, replace lines 323-324 (`let step_ids = ...` and `let marks = step_marks(case, &step_ids);`) with:

```rust
        let (step_ids, marks) = match step_marks_checked(case, cases.iter().find(|c| c.case_id == id)) {
            Ok((ids, marks)) => (Some(ids), Some(marks)),
            Err(why) => {
                problems.push(format!("case {id}: {why}"));
                (None, None)
            }
        };
```

and in the `PointOutcome { ... }` literal (lines 357-358) write `step_ids: step_ids.clone(),` and `step_outcomes: marks.clone(),`.

Regenerate the bindings (dev-app check first): from `src-tauri/`, `CARGO_TARGET_DIR=target/gate cargo test --test bindings`.

In `src/screens/AutoRun/RunReview.tsx`, after the `stepIds` prop (line 73) add:

```tsx
  /** Each case's Shared Steps rows, 1-based. Sent with the run so a script
   * numbered without them never has its step marks recorded a row off. */
  sharedSteps?: Record<number, number[]>;
```

and change line 238 to:

```tsx
          confirmedCases.map((c) => ({
            case_id: c.case_id,
            step_ids: props.stepIds[c.case_id] ?? [],
            shared_steps: props.sharedSteps?.[c.case_id] ?? [],
          })),
```

In `src/screens/AutoRun/index.tsx`, after line 605 (`stepIds={...}`) add:

```tsx
          sharedSteps={Object.fromEntries(
            rows.map((c) => [c.id, c.steps.flatMap((s, i) => (s.shared != null ? [i + 1] : []))]),
          )}
```

- [ ] **Step 4: Run** (one at a time, dev-app check before each cargo command), from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test autorun_floor`, then `--test autorun_publish`, then `--test ai_bridge`. Then `npx vitest run --exclude "**/.claude/**" src/screens/AutoRun/RunReview.test.tsx src/screens/AutoRun/index.test.tsx src/screens/AutoRun/PastRuns.test.tsx`, then `npx tsc --noEmit`. All green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/autorun/floor.rs src-tauri/src/autorun/publish.rs src-tauri/tests/autorun_floor.rs src-tauri/tests/autorun_publish.rs src/bindings.ts src/screens/AutoRun/RunReview.tsx src/screens/AutoRun/RunReview.test.tsx src/screens/AutoRun/index.tsx
git commit -q -F - <<'EOF'
fix(v2): a script step on a Shared Steps row is refused on save and its marks are never sent

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 5: Same-titled drafts pair by identity

**Files:**
- Modify: `src-tauri/src/model.rs:112-121` (`DraftEdit`)
- Modify: `src-tauri/src/import_parser/export.rs:316-377` (`apply_draft_edits`, `claim`)
- Modify: `src/lib/fileSync.ts:112-186` (`fileOwnedKeys`), `:399-418` (`ownerPaths`)
- Modify: `src/lib/queueStamp.ts:40-74` (`stampFileSlices`)
- Modify: `src/lib/uploadHold.ts` (type, `parseHold`, `holdFromResults`, `rowsNamedBy`/`heldRows`/`ambiguousRows`, new `holdSignature`, `narrowHold`)
- Modify: `src/components/QueueSection.tsx:23`, `:27-36` (imports), `:910-920` (Check), `:1060` (stamp), `:1199-1215` (write-back), `:1319-1333` (remove)
- Test: `src-tauri/tests/draft_merge.rs`, `src/lib/fileSync.test.ts`, `src/lib/uploadHold.test.ts`, `src/components/QueueSection.test.tsx`
- Generated: `src/bindings.ts`

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces:
  - `DraftEdit { before: TestCase, after: Option<TestCase>, occurrence: Option<u32> }`. In TS, `occurrence?: number | null`.
  - `export type FileOwner = { path: string; occurrence: number | null }` and `export function fileOwners(queue: TestCase[], watches: WatchedFile[]): FileOwner[]` in `fileSync.ts`
  - `stampFileSlices(prev, owners: string[], sent, results, occurrences: (number | null)[] = [])`
  - `UploadHold.sigs?: string[]` (aligned with `titles`), `export function holdSignature(tc: TestCase): string`, `export function narrowHold(h: UploadHold, ambiguous: string[], claimed: number[]): UploadHold | null`

- [ ] **Step 1: Write the failing tests.**

In `src-tauri/tests/draft_merge.rs`, add `use v2_lib::steps_xml::Step;` and change the helper to:

```rust
fn edit(before: TestCase, after: Option<TestCase>) -> DraftEdit {
    DraftEdit { before, after, occurrence: None }
}
```

Then append:

```rust
/// After a re-sort, the queue's first "X" is the file's SECOND entry. The
/// app says so (`occurrence`), and each write lands there: every entry keeps
/// its own extra keys instead of trading them with its twin.
#[test]
fn a_named_occurrence_wins_over_queue_order_so_extra_keys_stay_with_their_case() {
    let old = json!({ "test_cases": [
        { "title": "X", "author": "first", "steps": [{ "action": "A.", "expected": "" }] },
        { "title": "X", "author": "second", "steps": [{ "action": "B.", "expected": "" }] }
    ]})
    .to_string();
    let parsed = parse_json_text(&old).unwrap().cases;
    let row = |i: usize| TestCase { source: Default::default(), ..parsed[i].clone() };
    let b_edited = TestCase {
        steps: vec![Step { action: "B, edited.".into(), expected: String::new(), shared: None }],
        ..row(1)
    };
    let out = apply_draft_edits(
        &old,
        &[
            DraftEdit { before: row(1), after: Some(b_edited), occurrence: Some(2) },
            DraftEdit { before: row(0), after: Some(row(0)), occurrence: Some(1) },
        ],
    )
    .unwrap();
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["test_cases"][0]["author"], "first", "{out}");
    assert_eq!(doc["test_cases"][0]["steps"][0]["action"], "A.", "{out}");
    assert_eq!(doc["test_cases"][1]["author"], "second", "{out}");
    assert_eq!(doc["test_cases"][1]["steps"][0]["action"], "B, edited.", "{out}");
}

/// Titles are compared the way the app compares them: all of Unicode
/// lowercased, not ASCII only.
#[test]
fn a_title_differing_only_in_non_ascii_case_is_the_same_entry() {
    let old = json!({ "test_cases": [
        { "title": "ÜBERSICHT", "author": "a", "steps": [{ "action": "A.", "expected": "" }] }
    ]})
    .to_string();
    let parsed = parse_json_text(&old).unwrap().cases;
    let before = TestCase { title: "übersicht".into(), source: Default::default(), ..parsed[0].clone() };
    let after = TestCase { update_id: Some(3), ..before.clone() };
    let out = apply_draft_edits(&old, &[edit(before, Some(after))]).unwrap();
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["test_cases"].as_array().unwrap().len(), 1, "{out}");
    assert_eq!(doc["test_cases"][0]["author"], "a", "{out}");
}

/// Review Focus 3: the file lost the entry the app named (an assistant
/// deleted it since the snapshot). The row falls back to the first
/// unclaimed same-titled entry, and nothing is appended twice.
#[test]
fn an_occurrence_the_file_no_longer_has_falls_back_to_the_first_unclaimed_entry() {
    let old = json!({ "test_cases": [
        { "title": "X", "author": "only", "steps": [{ "action": "A.", "expected": "" }] }
    ]})
    .to_string();
    let parsed = parse_json_text(&old).unwrap().cases;
    let row = TestCase { source: Default::default(), ..parsed[0].clone() };
    let stamped = TestCase { update_id: Some(9), ..row.clone() };
    let out =
        apply_draft_edits(&old, &[DraftEdit { before: row, after: Some(stamped), occurrence: Some(2) }]).unwrap();
    let doc: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["test_cases"].as_array().unwrap().len(), 1, "{out}");
    assert_eq!(doc["test_cases"][0]["id"], 9, "{out}");
    assert_eq!(doc["test_cases"][0]["author"], "only", "{out}");
}
```

In `src/lib/fileSync.test.ts`, add `fileOwners` to the import from `./fileSync`. Then append:

```ts
/// The write-back tells Rust which same-titled file entry each row is -
/// the app's own pairing (exact rows first), not the queue's order.
test("a row's file occurrence follows the app's pairing, not queue order", () => {
  const first = tc("X", { steps: [{ action: "A.", expected: "" }] });
  const second = tc("X", { steps: [{ action: "B.", expected: "" }] });
  const w = { path: "C:/d/x.json", stamp: "s", snapshot: [first, second] };
  // Re-sorted: the second entry's row sits first in the queue.
  expect(fileOwners([second, first], [w])).toEqual([
    { path: "C:/d/x.json", occurrence: 2 },
    { path: "C:/d/x.json", occurrence: 1 },
  ]);
  // A hand-typed row belongs to no file.
  expect(fileOwners([tc("Typed")], [w])).toEqual([{ path: "", occurrence: null }]);
});
```

In `src/lib/uploadHold.test.ts`, add `holdSignature` and `narrowHold` to the import from `./uploadHold`. Change the two `holdFromResults(...)` expectations to include the signature:
- line 37-41: `.toEqual({ since: "S", titles: ["B"], ids: [901], sigs: [holdSignature(sent[1])] })`
- line 125: `.toEqual({ since: "S", titles: ["B"], ids: [50, 51, 77, 901], sigs: [holdSignature(sent[1])] })`

Then append:

```ts
/// Two drafts share a title. Only the second came back unknown, so the
/// hold must mark THAT row - first-come used to mark the first - and keep
/// marking it after a re-sort.
test("a hold marks the same-titled row that was sent, not merely the first one", () => {
  const a = tc("Login works", { steps: [{ action: "Open A", expected: "" }] });
  const b = tc("Login works", { steps: [{ action: "Open B", expected: "" }] });
  const hold = holdFromResults([res(0, "failed"), res(1, "unknown")], [a, b], "S")!;
  expect(heldRows([a, b], hold)).toEqual([false, true]);
  expect(heldRows([b, a], hold)).toEqual([true, false]);
  expect(ambiguousRows([a, b], { ...hold, ambiguous: ["Login works"] })).toEqual([false, true]);
});

/// Review Focus 2.
test("a hold stored before signatures existed still marks its rows by title", () => {
  localStorage.setItem("tcm-v2-upload-hold:acme/42", JSON.stringify({ since: "S", titles: ["B"] }));
  const hold = loadHold("acme", 42);
  expect(hold?.sigs).toBeUndefined();
  expect(heldRows([tc("A"), tc("B"), tc("B")], hold)).toEqual([false, true, false]);
});

test("a Check keeps each still-held row's own signature", () => {
  const h = { since: "S", titles: ["A", "B", "B"], ids: [1], sigs: ["sa", "sb1", "sb2"] };
  expect(narrowHold(h, ["B"], [7])).toEqual({
    since: "S",
    titles: ["B", "B"],
    ambiguous: ["B", "B"],
    ids: [1, 7],
    sigs: ["sb1", "sb2"],
  });
  expect(narrowHold(h, [], [7])).toBeNull();
});
```

In `src/components/QueueSection.test.tsx`, append:

```tsx
/// A double-click on Remove: the second click lands before the first has
/// re-rendered, still carrying the same index - which by then names the
/// NEXT row. Only the row that was clicked goes.
test("a double-click on Remove removes that one row, not the next one too", async () => {
  baseMocks();
  renderQueue([makeCase({ title: "Alpha case" }), makeCase({ title: "Beta case" })]);
  const remove = (await screen.findAllByRole("button", { name: "Remove" }))[0];
  act(() => {
    fireEvent.click(remove);
    fireEvent.click(remove);
  });
  expect(screen.queryByText("Alpha case")).not.toBeInTheDocument();
  expect(screen.getByText("Beta case")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run to see them fail.** Do the dev-app check first, then from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test draft_merge`. Expected: compile error (`DraftEdit` has no field `occurrence`). Then `npx vitest run --exclude "**/.claude/**" src/lib/fileSync.test.ts src/lib/uploadHold.test.ts src/components/QueueSection.test.tsx`. Expected: `fileOwners`, `holdSignature` and `narrowHold` do not exist, and the double-click test finds "Beta case" gone.

- [ ] **Step 3: Implement.**

In `src-tauri/src/model.rs`, replace `DraftEdit` (lines 112-121) with:

```rust
/// One queue row's part in a draft write-back (`save_draft_cases`): the row
/// as it was BEFORE the edit (how the file finds its own copy - a rename
/// changes the title), what it is now (`None` when the edit removed it), and
/// which of the file's id-less entries with that title the app paired the
/// row with (`occurrence`, 1 = the first in the file). The app pairs exact
/// rows first (fileSync.ts), so after a re-sort the Nth row with a title is
/// not always the Nth entry. Without `occurrence` a row claims the first
/// unclaimed same-titled entry, in queue order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DraftEdit {
    pub before: TestCase,
    pub after: Option<TestCase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<u32>,
}
```

In `src-tauri/src/import_parser/export.rs`, replace lines 316-377 (the doc comment of `apply_draft_edits` through the end of `claim`) with:

```rust
/// The write-back behind `save_draft_cases`. `edits` are the owned queue
/// rows IN QUEUE ORDER: each one's pre-edit row (`before`), what it is now
/// (`after`, `None` = removed), and, when the app knows it, which same-titled
/// entry it is (`occurrence`). Each `before` finds its entry in the file by
/// work item id, then by `occurrence` among the id-less entries with its
/// title, then by title alone: the first unclaimed one in file order. Rows
/// that carry an `occurrence` claim first, so a row the app did not pair
/// can never take an entry it did. Found entries are patched or removed.
/// File entries no row mentions are kept (cases an assistant added since
/// the last sync). A row whose entry is gone from the file is appended.
///
/// An UNCHANGED row (`after == before`: every modelled field, steps with
/// their Shared Steps references and the work item id included) still
/// claims its entry, so the rows after it keep their positions, but leaves
/// it exactly as the file has it now - and is not re-added when its entry
/// is gone. Every owned row is sent, and for the post-upload stamp its
/// before/after can be minutes old: replaying them reverted whatever an
/// assistant changed in the file during the upload.
pub fn apply_draft_edits(old_text: &str, edits: &[DraftEdit]) -> Result<String, String> {
    let old_text = super::strip_bom(old_text);
    let parsed = super::parse_json_text(old_text).map(|p| p.cases).unwrap_or_default();
    let mut claimed = vec![false; parsed.len()];
    let mut slot: Vec<Option<usize>> = vec![None; edits.len()];
    for (e, edit) in edits.iter().enumerate() {
        if edit.occurrence.is_some() {
            slot[e] = claim_named(&parsed, &mut claimed, &edit.before, edit.occurrence);
        }
    }
    for (e, edit) in edits.iter().enumerate() {
        if slot[e].is_none() {
            slot[e] = claim(&parsed, &mut claimed, &edit.before);
        }
    }
    // None = untouched; Some(None) = removed; Some(Some(case)) = edited.
    let mut fate: Vec<Option<Option<&TestCase>>> = vec![None; parsed.len()];
    let mut unmatched: Vec<TestCase> = vec![];
    for (e, edit) in edits.iter().enumerate() {
        let unchanged = edit.after.as_ref() == Some(&edit.before);
        match (slot[e], &edit.after) {
            // Claimed (above) but left alone / not brought back.
            _ if unchanged => {}
            (Some(k), after) => fate[k] = Some(after.as_ref()),
            (None, Some(after)) => {
                unmatched.push(TestCase { source: SourceIndex(None), ..after.clone() })
            }
            (None, None) => {} // removed, and already gone from the file
        }
    }
    let mut out: Vec<TestCase> = vec![];
    for (k, p) in parsed.iter().enumerate() {
        match fate[k] {
            None => out.push(p.clone()),
            Some(None) => {}
            Some(Some(after)) => out.push(TestCase { source: p.source, ..after.clone() }),
        }
    }
    out.extend(unmatched);
    merge_cases_into_draft(old_text, &out)
}

/// Titles as the app compares them (`caseKey` in fileSync.ts): trimmed, and
/// lowercased across all of Unicode, not ASCII only.
fn same_title(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// The first unclaimed entry for `want`: by work item id when it has one,
/// otherwise by title among the id-less entries.
fn claim(parsed: &[TestCase], claimed: &mut [bool], want: &TestCase) -> Option<usize> {
    let hit = (0..parsed.len()).find(|&k| {
        !claimed[k]
            && match want.update_id {
                Some(id) => parsed[k].update_id == Some(id),
                None => parsed[k].update_id.is_none() && same_title(&parsed[k].title, &want.title),
            }
    })?;
    claimed[hit] = true;
    Some(hit)
}

/// The entry the app named: by id when the row has one and the file has it,
/// else the `occurrence`-th id-less entry with the row's title, if that one
/// is still unclaimed. `None` sends the row to `claim`'s plain rule.
fn claim_named(
    parsed: &[TestCase],
    claimed: &mut [bool],
    want: &TestCase,
    occurrence: Option<u32>,
) -> Option<usize> {
    if want.update_id.is_some() {
        if let Some(k) = claim(parsed, claimed, want) {
            return Some(k);
        }
    }
    let n = occurrence.filter(|n| *n >= 1)? as usize;
    let k = (0..parsed.len())
        .filter(|&k| parsed[k].update_id.is_none() && same_title(&parsed[k].title, &want.title))
        .nth(n - 1)?;
    if claimed[k] {
        return None;
    }
    claimed[k] = true;
    Some(k)
}
```

Regenerate the bindings (dev-app check first): from `src-tauri/`, `CARGO_TARGET_DIR=target/gate cargo test --test bindings`.

In `src/lib/fileSync.ts`, replace `fileOwnedKeys` (lines 128-186, keeping its doc comment on lines 112-127 above it) with:

```ts
export function fileOwnedKeys(
  queue: TestCase[],
  snapshot: TestCase[],
  taken?: boolean[],
): (string | null)[] {
  return claimRows(queue, snapshot, taken).map((c) => c?.key ?? null);
}

/** One row's claim on a snapshot entry: the ownership key, and which entry
 * (`slot`, its index in the snapshot). */
type Claim = { key: string; slot: number };

/** `fileOwnedKeys`' pairing, keeping the snapshot index each row claimed. */
function claimRows(queue: TestCase[], snapshot: TestCase[], taken?: boolean[]): (Claim | null)[] {
  const snapKeys = keysFor(snapshot);
  const open = new Map<string, { key: string; c: TestCase; slot: number }[]>();
  snapshot.forEach((c, i) => {
    const base = caseKey(c);
    const entry = { key: snapKeys[i], c, slot: i };
    const list = open.get(base);
    if (list) list.push(entry);
    else open.set(base, [entry]);
  });
  const out: (Claim | null)[] = queue.map(() => null);
  const free = (i: number) => out[i] == null && !taken?.[i];
  // Exact matches first: an untouched file case is its own best evidence.
  queue.forEach((c, i) => {
    if (!free(i)) return;
    const list = open.get(caseKey(c));
    if (!list) return;
    const j = list.findIndex((o) => sameCase(o.c, c));
    if (j === -1) return;
    out[i] = { key: list[j].key, slot: list[j].slot };
    list.splice(j, 1);
  });
  // An id-stamped row whose id isn't in the snapshot yet (the post-upload
  // write-back racing a stale snapshot) claims the file's matching entry
  // next, before an unrelated id-less row gets to just by coming earlier in
  // the queue - an id is exact evidence once it exists, and outranks a
  // title match even one that hasn't caught up yet.
  //
  // Still an exact match, not a title guess: two files can share a title
  // while disagreeing on everything else, and the id-less snapshot has no
  // id of its own to rule the wrong one out by. So this only claims a
  // snapshot entry that is the same case in every field EXCEPT the id -
  // the same `sameCase` pass 1 uses, just with the id set aside first. A
  // row that was also edited since the snapshot stays unowned, same as
  // before this pass existed.
  queue.forEach((c, i) => {
    if (!free(i)) return;
    if (c.update_id == null) return;
    const idKey = caseKey(c);
    if (open.has(idKey)) return; // the id IS in the snapshot; the next pass handles it
    const withoutId = { ...c, update_id: null };
    const list = open.get(caseKey(withoutId));
    if (!list) return;
    const j = list.findIndex((o) => sameCase(o.c, withoutId));
    if (j === -1) return;
    out[i] = { key: idKey, slot: list[j].slot };
    list.splice(j, 1);
  });
  // Then rows edited in the app, in order.
  queue.forEach((c, i) => {
    if (!free(i)) return;
    const claim = open.get(caseKey(c))?.shift();
    if (claim) out[i] = { key: claim.key, slot: claim.slot };
  });
  return out;
}
```

and replace `ownerPaths` (lines 399-418, doc comment included) with:

```ts
/** Which file each queued case came from, and which of that file's id-less
 * entries with its title it is (1 = the first in the file), aligned with
 * `queue`. Path "" and occurrence null for a case typed by hand.
 *
 * A comment or an edit is written back into the file that put the case
 * there, so this decides where it lands. Files claim rows in the order they
 * were imported, so when two files hold the same title the first keeps the
 * row both match, and the second claims the next same-titled row if there
 * is one - the same claiming `withoutFileCases` and `syncFromFile` use. The
 * occurrence travels with the write (`DraftEdit.occurrence`), so Rust writes
 * to the entry this pairing chose, not to whichever comes first in queue
 * order. */
export type FileOwner = { path: string; occurrence: number | null };

export function fileOwners(queue: TestCase[], watches: WatchedFile[]): FileOwner[] {
  const out: FileOwner[] = queue.map(() => ({ path: "", occurrence: null }));
  const taken = queue.map(() => false);
  for (const w of watches) {
    claimRows(queue, w.snapshot, taken).forEach((c, i) => {
      if (c == null) return;
      out[i] = { path: w.path, occurrence: occurrenceIn(w.snapshot, c.slot) };
      taken[i] = true;
    });
  }
  return out;
}

/** 1-based: which of the snapshot's id-less entries titled like `slot`'s
 * this one is, compared the way Rust compares them (trimmed, lowercased).
 * Null for an entry that has an id - Rust finds that one by its id. */
function occurrenceIn(snapshot: TestCase[], slot: number): number | null {
  const at = snapshot[slot];
  if (!at || at.update_id != null) return null;
  const want = at.title.trim().toLowerCase();
  let n = 0;
  for (let k = 0; k <= slot; k++) {
    if (snapshot[k].update_id == null && snapshot[k].title.trim().toLowerCase() === want) n += 1;
  }
  return n;
}

/** The file each queued case came from, aligned with `queue`; empty for a
 * case that was typed by hand and belongs to no file. */
export function ownerPaths(queue: TestCase[], watches: WatchedFile[]): string[] {
  return fileOwners(queue, watches).map((o) => o.path);
}
```

In `src/lib/queueStamp.ts`, change `stampFileSlices`' signature and loop (lines 40-74) to:

```ts
export function stampFileSlices(
  prev: TestCase[],
  owners: string[],
  sent: TestCase[],
  results: StampOutcome[],
  /** Per queue index, which same-titled entry of its file the row is
   * (`fileOwners`); written with each edit so Rust pairs as the app did. */
  occurrences: (number | null)[] = [],
): Map<string, StampedFile> {
  // Post-submit form per queue index, only for cases that succeeded.
  const post = new Map<number, TestCase>();
  for (const r of results) {
    const sentCase = sent[r.index];
    if (!sentCase) continue;
    const qi = prev.indexOf(sentCase);
    if (qi < 0) continue;
    if (r.action === "created" && r.id != null) {
      post.set(qi, { ...sentCase, update_id: r.id });
    } else if (r.action === "updated") {
      post.set(qi, sentCase);
    }
    // "failed": the file keeps the case exactly as it was - still a draft,
    // still ready to retry.
  }

  const files = new Map<string, StampedFile>();
  prev.forEach((tc, i) => {
    const path = owners[i];
    if (!path) return;
    const f = files.get(path) ?? { slice: [], edits: [], changed: false };
    const after = post.get(i) ?? tc;
    f.slice.push(after);
    f.edits.push({ before: tc, after, occurrence: occurrences[i] ?? null });
    if (post.has(i)) f.changed = true;
    files.set(path, f);
  });
  return files;
}
```

In `src/lib/uploadHold.ts`:

1. Add to the `UploadHold` type, after `ids?`:

```ts
  /** Each held row's content (`holdSignature`), aligned with `titles`. The
   * row that was sent is marked by its content first, so of two drafts
   * sharing a title it is the one that was actually held, wherever the
   * queue has moved it. Absent on a hold stored before this field existed,
   * which then marks by title alone, as it always did. */
  sigs?: string[];
```

2. Replace `parseHold` (lines 83-105) with:

```ts
function parseHold(raw: string): UploadHold | null {
  try {
    const v = JSON.parse(raw);
    if (
      v &&
      typeof v.since === "string" &&
      Array.isArray(v.titles) &&
      v.titles.length > 0 &&
      v.titles.every((t: unknown) => typeof t === "string") &&
      (v.ambiguous === undefined ||
        (Array.isArray(v.ambiguous) && v.ambiguous.every((t: unknown) => typeof t === "string"))) &&
      (v.ids === undefined || (Array.isArray(v.ids) && v.ids.every((n: unknown) => Number.isInteger(n))))
    ) {
      const ids: number[] = v.ids ?? [];
      // Signatures that do not line up with the titles are dropped, not
      // trusted: the hold then marks by title, which is never worse than
      // before signatures existed.
      const sigsOk =
        Array.isArray(v.sigs) &&
        v.sigs.length === v.titles.length &&
        v.sigs.every((s: unknown) => typeof s === "string");
      const hold: UploadHold =
        v.ambiguous !== undefined
          ? { since: v.since, titles: v.titles, ambiguous: v.ambiguous, ids }
          : { since: v.since, titles: v.titles, ids };
      return sigsOk ? { ...hold, sigs: v.sigs } : hold;
    }
  } catch {
    // not a hold
  }
  return null;
}
```

3. Replace `holdFromResults` (lines 132-151, doc comment included) with:

```ts
/** A held row's content, as the hold remembers it: title, steps (Shared
 * Steps references included), preconditions, module and tags. */
export function holdSignature(tc: TestCase): string {
  return JSON.stringify([
    tc.title.trim(),
    tc.steps.map((s) => [s.action, s.expected, s.shared ?? null]),
    tc.preconditions,
    tc.module_value,
    tc.tags,
  ]);
}

/** The hold a finished submit leaves: every "unknown" result, named by the
 * title and the content of the row that was SENT at its index. Null when
 * there are none. `preExisting` is the ids of the PBI's test cases linked
 * before the upload began; they and every id a result reported go into
 * `ids`. */
export function holdFromResults(
  results: Pick<SubmitItemResult, "index" | "action" | "id">[],
  sent: TestCase[],
  since: string,
  preExisting: number[] = [],
): UploadHold | null {
  const held = results
    .filter((r) => r.action === "unknown")
    .map((r) => sent[r.index])
    .filter((c): c is TestCase => c != null);
  if (held.length === 0) return null;
  const reported = results
    .filter((r) => (r.action === "created" || r.action === "updated") && r.id != null)
    .map((r) => r.id as number);
  return {
    since,
    titles: held.map((c) => c.title),
    ids: [...new Set([...preExisting, ...reported])],
    sigs: held.map(holdSignature),
  };
}
```

4. Replace `rowsNamedBy`, `heldRows` and `ambiguousRows` (lines 161-191) with:

```ts
function countOf(list: string[]): Map<string, number> {
  const out = new Map<string, number>();
  for (const k of list) out.set(k, (out.get(k) ?? 0) + 1);
  return out;
}

function take(left: Map<string, number>, k: string): boolean {
  const n = left.get(k) ?? 0;
  if (n === 0) return false;
  left.set(k, n - 1);
  return true;
}

/** Which queue rows are held: create rows only, as many per title as the
 * hold names. The rows whose content is exactly what was sent are marked
 * first - of two drafts sharing a title, the one that was held, wherever
 * it now sits. Then first come first marked by title, for a held row
 * edited since and for a hold stored before signatures existed. The same
 * order the file sync pairs rows in (fileSync.fileOwnedKeys). */
export function heldRows(queue: TestCase[], hold: UploadHold | null): boolean[] {
  if (!hold) return queue.map(() => false);
  const titlesLeft = countOf(hold.titles.map((t) => t.trim()));
  const sigsLeft = countOf(hold.sigs && hold.sigs.length === hold.titles.length ? hold.sigs : []);
  const out = queue.map(() => false);
  queue.forEach((tc, i) => {
    if (tc.update_id != null || sigsLeft.size === 0) return;
    const sig = holdSignature(tc);
    const title = tc.title.trim();
    if ((sigsLeft.get(sig) ?? 0) === 0 || (titlesLeft.get(title) ?? 0) === 0) return;
    take(sigsLeft, sig);
    take(titlesLeft, title);
    out[i] = true;
  });
  queue.forEach((tc, i) => {
    if (out[i] || tc.update_id != null) return;
    if (take(titlesLeft, tc.title.trim())) out[i] = true;
  });
  return out;
}

/** Which of the held rows are held because their title is ambiguous in
 * Azure DevOps (more matches there than rows checked), not merely unknown -
 * so the row should say that instead of the generic "check before
 * uploading again". Always a subset of `heldRows`. */
export function ambiguousRows(queue: TestCase[], hold: UploadHold | null): boolean[] {
  if (!hold || !hold.ambiguous || hold.ambiguous.length === 0) return queue.map(() => false);
  const held = heldRows(queue, hold);
  const left = countOf(hold.ambiguous.map((t) => t.trim()));
  return queue.map((tc, i) => held[i] && take(left, tc.title.trim()));
}

/** What is left of a hold after a Check: the rows whose title the answer
 * still calls ambiguous, each with its own signature, and the hold's ids
 * plus what this Check claimed. Null when nothing is left. */
export function narrowHold(h: UploadHold, ambiguous: string[], claimed: number[]): UploadHold | null {
  const still = new Set(ambiguous.map((t) => t.trim()));
  const keep = h.titles.map((_, i) => i).filter((i) => still.has(h.titles[i].trim()));
  if (keep.length === 0) return null;
  const titles = keep.map((i) => h.titles[i]);
  const ids = [...new Set([...(h.ids ?? []), ...claimed])];
  const sigs = h.sigs && h.sigs.length === h.titles.length ? keep.map((i) => h.sigs![i]) : undefined;
  const narrowed: UploadHold = { since: h.since, titles, ambiguous: titles, ids };
  return sigs ? { ...narrowed, sigs } : narrowed;
}
```

In `src/components/QueueSection.tsx`:

1. Line 23: add `fileOwners` to the `../lib/fileSync` import.
2. Lines 27-36: add `narrowHold` to the `../lib/uploadHold` import.
3. In `checkHold`, replace lines 912-920 (from `const ambiguousSet = ...` through the `saveHold(...)` call) with:

```ts
      // A smaller hold keeps its ids, plus what this Check just claimed,
      // and each still-held row keeps its own content signature.
      const next = narrowHold(h, ambiguous, found.map((f) => f.id));
      const stillHeld = next?.titles ?? [];
      saveHold(org, pbiId, next);
```

4. Line 1060 becomes:

```ts
        const owned = fileOwners(prevQueue, known);
        const files = stampFileSlices(
          prevQueue,
          owned.map((o) => o.path),
          sent,
          outcomes,
          owned.map((o) => o.occurrence),
        );
```

5. In `writeBackOwned`, replace lines 1199-1215 (from `const owners = ownerPaths(prev, watches);` through the end of `prev.forEach(...)`) with:

```ts
    const owners = fileOwners(prev, watches);
    // Per file: one edit per owned row IN QUEUE ORDER - the row BEFORE the
    // edit (how the file finds its own copy - a rename changes the title),
    // after it (null = removed), and which same-titled entry of the file
    // the app paired it with, so Rust writes where the app thinks it does.
    // The file keeps everything else it holds.
    const files = new Map<string, { slice: TestCase[]; edits: DraftEdit[]; touched: boolean }>();
    prev.forEach((before, i) => {
      const { path: p, occurrence } = owners[i];
      if (!p) return;
      const f = files.get(p) ?? { slice: [], edits: [], touched: false };
      const after = next[i];
      if (after) f.slice.push(after);
      f.edits.push({ before, after, occurrence });
      if (changed.has(i)) f.touched = true;
      files.set(p, f);
    });
```

6. Replace `removeRow` (lines 1319-1333, comment included) with:

```tsx
  // The owning FILE follows a single removal exactly as it follows a bulk
  // one: otherwise the next outside save of that file sees the case in
  // both snapshots, not in the queue, and puts it back.
  //
  // By identity, not position: a second click that lands before the first
  // has re-rendered still carries index i, which by then names the NEXT
  // row. The row object is the identity; a row already on its way out is
  // not removed twice.
  const removing = useRef(new WeakSet<TestCase>());
  const removeRow = useCallback(
    (i: number) => {
      const { queue: prev, writeBackOwned: writeBack } = latest.current;
      const target = prev[i];
      if (!target || removing.current.has(target)) return;
      removing.current.add(target);
      setQueue((q) => q.filter((t) => t !== target));
      void writeBack(
        prev,
        prev.map((t) => (t === target ? null : t)),
        new Set([i]),
      );
    },
    [setQueue],
  );
```

- [ ] **Step 4: Run** (one at a time, dev-app check before each cargo command), from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test draft_merge`, then `--test draft_comments`, then `--test ai_bridge`. Then `npx vitest run --exclude "**/.claude/**" src/lib/fileSync.test.ts src/lib/queueStamp.test.ts src/lib/uploadHold.test.ts src/lib/queueUploaded.test.ts src/components/QueueSection.test.tsx`. Then `npx vitest run --exclude "**/.claude/**" src/App.test.tsx`. App.test is slow and has one documented load flake: one failure that passes on a re-run is that; two different ones are not. Then `npx tsc --noEmit`. All green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/model.rs src-tauri/src/import_parser/export.rs src-tauri/tests/draft_merge.rs src/bindings.ts src/lib/fileSync.ts src/lib/fileSync.test.ts src/lib/queueStamp.ts src/lib/uploadHold.ts src/lib/uploadHold.test.ts src/components/QueueSection.tsx src/components/QueueSection.test.tsx
git commit -q -F - <<'EOF'
fix(v2): same-titled drafts pair by identity in write-backs, holds and removals

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 6: Steps XML merge and draft-file writes

**Files:**
- Modify: `src-tauri/src/steps_xml.rs:122-132` (`Doc`), `:157-211` (`tokenize`), `:256-266` and `:372-374` (`merge_into`)
- Modify: `src-tauri/src/commands/queue.rs:272-296` (`draft_write_allowed`)
- Modify: `src-tauri/src/ai_bridge.rs:118-124` (`bridge_may_write`)
- Test: `src-tauri/tests/steps_xml.rs`, `src-tauri/tests/draft_comments.rs`, `src-tauri/tests/ai_bridge.rs` (append)

**Interfaces:**
- Consumes: `workspace::is_inside(dir: &Path, path: &Path) -> bool`, `workspace::cases_dir(root: &Path) -> PathBuf`, `import_parser::TITLE_KEYS` (exist).
- Produces: no new public names. `merge_steps_xml` and `draft_write_allowed` keep their signatures.

- [ ] **Step 1: Write the failing tests.**

Append to `src-tauri/tests/steps_xml.rs`:

```rust
/// A prolog or a comment around the root belongs to the original, and a
/// merge keeps it where it was.
#[test]
fn a_merge_keeps_what_sits_outside_the_root() {
    let xml = concat!(
        "<?xml version=\"1.0\"?><!-- kept -->",
        "<steps id=\"0\" last=\"2\"><step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Open</parameterizedString><parameterizedString isformatted=\"true\"></parameterizedString></step></steps>",
        "<!-- after -->"
    );
    let merged = merge(xml, &[step("Open the page", "")]);
    assert!(merged.starts_with("<?xml version=\"1.0\"?><!-- kept --><steps "), "{merged}");
    assert!(merged.ends_with("</steps><!-- after -->"), "{merged}");
    assert!(merged.contains("Open the page"), "{merged}");
    assert!(merged.contains("<step id=\"2\""), "the edited step keeps its id: {merged}");
}

/// Only a `<steps>` document is edited in place. Anything else used to be
/// closed with `</steps>` and stop being XML; it is rebuilt instead.
#[test]
fn a_root_that_is_not_steps_is_rebuilt_not_closed_as_steps() {
    let xml = "<list id=\"0\" last=\"2\"><step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\">Open</parameterizedString><parameterizedString isformatted=\"true\"></parameterizedString></step></list>";
    let steps = vec![step("Open the page", "")];
    assert_eq!(merge(xml, &steps), build_steps_xml(&steps));
}
```

Append to `src-tauri/tests/draft_comments.rs`:

```rust
/// A bare array is a draft only when it holds case objects with a title.
/// Any other JSON array is somebody else's file.
#[test]
fn a_bare_array_is_a_draft_only_when_it_holds_titled_cases() {
    use v2_lib::commands::queue::draft_write_allowed;
    let cases = tmp("bare-cases.json");
    let numbers = tmp("bare-numbers.json");
    let empty = tmp("bare-empty.json");
    let settings = tmp("bare-settings.json");
    std::fs::write(&cases, r#"[{"title":"Login works","steps":[]},{"steps":[]}]"#).unwrap();
    std::fs::write(&numbers, "[1, 2, 3]").unwrap();
    std::fs::write(&empty, "[]").unwrap();
    std::fs::write(&settings, r#"[{"theme":"dark"}]"#).unwrap();
    assert!(draft_write_allowed(&cases, &[]).is_ok(), "a half-written entry beside a titled one is still a draft");
    for p in [&numbers, &empty, &settings] {
        assert!(draft_write_allowed(p, &[]).is_err(), "{p}");
    }
    for p in [cases, numbers, empty, settings] {
        let _ = std::fs::remove_file(p);
    }
}
```

In `src-tauri/tests/ai_bridge.rs`, inside `bridge_writes_only_under_test_cases_or_to_a_watched_file`, add before its closing `}`:

```rust
    // A sibling that only shares the prefix is outside the folder.
    std::fs::create_dir_all(dir.0.join(".test-cases-extra")).unwrap();
    let sibling = dir.0.join(".test-cases-extra").join("c.json");
    std::fs::write(&sibling, "{}").unwrap();
    assert!(!bridge_may_write(&sibling.to_string_lossy(), Some(&root), &[]));
```

- [ ] **Step 2: Run to see them fail.** Do the dev-app check first, then from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test steps_xml`. Expected: both new tests FAIL (the prolog and trailer are dropped; `</list>` comes back as `</steps>`). Then `--test draft_comments`. Expected: FAIL, because `[1, 2, 3]` is accepted. Then `--test ai_bridge bridge_writes_only`. Expected: PASS already (`Path::starts_with` compares components). It pins the behaviour the refactor must keep.

- [ ] **Step 3: Implement.**

In `src-tauri/src/steps_xml.rs`:

1. Replace `struct Doc` (lines 122-132) with:

```rust
/// A Steps document, tokenised for editing in place.
struct Doc {
    /// Byte range of the root's opening tag.
    root_open: (usize, usize),
    /// Whether the root element is `<steps>`. Only then is it edited in
    /// place: the writer closes the document with `</steps>`.
    root_is_steps: bool,
    /// Where the root's closing tag starts; `None` for a self-closing root.
    root_close: Option<usize>,
    /// The root's `last` attribute (0 when absent or unreadable).
    last: i32,
    /// The highest step/compref id anywhere, nested ones included - they
    /// occupy the same id space.
    max_id: i32,
    nodes: Vec<Node>,
}
```

2. In `tokenize`, declare `let mut root_is_steps = false;` and `let mut root_close = None;` beside `let mut root_open = None;`. In the `Event::Start` arm, change the `if depth == 1 { ... }` block to:

```rust
                if depth == 1 {
                    root_open = Some((before, after));
                    root_is_steps = e.name().as_ref() == b"steps";
                    last = attr(&e, b"last").trim().parse().unwrap_or(0);
                }
```

In the `Event::Empty` arm, change `if depth == 0 { root_open = Some((before, after)); }` to:

```rust
                if depth == 0 {
                    root_open = Some((before, after));
                    root_is_steps = e.name().as_ref() == b"steps";
                }
```

In the `Event::End(_)` arm, add at its start (before `if depth == 2`):

```rust
                if depth == 1 {
                    root_close = Some(before);
                }
```

and change the final line to `Some(Doc { root_open: root_open?, root_is_steps, root_close, last, max_id, nodes })`.

3. In `merge_into`, right after `let doc = tokenize(original)?;` add:

```rust
    // Only a `<steps>` document is edited in place. Anything else would be
    // closed with `</steps>` below and stop being XML at all; the caller
    // builds it from the steps instead.
    let close = doc.root_close.filter(|_| doc.root_is_steps)?;
```

and replace its last two lines (`let (root_start, root_end) = doc.root_open;` through `Some(format!("{root}{body}</steps>"))`) with:

```rust
    let (root_start, root_end) = doc.root_open;
    let root = with_attr(&original[root_start..root_end], "last", &next.to_string());
    // What sits outside the root - an XML prolog, a comment - is the
    // original's and stays where it was, before and after.
    Some(format!("{}{root}{body}{}", &original[..root_start], &original[close..]))
```

In `src-tauri/src/commands/queue.rs`, replace lines 291-296 (the `let text = ...` line through the closing `}` of `draft_write_allowed`) with:

```rust
    let text = std::fs::read_to_string(p).map_err(|e| format!("could not read the file: {e}"))?;
    match serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) {
        Ok(v) if is_draft_shape(&v) => Ok(()),
        _ => Err("that file is not a test case draft, so the app will not write to it".into()),
    }
}

/// What a draft file holds: a `test_cases` list, or a bare array of case
/// objects - every entry an object, and at least one titled the way the
/// importer reads a title. A bare `[1, 2]`, `[]` or a list of settings is
/// somebody else's JSON.
fn is_draft_shape(v: &serde_json::Value) -> bool {
    if v.get("test_cases").is_some_and(|t| t.is_array()) {
        return true;
    }
    let Some(items) = v.as_array() else {
        return false;
    };
    !items.is_empty()
        && items.iter().all(|c| c.is_object())
        && items.iter().any(|c| {
            crate::import_parser::TITLE_KEYS
                .iter()
                .any(|k| c.get(*k).and_then(|t| t.as_str()).is_some_and(|t| !t.trim().is_empty()))
        })
}
```

Also update the doc comment above `draft_write_allowed` (lines 276-278) so its last sentence reads: `existing .json file that already holds a draft (a test_cases list, or a bare array of titled case objects - see is_draft_shape). Anything else is not ours to overwrite.`

In `src-tauri/src/ai_bridge.rs`, replace lines 118-124 (from `let Some(root) = ...` to the end of the `match`) with:

```rust
    let Some(root) = working_dir.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    // The one containment rule (workspace::is_inside), the same one the
    // intake and the merge route apply to their output paths.
    crate::workspace::is_inside(&crate::workspace::cases_dir(std::path::Path::new(root)), &file)
```

- [ ] **Step 4: Run** (one at a time, dev-app check before each), from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test steps_xml`, `--test ado`, `--test submit_mapping`, `--test draft_comments`, `--test draft_merge`, `--test ai_bridge`, `--test workspace`. All green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/steps_xml.rs src-tauri/src/commands/queue.rs src-tauri/src/ai_bridge.rs src-tauri/tests/steps_xml.rs src-tauri/tests/draft_comments.rs src-tauri/tests/ai_bridge.rs
git commit -q -F - <<'EOF'
fix(v2): the steps merge keeps the document around its root, and only real drafts are written

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 7: Review page and sign-in

**Files:**
- Modify: `src-tauri/web/cases-page.js:359-391` (the poll)
- Modify: `src-tauri/web/cases-notes.js:45`, `:56-77` (the save)
- Modify: `src-tauri/web/cases-specs.js:152-157` (`isWordChar`)
- Modify: `src-tauri/src/auth.rs:129-136` (sentences), `:328-373` (`await_redirect`, plus `read_request_line`)
- Test: `src/lib/casesPage.test.ts`, `src/lib/casesNotes.test.ts`, `src/lib/casesSpecs.test.ts`, `src-tauri/tests/auth.rs`, `src-tauri/tests/ado_network.rs`

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `pub const SIGN_IN_NET_TIMEOUT`, `SIGN_IN_NET_UNREACHABLE`, `SIGN_IN_NET_GENERIC: &str` in `v2_lib::auth`. `window.tcmNotes.timeoutMs` (10000).

- [ ] **Step 1: Write the failing tests.**

Append to `src/lib/casesPage.test.ts`:

```ts
async function later(ms: number) {
  await vi.advanceTimersByTimeAsync(ms);
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

/// A closed app answers nothing, and every refused ask is an error line in
/// the browser's console. Each failure doubles the wait (to at most a
/// minute); the first answer brings the 4 s poll back.
test("a closed app is asked less and less often, and an answer brings the 4 s poll back", async () => {
  let up = false;
  const asked: string[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((url: string) => {
      asked.push(url);
      if (!up) return Promise.reject(new TypeError("Failed to fetch"));
      return Promise.resolve({ ok: true, json: () => Promise.resolve({ revision: 1 }) });
    }),
  );
  const versionAsks = () => asked.filter((u) => u.includes("/version")).length;

  await poll(); // 4 s: asked, refused - the next ask waits 8 s
  expect(versionAsks()).toBe(1);
  await poll(); // 8 s: not yet
  expect(versionAsks()).toBe(1);
  await poll(); // 12 s: asked, refused - the next waits 16 s
  expect(versionAsks()).toBe(2);
  await later(15_000); // 27 s: not yet
  expect(versionAsks()).toBe(2);

  up = true;
  await later(1_000); // 28 s: asked, answered
  expect(versionAsks()).toBe(3);
  await poll(); // 32 s: back to every 4 s
  expect(versionAsks()).toBe(4);
});
```

In `src/lib/casesNotes.test.ts`, change the `Notes` type to add `timeoutMs: number;`. Then append:

```ts
/// A save the app never answers must still end, or the box stays "busy"
/// for good and the page's live update waits on it forever.
test("a save the app never answers ends after the limit, frees the box and says so", async () => {
  vi.stubGlobal("fetch", vi.fn(() => new Promise(() => {})));
  (window as unknown as Wire).__tcmWireNotes();
  const box = document.querySelector("textarea") as HTMLTextAreaElement;
  box.value = "hi";
  box.dispatchEvent(new Event("input"));
  await vi.advanceTimersByTimeAsync(600);
  expect(N.busy()).toBe(1);

  await vi.advanceTimersByTimeAsync(N.timeoutMs);
  await flush();
  expect(N.busy()).toBe(0);
  expect(document.getElementById("st")!.textContent).toBe("Not saved - the app did not answer");
});

/// Review Focus 4: slow is not dead. A reply inside the limit is saved.
test("a save the app answers slowly, inside the limit, is still saved", async () => {
  const gate: { answer?: (v: unknown) => void } = {};
  vi.stubGlobal(
    "fetch",
    vi.fn(
      () =>
        new Promise((ok) => {
          gate.answer = ok;
        }),
    ),
  );
  (window as unknown as Wire).__tcmWireNotes();
  const box = document.querySelector("textarea") as HTMLTextAreaElement;
  box.value = "hi";
  box.dispatchEvent(new Event("input"));
  await vi.advanceTimersByTimeAsync(600);
  await vi.advanceTimersByTimeAsync(N.timeoutMs - 1_000);
  gate.answer?.({ json: () => Promise.resolve({ ok: true }) });
  await flush();
  expect(document.getElementById("st")!.textContent).toBe("Saved ✓");

  // The limit passing afterwards changes nothing.
  await vi.advanceTimersByTimeAsync(2_000);
  await flush();
  expect(document.getElementById("st")!.textContent).toBe("Saved ✓");
  expect(N.busy()).toBe(0);
});
```

Append to `src/lib/casesSpecs.test.ts`:

```ts
test("a heading in a script without letter case keeps its words in the slug", () => {
  expect(H.slug("5.8 表示ルール")).toBe("5-8-表示ルール");
  expect(H.slug("ログイン　画面")).toBe("ログイン-画面"); // an ideographic space separates
  expect(H.slug("概要、目的。")).toBe("概要-目的");
  expect(H.slug("사용자 설정")).toBe("사용자-설정");
  expect(H.slug("🙂 Emoji")).toBe("emoji");
  // Nothing wordlike at all: empty, and the page falls back to "h".
  expect(H.slug("🙂 !!")).toBe("");
});

/// Citations resolve through matchHeading, not through the ids, so a
/// caseless heading must still be found by its words and by its number.
test("a citation still finds a heading written without letter case", () => {
  const headings = ["1 概要", "2 表示ルール", "3 ログイン 画面"];
  expect(H.matchHeading(headings, "2 表示ルール")).toBe(1);
  expect(H.matchHeading(headings, "表示ルール")).toBe(1);
  expect(H.matchHeading(headings, "ログイン 画面")).toBe(2);
});
```

Append to `src-tauri/tests/auth.rs`:

```rust
/// A connection that drips one byte at a time, each inside the per-read
/// timeout, used to hold the wait for as long as it kept dripping - past
/// the sign-in window itself.
#[test]
fn a_slow_drip_connection_cannot_hold_the_wait_past_the_window() {
    let (l, port) = loopback();
    let dripper = std::thread::spawn(move || {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        for _ in 0..100 {
            if s.write_all(b"G").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let started = std::time::Instant::now();
    let out = await_redirect(l, "s1", Duration::from_millis(600), Duration::from_millis(200));
    assert_eq!(out, Err(SIGN_IN_TIMEOUT.to_string()));
    assert!(started.elapsed() < Duration::from_secs(2), "held for {:?}", started.elapsed());
    dripper.join().unwrap();
}
```

In `src-tauri/tests/ado_network.rs`:
1. Add `use v2_lib::auth::{SIGN_IN_NET_GENERIC, SIGN_IN_NET_TIMEOUT, SIGN_IN_NET_UNREACHABLE};`.
2. In `no_message_can_carry_a_url`, change the loop to `for msg in [NET_TIMEOUT, NET_UNREACHABLE, NET_GENERIC, SIGN_IN_NET_TIMEOUT, SIGN_IN_NET_UNREACHABLE, SIGN_IN_NET_GENERIC] {`.
3. In `a_sign_in_network_failure_names_no_url`, change `assert_eq!(err, NET_UNREACHABLE);` to:

```rust
    // Offline, it is Microsoft's sign-in that cannot be reached, not Azure
    // DevOps - the sentence says which.
    assert_eq!(err, SIGN_IN_NET_UNREACHABLE);
    assert!(err.contains("Microsoft sign-in") && !err.contains("Azure DevOps"), "{err}");
```

- [ ] **Step 2: Run to see them fail.** First `npx vitest run --exclude "**/.claude/**" src/lib/casesPage.test.ts src/lib/casesNotes.test.ts src/lib/casesSpecs.test.ts`. Expected:
  - The backoff test fails (asked at 8 s).
  - Both notes tests fail (`timeoutMs` is undefined, and the box stays busy).
  - The slug test fails (`"5-8"`).
  - The citation test **passes**. It pins that the resolver does not depend on ids.

  Then do the dev-app check and, from `src-tauri/`, run `CARGO_TARGET_DIR=target/gate cargo test --test auth`. Expected: the drip test fails at about 5 s elapsed. Then `--test ado_network`. Expected: compile error (no `SIGN_IN_NET_*`).

- [ ] **Step 3: Implement.**

In `src-tauri/web/cases-page.js`, replace lines 359-391 (the `setInterval(function () { ... }, 4000);` call) with:

```js
    // How long until the next ask. A closed app answers nothing, and every
    // refused ask is an error line in the browser's console - so each
    // failure doubles the wait, up to a minute, and the first answer brings
    // it back to 4 s.
    var POLL_MS = 4000, POLL_MAX_MS = 60000, wait = POLL_MS;
    function schedule() { setTimeout(poll, wait); }
    function poll() {
      var s = staleBanner();
      if (document.hidden || (s && s.classList.contains('show'))) { schedule(); return; }
      fetch(base + '/version?' + qs)
        .then(function (r) { return r.json(); })
        .then(function (v) {
          wait = POLL_MS;
          // null means the app did not recognise this page; that is not
          // staleness and must not be reported as it.
          if (typeof v.revision !== 'number' || v.revision === rev) { return; }
          // Never swap under a reviewer's typing, or under an edit that
          // has not landed yet: the focused-textarea check alone misses a
          // box clicked away from before its debounce fired, or while its
          // save is still in flight or queued behind another (see
          // window.tcmNotes.busy in cases-notes.js) - any of that would
          // show the box's OLD text under whatever was typed next. The
          // revision stays ahead of ours either way, so the next poll
          // simply tries again.
          var ae = document.activeElement;
          if (ae && ae.tagName === 'TEXTAREA') { return; }
          if (window.tcmNotes && window.tcmNotes.busy() > 0) { return; }
          var target = v.revision;
          fetch(base + '/report?' + qs)
            .then(function (r) {
              if (!r.ok) { throw new Error('no report'); }
              return r.text();
            })
            .then(function (html) {
              if (swap(html)) { rev = target; } else { banner(); }
            })
            .catch(banner);
        })
        .catch(function () { wait = Math.min(wait * 2, POLL_MAX_MS); })
        .then(schedule);
    }
    schedule();
```

In `src-tauri/web/cases-notes.js`:
1. Line 45 becomes `window.tcmNotes = { makeQueue: makeQueue, busy: function () { return busyCount; }, timeoutMs: NOTE_TIMEOUT_MS };` and, directly above line 42 (`var busyCount = 0;`), add:

```js
  // A save the app never answers - it hung, or its listener stalled - must
  // still end: until it does the box counts as busy, and the page's live
  // update (cases-page.js) waits for busy() to reach 0 before it swaps.
  var NOTE_TIMEOUT_MS = 10000;
  function postNote(payload) {
    return new Promise(function (ok, fail) {
      var done = false;
      var ctl = typeof AbortController === 'function' ? new AbortController() : null;
      var timer = setTimeout(function () {
        if (done) return;
        done = true;
        if (ctl) ctl.abort();
        var e = new Error('the app did not answer');
        e.name = 'TimeoutError';
        fail(e);
      }, NOTE_TIMEOUT_MS);
      fetch('http://127.0.0.1:' + NOTE_PORT + '/note', {
        method: 'POST',
        headers: { 'Content-Type': 'text/plain' },
        body: JSON.stringify(payload),
        signal: ctl ? ctl.signal : undefined
      }).then(function (r) { return r.json(); }).then(
        function (v) { if (done) return; done = true; clearTimeout(timer); ok(v); },
        function (e) { if (done) return; done = true; clearTimeout(timer); fail(e); }
      );
    });
  }
```

2. In `wire`, replace the `var save = makeQueue(function (payload) { return fetch(...)...; }, function (r, err) {` opening and its `if (err) { ... }` branch (lines 56-69) with:

```js
    var save = makeQueue(postNote, function (r, err) {
      // The report callback fires only for the newest save once nothing is
      // queued behind it (see makeQueue's `finish`) - exactly when this box
      // stops being dirty, saved or not.
      settle();
      if (err) {
        status.className = 'note-status bad';
        status.textContent = err.name === 'TimeoutError'
          ? 'Not saved - the app did not answer'
          : 'Not saved — the app is closed';
```

leaving the `else if (r && r.ok)` and `else` branches after it unchanged.

In `src-tauri/web/cases-specs.js`, replace lines 152-157 (the comment and `isWordChar`) with:

```js
  // A word character is a letter or a digit in any script. ES5 has no
  // \p{L}, and the u flag breaks the whole script on an older engine, so:
  // anything with a case, the ASCII digits, and any other character past
  // ASCII that is not in a punctuation, symbol, space or surrogate block.
  // Scripts without case - CJK, kana, Hangul, Arabic, Thai, Devanagari -
  // used to be dropped whole, and every such heading got the id "h".
  var NOT_WORD = /[\u0080-\u00bf\u00d7\u00f7\u2000-\u206f\u20a0-\u20cf\u2100-\u214f\u2190-\u2bff\u2e00-\u2e7f\u3000-\u3004\u3008-\u3020\u3030\u303d\ufe10-\ufe1f\ufe30-\ufe6f\uff00-\uff0f\uff1a-\uff20\uff3b-\uff40\uff5b-\uff65\ud800-\udfff\ufeff\ufff0-\uffff]/;
  function isWordChar(c) {
    if (c.toLowerCase() !== c.toUpperCase() || /[0-9]/.test(c)) return true;
    return c > '\u007f' && !NOT_WORD.test(c);
  }
```

In `src-tauri/src/auth.rs`:

1. Replace `sign_in_network_error` (lines 129-136) with:

```rust
/// What sign-in says when it cannot talk to Microsoft's sign-in service.
/// Offline it used to say "Can't reach Azure DevOps" - but sign-in asks
/// Microsoft, not Azure DevOps. The same shape and the same rule as the
/// sentences in `ado/transport.rs`: no URL (reqwest's Display names the
/// login endpoint, which nobody can act on), and a way to the details.
pub const SIGN_IN_NET_TIMEOUT: &str =
    "Microsoft sign-in didn't respond in time. Check your connection and try again. Settings → Logs has the details.";
pub const SIGN_IN_NET_UNREACHABLE: &str =
    "Can't reach Microsoft sign-in. Check your internet connection or VPN, then try again. Settings → Logs has the details.";
pub const SIGN_IN_NET_GENERIC: &str =
    "The connection to Microsoft sign-in failed. Try again - restart the app if it keeps happening. Settings → Logs has the details.";

fn sign_in_network_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        SIGN_IN_NET_TIMEOUT
    } else if e.is_connect() {
        SIGN_IN_NET_UNREACHABLE
    } else {
        SIGN_IN_NET_GENERIC
    }
    .to_string()
}
```

2. Add directly above `pub fn await_redirect`:

```rust
/// The first line of one loopback request, read against ONE budget for the
/// whole line. A per-read timeout alone restarts with every byte, so a
/// connection that sent a byte at a time just inside it could hold the wait
/// far past the sign-in window. `None` when no whole line came in time.
fn read_request_line(stream: &mut std::net::TcpStream, budget: Duration) -> Option<String> {
    use std::io::Read;
    let until = Instant::now() + budget;
    let mut line: Vec<u8> = Vec::with_capacity(256);
    let mut chunk = [0u8; 512];
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return None;
        }
        stream.set_read_timeout(Some(left)).ok()?;
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        line.extend_from_slice(&chunk[..n]);
        if let Some(end) = line.iter().position(|&b| b == b'\n') {
            line.truncate(end + 1);
            return String::from_utf8(line).ok();
        }
        if line.len() >= 8192 {
            return None;
        }
    }
}
```

3. In `await_redirect`, change `use std::io::{BufRead, BufReader, ErrorKind, Read};` to `use std::io::ErrorKind;`. Then replace lines 354-360 (from `let _ = stream.set_read_timeout(...)` through the `read_line` `if` block) with:

```rust
        let _ = stream.set_write_timeout(Some(read_timeout));
        // The whole request line within `read_timeout`, and never past the
        // sign-in window itself.
        let budget = read_timeout.min(deadline.saturating_duration_since(Instant::now()));
        let Some(line) = read_request_line(&mut stream, budget) else {
            continue; // no whole line in time: drop it, keep waiting
        };
```

Also update the doc comment of `await_redirect` so its "Each connection gets `read_timeout` to send its request line" sentence reads "Each connection gets `read_timeout` in all to send its request line (never past `window`)".

- [ ] **Step 4: Run** (one at a time): `npx vitest run --exclude "**/.claude/**" src/lib/casesPage.test.ts src/lib/casesNotes.test.ts src/lib/casesSpecs.test.ts src/lib/webPages.test.ts src/lib/casesPageMarks.test.ts src/lib/casesPageFilter.test.ts`. Then, after the dev-app check and from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --test auth`, `--test ado_network`. All green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/web/cases-page.js src-tauri/web/cases-notes.js src-tauri/web/cases-specs.js src-tauri/src/auth.rs src-tauri/tests/auth.rs src-tauri/tests/ado_network.rs src/lib/casesPage.test.ts src/lib/casesNotes.test.ts src/lib/casesSpecs.test.ts
git commit -q -F - <<'EOF'
fix(v2): the review page backs off and times out, headings in any script get ids, and sign-in is bounded and names Microsoft

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 8: Shared Steps rows by name, the pre-flight label, and the Execution order modal's name

**Files:**
- Create: `src/lib/sharedSteps.ts`
- Modify: `src/components/SharedStepLabel.tsx:24` (query key)
- Modify: `src/screens/RunPanel/CasePreview.tsx:11-14` (imports), `:130-136` (rows)
- Modify: `src/components/BugDialog.tsx:1`, `:32-37`
- Modify: `src/lib/submitRun.ts` (phase stage, `submitUploading`, `submitLabel`)
- Modify: `src/components/QueueSection.tsx:37-45` (imports), `:714-716` (after the all-no-op return), `:1687`, `:1779`, `:1862-1866`, `:2039`, `:2054`
- Modify: `src/components/ui/modal.tsx:47-56`, `:109-113`
- Modify: `src/screens/RunPanel/ExecutionOrderModal.tsx:2`, `:204-205`
- Test: `src/screens/RunPanel/CasePreview.test.tsx` (create), `src/components/BugDialog.test.tsx`, `src/lib/submitRun.test.ts`, `src/components/QueueSection.test.tsx`, `src/components/ui/modal.test.tsx`, `src/screens/RunPanel/ExecutionOrderModal.test.tsx`

**Interfaces:**
- Consumes: `SharedStepLabel` (exists). It is unaffected by Task 5's `QueueSection.tsx` edits (different regions of the file).
- Produces:
  - `sharedStepText(id: number, title?: string | null): string` and `sharedStepQueryKey(org: string, id: number)` in `src/lib/sharedSteps.ts`
  - `SubmitPhase.stage: "checking" | "uploading"`, `submitUploading(run: number): void`, `submitLabel(phase: NonNullable<SubmitPhase>): string`
  - `Modal` props `labelledBy?: string`, `label?: string`

- [ ] **Step 1: Write the failing tests.**

Create `src/screens/RunPanel/CasePreview.test.tsx`:

```tsx
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { TestPoint } from "../../bindings";
import CasePreview from "./CasePreview";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

const full = (id: number, title: string, steps: unknown[]) => ({
  id,
  title,
  steps,
  step_ids: steps.map(() => ""),
  steps_xml: "",
  tags: "",
  automation_status: "Planned",
  module_value: "",
  preconditions: "",
});

test("a Shared Steps row reads as Shared steps #N with its title, not as a blank row", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "test_cases_by_ids") {
      const ids = (args as { ids: number[] }).ids;
      if (ids[0] === 812) return [full(812, "Sign in as an admin", [])];
      return [
        full(201, "Valid login", [
          { action: "Open page", expected: "Shown" },
          { action: "", expected: "", shared: 812 },
        ]),
      ];
    }
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const point = { test_case_id: 201, last_run_id: null, last_result_id: null } as unknown as TestPoint;
  render(
    <QueryClientProvider client={qc}>
      <CasePreview org="acme" project="Web" point={point} />
    </QueryClientProvider>,
  );
  expect(await screen.findByText("Shared steps #812")).toBeInTheDocument();
  expect(await screen.findByText(/Sign in as an admin/)).toBeInTheDocument();
});
```

Append to `src/components/BugDialog.test.tsx`:

```tsx
test("a Shared Steps row goes into the repro as Shared steps #N, with its title when known", () => {
  mockIPC(() => null);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  qc.setQueryData(["shared-step", "acme", 812], "Sign in as an admin");
  render(
    <QueryClientProvider client={qc}>
      <BugDialog
        org="acme"
        project="Web"
        testCase={{
          ...testCase,
          steps: [
            { action: "Open page", expected: "Shown" },
            { action: "", expected: "", shared: 812 },
            { action: "", expected: "", shared: 900 },
          ],
          step_ids: ["2", "", ""],
        }}
        pbiId={42}
        screenshots={[]}
        onClose={vi.fn()}
        onFiled={vi.fn()}
      />
    </QueryClientProvider>,
  );
  const repro = (screen.getByLabelText("Repro steps") as HTMLTextAreaElement).value;
  expect(repro).toContain("1. Open page -> expected: Shown");
  expect(repro).toContain("2. Shared steps #812 - Sign in as an admin");
  expect(repro).toContain("3. Shared steps #900");
  expect(repro).not.toMatch(/^\d+\. $/m);
});
```

Append to `src/lib/submitRun.test.ts` (add `submitLabel` and `submitUploading` to its existing import from `./submitRun`):

```ts
test("a submit reads as checking until it starts to upload", () => {
  const run = submitStarted("acme", 42, 3)!;
  try {
    expect(submitPhaseSnapshot()?.stage).toBe("checking");
    expect(submitLabel(submitPhaseSnapshot()!)).toBe("Checking");
    submitUploading(run);
    expect(submitLabel(submitPhaseSnapshot()!)).toBe("Processing");
  } finally {
    submitFinished(run);
  }
});
```

In `src/components/QueueSection.test.tsx`:
1. In `an upload in flight shows the sweeping bar first, then the count, and no Stop`, change its dynamic import to `const { submitStarted, submitUploading, submitProgressed, submitFinished } = await import("../lib/submitRun");` and add `submitUploading(run);` right after `const run = submitStarted("acme", 42, 10)!;`.
2. Append:

```tsx
/// Every queued update already matches Azure DevOps. The pre-flight read
/// finds nothing to write and the submit ends there - so nothing on screen
/// may say "Processing" while it looks.
test("an upload with nothing to change says Checking while it looks, never Processing", async () => {
  const gate: { open?: (v: unknown) => void } = {};
  let submits = 0;
  const base = {
    title: "Login works",
    tags: "smoke",
    automation_status: "Not Automated",
    steps: [{ action: "Open page", expected: "Page shown" }],
    step_ids: ["2"],
    module_value: "",
    preconditions: "",
  };
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "list_project_tags") return ["smoke"];
    if (cmd === "test_case_field_values") return [];
    if (cmd === "pbi_test_cases") return [];
    if (cmd === "test_cases_by_ids") {
      // The queue's own diff read answers at once; the upload's pre-flight
      // read (made while a submit is running) is held open.
      if (submitPhaseSnapshot() == null) return [{ id: 201, ...base }];
      return new Promise((resolve) => {
        gate.open = resolve;
      });
    }
    if (cmd === "submit_queue") {
      submits += 1;
      return [];
    }
    return undefined;
  });
  renderQueue([makeCase({ update_id: 201 })]);
  expect(await screen.findByText(/nothing will change/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /Review 1 test case/ }));
  const go = await screen.findByRole("button", { name: /Confirm & update 1/ });
  await waitFor(() => expect(go).toBeEnabled());
  fireEvent.click(go);

  await waitFor(() => expect(gate.open).toBeDefined());
  expect(screen.getByRole("progressbar", { name: "Checking what changed" })).toBeInTheDocument();
  expect(screen.getAllByText("Checking").length).toBeGreaterThan(0);
  expect(screen.queryByText(/Processing/)).not.toBeInTheDocument();

  await act(async () => {
    gate.open?.([{ id: 201, ...base }]);
  });
  await waitFor(() => expect(submitPhaseSnapshot()).toBeNull());
  expect(submits).toBe(0);
  expect(screen.queryByText(/Processing/)).not.toBeInTheDocument();
});
```

Append to `src/components/ui/modal.test.tsx`:

```tsx
test("a modal is named by the heading it points at, or by a label", () => {
  const { unmount } = render(
    <Modal onClose={vi.fn()} labelledBy="modal-title">
      <h2 id="modal-title">Execution order</h2>
    </Modal>,
  );
  expect(screen.getByRole("dialog", { name: "Execution order" })).toBeInTheDocument();
  unmount();
  render(
    <Modal onClose={vi.fn()} label="Screenshot">
      <p>picture</p>
    </Modal>,
  );
  expect(screen.getByRole("dialog", { name: "Screenshot" })).toBeInTheDocument();
});
```

Append to `src/screens/RunPanel/ExecutionOrderModal.test.tsx`:

```tsx
test("the dialog is named by its heading", () => {
  mount();
  expect(screen.getByRole("dialog", { name: "Execution order" })).toBeInTheDocument();
});
```

- [ ] **Step 2: Run to see them fail:** `npx vitest run --exclude "**/.claude/**" src/screens/RunPanel/CasePreview.test.tsx src/components/BugDialog.test.tsx src/lib/submitRun.test.ts src/components/QueueSection.test.tsx src/components/ui/modal.test.tsx src/screens/RunPanel/ExecutionOrderModal.test.tsx`. Expected:
  - The CasePreview test fails (no "Shared steps #812").
  - The BugDialog test fails (`2. `).
  - The submitRun test fails (`submitLabel` is not exported).
  - The in-flight test fails (`submitUploading` is not exported).
  - The no-op test fails ("Processing the upload" is shown).
  - Both name tests fail (no dialog with that name).

- [ ] **Step 3: Implement.**

Create `src/lib/sharedSteps.ts`:

```ts
// A Shared Steps reference in words - one helper, so the runner's preview,
// a bug's repro steps and the script editor all say the same thing.

/** A Shared Steps reference as plain text: "Shared steps #812", plus
 * " - <title>" when the title is known. The same words SharedStepLabel
 * shows, for places that need text rather than an element. */
export function sharedStepText(id: number, title?: string | null): string {
  return title ? `Shared steps #${id} - ${title}` : `Shared steps #${id}`;
}

/** The query SharedStepLabel reads a reference's title through. A caller
 * that needs the title synchronously reads it from the query client. */
export function sharedStepQueryKey(org: string, id: number) {
  return ["shared-step", org, id] as const;
}
```

In `src/components/SharedStepLabel.tsx`, add `import { sharedStepQueryKey } from "../lib/sharedSteps";` and change line 24 to `queryKey: sharedStepQueryKey(org, id),`.

In `src/screens/RunPanel/CasePreview.tsx`, add `import SharedStepLabel from "../../components/SharedStepLabel";` after the `AstryxIsland` import. Then replace the rows (lines 130-136) with:

```tsx
              {tc.steps.map((s, i) => (
                <tr key={i} className="border-t border-border/40 align-top">
                  <td className="px-2 py-1 text-faint">{i + 1}</td>
                  {s.shared != null ? (
                    // A Shared Steps reference: its steps live in that work
                    // item, so it is one named line, not an empty row.
                    <td colSpan={2} className="px-2 py-1">
                      <SharedStepLabel id={s.shared} org={org} />
                    </td>
                  ) : (
                    <>
                      <td className="whitespace-pre-wrap px-2 py-1 text-text">{s.action}</td>
                      <td className="whitespace-pre-wrap px-2 py-1 text-muted">{s.expected}</td>
                    </>
                  )}
                </tr>
              ))}
```

In `src/components/BugDialog.tsx`, change line 1 to `import { useMutation, useQueryClient } from "@tanstack/react-query";`, change line 5 to `import { commands, type Step, type TestCaseFull } from "../bindings";`, add `import { sharedStepQueryKey, sharedStepText } from "../lib/sharedSteps";` after the `blobToB64` import, and replace lines 32-37 with:

```tsx
  const qc = useQueryClient();
  // A Shared Steps reference has no text of its own: it is named, with its
  // title when a screen has already read it.
  const stepLine = (s: Step, i: number) =>
    s.shared != null
      ? `${i + 1}. ${sharedStepText(s.shared, qc.getQueryData<string>(sharedStepQueryKey(org, s.shared)))}`
      : `${i + 1}. ${s.action}${s.expected ? ` -> expected: ${s.expected}` : ""}`;
  const defaultRepro = [
    `Test case #${testCase.id}: ${testCase.title}`,
    "",
    "Steps:",
    ...testCase.steps.map(stepLine),
  ].join("\n");
```

In `src/lib/submitRun.ts`:
1. In `SubmitPhase`, after `title: string;` add:

```ts
  /** "checking" while the submit reads what changed - an all-no-op submit
   * ends there, having uploaded nothing - then "uploading". */
  stage: "checking" | "uploading";
```

2. In `submitStarted`, the phase becomes `{ run, org, pbiId, done: 0, total, title: "", stage: "checking" }`.
3. In `submitProgressed`, the phase becomes `{ ...phase, done, total, title, stage: "uploading" }`.
4. After `submitProgressed`, add:

```ts
/** The submit found something to write: from here on it is an upload. */
export function submitUploading(run: number): void {
  if (!phase || phase.run !== run || phase.stage === "uploading") return;
  phase = { ...phase, stage: "uploading" };
  emit();
}

/** The word the queue's action button shows while `phase` runs: "Checking"
 * while it is still reading what changed, "Processing" once it uploads. */
export function submitLabel(phase: NonNullable<SubmitPhase>): string {
  return phase.stage === "checking" ? "Checking" : "Processing";
}
```

In `src/components/QueueSection.tsx`:
1. Add `submitLabel` and `submitUploading` to the `../lib/submitRun` import (lines 37-45).
2. Right after the all-no-op return (after line 716, the `}` closing `if (toSend.length === 0) { ... }`) add:

```ts
        // Something to write: from here on the screen says it is uploading.
        submitUploading(run);
```

3. Line 1687 becomes:

```tsx
          label={
            progress.stage === "checking"
              ? "Checking what changed"
              : progress.done === 0
                ? "Processing the upload"
                : "Uploading"
          }
```

4. Line 1779 (`Processing` inside the disabled button) becomes `{submitLabel(progress)}`. Line 2039 is the same.
5. In the confirm button (lines 1862-1866), the first branch becomes `submit.isPending ? (progress ? submitLabel(progress) : "Checking")`. Line 2054 becomes `{submit.isPending ? (progress ? submitLabel(progress) : "Checking") : `Confirm & ${actionLabel || "create 0"}`}`.

In `src/components/ui/modal.tsx`, change the props (lines 47-56) to:

```tsx
export function Modal({
  onClose,
  className,
  children,
  labelledBy,
  label,
}: {
  onClose: () => void;
  /** Panel classes (width, max-height, padding, layout). */
  className?: string;
  children: ReactNode;
  /** The id of the element that names this dialog - usually its heading.
   * A dialog with no name is announced as just "dialog". */
  labelledBy?: string;
  /** The dialog's name, when nothing on it says it. */
  label?: string;
}) {
```

and the panel's attributes (lines 111-113) to:

```tsx
          role="dialog"
          aria-modal="true"
          aria-labelledby={labelledBy}
          aria-label={labelledBy ? undefined : label}
          tabIndex={-1}
```

In `src/screens/RunPanel/ExecutionOrderModal.tsx`, change line 2 to `import { useId, useMemo, useState } from "react";`. Add `const titleId = useId();` as the first line inside the component body. Change lines 204-205 to:

```tsx
    <Modal onClose={close} labelledBy={titleId} className="flex max-h-[85vh] w-[640px] max-w-full flex-col gap-3 p-4">
      <h2 id={titleId} className="text-sm font-semibold text-text">Execution order</h2>
```

- [ ] **Step 4: Run** (one at a time): `npx vitest run --exclude "**/.claude/**" src/screens/RunPanel/CasePreview.test.tsx src/components/BugDialog.test.tsx src/lib/submitRun.test.ts src/components/QueueSection.test.tsx src/components/QueueSection.floating.test.tsx src/components/ui/modal.test.tsx src/screens/RunPanel/ExecutionOrderModal.test.tsx src/screens/AutoRun/ScriptEditor.test.tsx src/ui-consistency.test.ts src/a11y.test.tsx`, then `npx tsc --noEmit`. All green.

- [ ] **Step 5: Commit**

```bash
git add src/lib/sharedSteps.ts src/components/SharedStepLabel.tsx src/screens/RunPanel/CasePreview.tsx src/screens/RunPanel/CasePreview.test.tsx src/components/BugDialog.tsx src/components/BugDialog.test.tsx src/lib/submitRun.ts src/lib/submitRun.test.ts src/components/QueueSection.tsx src/components/QueueSection.test.tsx src/components/ui/modal.tsx src/components/ui/modal.test.tsx src/screens/RunPanel/ExecutionOrderModal.tsx src/screens/RunPanel/ExecutionOrderModal.test.tsx
git commit -q -F - <<'EOF'
fix(v2): Shared Steps rows are named in the preview and the bug, the pre-flight says Checking, and Execution order has a name

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
git log -1
```

---

## After execution: the gates, then checks only a person can make

- [ ] Rust suite once, from `src-tauri/`: `CARGO_TARGET_DIR=target/gate cargo test --tests` (after the dev-app check). Then `npx vitest run --exclude "**/.claude/**"` and `npx tsc --noEmit`, one at a time. No release.

By hand, owed by the owner (jsdom does no layout and no hit testing, and nothing here talks to a real browser or a real org):
1. Minimise the app for more than five minutes and have someone @mention you. Nothing arrives while minimised. Restoring the window shows the mention at once, as a toast.
2. On a real board with Swimlanes on, open a lane parent that is not a card (a Feature above PBIs). The State dropdown lists that type's states, and changing the state saves.
3. Start a module recording, then close the app from the title bar. No Edge window is left, and `%TEMP%\tcm-autorun-*` has no folder from that session.
4. Import a file with two same-titled drafts, re-sort the queue, edit the second, and check the file on disk: the edit is on its own entry, and each entry kept its extra keys.
5. Open a draft's review page, close the app, and watch the browser console: failed asks slow down to one a minute. Reopen the app and the page picks up again.

---

## Self-review

**1. Spec coverage.**

| Item | Where |
| --- | --- |
| 1 no background polling, one check on return | Task 1 (`useMentions`, visibility test) |
| 2 linear PR scan | Task 1 (`usePrAttention` `scanned` map, rescan test) |
| 3 seen-set eviction | Task 1 (`remember` refresh, two tests) |
| 4 lane parent's states | Task 2 |
| 5 key-derived wipes, write-then-wipe test | Task 1 |
| 6 browsers closed on exit, profiles removed | Task 3 (`close_autorun_browsers`, `RunEvent::Exit`, `close_browser` waits) |
| 7 Try's own cancel sentence, shown right | Task 3 (`TRY_CANCELLED`, `ModuleTryResult.cancelled`, dialog) |
| 8 floor rule 5, re-check before publish, owner decision stated | Task 4; decision in the rulings |
| 9 consistent, stable, identity pairing; hold row; stale remove index | Task 5 |
| 10 Shared Steps rows by name | Task 8 |
| 11 | Dropped (unreachable; reason above) |
| 12 build unless root is `steps`; prolog and comments kept | Task 6 |
| 13 no "Processing" during pre-flight | Task 8 |
| 14 poll backoff; `/note` timeout | Task 7 |
| 15 bounded read; Microsoft sentence; no URL | Task 7 |
| 16 tighter draft shape; one containment helper | Task 6 |
| 17 slug keeps every script; writer and resolver checked | Task 7 |
| 18 accessible name; `Modal` support | Task 8 |

**2. Placeholder scan.** No TBD, no "similar to", no "add error handling". Every code step carries its code. Line numbers are those on `72e9301`. Tasks 5 and 8 both edit `QueueSection.tsx` in different regions, so a later task finds its region with Grep on the quoted code, not by line number.

**3. Type consistency.** These names are the same everywhere they appear: `KNOWN_CAP`, `work_item_type_states`/`workItemTypeStates`, `StateInfo`, `TRY_CANCELLED`, `unless_cancelled(work, cancelled)`, `ModuleTryResult.cancelled`, `close_autorun_browsers`, `close_autorun_on_exit`, `Expected.shared`, `steps_on_shared_rows`, `on_shared_row`, `PublishCase.shared_steps`, `step_marks_checked`, `sharedSteps` (RunReview prop), `DraftEdit.occurrence`, `claim_named`, `same_title`, `FileOwner`, `fileOwners`, `claimRows`, `occurrenceIn`, `stampFileSlices(..., occurrences)`, `UploadHold.sigs`, `holdSignature`, `narrowHold`, `is_draft_shape`, `read_request_line`, `SIGN_IN_NET_TIMEOUT`/`_UNREACHABLE`/`_GENERIC`, `NOTE_TIMEOUT_MS`/`tcmNotes.timeoutMs`, `sharedStepText`, `sharedStepQueryKey`, `SubmitPhase.stage`, `submitUploading`, `submitLabel`, `Modal` `labelledBy`/`label`. The TS argument order of `commands.workItemTypeStates(org, project, type)` matches the Rust `(organization, project, work_item_type)`.

**4. Review Focus.** Five lines, each with its test in the owning task: Task 2 (1), Task 5 (2, 3), Task 7 (4), Task 3 (5).
