# Findings In The File Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the 1.23.17 findings feature (a local store, two MCP tools, a card, a bell entry, a report section fed from the store) with findings that live inside each test case in the draft `.json`, round-trip through import and export like `comment` and `reviewer_notes`, never reach Azure DevOps, and are shown only in the browser view of the cases, in a block of their own under the case.

**Architecture:** `TestCase` gains `findings: Vec<CaseFinding>` (`kind`, `subject`, `title`, `detail`), `#[serde(default, skip_serializing_if = "Vec::is_empty")]` so the generated TypeScript field is optional and no frontend literal changes. The importer reads a case's `findings` list, the exporter writes it back when non-empty, the request-body guard test forbids it from `ado/endpoints.rs`, the writing guide tells the assistant to put a problem there, and the browser page renders the list under the case. Everything store-based from 1.23.17 is removed: `findings.rs`, its commands and event, the bridge routes, the two MCP tools, the frontend card, the bell kind and the App listener, the backup root, and the report's store-fed section and its extra command parameters.

**Tech Stack:** Rust (Tauri 2, tauri-specta, serde), React 19 + TypeScript, vitest, Rust integration tests under `src-tauri/tests/`.

## Global Constraints

- All Rust tests are integration tests under `src-tauri/tests/`; never a `#[cfg(test)]` module inside `src/`.
- `src/bindings.ts` is generated: run `cargo test --test bindings` from `src-tauri/` with `$env:CARGO_TARGET_DIR="target/gate"` after any command, event or IPC-type change. Never hand-edit it.
- Run one build or test command at a time on this shared machine. Full gates: `cargo test --tests` (in `src-tauri/`), `npx tsc --noEmit`, `npx vitest run` (repo root).
- No DELETE to Azure DevOps anywhere. `findings`, like `comment` and `reviewer_notes`, must never appear in `src-tauri/src/ado/endpoints.rs`; `tests/ado.rs::app_only_fields_never_reach_a_request_body` enforces it and must be extended.
- The `comment` field on a test case is the developer's; no tool or guide instruction writes it. `reviewer_notes` carries only provenance. A problem the assistant finds goes into the case's `findings` list.
- Findings are not a feature inside the app's screens: no card, no bell entry, no chip on the queue row, no switch. The only place they render is the browser page.
- The MCP tool list order is asserted verbatim in `src-tauri/tests/tcm_mcp.rs`; the slash-command stems in `src-tauri/tests/ai_tools.rs`; `CORE_TOOLS` is mirrored in `src/lib/mcpTools.ts` and a test compares the two files. Update each in the task that changes it.
- Changelog entries are end-user-facing: no file paths, no test names, no process notes.
- Commits use a Bash heredoc `git commit -q -F - <<'EOF' … EOF` ending with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- No em dashes in text an assistant or user reads.

---

## File map

| File | Responsibility |
| --- | --- |
| `src-tauri/src/model.rs` (modify) | `CaseFinding` type; `TestCase.findings`. |
| `src-tauri/src/import_parser/mod.rs`, `export.rs` (modify) | Parse and write `findings`; the export instructions name the field. |
| `src-tauri/src/import_parser/html.rs`, `src-tauri/web/cases-page.css` (modify) | Per-case findings block in the browser page; the store-fed section removed. |
| `src-tauri/src/ai_bridge.rs` (modify) | Routes removed; guide and validator point at the case's `findings` list. |
| `src-tauri/src/mcp.rs`, `src-tauri/src/ai_tools.rs` (modify) | The two tools and their core entries removed. |
| `src-tauri/src/commands/queue.rs` (modify) | The extra `organization`/`project` parameters and `open_findings` removed; callers pass no findings. |
| `src-tauri/src/lib.rs`, `events.rs`, `backup.rs`, `commands/mod.rs` (modify) | Module, event, commands and backup root removed. |
| `src-tauri/src/findings.rs`, `src-tauri/src/commands/findings.rs` (delete) | Gone. |
| `src-tauri/tests/findings.rs`, `findings_bridge.rs`, `findings_report.rs` (delete); `ado.rs`, `import_parser.rs`, `ai_bridge.rs`, `tcm_mcp.rs`, `ai_tools.rs`, `transform.rs`, `draft_comments.rs` (modify) | Tests follow. |
| `src/components/FindingsCard.tsx` + test (delete); `src/screens/AiBridge.tsx` + test, `src/App.tsx`, `src/lib/notifications.ts` + test, `src/components/NotificationBell.tsx`, `src/lib/mcpTools.ts` + test, `src/screens/Suites.tsx`, `src/screens/ViewCases/index.tsx`, `src/components/QueueSection.tsx` (modify) | Frontend pieces removed; callers back to the old command signatures. |
| `src/lib/changelog.ts` (at ship time) | One end-user entry. |

