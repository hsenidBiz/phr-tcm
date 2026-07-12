# Iteration 1 exit notes — UI shell & design system (v0.2.0, 2026-07-12)

Roadmap: claudedocs/plans/2026-07-12-v2-parity-roadmap.md (iteration 1 of 6).

## Delivered

- **Layout**: sidebar (Test Cases / Work / Settings) + context bar that owns
  org/project (moved out of Browse); the Test Cases screen is now the
  PBI-centric hub. Section + scope persist across launches (prefs migrate
  from the old "mode" key transparently).
- **Design system**: CSS-var theme tokens (dark primary, light mirrored,
  system-follow default), `.dark` class switching, vendored primitives
  (Button/Input/Textarea/Select/Badge/Skeleton on cva), all screens moved
  off hardcoded neutral-* so light mode is correct everywhere.
- **Feedback**: sonner toasts for every mutation outcome (submit, run,
  board move, import/export, update), skeletons for board + point loading.
- **Command palette** (Ctrl+K, cmdk): navigate sections, switch project,
  toggle theme, check for updates.
- **Settings screen**: theme picker, app version, manual update check.
- Tests: 55 cargo + 20 vitest.

## Gotchas

- cmdk requires ResizeObserver + scrollIntoView, both absent in jsdom —
  stubbed in test-setup.ts.

## Next (iteration 2 → v0.3.0)

Test case editing + review flow: field discovery (module/preconditions
reference names), Existing-cases steps editor, export selection, review
step with duplicate-title warnings, streaming submit progress, draft
queue persistence.
