# Spec Coverage (round-5 Part I) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Catch and prevent dropped spec sections: a `check_spec_coverage` MCP tool, an intake-time `scale` block that sizes the job, and a verbatim source-quote convention — all fed by ONE section parser that reads the document itself.

**Architecture:** A new pure Rust module `speccov.rs` parses section inventories out of spec documents (markdown headings, numbered headings, AC markers) and `Spec:` citations + quotes out of `reviewer_notes`. Three consumers: a new bridge route + MCP tool `check_spec_coverage` joins inventory×citations into six report buckets; `begin_test_case_writing` reuses the parser to emit a `scale` recommendation; the writing guide and plan text gain the quote convention and the new workflow step. A small `merge_case_files` tool closes the fan-out loop so slice merging never becomes a hand-rolled script.

**Tech Stack:** Rust (v2/src-tauri), serde_json, the existing ai_bridge route table + mcp.rs dispatch. No frontend changes except `src/lib/mcpTools.ts` (the tool toggle list).

## Global Constraints

- **The inventory is parsed FROM THE DOCUMENT, never supplied by the assistant** (round-5 §3 "design point that matters most"). No tool input may substitute an assistant-provided section list for the parsed one.
- **Coverage output is its own register — findings, not warnings** (§3): `check_spec_coverage` returns its buckets as top-level JSON keys and NEVER a `warnings` array; a partial-scope draft is legitimate.
- **Not a hard failure; not a second source of truth about scope; not strict about citation format; not semantic** (§5, verbatim). Unparsable citations land in `unattributed`, never silently dropped, never counted as coverage.
- **Quote rule:** "A quote must be verbatim, or it must not be presented as a quote" (§7). The no-quote advisory in `validate_cases` is an **advisory, not a warning** (round-3 register rule).
- **Thresholds: section count dominates, not line count** (§6): under 8 in-scope sections → `single-pass`; 8–15 → `choose`; over 15 → `fan-out`. Lines only tip the answer upward (>1500 lines upgrades `single-pass` to `choose`).
- **Four sync gates fire when an MCP tool is added** (two are added here): `tests/ai_tools.rs` `TOOLS: [&str; N]`, `tests/tcm_mcp.rs` name vec, `tests/tcm_mcp.rs` count assertion, `v2/src/lib/mcpTools.ts`.
- The bridge runs without sign-in for tools that read no org data: `check_spec_coverage` and `merge_case_files` must answer with `client: None` (like `/autorun-guide`).
- Rust: no `u64` across IPC; `bindings.ts` regenerated only by `cargo test --test bindings`. Gates: `CARGO_TARGET_DIR=target/gate cargo test` (the default dir may be locked by the dev app), `npx tsc --noEmit`, `npx vitest run` checking EXIT CODE.
- Commits via Bash heredoc `git commit -F - <<'EOF'`; never write source through a heredoc (use Write/Edit).

---

### Task 1: `speccov.rs` — the one parser (inventory + citations)

**Files:**
- Create: `v2/src-tauri/src/speccov.rs`
- Modify: `v2/src-tauri/src/lib.rs` (add `pub mod speccov;` to the module tree)
- Test: unit tests inside `speccov.rs` (`#[cfg(test)]`), run via `cargo test --lib`

**Interfaces:**
- Consumes: nothing (pure).
- Produces:
  ```rust
  pub struct Section { pub id: String, pub title: String, pub line: usize }
  pub struct Inventory { pub lines: usize, pub sections: Vec<Section> }
  pub fn parse_inventory(text: &str) -> Inventory;

  pub struct SpecCitation { pub file: String, pub section: String, pub quote: Option<String>, pub exemption: Option<String> }
  pub struct Citations { pub specs: Vec<SpecCitation>, pub has_code_ref: bool }
  pub fn parse_citations(reviewer_notes: &str) -> Option<Citations>; // None = nothing parseable
  pub fn normalize_section_id(raw: &str) -> String; // "7.7." -> "7.7", "AC-3" kept as-is
  ```

- [ ] **Step 1: Write the failing inventory tests** (in `speccov.rs` `#[cfg(test)] mod tests`)

