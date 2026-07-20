# AI Test-Case Guide Generator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An Import-tab wizard that generates a repo-droppable markdown guide teaching AI tools how to author test-case JSON that imports cleanly, with ADO-discovered picklists and repo doc pointers baked in.

**Architecture:** A pure Rust module (`ai_guide.rs`, beside the importer it documents) builds a single-source guide body and wraps it per flavor; two thin commands expose preview (clipboard) and write-to-directory. The frontend is a 4-step dialog in the Import File screen fed by existing discovery queries. A cargo round-trip test parses the guide's own worked example through `parse_json` so importer drift fails the gate.

**Tech Stack:** Rust (no new crates; `tempfile` is already a dev-dep — verify in `v2/src-tauri/Cargo.toml`, add under `[dev-dependencies]` if absent), tauri-specta bindings, React 19 + existing `ui/` primitives, TanStack Query, vitest + mockIPC.

**Spec:** `docs/superpowers/specs/2026-07-20-ai-guide-generator-design.md`

## Global Constraints

- All work under `v2/` on `master`; commit after each task (Bash heredoc `git commit -F - <<'EOF' … EOF`, confirm with `git log -1`).
- NO DELETE calls, no new ADO surface — the two new commands are pure/local-file-write only.
- Never hand-edit `v2/src/bindings.ts` — the dev server regenerates it after Rust command changes; if it isn't running, `cd v2 && npm run dev` briefly, or accept that vitest uses mockIPC and the real types appear on next dev-server run.
- If `cargo test` fails with "Access is denied (os error 5)": `Get-Process | Where-Object { $_.Path -like "*v2\src-tauri\target*" } | Stop-Process -Force` then re-run.
- Automation status values come from `model::VALID_STATUSES` — never string-literal them in the guide builder.
- Dev-only demo fakes live in `v2/src/dev/demo.ts` and must stay behind the existing compile-time DEV gate (just add entries to the existing object — the gating is already in place).
- Run gates before each commit that touches the respective side: `cd v2/src-tauri && cargo test` (Rust), `cd v2 && npx vitest run` (frontend).

---

### Task 1: Rust guide body builder (`build_guide_body`)

**Files:**
- Create: `v2/src-tauri/src/ai_guide.rs`
- Modify: `v2/src-tauri/src/lib.rs` (add `pub mod ai_guide;` to the module list, alphabetical — after `pub mod ado_testplan;`)

**Interfaces:**
- Produces: `GuideOptions` (specta::Type struct), `build_guide_body(&GuideOptions) -> String`, `WORKED_EXAMPLE_JSON: &str` — Tasks 2–4 consume these exact names.

