# Discovery recordings: design

Approved by the owner on 27 November 2026 (short design, small-to-medium change).

## Intent

While discovering, the person sometimes verifies a bug fix and wants **video evidence** that it now works. The assistant decides from the person's prompt whether to record its checks. Separately, every Auto Test video is recorded at the **page size set in Auto Test settings**, not at a fixed 1280 x 720.

## Goals

- Two assistant tools for discovery:
  - **start recording**, which takes a short title, e.g. "Bug 4521: leave balance fix";
  - **stop recording**.
- The guide tells the assistant to record only when the prompt asks for evidence or a recording, or says a fix is being verified. It never records by default.
  - It starts just before the checks that prove the point, not during sign-in or navigation, and stops right after them.
- Safety stops: the recording ends and is kept when discovery ends or is stopped, or at a 10-minute cap.
- The step highlight (box and label) appears in the video. Its existing redaction already keeps secrets out.
- The video is kept **in the app only**. This is the owner's choice (option A): no Azure DevOps attachment and no test-result upload for now.
- A **Recordings** card on the Auto Test screen lists each recording:
  - its title, date and time, and length;
  - **Play**, **Save** and **Remove**.
- Recordings are kept until removed. They are never pruned with failed-case videos.
- When the assistant stops, its tool reply says the recording is ready under Auto Test → Recordings.
- Every recording, failed-case videos included, uses the Auto Test page size (`browser::viewport::current()`) for the recorder's max width and height, instead of `recording::MAX_WIDTH` / `MAX_HEIGHT`.

## Non-goals

- Attaching to a bug, or uploading to a test run. These are possible later (options B and C).
- A person-started record button.
- Trimming or editing videos.
- Recording outside discovery.

## Anti-goals

- Recording without the prompt asking for it.
- A secret on screen in a label: the existing redaction applies.
- A recording that fails or slows discovery: recorder errors are logged and mean "no video", as today.
- Recordings mixed with or pruned like failed-case videos.

## Approach (chosen)

Reuse the browser's own recorder (`browser/recording.rs`: `Page.startScreenRecording` / `stopScreenRecording`, read back with `IO.read`) in the discovery browser.

Recordings live in their own folder under the Auto Test data root, with a small index file per environment/project, kept the way other Auto Test files are. Each entry holds:
- id;
- title (sanitised, at most 80 characters);
- created time;
- duration;
- file name.

Rejected alternatives:
- **Record all of discovery and trim:** long, large videos that need editing, which is what the owner did not want.
- **Stitch screenshots into a video:** choppy, needs a bundled encoder, and loses smooth checks.

## Constraints

- A browser without the recorder (below Chromium 153): start answers with a plain sentence that recording is not available. Discovery goes on.
- One recording at a time per discovery browser. A second start while one is running is refused with a sentence.
- Titles and tool replies name no URL or secret.
- The AI tools list (`ai_tools.rs`, mcp.rs and the roles) carries the two new tools, following the existing pattern. They are allowed in the roles that run discovery: writer, explorer and healer where discovery applies.
- In Basic mode, tools are always on. The card shows in Basic mode too.
- No hardcoded colours. Icons come from `actionIcons`. Bindings are regenerated, never hand-edited.
- Tests: Rust in `src-tauri/tests/suite/`, webview in `tests/`.

## Left to the builder

- The exact tool names: suggested `start_autorun_recording` / `stop_autorun_recording`.
- The index file's name and shape.
- How the safety stop hooks into discovery end, stop and the cap.
- The card's placement on the Auto Test screen: near the existing results or activity, matching its patterns.
- Reusing the existing video player (`CaseVideo.tsx`) and save command where possible.

## Also in this round: the assistant may change a script's account (owner, 27 November 2026)

**Today.** Rule 5 in `autorun/edits.rs` (~311) refuses any re-save that changes `account`:

> "the account a script runs as cannot be changed by a repair - a person picks it in the app"

The rule exists so that a failing case is never "fixed" by quietly moving it to an account with more access.

**Owner's decision.**
- **The writer and the case rewriter may change it.** The healer may not.
- **Declared like an area change.** The case's `edits` entry says `"account": true` with its `why`. An undeclared change is still refused, with a sentence telling the assistant how to declare it. A declaration that changes nothing is refused, as `area` is.
- **Saved accounts only.** The new account must be one of the environment's saved accounts. An unknown key is refused, and nothing is invented.
- **The healer is kept out by the tool connector** (`mcp_session` already tracks which roles' rules each agent has been given; the app does not know the role).
  - A `save_autorun_script` that declares `"account": true` from an agent session that has been given the healer's rules is refused in the connector with a plain sentence, before it reaches the app.
  - `healer.md` says it never changes the account. The new account is a person's or a writer's call.
- **Rule text.**
  - `writer.md` and `case-rewriter.md` say how to declare the change.
  - `case-rewriter.md`'s "A repair never changes the account a script runs as" is reworded.
  - The guide's refusal list (`guide.rs` ~3041) is updated.
- **Recorded.** The edit's `why` is kept with the repair, as other declared edits are. A person sees the change in the script editor.

**Tests.**
- A declared change by a writer or rewriter save is accepted.
- An undeclared change is refused.
- A declared no-op is refused.
- An unknown account is refused.
- The connector refuses the declaration in a session that has the healer's rules.
