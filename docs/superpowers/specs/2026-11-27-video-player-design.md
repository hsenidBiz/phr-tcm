# Video player with step markers: design

Approved by the owner on 27 November 2026 (short design, medium change). Built after the discovery recordings round (`2026-11-27-discovery-recordings-design.md`), which it depends on.

## Intent

Watching a failed-case video in a small inline box means hunting for the moment that matters. The owner wants:
- a player that opens over the app with the background darkened, like YouTube or Netflix;
- jumps straight to steps (sign-in, a click, data entered);
- the failed part marked on the timeline, so it is easy to look at the steps before it and see what happened.

## Goals

- **Opening.** **Watch** on a failed-case video, or **Play** on a recording, opens a player over the app with the background darkened. Esc or the close button returns to where the person was.
- **Timeline.**
  - A tick marks each test-case step, with its name on hover.
  - The failed step's stretch is red, from its start to the end of the video.
- **Step list.**
  - It is beside the video and highlights the step playing now.
  - Clicking a step jumps to it.
  - Each step opens into its actions ("Click Save", "Type 'Smith'", "Check: shows 'Saved'"), and clicking an action jumps to it.
  - The failed step is marked, and the list scrolls to it on open. This is the owner's option C: steps on the timeline, actions in the list.
- **Controls.**
  - Play/pause, speed (0.5x, 1x, 2x) and Save.
  - Keys: Space play/pause; ← → skip 5 s; [ ] previous/next step.
- **Discovery recordings.** Each action the assistant tried is a marker. There are no test-case steps there, so these videos get a flat list.
- **Old videos.** Videos recorded before this change open in the same player without markers.
- **Action names.** They reuse the step highlight labels (`browser::highlight::Say`), so passwords and secret values show as "Type a hidden value", as they do on screen.

## Non-goals

- Editing or trimming video.
- Chapters written into the MP4.
- An external player.
- Thumbnails on hover.
- Uploading anywhere.

## Anti-goals

- A secret value in any marker or label.
- Markers that drift from the video: they come from recorded start times, never from summed durations.
- Slowing a run to record timings: one clock read per step and per action.

## Approach (chosen)

- The in-app player plays the saved MP4 with a `<video>` element in a modal.
- During a run, each step and each action notes its start time measured against the recording's own start, stored with the case record (failed-case videos) or with the recording's index entry (discovery recordings).
- Markers, the red failed stretch and the step list are drawn from those times.

Rejected:
- **MP4 chapter metadata:** it re-processes every file, and it cannot open into actions or highlight the playing step.
- **An external player:** it cannot jump to steps, and the person leaves TCM.

## Constraints

- The new timing fields deserialize with defaults, so old records still load.
- No hardcoded colours: tokens only. The ui-consistency gate passes. Icons come from `actionIcons`.
- The modal follows the app's existing modal (portal, focus trap, Esc) and works with keyboard and screen reader. The step list is a real list of buttons with accessible names.
- Rust tests go in `src-tauri/tests/suite/`, webview tests in `tests/`. Bindings are regenerated, never hand-edited.

## Left to the builder

- The exact field names and where the times are captured (the replay step loop, `browser::actions::execute_in`, discovery's try/act routes).
- How the inline `CaseVideo` hands over to the modal: replace it, or keep a small preview with a "Watch" button.
- The timeline drawing: a div bar or SVG.

## Added 27 November 2026 (owner)

The same player, with its step markers and its jump-to-step / jump-to-action list, also appears in the **run report** (the HTML report page), for each case that has a video. It replaces any inline video there. The report is a self-contained page, so the player is built from the page's own JS and CSS (`src-tauri/web/`) using `PagePalette` colours rather than the app's React component. It shares the same timing data.