```rust
#[test]
fn markdown_headings_become_sections_with_their_line_numbers() {
    let inv = parse_inventory("# Intro\n\ntext\n\n## 7.7 Copy from previous cycle\n\nbody\n\n### 7.7.1 Empty state\n");
    assert_eq!(inv.lines, 9);
    let ids: Vec<&str> = inv.sections.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["Intro", "7.7", "7.7.1"]);
    assert_eq!(inv.sections[1].title, "Copy from previous cycle");
    assert_eq!(inv.sections[1].line, 5);
}

#[test]
fn numbered_headings_without_hashes_are_found() {
    // Specs exported from Word often have bare "7.7 Title" lines.
    let inv = parse_inventory("7.7 Copy from previous cycle\nbody\n8.2 Archive\n");
    let ids: Vec<&str> = inv.sections.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["7.7", "8.2"]);
}

#[test]
fn ac_markers_inside_a_section_become_child_sections() {
    let inv = parse_inventory("## 8.2 Archive\n\nAC-1: it archives\nAC-2: it restores\n");
    let ids: Vec<&str> = inv.sections.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["8.2", "8.2 (AC-1)", "8.2 (AC-2)"]);
}

#[test]
fn prose_that_merely_mentions_a_number_is_not_a_section() {
    // "see section 7.7 for details" must not create a section.
    let inv = parse_inventory("Intro text mentioning 7.7 mid-sentence.\nAnd a version number 2.1 in prose.\n");
    assert!(inv.sections.is_empty(), "{:?}", inv.sections.iter().map(|s| &s.id).collect::<Vec<_>>());
}
```

Parsing rules the implementation must follow: a markdown heading is `^#{1,6}\s+`; a numbered heading is a line that STARTS with `\d+(\.\d+)*` followed by whitespace and a non-empty title (start-of-line only — that is what keeps prose mentions out); an AC marker is a line starting `AC-\d+` (case-insensitive), attributed to the nearest preceding numbered/heading section as `"<id> (AC-n)"`. A heading with no leading number uses its full text as `id`. `lines` is the total line count of the document.

- [ ] **Step 2: Run to verify they fail**

Run: `cd v2/src-tauri && CARGO_TARGET_DIR=target/gate cargo test --lib speccov`
Expected: compile error (module does not exist) — add the module skeleton with `todo!()`-free empty impls only if needed to see red assertions.

- [ ] **Step 3: Implement `parse_inventory` + `normalize_section_id`** (minimal, per the rules above)

- [ ] **Step 4: Write the failing citation tests**

```rust
#[test]
fn a_standard_citation_parses_file_section_and_quote() {
    let c = parse_citations(
        "Checks the copy affordance.\nSpec: Step10.md 7.7 (AC-3)\n> \"Copy from previous cycle is offered only when a completed cycle exists.\"",
    ).unwrap();
    assert_eq!(c.specs.len(), 1);
    assert_eq!(c.specs[0].file, "Step10.md");
    assert_eq!(c.specs[0].section, "7.7 (AC-3)");
    assert!(c.specs[0].quote.as_deref().unwrap().starts_with("Copy from previous cycle"));
}

#[test]
fn reasonable_variants_parse_and_garbage_reads_as_none() {
    // Tolerated: "Spec:" / "spec:" / extra spaces / trailing period on the section.
    assert!(parse_citations("spec:  Step10.md   7.9.").is_some());
    // Code-only citation: no spec entry, but has_code_ref is true.
    let code = parse_citations("Code: IndexModel.CanCopy").unwrap();
    assert!(code.specs.is_empty() && code.has_code_ref);
    // Nothing parseable at all.
    assert!(parse_citations("just prose with no citation").is_none());
}

#[test]
fn the_fixed_exemption_form_is_recognised() {
    let c = parse_citations("Spec: Step10.md 7.9 - no quotable text (requirement is a state table)").unwrap();
    assert_eq!(c.specs[0].exemption.as_deref(), Some("requirement is a state table"));
    assert!(c.specs[0].quote.is_none());
}
```

- [ ] **Step 5: Implement `parse_citations`**, run `cargo test --lib speccov` to green.

- [ ] **Step 6: Commit**

```bash
git add v2/src-tauri/src/speccov.rs v2/src-tauri/src/lib.rs
git commit -F - <<'EOF'
feat(v2): one spec parser for coverage and sizing

Sections are parsed FROM the document (headings, numbered headings, AC
markers) - never taken from the assistant, whose skipped section would be
skipped again while listing headings. Citations parse tolerantly out of
reviewer_notes; what cannot be parsed reads as None, never as coverage.
EOF
```

---

### Task 2: the coverage join — six buckets

