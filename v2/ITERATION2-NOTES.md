# Iteration 2 exit notes — editing + review flow (v0.3.0, 2026-07-12)

Roadmap: claudedocs/plans/2026-07-12-v2-parity-roadmap.md (iteration 2 of 6).

## Delivered

- **Field discovery** (v1 config_screen parity): Test Case type fields
  listed with the v1 filter; Module/Preconditions auto-picked by the
  name-contains rule, editable per project in Settings, stored per
  org/project; discovery failure degrades to "skip", never blocks.
- **Existing-cases editor** (v1 Edit tab): expand-to-edit title, tags,
  automation status, module, preconditions, and a steps grid
  (add/remove/reorder via buttons); saves through the blank-skip PATCH;
  export selected cases to xlsx or AI-JSON.
- **AI-JSON export** in Rust with the exact v1 wrapper
  (format/version/instructions/test_cases) - golden-tested to round-trip
  through the importer, ids preserved as UPDATE markers.
- **Review gate before create**: client-side mirror of is_valid blocks
  submission; duplicate-title check warns only (v1 rule: never an implicit
  update). Live per-item progress bar via the SubmitProgress specta event.
- **Draft queue persistence** per PBI across restarts.
- Tests: 58 cargo + 28 vitest.

## Deviations

- Step reordering uses up/down buttons instead of dnd-kit (fewer deps,
  fully testable; dnd-kit remains an option when the editor grows).

## Still open for later iterations

- Runner desktop UX (iteration 3), Work Manager depth (4), suites browser,
  area/iteration pickers, recently-used list (5).