---

### Task 1: Remove the store-based feature

**Files:** everything in the file map marked delete or "removed"; regenerate `src/bindings.ts`.

**Interfaces:**
- Produces: the tree as it was before 1.23.17 for findings, while KEEPING these 1.23.17 changes untouched: the `set_comment` op removal and `transform::SUPPORTED_OPS` (transform.rs, mcp.rs description, their tests), the trimmed slash commands and the stale-file sweep (ai_tools.rs, commands/ai_tools.rs, their tests), the validator's two advisories in `ai_bridge.rs` (their wording changes in Task 3), and the guide's `comment` / `reviewer_notes` wording (its findings paragraph changes in Task 3).

- [ ] **Step 1: Delete and unwire, in this order**

Rust:
- Delete `src-tauri/src/findings.rs`, `src-tauri/src/commands/findings.rs`, `src-tauri/tests/findings.rs`, `src-tauri/tests/findings_bridge.rs`, `src-tauri/tests/findings_report.rs`.
- `src-tauri/src/lib.rs`: remove `pub mod findings;`, the two `findings::set_root(...)` / `findings::set_app_handle(...)` lines in setup, `events::FindingRecorded` from the events list, and the three `findings::*` commands from the command list.
- `src-tauri/src/commands/mod.rs`: remove `pub mod findings;`.
- `src-tauri/src/events.rs`: remove `FindingRecorded`.
- `src-tauri/src/backup.rs`: `ROOTS` back to the three original entries; drop "AI findings" from the module comment.
- `src-tauri/src/ai_bridge.rs`: remove the `("POST", "/findings")` and `("GET", "/findings")` arms and the functions `findings_root`, `record_finding`, `list_findings`. Leave the guide text and the validator advisories for Task 3.
- `src-tauri/src/mcp.rs`: remove the `record_finding` and `list_findings` entries from `tools_list` and their arms in `tools_call`. In the `record_finding`-free description of `transform_cases`, leave the sentence "There is no op for `comment`: it is the developer's field and is never written by an assistant." as it is.
- `src-tauri/src/ai_tools.rs`: remove `"record_finding"` and `"list_findings"` (and their comment) from `CORE_TOOLS`.
- `src-tauri/src/import_parser/html.rs`: remove the sixth `findings` parameter of `export_queue_to_html` and the whole store-fed `<section class='findings'>` block; `src-tauri/web/cases-page.css`: remove the `.findings` / `.finding` rules added in 1.23.17 (Task 4 adds new ones).
- `src-tauri/src/commands/queue.rs`: remove `open_findings`; `export_queue_html`, `view_queue_html`, `refresh_queue_html`, `view_draft_html`, `refresh_draft_html` pass no findings; remove the `project` parameter from `view_queue_html` / `refresh_queue_html` and the `organization, project` parameters from `view_draft_html` / `refresh_draft_html` (back to the 1.23.16 signatures).
- Tests: `src-tauri/tests/tcm_mcp.rs` (tool list back to 15 names, the count in `an_unreachable_bridge_disables_nothing` back to 15, delete `the_transform_description_names_every_supported_op_and_nothing_else`? NO, keep it; delete `record_finding_posts_the_body_and_list_findings_passes_status` and `the_finding_tools_are_core`); `src-tauri/tests/ai_tools.rs` (delete `the_finding_tools_are_always_on`; the `CORE_TOOLS` exact-array assertion back to eight names); `src-tauri/tests/ai_bridge.rs` (delete the `## Findings` section assertions in the guide test, the two `record_finding`-naming `notes` assertions, and the `validate_advises_when_the_human_fields_carry_the_assistants_words` test - Task 3 re-adds a version); `src-tauri/tests/import_parser.rs` and `draft_comments.rs` (calls back to five arguments).

