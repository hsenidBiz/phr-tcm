> **SUPERSEDED (2026-07-24)** by the AI MCP bridge:
> `docs/superpowers/plans/2026-07-24-ai-mcp-bridge.md`. Kept for the
> format/drift-gate reasoning the bridge's /guide and /validate reuse.

# AI Test-Case Guide generator — design spec

**Date:** 2026-07-20
**Status:** approved design, pre-implementation
**Scope:** v2 only (Tauri/Rust/React)

## Problem

The app can already import AI-authored test cases (the JSON import format was
built for exactly that), but nothing *teaches* an AI tool the format, the
validation rules, or a team's specifics (allowed Module values, tag
conventions, where the repo documents its screens). Developers hand-explain
this per session, and the AI guesses values that fail import.

## Solution overview

An in-app wizard ("Generate AI guide…") that produces a markdown guide a
developer drops into their repo. Any AI tool that reads it can then author
test cases that import cleanly.

The guide has two layers:

- **Fixed layer** — baked into the app and versioned with the importer: the
  JSON import schema, hard validation rules, a worked example, the
  import workflow, and test-case craft guidance.
- **Custom layer** — from the wizard: ADO-discovered picklists (Module
  values, tags in use) the user can prune, repo documentation pointers, and
  freeform team conventions.

**Single-source body, thin flavor wrappers.** The guide body is generated
once; each "flavor" only changes packaging (frontmatter, filename, folder
layout). Universality comes from the generic markdown itself — every AI tool
reads plain markdown — so adding future flavors is cheap and the body never
forks.

## Wizard UX

Entry point: a button in the **Import File** tab (where the output gets
used). Opens a 4-step dialog:

1. **Context** (mostly read-only): shows current org/project/area. Discovers
   the Module picklist values and tags-in-use via existing commands
   (`test_case_field_values` / `list_project_tags` — no new ADO surface).
   Values render as checked checkboxes; the user can untick values the AI
   shouldn't use. Discovery failures degrade gracefully: the section is
   omitted from the guide with a visible note, never a blocking error.
2. **Repo knowledge** (the human step): optional list of documentation
   paths/globs (e.g. `docs/screens/**`, `README.md`) — each becomes a "read
   these files before writing cases" instruction — plus an optional freeform
   conventions textarea (naming rules, tag taxonomy, anything).
3. **Flavor** (multi-select checkboxes, generic `.md` checked by default):
   - Generic `AI_TEST_CASES.md` — the universal one
   - Claude Code skill folder (`.claude/skills/generate-test-cases/SKILL.md`
     with name/description frontmatter)
   - Cursor rules file (`.cursor/rules/test-cases.mdc` with its header)
   - `AGENTS.md` snippet (body prefixed with a section heading, meant to be
     appended to an existing AGENTS.md)
4. **Output**: save via the existing dialog plugin (folder picker when the
   Claude-skill flavor is selected; file save otherwise), and/or copy the
   generic body to the clipboard.

Wizard state is ephemeral — nothing persisted except the saved files.
(A later nicety, out of scope now: remember the last-used doc paths in
localStorage.)

## Generated guide body (fixed-layer content)

Ordered sections:

1. **What this file is** — one paragraph: "instructions for AI tools
   generating test cases importable by Test Case Manager", plus the
   workflow: AI writes a JSON file → developer imports via the Import File
   tab → reviews in the review screen → creates in Azure DevOps.
2. **Output contract** — produce a single `.json` file; JSON only, no
   surrounding prose.
3. **JSON schema + worked example** — the accepted shape as the parser
   actually reads it (`parse_json` accepts key aliases:
   `title|name|test_case_name`; `id|test_case_id|work_item_id` ⇒ UPDATE that
   work item, omit to create; steps as strings or
   `{action, expected}` objects; `tags`, `module`, `preconditions`,
   `automation_status`). One complete two-case example: one create, one
   update.
4. **Hard rules (MUSTs)** — mirror the importer's real validation:
   `automation_status` exactly `"Not Automated"` or `"Planned"`; at least
   one step; non-empty title; Module only from the allowed list below; an
   `id` updates that exact work item (title matching never updates).
5. **Allowed values** (custom layer) — the pruned Module picklist and tag
   list, stamped: "values discovered from <org>/<project> on <date> —
   regenerate this guide if picklists change".
6. **Repo documentation** (custom layer, omitted if none given) — "before
   writing cases, read: <paths>".
7. **Team conventions** (custom layer, omitted if empty) — the freeform text
   verbatim.
8. **Writing good test cases** — brief craft guidance: one behavior per
   case, imperative atomic actions, observable expected results, cover
   negative/edge paths, preconditions state setup not steps.

## Architecture

- **Rust** (`v2/src-tauri/src/ai_guide.rs` + `commands/` wrapper): pure
  function `build_guide(options) -> Vec<GuideFile {relative_path, content}>`
  where `options` carries org/project/area, pruned picklists, doc paths,
  conventions text, selected flavors, and the generation date **passed in
  from the frontend** (keeps the function deterministic/testable). No
  network calls — the frontend feeds it values from the existing discovery
  queries. Registered in `lib.rs` `collect_commands![]`; bindings
  regenerated by the dev server as usual.
- **Why Rust owns the body**: the fixed layer must move in lockstep with the
  importer, and the load-bearing rule says Rust owns wire formats. The guide
  template lives beside `import_parser/` so a format change touches both in
  one commit.
- **Frontend**: one wizard dialog component under the Import screen; a
  save step using the dialog plugin + clipboard API. React Query supplies
  the discovery data (already cached for the Import tab's own use).
- **Safety**: read-only feature — no new ADO calls, no DELETE surface,
  nothing persisted server-side.

## Drift control (the one real risk)

Two mitigations for the guide going stale against the importer:

1. **Schema-sync test** (cargo): asserts every field name/alias the guide's
   schema section documents appears in `parse_json`'s accepted keys, and
   that the guide's worked example actually parses through `parse_json`
   round-trip (write to temp file, parse, expect 2 cases with the right
   update semantics). A format change that forgets the guide fails the gate.
2. **Snapshot stamp** in the output: generated guides carry the app version
   and date, and state that picklist values are a snapshot.

## Testing

- cargo: `build_guide` unit tests (flavor wrapping, section omission when
  custom inputs are empty, deterministic output) + the schema-sync/example
  round-trip test above.
- vitest: wizard flow (steps advance, discovery values render as
  checkboxes, unticking prunes the payload, flavor selection changes the
  save call), using `mockIPC` + demo fakes.
- Demo mode: `dev/demo.ts` fakes for the discovery values so the wizard is
  fully explorable under skip-sign-in.
- Gates as always: cargo test + vitest + build + dev-code-elimination check.

## Out of scope (parked)

- Live drift detection (guide phoning home to compare picklists).
- Tool-specific flavors beyond the four listed (add-on demand later — each
  is a wrapper function).
- Remembering wizard inputs across sessions.
- An MCP server exposing the importer directly to AI tools (a bigger,
  different feature).