**Files:**
- Modify: `v2/src-tauri/src/speccov.rs` (add the join)
- Test: unit tests in the same file

**Interfaces:**
- Consumes: Task 1's `Inventory`, `parse_citations`, plus `crate::model::TestCase` (has `title: String`, `reviewer_notes: String`).
- Produces:
  ```rust
  pub struct CoverageInput<'a> { pub inventories: Vec<(String /*file name*/, Inventory)>, pub cases: &'a [crate::model::TestCase], pub sections_scope: &'a str, pub out_of_scope: &'a str }
  pub fn check_coverage(input: CoverageInput) -> serde_json::Value;
  ```
  The value has EXACTLY these keys: `sections_in_document` (number), `covered` (object: section id → array of case titles), `uncovered` (array), `unattributed` (array of "title — reason"), `cited_but_absent` (array of "section — cited by 'title', no such section in file"), `quote_not_in_document` (array of "title — quoted text not found in file"), `excluded_by_plan` (array). No `warnings` key — this register is findings.

- [ ] **Step 1: Write the failing bucket tests** — one per bucket:

```rust
fn case(title: &str, notes: &str) -> crate::model::TestCase { /* build with empty steps/tags, reviewer_notes = notes */ }

#[test]
fn covered_and_uncovered_split_on_citations() {
    let inv = parse_inventory("## 7.1 List\n## 7.4 Export\n## 7.7 Copy\n");
    let cases = vec![case("List loads", "Spec: S.md 7.1"), case("Copy offered", "Spec: S.md 7.7")];
    let v = check_coverage(CoverageInput { inventories: vec![("S.md".into(), inv)], cases: &cases, sections_scope: "", out_of_scope: "" });
    assert_eq!(v["uncovered"], serde_json::json!(["7.4"]));
    assert_eq!(v["covered"]["7.1"], serde_json::json!(["List loads"]));
}

#[test]
fn a_case_without_a_parseable_citation_is_unattributed_not_a_gap() {
    // and a cited section that does not exist lands in cited_but_absent
    // (both asserted, with the exact "— reason" string shapes from the plan header)
}

#[test]
fn a_quote_that_is_not_in_the_document_is_reported() {
    // inventory built from text containing a known sentence; one case quotes it
    // verbatim (found -> NOT reported), one quotes a paraphrase (reported).
    // Whitespace inside quotes is normalised (runs of whitespace equal one
    // space) before matching - line wraps in the md must not defeat the check.
}

#[test]
fn plan_scope_moves_sections_to_excluded_not_uncovered() {
    // sections_scope "7.1, 7.7" -> 7.4 is excluded_by_plan, not uncovered.
    // An enumerated list filters; free text ("everything") excludes nothing.
    // out_of_scope entries that name a section id exclude it too.
}
```

Scope parsing rule (keep simple, per §5 "not a second source of truth"): split `sections_scope` on `,`/`;`, trim; if EVERY token normalises to a section-id shape (`\d+(\.\d+)*` optionally with `(AC-n)`), treat it as an enumerated in-scope list; otherwise the scope excludes nothing. `out_of_scope` lines that contain a section-id token exclude those ids.

- [ ] **Step 2: Run red, implement `check_coverage`, run green** (`cargo test --lib speccov`)

- [ ] **Step 3: Commit** (`feat(v2): the coverage join - six buckets, findings not warnings`)

---

### Task 3: bridge route + MCP tool `check_spec_coverage`

**Files:**
- Modify: `v2/src-tauri/src/ai_bridge.rs` (route `("POST", "/check-coverage")`, handler `check_coverage_route`), `v2/src-tauri/src/mcp.rs` (tool decl + dispatch), `v2/src-tauri/src/ai_tools.rs` (COMMANDS entry, stem `coverage`), `v2/src/lib/mcpTools.ts`
- Test: `v2/src-tauri/tests/speccov_bridge.rs` (new), plus updating `tests/ai_tools.rs` TOOLS array and `tests/tcm_mcp.rs` names + count

**Interfaces:**
- Consumes: Task 2's `check_coverage`; the existing `q(target, key)` query parser; `crate::import_parser::parse_file` and `parse_cases_with_warnings` (the same pair `validate_json` uses — see `ai_bridge.rs:434` for the pattern to copy, including the `?path=` branch and its "does not exist" error).
- Produces: MCP tool `check_spec_coverage` with input schema `{ json?: string, path?: string, spec_paths: string[], sections?: string, out_of_scope?: string }` (`required: ["spec_paths"]`). The tool description MUST say: reports coverage as findings to read and account for — a partial draft is a normal state, not an error.