Frontend:
- Delete `src/components/FindingsCard.tsx` and `src/components/FindingsCard.test.tsx`.
- `src/screens/AiBridge.tsx`: remove the import and the `<FindingsCard .../>`; the `org`/`project` props may stay (harmless) or go; `src/screens/AiBridge.test.tsx`: delete `the findings card is on the tab`.
- `src/App.tsx`: remove the `findingRecorded` listener effect and the `noteFinding` import; render `<AiBridge />` again if the props were removed.
- `src/lib/notifications.ts`: remove `"ai-finding"` from `NotificationKind` and `noteFinding`; `src/lib/notifications.test.ts`: delete its test. `src/components/NotificationBell.tsx`: remove the `ai-finding` entries from the two kind maps.
- `src/lib/mcpTools.ts`: remove the two `MCP_TOOLS` entries and the two `CORE_TOOLS` names (and their comment); `src/lib/mcpTools.test.ts`: delete `the finding tools are core and not listed as switches`.
- `src/screens/Suites.tsx`, `src/screens/ViewCases/index.tsx`, `src/components/QueueSection.tsx`: callers of `viewQueueHtml` / `viewDraftHtml` back to the 1.23.16 argument lists (drop the project / org+project arguments).

- [ ] **Step 2: Regenerate bindings and run the gates**

From `src-tauri/`: `cargo test --test bindings` (the `Finding` type, the three commands and the event must disappear from `src/bindings.ts`; the two command signatures shrink). Then `cargo test --tests`. Then from the repo root `npx tsc --noEmit`, then `npx vitest run`. All green before the commit. Grep the whole tree for `record_finding`, `list_findings`, `FindingRecorded`, `findings.rs`, `FindingsCard`, `noteFinding`, `ai-finding`, `open_findings`: only `src/lib/changelog.ts` may still mention the words (history), plus the plan files under `docs/`.

- [ ] **Step 3: Commit**

