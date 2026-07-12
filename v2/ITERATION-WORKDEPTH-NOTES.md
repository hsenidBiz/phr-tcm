# Work Manager depth exit notes (v0.6.0, 2026-07-12)

Roadmap iteration "Work Manager depth" (v0.6.0 after renumbering).

## Delivered

- **Detail drawer** (card click): title, state (the type's legal states),
  assignee (project team members, deduped/sorted, 24h-stale cached like
  v1's members cache), Activity picklist (empty when the process has no
  such field), Remaining/Completed/Original estimate, plain-text
  description editor (ReproSteps for Bugs, html flattened both ways).
  Save PATCHes only dirty fields; ADO rejections (invalid transition /
  required field) surface verbatim.
- **Comments**: newest-first with the v1 avatar chain (identity avatar ->
  imageUrl -> deterministic initials disc), friendly timestamps, posting.
  Avatar bytes fetched best-effort through avatar_b64 (Graph base64-JSON
  variant handled) - failures never break the panel.
- **Team scoping**: My work (AssignedTo = @Me) or any project team
  (everything under the team's area path(s) via the v1 WIQL clause -
  UNDER for includeChildren tree values, quote-escaped, golden tested).
- **Board filters**: free text (title/id/tag) + type.
- **Quick create**: Task/Bug title (Enter or button), assigned to the
  signed-in UPN, board refetches.
- Tests: 65 cargo + 35 vitest.

## Deferred

- Start/Finish date editing (fields fetched and shown in Rust detail;
  date-picker UI postponed - low usage in v1).
- System.Rev optimistic-concurrency check (v1 didn't send it either;
  last-writer-wins like v1).
- Focus timer on work items (v1 had one; the runner's timer covers the
  test-execution use; revisit on request).

## Remaining roadmap

v0.7.0 polish (area/iteration pickers on create, recently-used PBIs,
keyboard shortcuts, empty states, Playwright E2E smoke) -> v1.0.0
retirement sign-off.