- [ ] **Step 1: Write the failing tests** (bottom of the new `ai_guide.rs`, `#[cfg(test)] mod tests`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> GuideOptions {
        GuideOptions {
            organization: "acme".into(),
            project: "Web".into(),
            area: Some("Web\\Gamma Guardians".into()),
            modules: vec!["Login".into(), "Checkout".into()],
            tags: vec!["smoke".into(), "regression".into()],
            modules_discovered: true,
            doc_paths: vec!["docs/screens/**".into(), "README.md".into()],
            conventions: "Tag UI cases with 'ui'.".into(),
            flavors: vec![GuideFlavor::Generic],
            generated_on: "2026-07-20".into(),
        }
    }

    #[test]
    fn body_contains_fixed_sections_and_custom_values() {
        let body = build_guide_body(&opts());
        // Fixed layer
        assert!(body.contains("Test Case Manager"));
        assert!(body.contains("Import File"));           // workflow section
        assert!(body.contains("```json"));               // worked example fence
        assert!(body.contains("\"Not Automated\""));     // statuses from VALID_STATUSES
        assert!(body.contains("\"Planned\""));
        assert!(body.contains("semicolon"));             // tag separator rule
        // Custom layer
        assert!(body.contains("Login") && body.contains("Checkout"));
        assert!(body.contains("smoke"));
        assert!(body.contains("docs/screens/**"));
        assert!(body.contains("Tag UI cases with 'ui'."));
        assert!(body.contains("2026-07-20"));            // snapshot stamp
        assert!(body.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn empty_custom_inputs_omit_their_sections() {
        let mut o = opts();
        o.doc_paths.clear();
        o.conventions = String::new();
        o.modules.clear();
        o.modules_discovered = false;
        o.tags.clear();
        let body = build_guide_body(&o);
        assert!(!body.contains("## Repository documentation"));
        assert!(!body.contains("## Team conventions"));
        // Discovery failure => visible degradation note instead of a list
        assert!(body.contains("could not be discovered"));
    }

    #[test]
    fn body_is_deterministic() {
        assert_eq!(build_guide_body(&opts()), build_guide_body(&opts()));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd v2/src-tauri && cargo test ai_guide`
Expected: compile FAIL (`GuideOptions` not defined).

- [ ] **Step 3: Implement the module**

```rust
//! Generates the "AI test-case guide" — a markdown file developers drop
//! into their repo so AI tools author import-ready test-case JSON.
//! Lives beside `import_parser` deliberately: the guide documents that
//! parser, and `tests::worked_example_round_trips` (Task 2) fails the
//! gate if the two ever drift.

use crate::model::VALID_STATUSES;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, specta::Type)]
pub enum GuideFlavor {
    Generic,
    ClaudeSkill,
    CursorRules,
    AgentsSnippet,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct GuideOptions {
    pub organization: String,
    pub project: String,
    pub area: Option<String>,
    /// Pruned Module picklist (empty + !modules_discovered => discovery failed).
    pub modules: Vec<String>,
    pub tags: Vec<String>,
    /// False when picklist discovery failed — the guide then carries a
    /// visible degradation note instead of silently omitting the list.
    pub modules_discovered: bool,
    pub doc_paths: Vec<String>,
    pub conventions: String,
    pub flavors: Vec<GuideFlavor>,
    /// Stamp date, passed in by the frontend (keeps this function pure).
    pub generated_on: String,
}

/// The guide's worked example. Task 2's round-trip test feeds this exact
/// string through `import_parser::parse_json`, so it can never drift from
/// what the importer accepts.
pub const WORKED_EXAMPLE_JSON: &str = r#"[
  {
    "title": "Login - valid credentials reach the dashboard",
    "steps": [
      { "action": "Open the sign-in page", "expected": "Sign-in form is shown" },
      { "action": "Enter a valid username and password and submit", "expected": "The dashboard loads and shows the signed-in user's name" }
    ],
    "tags": "smoke; login",
    "automation_status": "Not Automated",
    "module": "Login",
    "preconditions": "A test account exists"
  },
  {
    "id": 143001,
    "title": "Checkout - expired card is rejected with a clear error",
    "steps": [
      { "action": "Pay with an expired card", "expected": "An 'expired card' error is shown; no order is created" }
    ],
    "tags": "regression; checkout",
    "automation_status": "Planned",
    "module": "Checkout",
    "preconditions": ""
  }
]"#;

pub fn build_guide_body(o: &GuideOptions) -> String {
    let mut s = String::with_capacity(8 * 1024);
    let statuses = VALID_STATUSES
        .iter()
        .map(|v| format!("\"{v}\""))
        .collect::<Vec<_>>()
        .join(" or ");

    // 1. What this file is + workflow
    s.push_str(&format!(
        "# AI guide: writing test cases for Test Case Manager\n\n\
        Instructions for AI tools generating Azure DevOps test cases for \
        **{org}/{proj}**{area}. Output a JSON file in the format below; a developer \
        imports it in Test Case Manager's **Import File** tab, reviews the queue, and \
        creates the cases in Azure DevOps.\n\n\
        > Generated by Test Case Manager v{ver} on {date}. Listed values are a \
        snapshot — regenerate this guide if project picklists change.\n\n",
        org = o.organization,
        proj = o.project,
        area = o
            .area
            .as_deref()
            .map(|a| format!(" (area `{a}`)"))
            .unwrap_or_default(),
        ver = env!("CARGO_PKG_VERSION"),
        date = o.generated_on,
    ));

    // 2. Output contract
    s.push_str(
        "## Output contract\n\n\
        Produce a single `.json` file containing only a JSON array of test cases - \
        no surrounding prose, no markdown fences.\n\n",
    );

    // 3. Schema + worked example
    s.push_str(&format!(
        "## JSON format\n\n\
        Each array element is one test case:\n\n\
        | Key | Type | Notes |\n|---|---|---|\n\
        | `title` | string | Required, max 255 chars. Aliases accepted: `name`, `test_case_name`. |\n\
        | `id` | number | OPTIONAL. When present, UPDATES that exact work item; omit (or null) to CREATE. Aliases: `test_case_id`, `work_item_id`. |\n\
        | `steps` | array | Required, at least one. Each item is `{{\"action\": ..., \"expected\": ...}}` (aliases: `step` for action; `expected_result`/`result` for expected) or a plain string (action only). |\n\
        | `tags` | string | Semicolon-separated. Commas are NOT allowed in tags. |\n\
        | `automation_status` | string | Exactly {statuses}. Anything else is replaced with \"Not Automated\" and flagged. |\n\
        | `module` | string | Alias: `module_value`. Only use the allowed values listed below. |\n\
        | `preconditions` | string | Setup/state, not steps. Alias: `prerequisites`. |\n\n\
        ### Worked example (one create, one update)\n\n```json\n{example}\n```\n\n",
        example = WORKED_EXAMPLE_JSON,
    ));

    // 4. Hard rules
    s.push_str(&format!(
        "## Hard rules\n\n\
        - `automation_status` MUST be exactly {statuses}.\n\
        - Every case MUST have a non-empty `title` (<= 255 chars) and at least one step with a non-empty action.\n\
        - Tags MUST be semicolon-separated - never commas.\n\
        - An `id` updates that exact work item. Matching by title NEVER updates - omit `id` to create.\n\
        - `module` MUST come from the allowed values below - do not invent new ones.\n\n",
    ));

    // 5. Allowed values (custom layer, with visible degradation)
    s.push_str("## Allowed values\n\n");
    if o.modules_discovered && !o.modules.is_empty() {
        s.push_str("**Module** (use exactly one of):\n");
        for m in &o.modules {
            s.push_str(&format!("- `{m}`\n"));
        }
        s.push('\n');
    } else {
        s.push_str(
            "Module values could not be discovered from Azure DevOps when this guide \
            was generated. Ask the developer for the allowed Module values before using any.\n\n",
        );
    }
    if !o.tags.is_empty() {
        s.push_str("**Tags in use** (prefer these; new tags are allowed):\n");
        for t in &o.tags {
            s.push_str(&format!("- `{t}`\n"));
        }
        s.push('\n');
    }

    // 6. Repo documentation (omitted when none given)
    if !o.doc_paths.is_empty() {
        s.push_str("## Repository documentation\n\nBefore writing cases, read:\n");
        for p in &o.doc_paths {
            s.push_str(&format!("- `{p}`\n"));
        }
        s.push('\n');
    }

    // 7. Team conventions (omitted when empty)
    if !o.conventions.trim().is_empty() {
        s.push_str(&format!("## Team conventions\n\n{}\n\n", o.conventions.trim()));
    }

    // 8. Craft guidance
    s.push_str(
        "## Writing good test cases\n\n\
        - One observable behavior per case; split compound scenarios.\n\
        - Actions are imperative and atomic (\"Click Save\"), expecteds are observable outcomes.\n\
        - Cover the negative and edge paths, not just the happy path.\n\
        - Preconditions describe required state, not actions to perform.\n",
    );
    s
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd v2/src-tauri && cargo test ai_guide`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add v2/src-tauri/src/ai_guide.rs v2/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(v2): AI guide body builder with fixed + custom layers

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 2: Schema-sync round-trip test (drift gate)

**Files:**
- Modify: `v2/src-tauri/src/ai_guide.rs` (append to `tests` module)

**Interfaces:**
- Consumes: `WORKED_EXAMPLE_JSON` (Task 1), `crate::import_parser::parse_import_file(path) -> Result<(Vec<TestCase>, Vec<String>), String>` — check the real public name first: `grep -n "pub fn" v2/src-tauri/src/import_parser/mod.rs`. If only a private `parse_json` exists behind a `parse_import_file(path)` dispatcher, call the dispatcher with a `.json` temp file (that IS the production path).

- [ ] **Step 1: Write the failing test**

```rust
    /// THE drift gate: the guide's own worked example must parse through
    /// the real importer. If the import format changes without updating
    /// the guide (or vice versa), this fails the build.
    #[test]
    fn worked_example_round_trips_through_the_importer() {
        let dir = std::env::temp_dir().join("tcm_ai_guide_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("example.json");
        std::fs::write(&path, WORKED_EXAMPLE_JSON).unwrap();

        let (cases, warnings) =
            crate::import_parser::parse_import_file(path.to_str().unwrap()).unwrap();

        assert_eq!(warnings, Vec::<String>::new(), "example must import warning-free");
        assert_eq!(cases.len(), 2);
        // Create vs update semantics
        assert_eq!(cases[0].update_id, None);
        assert_eq!(cases[1].update_id, Some(143001));
        // Fields survive the trip
        assert_eq!(cases[0].steps.len(), 2);
        assert_eq!(cases[0].steps[0].expected, "Sign-in form is shown");
        assert_eq!(cases[0].module_value, "Login");
        assert_eq!(cases[1].automation_status, "Planned");
        // Both must be submit-ready
        assert!(cases[0].is_valid().is_ok() && cases[1].is_valid().is_ok());
        let _ = std::fs::remove_file(&path);
    }
```

- [ ] **Step 2: Run it**

Run: `cd v2/src-tauri && cargo test worked_example`
Expected: PASS if Task 1's example is correct — but treat a first-run pass with suspicion: temporarily change `"Planned"` to `"Bogus"` in `WORKED_EXAMPLE_JSON`, re-run, confirm it FAILS (warning emitted), then revert. That proves the gate bites.

- [ ] **Step 3: Commit**

```bash
git add v2/src-tauri/src/ai_guide.rs
git commit -F - <<'EOF'
test(v2): AI guide worked example round-trips through the importer

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 3: Flavor wrappers

**Files:**
- Modify: `v2/src-tauri/src/ai_guide.rs`

**Interfaces:**
- Produces: `flavor_files(body: &str, flavors: &[GuideFlavor]) -> Vec<(String, String)>` — `(relative_path, content)` pairs. Task 4 consumes this exact signature.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn flavor_files_wrap_one_body_per_selected_flavor() {
        let body = "# body\ncontent";
        let all = [
            GuideFlavor::Generic,
            GuideFlavor::ClaudeSkill,
            GuideFlavor::CursorRules,
            GuideFlavor::AgentsSnippet,
        ];
        let files = flavor_files(body, &all);
        let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "AI_TEST_CASES.md",
                ".claude/skills/generate-test-cases/SKILL.md",
                ".cursor/rules/test-cases.mdc",
                "AGENTS-test-cases.md",
            ]
        );
        // Generic is the body verbatim; wrappers contain the body unchanged.
        assert_eq!(files[0].1, body);
        assert!(files[1].1.starts_with("---\nname: generate-test-cases\n"));
        assert!(files.iter().all(|(_, c)| c.contains("content")));
        // Cursor frontmatter + Agents append note
        assert!(files[2].1.starts_with("---\ndescription:"));
        assert!(files[3].1.contains("append"));
    }

    #[test]
    fn no_flavors_yields_no_files() {
        assert!(flavor_files("x", &[]).is_empty());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cd v2/src-tauri && cargo test flavor`
Expected: compile FAIL (`flavor_files` not defined).

- [ ] **Step 3: Implement**

```rust
/// Thin packaging around the single-source body — flavors never fork the
/// content, only wrap it. Returned paths are relative to the chosen repo root.
pub fn flavor_files(body: &str, flavors: &[GuideFlavor]) -> Vec<(String, String)> {
    flavors
        .iter()
        .map(|f| match f {
            GuideFlavor::Generic => ("AI_TEST_CASES.md".to_string(), body.to_string()),
            GuideFlavor::ClaudeSkill => (
                ".claude/skills/generate-test-cases/SKILL.md".to_string(),
                format!(
                    "---\nname: generate-test-cases\ndescription: Write Azure DevOps \
                    test cases as JSON importable by Test Case Manager. Use when asked \
                    to create, write, or generate test cases for this project.\n---\n\n{body}"
                ),
            ),
            GuideFlavor::CursorRules => (
                ".cursor/rules/test-cases.mdc".to_string(),
                format!(
                    "---\ndescription: Writing test cases importable by Test Case Manager\nalwaysApply: false\n---\n\n{body}"
                ),
            ),
            GuideFlavor::AgentsSnippet => (
                "AGENTS-test-cases.md".to_string(),
                format!(
                    "<!-- Snippet: append this section to your existing AGENTS.md -->\n\n\
                    ## Writing test cases for Test Case Manager\n\n{body}"
                ),
            ),
        })
        .collect()
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd v2/src-tauri && cargo test ai_guide`
Expected: all ai_guide tests pass (6 total).

- [ ] **Step 5: Commit**

```bash
git add v2/src-tauri/src/ai_guide.rs
git commit -F - <<'EOF'
feat(v2): AI guide flavor wrappers (generic, Claude skill, Cursor, AGENTS)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 4: Commands `preview_ai_guide` + `write_ai_guide`

**Files:**
- Create: `v2/src-tauri/src/commands/ai_guide.rs`
- Modify: `v2/src-tauri/src/commands/mod.rs` (add `pub mod ai_guide;` alphabetically — first entry, before `auth`)
- Modify: `v2/src-tauri/src/lib.rs` (import + register both commands at the END of `collect_commands![]` — appending keeps binding churn minimal)

**Interfaces:**
- Consumes: `ai_guide::{GuideOptions, build_guide_body, flavor_files}` (Tasks 1, 3).
- Produces commands (camelCase in bindings): `previewAiGuide(options) -> string`, `writeAiGuide(dir, options) -> string[]` (written relative paths). Task 5 consumes these names.

- [ ] **Step 1: Write the failing test** (in the new command file — the write logic is testable without Tauri because neither command touches AppHandle)

```rust
//! AI-guide generation commands. Pure/local-file-write only - no ADO calls,
//! no DELETE surface. Discovery values arrive from the frontend's existing
//! queries; these commands never touch the network.

use crate::ai_guide::{build_guide_body, flavor_files, GuideOptions};

#[tauri::command]
#[specta::specta]
pub fn preview_ai_guide(options: GuideOptions) -> String {
    build_guide_body(&options)
}

#[tauri::command]
#[specta::specta]
pub fn write_ai_guide(dir: String, options: GuideOptions) -> Result<Vec<String>, String> {
    let body = build_guide_body(&options);
    let files = flavor_files(&body, &options.flavors);
    if files.is_empty() {
        return Err("No output flavor selected.".into());
    }
    let base = std::path::Path::new(&dir);
    let mut written = Vec::with_capacity(files.len());
    for (rel, content) in files {
        let target = base.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, content)
            .map_err(|e| format!("Could not write {}: {e}", target.display()))?;
        written.push(rel);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_guide::GuideFlavor;

    fn opts(flavors: Vec<GuideFlavor>) -> GuideOptions {
        GuideOptions {
            organization: "acme".into(),
            project: "Web".into(),
            area: None,
            modules: vec!["Login".into()],
            tags: vec![],
            modules_discovered: true,
            doc_paths: vec![],
            conventions: String::new(),
            flavors,
            generated_on: "2026-07-20".into(),
        }
    }

    #[test]
    fn write_creates_nested_flavor_files() {
        let dir = std::env::temp_dir().join("tcm_ai_guide_write_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let written = write_ai_guide(
            dir.to_str().unwrap().into(),
            opts(vec![GuideFlavor::Generic, GuideFlavor::ClaudeSkill]),
        )
        .unwrap();

        assert_eq!(
            written,
            vec!["AI_TEST_CASES.md", ".claude/skills/generate-test-cases/SKILL.md"]
        );
        let skill = std::fs::read_to_string(
            dir.join(".claude/skills/generate-test-cases/SKILL.md"),
        )
        .unwrap();
        assert!(skill.contains("acme/Web"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_with_no_flavors_errors() {
        assert!(write_ai_guide("/tmp".into(), opts(vec![])).is_err());
    }

    #[test]
    fn preview_returns_the_generic_body() {
        assert!(preview_ai_guide(opts(vec![])).contains("# AI guide"));
    }
}
```

(Write test-first by adding the `tests` module with `use super::*;` before the functions exist if you want the strict red step; the file above is the end state.)

- [ ] **Step 2: Register in `lib.rs`** — add `ai_guide` to the `use commands::{...}` list inside `specta_builder()` and append to `collect_commands![]`:

```rust
            prs::pr_work_items,
            ai_guide::preview_ai_guide,
            ai_guide::write_ai_guide
```

- [ ] **Step 3: Run the full Rust gate**

Run: `cd v2/src-tauri && cargo test`
Expected: all tests pass (91 existing + 9 new = ~100). If v2.exe lock error, kill per Global Constraints.

- [ ] **Step 4: Regenerate bindings** — if the dev server is running it already rewrote `v2/src/bindings.ts`; verify `grep -n "previewAiGuide\|writeAiGuide" v2/src/bindings.ts` shows both. If not running, start `npm run dev` briefly. NEVER hand-edit.

- [ ] **Step 5: Commit**

```bash
git add v2/src-tauri/src/commands/ai_guide.rs v2/src-tauri/src/commands/mod.rs v2/src-tauri/src/lib.rs v2/src/bindings.ts
git commit -F - <<'EOF'
feat(v2): preview/write commands for the AI test-case guide

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 5: Wizard dialog component + demo fakes

**Files:**
- Create: `v2/src/components/AiGuideWizard.tsx`
- Create: `v2/src/components/AiGuideWizard.test.tsx`
- Modify: `v2/src/dev/demo.ts` (add `previewAiGuide` + `writeAiGuide` fakes next to `testCaseFieldValues`)

**Interfaces:**
- Consumes: `commands.previewAiGuide(options)`, `commands.writeAiGuide(dir, options)`, `commands.testCaseFieldValues(org, project, fieldRef)`, `commands.listProjectTags(org, project)` — via `unwrap` from `../lib/ipc`; `loadFieldPrefs(org, project)` from `../lib/fieldPrefs` for the module field ref; `open` (directory picker) from `@tauri-apps/plugin-dialog`; ui primitives `Button`, `Checkbox`, `Input` from `./ui/*`.
- Produces: `<AiGuideWizard org project area onClose />` — Task 6 consumes exactly these props (`area: string | null`).

- [ ] **Step 1: Write the failing tests**

```tsx
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AiGuideWizard from "./AiGuideWizard";

// The directory picker is a plugin call; mock the module, not IPC.
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => "C:/repo") }));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

function renderWizard(onClose = vi.fn()) {
  localStorage.setItem(
    "tcm-v2-fields:acme/Web",
    JSON.stringify({ moduleRef: "Custom.Module", preconditionsRef: null }),
  );
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AiGuideWizard org="acme" project="Web" area="Web\\Gamma" onClose={onClose} />
    </QueryClientProvider>,
  );
  return onClose;
}

test("step 1 shows discovered modules and tags as checked boxes; unticking prunes", async () => {
  mockIPC((cmd) => {
    if (cmd === "test_case_field_values") return ["Login", "Checkout"];
    if (cmd === "list_project_tags") return ["smoke"];
  });
  renderWizard();
  expect(await screen.findByLabelText("Login")).toBeChecked();
  expect(screen.getByLabelText("Checkout")).toBeChecked();
  expect(screen.getByLabelText("smoke")).toBeChecked();
  fireEvent.click(screen.getByLabelText("Checkout")); // prune it
  expect(screen.getByLabelText("Checkout")).not.toBeChecked();
});

test("discovery failure degrades with a note, not a blocked wizard", async () => {
  mockIPC((cmd) => {
    if (cmd === "test_case_field_values") throw "boom";
    if (cmd === "list_project_tags") return [];
  });
  renderWizard();
  expect(await screen.findByText(/could not be discovered/i)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
});

test("full walk-through: doc paths + flavors reach writeAiGuide with the pruned payload", async () => {
  let writeArgs: Record<string, unknown> | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "test_case_field_values") return ["Login", "Checkout"];
    if (cmd === "list_project_tags") return [];
    if (cmd === "write_ai_guide") {
      writeArgs = args as Record<string, unknown>;
      return ["AI_TEST_CASES.md"];
    }
  });
  const onClose = renderWizard();

  fireEvent.click(await screen.findByLabelText("Checkout")); // prune
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> repo knowledge
  fireEvent.change(screen.getByLabelText("Documentation paths"), {
    target: { value: "docs/screens/**\nREADME.md" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> flavor
  expect(screen.getByLabelText("Generic markdown (AI_TEST_CASES.md)")).toBeChecked();
  fireEvent.click(screen.getByLabelText("Claude Code skill"));
  fireEvent.click(screen.getByRole("button", { name: "Next" })); // -> output
  fireEvent.click(screen.getByRole("button", { name: "Save to folder…" }));

  await waitFor(() => expect(writeArgs).not.toBeNull());
  const opts = (writeArgs as { options: Record<string, unknown> }).options;
  expect(opts.modules).toEqual(["Login"]); // pruned
  expect(opts.docPaths ?? opts.doc_paths).toEqual(["docs/screens/**", "README.md"]);
  expect((opts.flavors as string[]).length).toBe(2);
  await screen.findByText(/AI_TEST_CASES\.md/); // success summary
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(onClose).toHaveBeenCalled();
});
```

- [ ] **Step 2: Run to verify failure**

Run: `cd v2 && npx vitest run src/components/AiGuideWizard.test.tsx`
Expected: FAIL (module not found).

- [ ] **Step 3: Implement the wizard.** Follow the house dialog style — read `v2/src/components/BulkEditDialog.tsx` first and reuse its overlay/panel classes verbatim. Component outline (adapt freely to house style; keep every labelled control name used by the tests):

```tsx
// 4-step wizard that generates the repo-droppable AI test-case guide.
// Discovery values arrive pre-fetched via React Query; the Rust side is
// pure generation + file writes (no network).
import { useQuery } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type GuideFlavor } from "../bindings";
import { loadFieldPrefs } from "../lib/fieldPrefs";
import { unwrap } from "../lib/ipc";
import { Button } from "./ui/button";
import { Checkbox } from "./ui/checkbox";

const FLAVORS: { id: GuideFlavor; label: string }[] = [
  { id: "Generic", label: "Generic markdown (AI_TEST_CASES.md)" },
  { id: "ClaudeSkill", label: "Claude Code skill" },
  { id: "CursorRules", label: "Cursor rules" },
  { id: "AgentsSnippet", label: "AGENTS.md snippet" },
];

export default function AiGuideWizard({
  org, project, area, onClose,
}: { org: string; project: string; area: string | null; onClose: () => void }) {
  const [step, setStep] = useState(0);
  const moduleRef = loadFieldPrefs(org, project)?.moduleRef ?? null;

  const modules = useQuery({
    queryKey: ["ai-guide-modules", org, project, moduleRef],
    queryFn: () => unwrap(commands.testCaseFieldValues(org, project, moduleRef!)),
    enabled: Boolean(moduleRef), retry: false,
  });
  const tags = useQuery({
    queryKey: ["ai-guide-tags", org, project],
    queryFn: () => unwrap(commands.listProjectTags(org, project)),
    retry: false,
  });

  // Pruning state: checked-by-default once data arrives.
  const [pruned, setPruned] = useState<Set<string>>(new Set());
  const [prunedTags, setPrunedTags] = useState<Set<string>>(new Set());
  const [docPathsText, setDocPathsText] = useState("");
  const [conventions, setConventions] = useState("");
  const [flavors, setFlavors] = useState<Set<GuideFlavor>>(new Set(["Generic"]));
  const [written, setWritten] = useState<string[] | null>(null);

  const modulesDiscovered = modules.isSuccess && Boolean(moduleRef);
  const buildOptions = () => ({
    organization: org, project, area,
    modules: (modules.data ?? []).filter((m) => !pruned.has(m)),
    tags: (tags.data ?? []).filter((t) => !prunedTags.has(t)),
    modules_discovered: modulesDiscovered,
    doc_paths: docPathsText.split("\n").map((s) => s.trim()).filter(Boolean),
    conventions,
    flavors: [...flavors],
    generated_on: new Date().toISOString().slice(0, 10),
  });

  const saveToFolder = async () => {
    if (flavors.size === 0) { toast.error("Pick at least one flavor."); return; }
    const dir = await open({ directory: true });
    if (typeof dir !== "string") return;
    try {
      setWritten(await unwrap(commands.writeAiGuide(dir, buildOptions())));
      toast.success("Guide written.");
    } catch (e) { toast.error(`Could not write the guide: ${(e as Error).message}`); }
  };
  const copyBody = async () => {
    const body = await unwrap(commands.previewAiGuide(buildOptions()));
    await navigator.clipboard.writeText(body);
    toast.success("Copied the guide to the clipboard.");
  };
  // ...render: overlay + panel (BulkEditDialog classes); step content per
  // `step`; step 0 = context summary + module/tag checkboxes (label = value,
  // aria via <label>) with the "could not be discovered" note when
  // !moduleRef || modules.isError; step 1 = <textarea aria-label="Documentation paths">
  // + <textarea aria-label="Team conventions">; step 2 = FLAVORS checkboxes;
  // step 3 = "Save to folder…" + "Copy to clipboard" buttons, then a written-files
  // summary list and a "Done" button calling onClose. Back/Next buttons
  // navigate; Next is always enabled (every step is optional).
}
```

Note: if the regenerated binding for `GuideFlavor` serializes differently (e.g. object enum), match what `bindings.ts` actually emitted — check it before writing the FLAVORS array. Field-name casing (`doc_paths` vs `docPaths`) must also match the generated `GuideOptions` type — the test's `??` fallback covers reading both, but the component must compile against the real type.

- [ ] **Step 4: Add demo fakes** in `v2/src/dev/demo.ts` next to `listProjectTags`:

```ts
    previewAiGuide: () => ok("# AI guide: writing test cases for Test Case Manager\n(demo preview)"),
    writeAiGuide: () => ok(["AI_TEST_CASES.md"]),
```

(Match the file's existing fake signature style — look at the neighbors and copy their shape exactly.)

- [ ] **Step 5: Run to verify pass**

Run: `cd v2 && npx vitest run src/components/AiGuideWizard.test.tsx`
Expected: 3 passed.

- [ ] **Step 6: Commit**

```bash
git add v2/src/components/AiGuideWizard.tsx v2/src/components/AiGuideWizard.test.tsx v2/src/dev/demo.ts
git commit -F - <<'EOF'
feat(v2): AI-guide wizard dialog (context, repo docs, flavors, output)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
git log -1
```

---

### Task 6: Import-tab entry point + full gates

**Files:**
- Modify: `v2/src/screens/ImportFile.tsx` (button + wizard mount)
- Modify: `v2/src/screens/ImportFile.test.tsx` (new test)

**Interfaces:**
- Consumes: `<AiGuideWizard org project area onClose />` (Task 5). `ImportFile` does not currently receive `area` — pass `area={null}` (the wizard treats it as optional); if `App.tsx` has the area handy near the ImportFile mount, thread it through instead, but do NOT widen other screens' props for this.

- [ ] **Step 1: Write the failing test** (append to `ImportFile.test.tsx`, reusing its existing render helper/mocks — read the file first and follow its conventions)

```tsx
test("Generate AI guide opens the wizard", async () => {
  mockIPC((cmd) => {
    if (cmd === "test_case_field_values") return [];
    if (cmd === "list_project_tags") return [];
  });
  renderImport(); // the file's existing helper with a picked PBI
  fireEvent.click(screen.getByRole("button", { name: "Generate AI guide…" }));
  expect(await screen.findByText(/AI guide/)).toBeInTheDocument();
});
```

- [ ] **Step 2: Wire the button** — in `ImportFile.tsx`, add state + button beside "Import JSON":

```tsx
const [guideOpen, setGuideOpen] = useState(false);
// in the buttons row:
<Button variant="outline" onClick={() => setGuideOpen(true)}>Generate AI guide…</Button>
// after the section:
{guideOpen && (
  <AiGuideWizard org={org} project={project} area={null} onClose={() => setGuideOpen(false)} />
)}
```

(Check `ui/button.tsx` for the actual secondary variant name — use whatever the codebase's quiet-button convention is.)

- [ ] **Step 3: Run the full gates**

Run: `cd v2/src-tauri && cargo test` → all pass.
Run: `cd v2 && npx vitest run` → all pass (106 existing + new).
Run: `cd v2 && npm run build` → clean; then confirm dev-code elimination per the session's usual check (demo fakes absent from the production bundle).

- [ ] **Step 4: Manual smoke (dev mode)** — `npm run dev`, skip sign-in, Import tab → Generate AI guide… → walk the 4 steps with demo data → Save to folder into the scratchpad dir → open the written `AI_TEST_CASES.md` and eyeball all 8 sections.

- [ ] **Step 5: Commit**

```bash
git add v2/src/screens/ImportFile.tsx v2/src/screens/ImportFile.test.tsx
git commit -F - <<'EOF'
feat(v2): "Generate AI guide" entry point in the Import File tab

Wizard generates a repo-droppable markdown guide (generic / Claude skill /
Cursor rules / AGENTS snippet) teaching AI tools the JSON import format,
with ADO-discovered picklists and repo doc pointers baked in.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
EOF
git log -1
```