```bash
git commit -q -F - <<'EOF'
refactor(v2): findings leave the app - store, tools, card, bell and report section removed

The 1.23.17 findings feature kept a local store with its own tools and
screen. Findings belong in the draft file beside the case they are about
and only need showing in the browser page; the next commits put them
there.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 2: `findings` on the case: model, import, export, guard

**Files:**
- Modify: `src-tauri/src/model.rs`, `src-tauri/src/import_parser/mod.rs`, `src-tauri/src/import_parser/export.rs`, `src-tauri/src/transform.rs:456` (the `TestCase { ... }` literal in `insert_cases`), `src-tauri/src/import_parser/mod.rs:283` (the other literal), `src-tauri/src/branchcheck.rs` and `src-tauri/src/speccov.rs` test-helper literals if they do not use `..Default::default()`.
- Test: `src-tauri/tests/import_parser.rs`, `src-tauri/tests/ado.rs`, `src-tauri/tests/transform.rs`, then `cargo test --test bindings`.

**Interfaces:**
- Produces:
  ```rust
  pub const FINDING_KINDS: [&str; 3] = ["test_case", "spec", "code"];
  #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
  pub struct CaseFinding {
      /// One of FINDING_KINDS.
      pub kind: String,
      /// What it is about: the spec file and section, the code symbol, or empty for the case itself.
      #[serde(default, skip_serializing_if = "String::is_empty")]
      pub subject: String,
      pub title: String,
      /// Markdown.
      #[serde(default, skip_serializing_if = "String::is_empty")]
      pub detail: String,
  }
  // on TestCase:
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub findings: Vec<CaseFinding>,
  ```
  JSON shape on a case: `"findings": [{ "kind": "spec", "subject": "S.md 7.7", "title": "...", "detail": "..." }]`. A bare string entry is read as `{kind: "test_case", title: <string>}`.

- [ ] **Step 1: Write the failing tests**

In `src-tauri/tests/import_parser.rs` (find the existing round-trip test that imports a JSON string with `comment` and `reviewer_notes` and re-exports it; add beside it):

```rust
/// A problem an assistant found travels with its case: read from the
/// file, written back on export, kind validated, bare strings allowed.
#[test]
fn findings_round_trip_with_the_case_and_bad_kinds_warn() {
    let json = serde_json::json!({
        "test_cases": [{
            "title": "Cut-off closes the order",
            "automation_status": "Not Automated",
            "steps": [{ "action": "Open the page.", "expected": "Closed." }],
            "findings": [
                { "kind": "spec", "subject": "Orders.md 7.7", "title": "AC-3 contradicts the table", "detail": "Table says **closed**." },
                "Step 3 expects a toast the spec never mentions",
                { "kind": "vibes", "title": "Not a kind" }
            ]
        }]
    })
    .to_string();
    let (cases, warnings) = v2_lib::import_parser::parse_json_str(&json).unwrap();
    assert_eq!(cases.len(), 1);
    let f = &cases[0].findings;
    assert_eq!(f.len(), 2, "the bad kind is dropped: {f:?}");
    assert_eq!(f[0].kind, "spec");
    assert_eq!(f[0].subject, "Orders.md 7.7");
    assert_eq!(f[0].detail, "Table says **closed**.");
    assert_eq!(f[1].kind, "test_case", "a bare string is a finding about the case itself");
    assert_eq!(f[1].title, "Step 3 expects a toast the spec never mentions");
    assert!(warnings.iter().any(|w| w.contains("findings") && w.contains("vibes")), "{warnings:?}");

    let out = v2_lib::import_parser::queue_to_json_string(&cases).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let back = &v["test_cases"][0]["findings"];
    assert_eq!(back.as_array().unwrap().len(), 2);
    assert_eq!(back[0]["subject"], "Orders.md 7.7");
    assert!(back[1].get("subject").is_none(), "empty subject is not written");

    // A case with no findings writes no key.
    let plain = v2_lib::model::TestCase { title: "P".into(), steps: cases[0].steps.clone(), automation_status: "Planned".into(), ..Default::default() };
    let out = v2_lib::import_parser::queue_to_json_string(&[plain]).unwrap();
    assert!(!out.contains("findings"));
}
```

(`parse_json_str` stands for whatever this test file already calls to parse a JSON string into `(Vec<TestCase>, Vec<String>)`; use that name.)

In `src-tauri/tests/ado.rs::app_only_fields_never_reach_a_request_body`, after the `reviewer_notes` assertion add:

```rust
    assert!(!src.contains("findings"), "findings must not appear in request-building code");