- [ ] **Step 1: Write the failing bridge tests** (`tests/speccov_bridge.rs`, modeled on `tests/autorun_bridge.rs` — `route(&ctx(), None, ...)`, no client needed):

```rust
#[tokio::test]
async fn coverage_answers_without_a_signed_in_client() { /* POST /check-coverage with a temp spec file + inline cases; expect 200 and an "uncovered" key */ }

#[tokio::test]
async fn a_missing_spec_file_is_a_400_naming_the_path() { /* spec_paths -> nonexistent; NOT a silent empty inventory */ }

#[tokio::test]
async fn path_and_json_together_is_a_400() { /* round-5 §10 lesson: never silently prefer one source */ }

#[tokio::test]
async fn the_report_carries_no_warnings_key() { /* findings register - assert !body.contains("\"warnings\"") */ }
```

- [ ] **Step 2: Run red; implement the route** (body = JSON object with the schema above; read each spec path with `std::fs::read_to_string`, build inventories, load cases from `path` XOR inline `json` — both present is a 400, echoing §10's "silently ignored" lesson).

- [ ] **Step 3: Register the MCP tool + slash command; update all four sync gates.** `mcp.rs` dispatch serialises the tool arguments straight through as the POST body. `ai_tools.rs` stem `coverage`, description "Which parts of the spec have no case yet - run before optimize_cases", prompt instructing: report `uncovered` to the developer and account for every entry before handing the file over.

- [ ] **Step 4: Run the gates** — `CARGO_TARGET_DIR=target/gate cargo test --test speccov_bridge --test ai_tools --test tcm_mcp`, `npx vitest run src/lib` (mcpTools has its own drift test), `npx tsc --noEmit`.

- [ ] **Step 5: Commit** (`feat(v2): check_spec_coverage - which spec sections have no case`)

---

### Task 4: intake `scale` block

**Files:**
- Modify: `v2/src-tauri/src/intake.rs` (compute scale), `v2/src-tauri/src/ai_bridge.rs` (include `scale` in the `/begin` "ready" JSON, near `ai_bridge.rs:404` where `plan_path` is emitted)
- Test: extend the existing intake tests (find them with `grep -rn "begin" v2/src-tauri/tests/*.rs`; they exercise the `/begin` route end-to-end)

**Interfaces:**
- Consumes: Task 1's `parse_inventory`; the intake answers struct (`spec_paths: Vec<String>`, `sections: String` — `intake.rs:39-42`).
- Produces:
  ```rust
  // in intake.rs
  pub fn job_scale(spec_paths: &[String], sections_scope: &str) -> Option<serde_json::Value>;
  // None when no spec file could be read (never an error - sizing is advice).
  ```
  Value shape (verbatim from the feedback §6): `{ "spec_lines": n, "sections_in_scope": n, "recommendation": "single-pass"|"choose"|"fan-out", "why": "<one sentence with the numbers in it>" }`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn scale_thresholds_let_section_count_dominate() {
    // 5 sections / 3000 lines -> "choose" (lines only upgrade single-pass to choose)
    // 20 sections / 400 lines -> "fan-out" (sections dominate)
    // 6 sections / 500 lines  -> "single-pass"
}

#[tokio::test]
async fn a_ready_intake_carries_the_scale_block() {
    // Drive the /begin route with a temp spec of 16 headings; the ready
    // JSON has scale.recommendation == "fan-out" and a why naming both numbers.
}
```

Sections-in-scope counts only TOP-level parsed sections (not AC children) after applying the same scope filter as Task 2. Thresholds exactly as Global Constraints.

- [ ] **Step 2: Run red, implement, run green.**

- [ ] **Step 3: Commit** (`feat(v2): intake sizes the job and says when one pass is not enough`)

---

### Task 5: quote convention + workflow step 3.5 + validate advisory

**Files:**
- Modify: `v2/src-tauri/src/ai_bridge.rs` — the guide body (`guide()` at `ai_bridge.rs:691`): extend the `## reviewer_notes` section and the `## Workflow` list; the `validate_json` advisory block (`ai_bridge.rs:434` onward)
- Modify: `v2/src-tauri/src/intake.rs` — "Before handing the file over" gains item 5
- Test: extend the guide/validate tests (find with `grep -rn "reviewer_notes\|advisories" v2/src-tauri/tests/*.rs`)

**Interfaces:** consumes Task 1's `parse_citations` (the advisory must use the SAME parser — one parser, two consumers).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn the_guide_teaches_the_quote_rule_and_its_exemptions() {
    // guide text contains: "verbatim, or it must not be presented as a quote",
    // the fixed exemption form "no quotable text", and a workflow step
    // mentioning check_spec_coverage between drafting and validation.
}