```

In `src-tauri/tests/transform.rs` add:

```rust
/// Findings ride along: an op that rewrites a case leaves its findings as
/// they were, and none can write them.
#[test]
fn transforms_carry_findings_through_unchanged() {
    let mut c = noted("A", "n");
    c.findings = vec![v2_lib::model::CaseFinding { kind: "code".into(), subject: "Index.cs".into(), title: "Null check".into(), detail: String::new() }];
    let ops = parse_ops(&serde_json::json!([{ "op": "prefix_title", "value": "X " }])).unwrap();
    let (out, _) = apply(vec![c], &ops);
    assert_eq!(out[0].title, "X A");
    assert_eq!(out[0].findings.len(), 1);
    assert_eq!(out[0].findings[0].title, "Null check");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

`cargo test --test import_parser findings_round_trip` → compile error (`findings` field missing).

- [ ] **Step 3: Implement**

`src-tauri/src/model.rs`: add `FINDING_KINDS` and `CaseFinding` (the exact code above) and the `findings` field on `TestCase` with this doc comment: "Problems an assistant found while writing this case: in the spec, the code, or the case itself. Lives in the draft file, shown only in the browser page, never sent to Azure DevOps, never written by a transform. `comment` stays the developer's."

`src-tauri/src/import_parser/mod.rs`, in the per-case parse after `tester_order`:

```rust
        // Findings: what the assistant found wrong while writing this case.
        // Objects carry kind/subject/title/detail; a bare string is a
        // finding about the case itself. A kind outside the three is a
        // typo, not a new category - warn and drop that entry.
        let mut findings = vec![];
        if let Some(list) = obj.get("findings").and_then(|v| v.as_array()) {
            for (k, rf) in list.iter().enumerate() {
                let (kind, subject, title, detail) = match rf {
                    serde_json::Value::String(s) => ("test_case".to_string(), String::new(), s.trim().to_string(), String::new()),
                    serde_json::Value::Object(_) => (
                        json_value(rf, &["kind"]).map(value_to_string).unwrap_or_else(|| "test_case".into()).trim().to_string(),
                        json_value(rf, &["subject"]).map(value_to_string).unwrap_or_default().trim().to_string(),
                        json_value(rf, &["title"]).map(value_to_string).unwrap_or_default().trim().to_string(),
                        json_value(rf, &["detail"]).map(value_to_string).unwrap_or_default().trim().to_string(),
                    ),
                    _ => {
                        warnings.push(format!("{label} ('{title}') findings entry {}: expected an object or string - skipped.", k + 1));
                        continue;
                    }
                };
                if !crate::model::FINDING_KINDS.contains(&kind.as_str()) {
                    warnings.push(format!(
                        "{label} ('{title}') findings entry {}: kind '{kind}' is not test_case, spec or code - skipped.",
                        k + 1
                    ));
                    continue;
                }
                if title.is_empty() {
                    warnings.push(format!("{label} ('{title}') findings entry {}: has no title - skipped.", k + 1));
                    continue;
                }
                findings.push(crate::model::CaseFinding { kind, subject, title, detail });
            }
        } else if obj.get("findings").is_some_and(|v| !v.is_null()) {
            warnings.push(format!("{label} ('{title}'): 'findings' must be a list - ignored."));
        }
```

(The inner `title` shadows the case title inside the tuple binding; rename the finding's to `ftitle` if the compiler or the warning text gets confused.) Add `findings` to the `TestCase { ... }` literal at the end of the function. Do the same for the literal near line 283 (`findings: vec![]`), and for `transform.rs:456` (`findings: vec![]` - an inserted case starts with none). Rust test-helper literals in `branchcheck.rs` / `speccov.rs` use `..Default::default()` or gain `findings: vec![]`.

`src-tauri/src/import_parser/export.rs`: in `queue_to_json_string` after the `reviewer_notes` block:

```rust
            if !tc.findings.is_empty() {
                rec["findings"] = serde_json::json!(tc.findings);
            }
```

and extend `AI_INSTRUCTIONS` with: ` A third file-only field, 'findings', is a list of problems found while writing THIS case: each entry has 'kind' (test_case, spec or code), an optional 'subject' (the spec section or code symbol), a one-line 'title' and an optional markdown 'detail'. Put a contradiction between the spec and the code here, never in 'comment' (the developer's field) and never in 'reviewer_notes'.`

- [ ] **Step 4: Run the tests and regenerate bindings**

`cargo test --test import_parser --test ado --test transform`, then `cargo test --test bindings` (expect `findings?: CaseFinding[]` on `TestCase` and a `CaseFinding` type in `src/bindings.ts`), then `npx tsc --noEmit`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/model.rs src-tauri/src/import_parser/mod.rs src-tauri/src/import_parser/export.rs src-tauri/src/transform.rs src-tauri/src/branchcheck.rs src-tauri/src/speccov.rs src-tauri/tests/import_parser.rs src-tauri/tests/ado.rs src-tauri/tests/transform.rs src/bindings.ts
git commit -q -F - <<'EOF'
feat(v2): a test case carries its findings in the draft file

kind, subject, title, detail per finding; read on import, written on
export when present, never sent to Azure DevOps, untouched by transforms.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 3: The guide and the validator point at the case's findings

**Files:**
- Modify: `src-tauri/src/ai_bridge.rs` (guide `## Findings` paragraph; the `reviewer_notes` "Leave OUT" bullet; the two validator advisories; a validator warning for a bad `findings` shape is already the importer's)
- Test: `src-tauri/tests/ai_bridge.rs`

**Interfaces:**
- Produces: guide text naming the `findings` list; advisories naming it.

- [ ] **Step 1: Write the failing tests**

In `src-tauri/tests/ai_bridge.rs::guide_carries_format_rules_and_live_modules` add:

```rust
    let findings = body
        .split("## Findings")
        .nth(1)
        .and_then(|rest| rest.split("## reviewer_notes").next())
        .expect("the guide has a findings section");
    assert!(findings.contains("`findings`"), "{findings}");
    assert!(findings.contains("test_case, spec or code"), "{findings}");
    assert!(findings.contains("never write `comment`"), "{findings}");
    assert!(!findings.contains("record_finding"), "the tool is gone: {findings}");
    let notes = body.split("## reviewer_notes").nth(1).and_then(|r| r.split("## One branch per case").next()).unwrap();
    assert!(notes.contains("`findings`"), "a problem in a note is redirected to the case's findings: {notes}");
```

And re-add the validator test, with the expectations changed to the new wording:

```rust
#[tokio::test]
async fn validate_advises_when_the_human_fields_carry_the_assistants_words() {
    let draft = serde_json::json!({
        "test_cases": [
            { "title": "New case with a comment", "automation_status": "Not Automated",
              "comment": "Spec and code disagree here",
              "steps": [{ "action": "Open the page.", "expected": "It opens." }] },
            { "id": 155170, "title": "Existing case with the developer's comment", "automation_status": "Not Automated",
              "comment": "Blocked until the API lands",
              "steps": [{ "action": "Open the page.", "expected": "It opens." }] },
            { "title": "Note that reports a problem", "automation_status": "Not Automated",
              "reviewer_notes": "Checks the cut-off. Spec: S.md 7.7\n> \"closed at cut-off\"\nNote: the code contradicts the spec here.",
              "steps": [{ "action": "Open the page.", "expected": "It opens." }] }
        ]
    })
    .to_string();
    let (status, body) = route(&ctx(), None, "POST", "/validate", &draft, "1.10.3").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let adv: Vec<String> = v["advisories"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect();
    assert!(adv.iter().any(|a| a.contains("Test case 1") && a.contains("comment") && a.contains("`findings`")), "{adv:?}");
    assert!(!adv.iter().any(|a| a.contains("Test case 2") && a.contains("comment")), "{adv:?}");
    assert!(adv.iter().any(|a| a.contains("Test case 3") && a.contains("reviewer_notes") && a.contains("`findings`")), "{adv:?}");
}
```

- [ ] **Step 2: Run to verify they fail** — `cargo test --test ai_bridge guide_carries validate_advises`.

- [ ] **Step 3: Implement**

Guide, the `## Findings` section (replace the whole section written in 1.23.17):

```text
        ## Findings\n\
        When something you read is WRONG - a case that contradicts its spec,\n\
        a spec that contradicts itself, code that does what neither says -\n\
        put it in that case's `findings` list in the file: `{{\"kind\":\n\
        \"test_case\"|\"spec\"|\"code\", \"subject\": \"<spec section or code\n\
        symbol>\", \"title\": \"<one line>\", \"detail\": \"<markdown>\"}}`. A\n\
        finding about the spec or the code goes on the case it affects; if\n\
        several, on the first. The developer reads findings in the browser\n\
        page under each case. Do this on your own when it applies; nobody\n\
        will ask you to. And never write `comment` for this or for anything\n\
        else, and never put it in reviewer_notes: the first is the\n\
        developer's field, the second says where a case came from and\n\
        nothing more. Do not write a case around a defect as if the defect\n\
        were the requirement - record the finding and say so in the\n\
        conversation.\n\n\
```

(kinds sentence must contain the literal text `test_case, spec or code` somewhere, e.g. "`kind` is one of test_case, spec or code.") The `reviewer_notes` "Leave OUT" bullet: replace "call `record_finding` and keep the note..." with "put it in the case's `findings` list and keep the note to what the case checks and where its requirement lives."

Validator advisories: replace "call record_finding" in both with "put it in the case's `findings` list" (comment advisory: "If this is a problem you found, move it to the case's `findings` list."; notes advisory: "a problem is a finding - move it to the case's `findings` list and take it out of the note.").

- [ ] **Step 4: Run** `cargo test --test ai_bridge`, then commit:

```bash
git add src-tauri/src/ai_bridge.rs src-tauri/tests/ai_bridge.rs
git commit -q -F - <<'EOF'
feat(v2): the guide and the validator send problems to the case's findings list

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 4: The browser page shows a case's findings

**Files:**
- Modify: `src-tauri/src/import_parser/html.rs` (after the reviewer-notes block, before the steps table), `src-tauri/web/cases-page.css`
- Test: `src-tauri/tests/import_parser.rs`

- [ ] **Step 1: Write the failing test**

```rust
/// Findings render under their case in a block of their own, kind first,
/// detail as markdown with raw HTML dropped; a case without any shows
/// nothing extra.
#[test]
fn a_cases_findings_render_in_their_own_block() {
    let mut with = case_with_steps("Cut-off closes the order");
    with.findings = vec![
        v2_lib::model::CaseFinding { kind: "spec".into(), subject: "Orders.md 7.7".into(), title: "AC-3 contradicts the table".into(), detail: "Table says **closed**. <img src=x onerror=alert(1)>".into() },
        v2_lib::model::CaseFinding { kind: "test_case".into(), subject: String::new(), title: "Step 3 expects a toast".into(), detail: String::new() },
    ];
    let without = case_with_steps("Plain");
    let path = std::env::temp_dir().join(format!("tcm-findings-page-{}.html", std::process::id()));
    v2_lib::import_parser::export_queue_to_html(&[with, without], path.to_str().unwrap(), "", None, &Default::default()).unwrap();
    let html = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(html.matches("<details class='findings'").count(), 1, "one block, on the one case that has findings");
    let block = html.split("<details class='findings'").nth(1).unwrap().split("</details>").next().unwrap();
    assert!(block.contains("Findings (2)"), "{block}");
    assert!(block.contains("Spec") && block.contains("Orders.md 7.7") && block.contains("AC-3 contradicts the table"), "{block}");
    assert!(block.contains("<strong>closed</strong>"), "detail is markdown: {block}");
    assert!(!block.contains("<img") && !block.contains("onerror"), "raw HTML never reaches the page: {block}");
    assert!(block.contains("Test case") && block.contains("Step 3 expects a toast"), "{block}");
}
```

(`case_with_steps` stands for this file's existing helper that builds a `TestCase` with one step; use its real name.)

- [ ] **Step 2: Run to verify it fails** — `cargo test --test import_parser a_cases_findings`.

- [ ] **Step 3: Implement**

`html.rs`, right after the reviewer-notes `if` block and before `if !tc.steps.is_empty()`:

```rust
        // Findings: what the assistant found wrong while writing this case.
        // Its own block, not part of the notes (provenance) or the comment
        // box (the developer's). Open by default for the same reason the
        // notes are: a problem behind a closed disclosure is a problem
        // nobody reads. Markdown through crate::markdown, which drops raw
        // HTML, so an assistant cannot put script on this page.
        if !tc.findings.is_empty() {
            parts.push(format!(
                "<details class='findings' open><summary>Findings ({})</summary>",
                tc.findings.len()
            ));
            for f in &tc.findings {
                let kind = match f.kind.as_str() {
                    "test_case" => "Test case",
                    "spec" => "Spec",
                    "code" => "Code",
                    other => other,
                };
                let subject = if f.subject.is_empty() {
                    String::new()
                } else {
                    format!("<span class='subject'>{}</span>", esc(&f.subject))
                };
                let detail = if f.detail.is_empty() {
                    String::new()
                } else {
                    format!("<div class='fdetail'>{}</div>", crate::markdown::to_html(&f.detail))
                };
                parts.push(format!(
                    "<article class='finding'><div class='meta'><span class='kind'>{}</span>{subject}</div>\
                     <p class='ftitle'>{}</p>{detail}</article>",
                    esc(kind),
                    esc(&f.title)
                ));
            }
            parts.push("</details>".into());
        }
```

`src-tauri/web/cases-page.css`, beside the `.rev` rules and using the same colour variables they use:

```css
/* Findings block: what the assistant found wrong while writing the case.
   Sits under the reviewer notes, above the steps. */
.findings{margin:8px 0;padding:8px 12px;border:1px solid var(--border);border-left:3px solid var(--accent);border-radius:8px;background:var(--surface)}
.findings summary{cursor:pointer;font-weight:600;font-size:13px}
.finding{padding:8px 0;border-top:1px solid var(--border)}
.finding:first-of-type{border-top:0}
.finding .meta{display:flex;gap:10px;font-size:11px;opacity:.75;align-items:center}
.finding .kind{font-weight:600;text-transform:uppercase;letter-spacing:.04em}
.finding .ftitle{margin:4px 0 2px;font-weight:600}
.finding .fdetail{font-size:13px}
```

(Use the variable names the file already defines for border, accent and surface; if `--accent` is not one of them, use whichever the `.rev` summary or the active badge uses.)

- [ ] **Step 4: Run** `cargo test --test import_parser --test draft_comments`, then commit:

```bash
git add src-tauri/src/import_parser/html.rs src-tauri/web/cases-page.css src-tauri/tests/import_parser.rs
git commit -q -F - <<'EOF'
feat(v2): the browser page shows a case's findings in a block of their own

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
EOF
```

---

### Task 5: Changelog entry (at ship time)

```ts
  {
    version: "1.23.X",
    date: "2026-09-XX",
    items: [
      "AI findings now live in the test case file. When an assistant finds a problem in a case, its spec or the code while writing, it records it on that case in the .json, and the browser view of the cases shows it in a block under the case. The separate findings card, the bell entries and the two findings tools from 1.23.17 are gone; nothing is kept outside the file.",
    ],
  },
```

Then the usual three-file bump, `cargo check --lib`, and `scripts/release-v2.ps1 -Version X.Y.Z`.

---

## Self-review

**Coverage.** Remove everything store-based (Task 1); findings in the file per case, round-tripped, never sent to ADO, untouched by transforms (Task 2); the assistant told where they go, the validator pointing there (Task 3); shown only in the browser page (Task 4). No card, bell, chip or switch anywhere. `comment` and `reviewer_notes` rules kept.

**Placeholders.** Two helper names are left to the implementer by design (`parse_json_str`, `case_with_steps`), each identified as "this file's existing helper".

**Type consistency.** `CaseFinding { kind, subject, title, detail }` in Tasks 2, 4; `TestCase.findings: Vec<CaseFinding>` in Tasks 2, 3 (JSON `findings`), 4; the JSON key `findings` everywhere; `FINDING_KINDS` in Task 2; the guide's kinds sentence matches the importer's three values.