#[tokio::test]
async fn a_spec_cited_case_without_quote_or_exemption_is_an_advisory_not_a_warning() {
    // POST /validate with one case citing "Spec: S.md 7.7" and no quote:
    // advisories gains one entry naming the case; warnings stays [].
    // A case with the exemption form, and a case with Code:-only citation,
    // produce NO advisory.
}
```

- [ ] **Step 2: Run red; write the guide text.** The reviewer_notes section gains, after the two-part list: the quote instruction ("alongside the Spec: pointer, quote the source sentence verbatim — `> \"...\"` — or state the exemption in the fixed form `Spec: <file> <section> — no quotable text (<why>)`. Never rewrite inside quotation marks; elide with an ellipsis instead."), naming the four §7 exemptions (code-not-prose, absence, table/diagram, synthesis). The Workflow gains step 3.5: "Call `check_spec_coverage` with the draft and the plan's spec paths. Report `uncovered` to the developer and account for every entry — 'out of scope for this batch' is a fine answer, silence is not." The plan checklist gains item 5 in the same voice: "Run `check_spec_coverage` and account for every `uncovered` entry out loud."

- [ ] **Step 3: Wire the advisory in `validate_json`** using `speccov::parse_citations`; run green.

- [ ] **Step 4: Commit** (`feat(v2): quote the source, and a coverage step in the workflow`)

---

### Task 6: `merge_case_files` + the fan-out recipe

**Files:**
- Modify: `v2/src-tauri/src/ai_bridge.rs` (route `("POST", "/merge-cases")`), `v2/src-tauri/src/mcp.rs`, `v2/src-tauri/src/ai_tools.rs` (a `fanout` command whose prompt IS the recipe), `v2/src/lib/mcpTools.ts`
- Test: `v2/src-tauri/tests/speccov_bridge.rs` (extend), the four sync gates again

**Interfaces:**
- Consumes: `crate::import_parser::parse_file`.
- Produces: MCP tool `merge_case_files` `{ paths: string[], output_path: string }` → reads every slice through the REAL importer, concatenates in the order given, writes `output_path` (must not already exist — refuse rather than overwrite), returns `{ cases, per_file: [{path, cases}], warnings }` where warnings aggregates the importer's per-slice warnings.

- [ ] **Step 1: Failing tests** — merge of two temp slice files preserves order and counts; an unreadable slice fails the whole merge (nothing written); an existing output path is refused with its name in the error.

- [ ] **Step 2: Implement route + tool + gates.**

- [ ] **Step 3: The `fanout` command prompt** (content, not machinery — §6 "the server recommends; it cannot orchestrate") carries the six constraints verbatim-in-spirit: slices from the document's own headings (tell the assistant to take the slice list from `check_spec_coverage`'s section inventory — er, from the intake `scale`/coverage output, never invent one); one output file per slice named `<output-stem>-slice-<n>.json`; `optimize_cases` once on the merged file; each slice-writer gets the plan path, its section range, its own output path, an instruction to call `get_writing_guide` itself, and 2–3 exemplars via `get_test_cases`; intake completes before dispatch; `check_spec_coverage` runs on the merged file. Merge via `merge_case_files`, never by hand.

- [ ] **Step 4: Full gates + commit** (`feat(v2): merge tool + fan-out recipe, so slicing never ends in a hand-rolled merge`)

---

## Verification checklist for the final review

- §3: all six buckets present and named exactly as the feedback's example (plus `quote_not_in_document` from §7); findings register (no `warnings`).
- §5: not a hard failure (a fully-uncovered draft still returns 200); scope comes from the plan's strings, tolerant citations, no semantic judgement anywhere.
- §6: parser shared with intake (one parser, two consumers); thresholds section-dominated; recipe is content in `ai_tools.rs`, not server orchestration.
- §7: verbatim-or-not rule in the guide; fixed exemption form; advisory (not warning) in validate.
- Sync gates: `TOOLS` array count, tcm_mcp names + count, mcpTools.ts — two new tools land in all four.
