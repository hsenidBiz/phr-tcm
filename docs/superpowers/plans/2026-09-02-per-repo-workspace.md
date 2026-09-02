# Per-repo Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Constrain the app's AI tooling to one chosen working repository: test-case JSON lives in `<repo>/.test-cases/`, the `/tcm:*` skills and the MCP registrations go into that repo, and the AI Bridge tab is unavailable until a repository is picked.

**Architecture:** A single "working repository" path is stored in the frontend (`tcm-v2-working-dir`) and pushed into the bridge context like org/project already are. A new pure Rust module `workspace` owns the path rules (`.test-cases` folder, copy-in, inside-check, name resolution, git-exclude); `intake`/`begin_writing` and the AI-tool registration code take the working directory as a parameter and choose project-scoped targets when the tool supports them. The frontend gates the AI Bridge on the path, keys detection on it, and copies picked imports into the folder.

**Tech Stack:** Rust (Tauri 2 commands, `serde_json`, `std::fs`), TypeScript/React 19 (React Query, `@tauri-apps/plugin-dialog`), vitest + `@tauri-apps/api/mocks`, cargo integration tests under `v2/src-tauri/tests/`.

**Spec:** `docs/superpowers/specs/2026-09-02-per-repo-workspace-design.md`

## Global Constraints

- All paths below are relative to the repo root `D:\azure-devops-test-case-creator`; the app is `v2/`, Rust is `v2/src-tauri/`.
- Toolchain on this machine is not on the shell PATH: prefix `$env:Path = "C:\Program Files\nodejs;$env:USERPROFILE\.cargo\bin;$env:Path"` in PowerShell (or `PATH="$HOME/.cargo/bin:/c/Program Files/nodejs:$PATH"` in bash) before `cargo` / `npm` / `npx`.
- The folder name is exactly `.test-cases`; the skills directory is exactly `<repo>/.claude/commands/tcm/`; project MCP configs are exactly `<repo>/.mcp.json` (Claude Code, key `mcpServers`), `<repo>/.cursor/mcp.json` (Cursor, key `mcpServers`), `<repo>/.vscode/mcp.json` (VS Code, key `servers`). Claude Desktop and Windsurf stay global.
- Managed server names are the existing constants `TCM_SERVER = "tcm-testcases"` and `DB_SERVER = "phr-db-mcp"`; the command-file marker is the existing `COMMAND_MARKER`. Only entries/files carrying these are ever removed.
- Never overwrite a file in `.test-cases`: same bytes → reuse, different bytes → `-2`, `-3` suffix.
- Every Tauri command signature change must be followed by regenerating `v2/src/bindings.ts` with `cargo test --test bindings` (run from `v2/src-tauri`); commit the regenerated file with the change.
- Do not run `cargo test` and `npm test` concurrently (vitest timeouts flake under load). Re-run a failed vitest file alone before believing a failure.
- Commit messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`; commit with `git commit -F - <<'EOF' … EOF` from bash.

---

### Task 1: `workspace` rules module (Rust, pure)

**Files:**
- Create: `v2/src-tauri/src/workspace.rs`
- Modify: `v2/src-tauri/src/lib.rs` (module list, next to `pub mod intake;`)
- Test: `v2/src-tauri/tests/workspace.rs`

**Interfaces:**
- Produces (all `pub` in `v2_lib::workspace`):
  - `const CASES_DIR: &str = ".test-cases"`
  - `fn cases_dir(root: &Path) -> PathBuf`
  - `fn ensure_cases_dir(root: &Path) -> Result<PathBuf, String>`
  - `fn is_inside(dir: &Path, path: &Path) -> bool`
  - `fn copy_into_cases(root: &Path, source: &Path) -> Result<PathBuf, String>`
  - `fn slug(feature: &str) -> String`
  - `fn default_output_path(root: &Path, feature: &str) -> String`
  - `fn resolve_output(root: &Path, output_path: &str) -> String`
  - `fn exclude_locally(root: &Path, rel: &str) -> Result<bool, String>`

- [ ] **Step 1: Write the failing tests**

Create `v2/src-tauri/tests/workspace.rs`:

```rust
//! The working repository's path rules: the `.test-cases` folder, copying a
//! picked file into it without ever overwriting, and the checks the intake
//! and the importer both defer to.

use std::path::{Path, PathBuf};
use v2_lib::workspace::{
    cases_dir, copy_into_cases, default_output_path, ensure_cases_dir, exclude_locally,
    is_inside, resolve_output, slug, CASES_DIR,
};

fn temp_root(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tcm-ws-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_cases_folder_is_created_once_and_named() {
    let root = temp_root("ensure");
    let dir = ensure_cases_dir(&root).unwrap();
    assert!(dir.is_dir());
    assert_eq!(dir.file_name().unwrap().to_string_lossy(), CASES_DIR);
    assert_eq!(ensure_cases_dir(&root).unwrap(), dir, "idempotent");
    assert_eq!(cases_dir(&root), dir);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn ensuring_inside_a_missing_root_is_refused_not_created() {
    let ghost = std::env::temp_dir().join("tcm-ws-no-such-root-9f3a");
    assert!(ensure_cases_dir(&ghost).is_err());
    assert!(!ghost.exists(), "a typo in the root must not become a folder");
}

#[test]
fn a_picked_file_is_copied_in_and_picking_it_again_reuses_the_copy() {
    let root = temp_root("copy");
    let src = root.join("elsewhere").join("login.json");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, r#"{"test_cases":[]}"#).unwrap();

    let copied = copy_into_cases(&root, &src).unwrap();
    assert!(is_inside(&cases_dir(&root), &copied), "{}", copied.display());
    assert_eq!(std::fs::read(&copied).unwrap(), std::fs::read(&src).unwrap());
    assert!(src.exists(), "the original is copied, never moved");

    let again = copy_into_cases(&root, &src).unwrap();
    assert_eq!(again, copied, "same bytes already there - no second file");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_different_file_with_the_same_name_gets_a_suffix_not_an_overwrite() {
    let root = temp_root("suffix");
    let dir = ensure_cases_dir(&root).unwrap();
    std::fs::write(dir.join("login.json"), "old").unwrap();
    let src = root.join("in").join("login.json");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, "new").unwrap();

    let copied = copy_into_cases(&root, &src).unwrap();
    assert_eq!(copied.file_name().unwrap().to_string_lossy(), "login-2.json");
    assert_eq!(std::fs::read_to_string(dir.join("login.json")).unwrap(), "old");
    assert_eq!(std::fs::read_to_string(&copied).unwrap(), "new");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_file_already_in_the_folder_is_not_copied() {
    let root = temp_root("inside");
    let dir = ensure_cases_dir(&root).unwrap();
    let inside = dir.join("x.json");
    std::fs::write(&inside, "{}").unwrap();
    assert_eq!(copy_into_cases(&root, &inside).unwrap(), inside);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no copy of a copy");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn is_inside_is_case_insensitive_and_exact_about_prefixes() {
    let root = temp_root("inside-check");
    let dir = ensure_cases_dir(&root).unwrap();
    assert!(is_inside(&dir, &dir.join("a.json")), "a file not yet written still counts");
    let shouted = PathBuf::from(dir.to_string_lossy().to_uppercase()).join("a.json");
    assert!(is_inside(&dir, &shouted), "Windows paths compare case-insensitively");
    let sibling = root.join(".test-cases-extra").join("a.json");
    assert!(!is_inside(&dir, &sibling), "a sibling sharing the prefix is outside");
    assert!(!is_inside(&dir, &root.join("a.json")));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn output_names_derive_from_the_feature() {
    assert_eq!(slug("Login & Session flow"), "login-session-flow");
    assert_eq!(slug("   "), "test-cases");
    let p = default_output_path(Path::new("D:/repo"), "Login flow");
    assert!(p.ends_with("login-flow.json"), "{p}");
    assert!(p.contains(CASES_DIR), "{p}");
}

#[test]
fn a_bare_name_resolves_into_the_folder_and_a_path_is_left_alone() {
    let root = Path::new("D:/repo");
    let resolved = resolve_output(root, "login.json");
    assert_eq!(resolved, cases_dir(root).join("login.json").to_string_lossy());
    assert_eq!(resolve_output(root, "D:/x/y.json"), "D:/x/y.json");
    assert_eq!(resolve_output(root, "  "), "");
}

#[test]
fn exclude_writes_once_and_only_in_a_git_checkout() {
    let plain = temp_root("plain");
    assert_eq!(exclude_locally(&plain, ".mcp.json").unwrap(), false);
    assert!(!plain.join(".git").exists());

    let repo = temp_root("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    assert_eq!(exclude_locally(&repo, ".mcp.json").unwrap(), true);
    assert_eq!(exclude_locally(&repo, ".mcp.json").unwrap(), true);
    let text = std::fs::read_to_string(repo.join(".git").join("info").join("exclude")).unwrap();
    assert_eq!(text.matches(".mcp.json").count(), 1, "{text}");
    let _ = std::fs::remove_dir_all(&plain);
    let _ = std::fs::remove_dir_all(&repo);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `v2/src-tauri`): `cargo test --test workspace`
Expected: compile error `could not find `workspace` in `v2_lib``.

- [ ] **Step 3: Write the module**

Create `v2/src-tauri/src/workspace.rs`:

```rust
//! The working repository: the one folder per-repo test-case files belong
//! to, and the path rules the intake, the importer and the registration
//! code all defer to. Pure functions over `std::fs` - the Tauri commands
//! in `commands/workspace.rs` are thin wrappers.

use std::path::{Path, PathBuf};

/// The per-repo home of test-case JSON. A dot-folder so it sits beside
/// `.claude/` and `.mcp.json` rather than among the repo's own sources.
pub const CASES_DIR: &str = ".test-cases";

pub fn cases_dir(root: &Path) -> PathBuf {
    root.join(CASES_DIR)
}

/// Create the folder if needed. The ROOT is never created: a mistyped
/// repository path must not quietly become a new folder somewhere.
pub fn ensure_cases_dir(root: &Path) -> Result<PathBuf, String> {
    if !root.is_dir() {
        return Err(format!("working repository does not exist: {}", root.display()));
    }
    let dir = cases_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// One comparable spelling of a path: canonical where it exists (so
/// `D:\repo` and `D:/REPO/` agree), the nearest existing parent joined
/// with the file name where it does not yet - an output path is checked
/// before the assistant has written the file. Lower-cased and
/// backslashed because the app is Windows-first and NTFS is
/// case-insensitive.
fn normalized(p: &Path) -> String {
    let full = match p.canonicalize() {
        Ok(c) => c,
        Err(_) => match (p.parent().and_then(|d| d.canonicalize().ok()), p.file_name()) {
            (Some(d), Some(f)) => d.join(f),
            _ => p.to_path_buf(),
        },
    };
    full.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Is `path` `dir` itself or somewhere beneath it? A sibling that merely
/// shares the prefix (`.test-cases-extra`) is outside.
pub fn is_inside(dir: &Path, path: &Path) -> bool {
    let d = normalized(dir);
    let p = normalized(path);
    p == d || p.starts_with(&format!("{d}\\"))
}

/// Copy `source` into the cases folder and return where it landed. A
/// file already inside is returned as is. Never overwrites: identical
/// bytes reuse the existing file, different bytes take `name-2.json`,
/// `name-3.json`, ...
pub fn copy_into_cases(root: &Path, source: &Path) -> Result<PathBuf, String> {
    let dir = ensure_cases_dir(root)?;
    if is_inside(&dir, source) {
        return Ok(source.to_path_buf());
    }
    let name = source
        .file_name()
        .ok_or_else(|| format!("not a file: {}", source.display()))?;
    let bytes =
        std::fs::read(source).map_err(|e| format!("could not read {}: {e}", source.display()))?;
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "test-cases".to_string());
    let ext = Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut n = 1u32;
    loop {
        let file = if n == 1 { format!("{stem}{ext}") } else { format!("{stem}-{n}{ext}") };
        let candidate = dir.join(file);
        match std::fs::read(&candidate) {
            Ok(existing) if existing == bytes => return Ok(candidate),
            Ok(_) => n += 1,
            Err(_) => {
                std::fs::write(&candidate, &bytes)
                    .map_err(|e| format!("could not write {}: {e}", candidate.display()))?;
                return Ok(candidate);
            }
        }
    }
}

/// A file-name-safe version of a feature name: lower-case, runs of
/// anything non-alphanumeric collapsed to one dash.
pub fn slug(feature: &str) -> String {
    let dashed: String = feature
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let joined = dashed.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if joined.is_empty() { "test-cases".to_string() } else { joined }
}

/// The path the intake suggests for a job: `<root>/.test-cases/<slug>.json`.
pub fn default_output_path(root: &Path, feature: &str) -> String {
    cases_dir(root).join(format!("{}.json", slug(feature))).to_string_lossy().to_string()
}

/// A bare file name means "in the cases folder"; anything carrying a
/// directory part is returned unchanged (and judged by `is_inside` later).
pub fn resolve_output(root: &Path, output_path: &str) -> String {
    let t = output_path.trim();
    if t.is_empty() || t.contains('/') || t.contains('\\') {
        return t.to_string();
    }
    cases_dir(root).join(t).to_string_lossy().to_string()
}

/// Keep `rel` out of `git status` for THIS checkout only, via
/// `.git/info/exclude` - the repo's `.gitignore` is the user's and is not
/// edited. `Ok(false)` when the root is not a git checkout.
pub fn exclude_locally(root: &Path, rel: &str) -> Result<bool, String> {
    if !root.join(".git").is_dir() {
        return Ok(false);
    }
    let info = root.join(".git").join("info");
    std::fs::create_dir_all(&info).map_err(|e| format!("could not create {}: {e}", info.display()))?;
    let file = info.join("exclude");
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == rel) {
        return Ok(true);
    }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(rel);
    next.push('\n');
    std::fs::write(&file, next).map_err(|e| format!("could not write {}: {e}", file.display()))?;
    Ok(true)
}
```

In `v2/src-tauri/src/lib.rs`, add `pub mod workspace;` directly after `pub mod webtheme;`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test workspace`
Expected: `test result: ok. 9 passed`.

- [ ] **Step 5: Commit**

```bash
cd D:/azure-devops-test-case-creator && git add v2/src-tauri/src/workspace.rs v2/src-tauri/src/lib.rs v2/src-tauri/tests/workspace.rs && git commit -F - <<'EOF'
feat(v2): workspace rules - the per-repo .test-cases folder and its path checks

Pure functions the intake, the importer and registration will defer to:
create the folder (never the root), copy a picked file in without ever
overwriting, decide inside/outside case-insensitively, resolve a bare
name into the folder, and keep .mcp.json out of git status locally.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
```

---

### Task 2: Workspace Tauri commands + bindings

**Files:**
- Create: `v2/src-tauri/src/commands/workspace.rs`
- Modify: `v2/src-tauri/src/commands/mod.rs` (add `pub mod workspace;` after `pub mod testplan;`)
- Modify: `v2/src-tauri/src/lib.rs` (the `collect_commands![...]` list, after `ai_tools::unregister_db_server`)
- Regenerate: `v2/src/bindings.ts`

**Interfaces:**
- Consumes: `crate::workspace::{ensure_cases_dir, copy_into_cases}` (Task 1).
- Produces (TypeScript, via bindings): `commands.ensureCasesDir(root: string): Promise<Result<string, string>>`, `commands.copyIntoCases(root: string, source: string): Promise<Result<string, string>>` — both return `{status:"ok", data}` / `{status:"error", error}` like `parseImportFile`.

- [ ] **Step 1: Write the command module**

Create `v2/src-tauri/src/commands/workspace.rs`:

```rust
//! Working-repository commands: the `.test-cases` folder and copying a
//! picked file into it. The rules live in `crate::workspace`; this is I/O
//! plumbing for the frontend.

use std::path::Path;

/// Create `<root>/.test-cases` if needed and return its path.
#[tauri::command]
#[specta::specta]
pub fn ensure_cases_dir(root: String) -> Result<String, String> {
    crate::workspace::ensure_cases_dir(Path::new(&root)).map(|p| p.to_string_lossy().to_string())
}

/// Copy a picked JSON file into `<root>/.test-cases` (a file already there
/// is returned as is) and return the path the app should import from.
#[tauri::command]
#[specta::specta]
pub fn copy_into_cases(root: String, source: String) -> Result<String, String> {
    crate::workspace::copy_into_cases(Path::new(&root), Path::new(&source))
        .map(|p| p.to_string_lossy().to_string())
}
```

- [ ] **Step 2: Register the module and the commands**

In `v2/src-tauri/src/commands/mod.rs` add `pub mod workspace;` at the end of the module list. In `v2/src-tauri/src/lib.rs` inside `collect_commands![` append, after `ai_tools::unregister_db_server`:

```rust
            ai_tools::unregister_db_server,
            workspace::ensure_cases_dir,
            workspace::copy_into_cases
```

(Mind the trailing comma on the previous last entry.)

- [ ] **Step 3: Regenerate bindings and check them**

Run (from `v2/src-tauri`): `cargo test --test bindings`
Expected: `test result: ok.` and `grep -n "copyIntoCases\|ensureCasesDir" ../src/bindings.ts` shows both functions.

- [ ] **Step 4: Commit**

```bash
cd D:/azure-devops-test-case-creator && git add v2/src-tauri/src/commands/workspace.rs v2/src-tauri/src/commands/mod.rs v2/src-tauri/src/lib.rs v2/src/bindings.ts && git commit -F - <<'EOF'
feat(v2): ensure_cases_dir / copy_into_cases commands

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
```

---

### Task 3: The working directory travels in the bridge context

**Files:**
- Modify: `v2/src-tauri/src/ai_bridge.rs:19-28` (`BridgeContext`)
- Modify: `v2/src-tauri/src/commands/ai_bridge.rs:73-95` (`set_bridge_context`)
- Modify (struct literals gain the field): `v2/src-tauri/tests/ai_bridge.rs:8` and `:1096`, `v2/src-tauri/tests/intake.rs:452`, `v2/src-tauri/tests/autorun_bridge.rs:36`, `v2/src-tauri/tests/speccov_bridge.rs:35`, `v2/src-tauri/tests/transform.rs:10`
- Modify: `v2/src/App.tsx:394-414` (the `setBridgeContext` effect), `v2/src/App.test.tsx:160-190`
- Regenerate: `v2/src/bindings.ts`

**Interfaces:**
- Produces: `BridgeContext.working_dir: Option<String>`; command `set_bridge_context(app, organization, project, module_ref, preconditions_ref, disabled_tools, working_dir: Option<String>)`; TS `commands.setBridgeContext(org, project, moduleRef, preconditionsRef, disabledTools, workingDir: string | null)`.
- Consumes: `src/lib/workingDir.ts` from Task 7 — **this task lands the Rust side and a temporary `null` from App; Task 8 wires the real value.**

- [ ] **Step 1: Add the field**

In `v2/src-tauri/src/ai_bridge.rs` change the struct to:

```rust
#[derive(Debug, Clone, Default, Serialize)]
pub struct BridgeContext {
    pub org: String,
    pub project: String,
    pub module_ref: Option<String>,
    pub preconditions_ref: Option<String>,
    /// Tools the user has switched off in the AI Bridge tab. Empty means
    /// everything is available - the default - so an unset context can
    /// never accidentally disable the whole server.
    pub disabled_tools: Vec<String>,
    /// The working repository picked on the AI Bridge tab, or None when
    /// none is set - in which case a writing job cannot start, because
    /// there is nowhere agreed for its file to go.
    pub working_dir: Option<String>,
}
```

- [ ] **Step 2: Thread it through the command**

In `v2/src-tauri/src/commands/ai_bridge.rs` change `set_bridge_context` to:

```rust
pub fn set_bridge_context(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    module_ref: Option<String>,
    preconditions_ref: Option<String>,
    disabled_tools: Vec<String>,
    working_dir: Option<String>,
) {
    use tauri::Manager;
    let handle = app.state::<BridgeHandle>();
    let guard = handle.running.lock().unwrap();
    if let Some((shared, _)) = guard.as_ref() {
        *shared.ctx.lock().unwrap() = BridgeContext {
            org: organization,
            project,
            module_ref,
            preconditions_ref,
            disabled_tools: disabled_tools.clone(),
            working_dir: working_dir.clone(),
        };
    }
```

(the rest of the function - the `LAST` / `sync_commands` block - stays as it is for now; Task 6 changes it.)

- [ ] **Step 3: Update every test literal**

In each file listed above, add `working_dir: None,` as the last field of the `BridgeContext { ... }` literal. For `tests/intake.rs` line 452 the helper becomes:

```rust
fn ctx() -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        module_ref: None,
        preconditions_ref: None,
        disabled_tools: vec![],
        working_dir: None,
    }
}
```

- [ ] **Step 4: Regenerate bindings, then fix the frontend call**

Run (from `v2/src-tauri`): `cargo test --test bindings` — expect ok, and `commands.setBridgeContext` in `v2/src/bindings.ts` now takes a sixth `workingDir: string | null` argument.

In `v2/src/App.tsx` the effect becomes (temporary `null`, replaced in Task 8):

```tsx
        commands.setBridgeContext(
          org,
          project,
          bridgePrefs.moduleRef,
          bridgePrefs.preconditionsRef,
          disabledTools,
          null,
        ),
```

- [ ] **Step 5: Run the Rust suites that touch the context and the App tests**

Run (from `v2/src-tauri`): `cargo test --test ai_bridge --test intake --test autorun_bridge --test speccov_bridge --test transform`
Expected: all `ok`.
Run (from `v2`): `npx vitest run src/App.test.tsx` — expected: all pass (`npx tsc --noEmit` also clean).

- [ ] **Step 6: Commit**

```bash
cd D:/azure-devops-test-case-creator && git add v2/src-tauri v2/src/App.tsx v2/src/bindings.ts && git commit -F - <<'EOF'
feat(v2): the bridge context carries the working repository

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
```

---

### Task 4: The intake writes into `.test-cases` and refuses to start without a repo

**Files:**
- Modify: `v2/src-tauri/src/intake.rs:92-130` (`questions`), `:181-200` (`problems`)
- Modify: `v2/src-tauri/src/ai_bridge.rs:525-660` (`begin_writing`)
- Test: `v2/src-tauri/tests/intake.rs`

**Interfaces:**
- Consumes: `crate::workspace::{ensure_cases_dir, default_output_path, resolve_output, is_inside, CASES_DIR}` (Task 1), `BridgeContext.working_dir` (Task 3).
- Produces: `intake::questions_for(output_dir: Option<&str>) -> Vec<Question>` (`questions()` = `questions_for(None)`); `intake::problems_in(a, allowed_modules, cases_dir: Option<&Path>) -> Vec<String>` (`problems()` = `problems_in(.., None)`); `/begin` responses: `409 {"status":"blocked","error":…}` without a working dir; phase 1 `context.output_dir` and `context.suggested_output_path`; phase 2 `answers.output_path` resolved into the folder.

- [ ] **Step 1: Write the failing tests**

In `v2/src-tauri/tests/intake.rs`:

(a) Change `good()` so its output lives in the cases folder (create it, since `problems` checks the folder exists):

```rust
fn good(dir: &std::path::Path, spec: &std::path::Path) -> IntakeAnswers {
    let cases = dir.join(".test-cases");
    std::fs::create_dir_all(&cases).unwrap();
    IntakeAnswers {
        output_path: cases.join("cases.json").to_string_lossy().to_string(),
```

(rest of the literal unchanged).

(b) Add a context helper next to `ctx()`:

```rust
/// A context whose working repository is `root` - what every `/begin`
/// call needs, because without one the intake cannot say where the file
/// goes and refuses to start.
fn ctx_in(root: &std::path::Path) -> BridgeContext {
    BridgeContext { working_dir: Some(root.to_string_lossy().to_string()), ..ctx() }
}
```

(c) Every existing `route(&ctx(), "POST", "/begin…", …)` call in this file (lines ~465, 487, 514, 592, 623) becomes `route(&ctx_in(&dir), …)` where `dir` is that test's temp dir - each of those tests already has one.

(d) Append these tests:

```rust
#[tokio::test]
async fn begin_is_blocked_without_a_working_repository() {
    let (status, body) = route(&ctx(), None, "POST", "/begin?feature=Login", "", "test").await;
    assert_eq!(status, 409, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["status"], "blocked");
    assert!(
        v["error"].as_str().unwrap().contains("AI Bridge"),
        "must tell the assistant where the developer fixes it: {body}"
    );
}

#[tokio::test]
async fn begin_creates_the_cases_folder_and_names_it_in_the_questions() {
    let dir = temp_dir("begin-folder");
    let (status, body) = route(&ctx_in(&dir), None, "POST", "/begin?feature=Login flow", "", "test").await;
    assert_eq!(status, 200, "{body}");
    assert!(dir.join(".test-cases").is_dir(), "the folder is the app's to create, not the developer's");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["status"], "questions");
    let out_dir = v["context"]["output_dir"].as_str().unwrap();
    assert!(out_dir.ends_with(".test-cases"), "{out_dir}");
    let suggested = v["context"]["suggested_output_path"].as_str().unwrap();
    assert!(suggested.ends_with("login-flow.json"), "{suggested}");
    // The question itself says where the file goes, so the assistant asks
    // for a NAME rather than inviting an arbitrary path.
    let first = v["ask_the_developer"][0]["ask"].as_str().unwrap();
    assert!(first.contains(".test-cases"), "{first}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_bare_file_name_lands_in_the_cases_folder() {
    let dir = temp_dir("begin-bare");
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "# spec").unwrap();
    let mut a = good(&dir, &spec);
    a.output_path = "login.json".into();
    let body_in = serde_json::to_string(&a).unwrap();
    let (status, body) = route(&ctx_in(&dir), None, "POST", "/begin?feature=Login", &body_in, "test").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["status"], "ready", "{body}");
    let out = v["answers"]["output_path"].as_str().unwrap();
    assert!(out.contains(".test-cases") && out.ends_with("login.json"), "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn an_output_path_outside_the_cases_folder_is_refused() {
    let dir = temp_dir("begin-outside");
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "# spec").unwrap();
    let mut a = good(&dir, &spec);
    a.output_path = dir.join("elsewhere.json").to_string_lossy().to_string();
    let body_in = serde_json::to_string(&a).unwrap();
    let (status, body) = route(&ctx_in(&dir), None, "POST", "/begin?feature=Login", &body_in, "test").await;
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["status"], "needs_answers", "{body}");
    let problems: Vec<String> = v["problems"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect();
    assert!(problems.iter().any(|p| p.contains(".test-cases")), "{problems:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: Run to verify they fail**

Run (from `v2/src-tauri`): `cargo test --test intake`
Expected: compile error (`ctx_in` uses `..ctx()` fine, but the four new tests fail: the first gets 200 not 409; the folder test fails on `output_dir` missing).

- [ ] **Step 3: Implement `questions_for` and `problems_in`**

In `v2/src-tauri/src/intake.rs` replace the head of `questions()`:

```rust
pub fn questions() -> Vec<Question> {
    questions_for(None)
}

/// The same checklist, with the output question phrased for a working
/// repository when one is set: the folder is decided, so the assistant asks
/// for a NAME rather than inviting an arbitrary path.
pub fn questions_for(output_dir: Option<&str>) -> Vec<Question> {
    let q = |field: &str, ask: &str, why: &str, required: bool| Question {
        field: field.into(),
        ask: ask.into(),
        why: why.into(),
        required,
    };
    let output_ask = match output_dir {
        Some(dir) => format!(
            "What should the finished JSON file be called? It goes in {dir} - give the file name (e.g. login.json), or a full path inside that folder."
        ),
        None => "Where should the finished JSON be written? Give the full path, including the file name.".to_string(),
    };
    vec![
        q(
            "output_path",
            &output_ask,
            "The developer imports this file by hand; it has to land somewhere they expect.",
            true,
        ),
```

(the remaining `q(...)` entries are unchanged). Then replace the head of `problems()`:

```rust
pub fn problems(a: &IntakeAnswers, allowed_modules: &[String]) -> Vec<String> {
    problems_in(a, allowed_modules, None)
}

/// `problems`, plus - when a working repository's cases folder is given -
/// the rule that the output must be inside it: the app imports and watches
/// files there and nowhere else.
pub fn problems_in(
    a: &IntakeAnswers,
    allowed_modules: &[String],
    cases_dir: Option<&std::path::Path>,
) -> Vec<String> {
    let mut out = vec![];

    let output = a.output_path.trim();
    if output.is_empty() {
        out.push("output_path is required - ask the developer where the JSON should go.".into());
    } else {
        if !output.to_lowercase().ends_with(".json") {
            out.push(format!("output_path '{output}' does not end in .json."));
        }
        if let Some(dir) = std::path::Path::new(output).parent() {
            if !dir.as_os_str().is_empty() && !dir.is_dir() {
                out.push(format!(
                    "the folder for output_path does not exist: {} - confirm the path with the developer rather than creating it.",
                    dir.display()
                ));
            }
        }
        if let Some(dir) = cases_dir {
            if !crate::workspace::is_inside(dir, std::path::Path::new(output)) {
                out.push(format!(
                    "output_path must be inside the working repository's {} folder ({}) - the app only imports and watches files there. Give a file name, or a path under that folder.",
                    crate::workspace::CASES_DIR,
                    dir.display()
                ));
            }
        }
    }
```

(everything from `if a.spec_paths.iter().all(...)` down is unchanged.)

- [ ] **Step 4: Gate and redirect `begin_writing`**

In `v2/src-tauri/src/ai_bridge.rs`, inside `begin_writing`, insert immediately after `let modules: Vec<String> = allowed.known().to_vec();`:

```rust
    // No repository, no job: the whole point of the intake is agreeing
    // where the file goes, and that place is now `<repo>/.test-cases`.
    let root = ctx
        .working_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from);
    let Some(root) = root else {
        return (
            409,
            serde_json::json!({
                "status": "blocked",
                "error": "No working repository is set in Test Case Manager. Ask the developer to open the AI Bridge tab and pick the repository these cases belong to, then call this tool again.",
            })
            .to_string(),
        );
    };
    let cases_dir = match crate::workspace::ensure_cases_dir(&root) {
        Ok(d) => d,
        Err(e) => {
            return (500, serde_json::json!({ "status": "error", "error": e }).to_string());
        }
    };
    let cases_dir_str = cases_dir.to_string_lossy().to_string();
```

Change phase 1's response: `"ask_the_developer": crate::intake::questions_for(Some(&cases_dir_str)),` and inside `"context"` add two keys after `"allowed_modules": modules,`:

```rust
                    "output_dir": cases_dir_str,
                    "suggested_output_path": crate::workspace::default_output_path(&root, &feature),
```

Change phase 2: make `answers` mutable and resolve before checking:

```rust
    let mut answers: crate::intake::IntakeAnswers = match serde_json::from_str(body) {
```
```rust
    answers.output_path = crate::workspace::resolve_output(&root, &answers.output_path);
    let problems = crate::intake::problems_in(&answers, &modules, Some(&cases_dir));
```

(`announce_intake_path(answers.output_path.trim())` further down now announces the resolved path - nothing else to change there.)

- [ ] **Step 5: Run the intake and bridge suites**

Run: `cargo test --test intake --test ai_bridge`
Expected: all `ok` (including the four new tests).

- [ ] **Step 6: Commit**

```bash
cd D:/azure-devops-test-case-creator && git add v2/src-tauri/src/intake.rs v2/src-tauri/src/ai_bridge.rs v2/src-tauri/tests/intake.rs && git commit -F - <<'EOF'
feat(v2): the intake writes into the repo's .test-cases folder

begin_test_case_writing refuses to start without a working repository
(409 blocked, naming the AI Bridge tab), creates <repo>/.test-cases
itself, suggests <slug>.json inside it, resolves a bare file name into
the folder, and refuses an output path outside it - the app imports and
watches files there and nowhere else.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
```

---

### Task 5: Per-repo targets in the `ai_tools` rules module

**Files:**
- Modify: `v2/src-tauri/src/ai_tools.rs` (`TOOL_SPECS` :7-60, `ToolSpec` :385-400, `DetectedTool` :416-425, `detect` :437-463, command dir helpers :493-525)
- Test: `v2/src-tauri/tests/ai_tools.rs`

**Interfaces:**
- Produces (all `pub` in `v2_lib::ai_tools`):
  - `ToolSpec.project_config: Option<fn(root: &str) -> PathBuf>`
  - `DetectedTool.scope: String` — `"project"` or `"global"`
  - `fn config_for(spec: &ToolSpec, home: &str, appdata: &str, root: Option<&str>) -> (PathBuf, &'static str, &'static str)` → (config path, entry key, scope)
  - `fn detect_in(home, appdata, on_path, root: Option<&str>) -> Vec<DetectedTool>` (`detect` = `detect_in(.., None)`)
  - `fn project_command_dir(root: &str) -> PathBuf` → `<root>/.claude/commands/tcm`
  - `fn command_files_in(dir: &Path, disabled: &[String]) -> Vec<(PathBuf, String)>` (`command_files_for(home, d)` = `command_files_in(&command_dir(home), d)`)

- [ ] **Step 1: Write the failing tests**

Append to `v2/src-tauri/tests/ai_tools.rs` (and add `config_for, detect_in, project_command_dir, command_files_in, TOOL_SPECS` to the `use v2_lib::ai_tools::{...}` import):

```rust
// ---------------------------------------------------------- per-repo scope

/// Which tools can be told about a server per repository, and where.
#[test]
fn project_configs_sit_in_the_repo_for_the_tools_that_have_them() {
    let at = |id: &str| TOOL_SPECS.iter().find(|s| s.id == id).unwrap();
    let path = |id: &str| {
        let (p, _, _) = config_for(at(id), "C:/Users/Sam", "C:/Users/Sam/AppData/Roaming", Some("D:/repo"));
        p.display().to_string().replace(std::path::MAIN_SEPARATOR, "/")
    };
    assert_eq!(path("claude-code"), "D:/repo/.mcp.json");
    assert_eq!(path("cursor"), "D:/repo/.cursor/mcp.json");
    assert_eq!(path("vscode"), "D:/repo/.vscode/mcp.json");
    // No project scope exists for these two - a repo changes nothing.
    assert!(path("claude-desktop").contains("claude_desktop_config.json"));
    assert!(path("windsurf").contains("mcp_config.json"));
    assert_eq!(config_for(at("vscode"), "h", "a", Some("D:/repo")).1, "servers");
    assert_eq!(config_for(at("claude-code"), "h", "a", Some("D:/repo")).2, "project");
    assert_eq!(config_for(at("claude-code"), "h", "a", None).2, "global");
    assert_eq!(config_for(at("windsurf"), "h", "a", Some("D:/repo")).2, "global");
}

/// With a repo given, registration state is read from the repo's own
/// config - a user-scope entry must not make the repo look registered.
#[test]
fn detect_reads_the_repo_config_when_a_working_dir_is_given() {
    let (home, appdata) = fake_layout(); // ~/.claude.json carries tcm-testcases globally
    let repo = TempDir::new();
    std::fs::write(
        repo.path().join(".mcp.json"),
        r#"{"mcpServers": {"phr-db-mcp": {"command": "db.exe"}}}"#,
    )
    .unwrap();
    let home_str = home.path().to_string_lossy().to_string();
    let appdata_str = appdata.path().to_string_lossy().to_string();
    let repo_str = repo.path().to_string_lossy().to_string();
    let on_path = |_cmd: &str| false;

    let tools = detect_in(&home_str, &appdata_str, &on_path, Some(&repo_str));
    let cc = tools.iter().find(|t| t.id == "claude-code").unwrap();
    assert_eq!(cc.scope, "project");
    assert_eq!(cc.registered_servers, vec![DB_SERVER], "the repo has the DB server, not ours");
    let cd = tools.iter().find(|t| t.id == "claude-desktop").unwrap();
    assert_eq!(cd.scope, "global", "no project config exists for Claude Desktop");
}

#[test]
fn without_a_working_dir_detection_is_global_and_says_so() {
    let (home, appdata) = fake_layout();
    let home_str = home.path().to_string_lossy().to_string();
    let appdata_str = appdata.path().to_string_lossy().to_string();
    let on_path = |_cmd: &str| false;
    let tools = detect_in(&home_str, &appdata_str, &on_path, None);
    assert!(tools.iter().all(|t| t.scope == "global"));
    assert_eq!(tools, detect(&home_str, &appdata_str, &on_path));
}

/// The skills go where Claude Code looks for a repository's own commands.
#[test]
fn the_repo_commands_land_under_the_repos_dot_claude() {
    let dir = project_command_dir("D:/repo").display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    assert_eq!(dir, "D:/repo/.claude/commands/tcm");
    let files = command_files_in(&project_command_dir("D:/repo"), &[]);
    assert_eq!(files.len(), COMMANDS.len());
    let first = files[0].0.display().to_string().replace(std::path::MAIN_SEPARATOR, "/");
    assert_eq!(first, "D:/repo/.claude/commands/tcm/write.md");
    assert!(files[0].1.contains(COMMAND_MARKER), "still ours to remove later");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test ai_tools`
Expected: compile errors for `config_for`, `detect_in`, `project_command_dir`, `command_files_in`, `TOOL_SPECS`.

- [ ] **Step 3: Implement**

In `v2/src-tauri/src/ai_tools.rs`:

(a) Add the field to `ToolSpec` (after `entry_key`):

```rust
    /// Where a REPOSITORY's own copy of the config lives, for tools that
    /// read one - `None` for tools that only know a global config.
    pub project_config: Option<fn(root: &str) -> PathBuf>,
```

(b) In `TOOL_SPECS`, add to each entry after `entry_key`:
- claude-code: `project_config: Some(|root| PathBuf::from(root).join(".mcp.json")),`
- claude-desktop: `project_config: None,`
- vscode: `project_config: Some(|root| PathBuf::from(root).join(".vscode").join("mcp.json")),`
- cursor: `project_config: Some(|root| PathBuf::from(root).join(".cursor").join("mcp.json")),`
- windsurf: `project_config: None,`

(c) Add `pub scope: String,` to `DetectedTool` (after `registered_servers`) with the doc comment `/// "project" when this row reflects the working repository's config, "global" when the tool has none and the machine-wide config was read.`

(d) Replace `detect` with:

```rust
/// The config a registration for `spec` goes into, and which scope that
/// is. A repository wins for every tool that reads one; the two that do
/// not (Claude Desktop, Windsurf) stay global however the app is set.
pub fn config_for(
    spec: &ToolSpec,
    home: &str,
    appdata: &str,
    root: Option<&str>,
) -> (PathBuf, &'static str, &'static str) {
    match (root, spec.project_config) {
        (Some(r), Some(f)) => (f(r), spec.entry_key, "project"),
        _ => ((spec.config_path)(home, appdata), spec.entry_key, "global"),
    }
}

pub fn detect(home: &str, appdata: &str, on_path: &dyn Fn(&str) -> bool) -> Vec<DetectedTool> {
    detect_in(home, appdata, on_path, None)
}

/// Detects installed/registered state for every known tool, reading each
/// tool's REPOSITORY config when `root` is given and the tool has one.
pub fn detect_in(
    home: &str,
    appdata: &str,
    on_path: &dyn Fn(&str) -> bool,
    root: Option<&str>,
) -> Vec<DetectedTool> {
    TOOL_SPECS
        .iter()
        .map(|spec| {
            let installed = is_installed(spec, home, appdata, on_path);
            let (config_path, key, scope) = config_for(spec, home, appdata, root);
            let entries = std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get(key).cloned());
            let registered_servers = MANAGED_SERVERS
                .iter()
                .filter(|name| entries.as_ref().and_then(|e| e.get(**name)).is_some())
                .map(|name| name.to_string())
                .collect();
            DetectedTool {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                installed,
                registered_servers,
                scope: scope.to_string(),
            }
        })
        .collect()
}
```

(e) Add after `command_dir`:

```rust
/// `<repo>/.claude/commands/tcm/` - the same namespace, inside the
/// repository, so `/tcm:*` exists only where the app was pointed.
pub fn project_command_dir(root: &str) -> PathBuf {
    PathBuf::from(root).join(".claude").join("commands").join("tcm")
}
```

and change `command_files_for` to delegate:

```rust
pub fn command_files_for(home: &str, disabled: &[String]) -> Vec<(PathBuf, String)> {
    command_files_in(&command_dir(home), disabled)
}

/// Every command file for `dir` - the global or the repository set.
pub fn command_files_in(dir: &std::path::Path, disabled: &[String]) -> Vec<(PathBuf, String)> {
    COMMANDS
        .iter()
        .filter(|c| !disabled.iter().any(|d| d == c.tool))
        .map(|c| (dir.join(format!("{}.md", c.stem)), command_markdown(c)))
        .collect()
}
```

- [ ] **Step 4: Run the suite**

Run: `cargo test --test ai_tools`
Expected: all `ok` (the four new tests plus every existing one - `detect` keeps its signature).

- [ ] **Step 5: Commit**

```bash
cd D:/azure-devops-test-case-creator && git add v2/src-tauri/src/ai_tools.rs v2/src-tauri/tests/ai_tools.rs && git commit -F - <<'EOF'
feat(v2): ai_tools knows each tool's repository config and command dir

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
```

---

### Task 6: Register, unregister and sync per repository (Rust I/O)

**Files:**
- Modify: `v2/src-tauri/src/commands/ai_tools.rs` (whole registration half: `detect_ai_tools`, `register_ai_tool`, `register_db_server`, `unregister_*`, `register_server`, `unregister_server`, `write_command*`, `remove_command`, `sync_commands`, the claude helpers, `mcp_add_args`, and its `#[cfg(test)]` module)
- Modify: `v2/src-tauri/src/commands/ai_bridge.rs:95-105` (the `sync_commands` call)
- Regenerate: `v2/src/bindings.ts`

**Interfaces:**
- Consumes: Task 5's `config_for`, `detect_in`, `project_command_dir`, `command_files_in`; Task 1's `exclude_locally`.
- Produces (Tauri commands; TS names in brackets):
  - `detect_ai_tools(working_dir: Option<String>)` [`detectAiTools(workingDir)`]
  - `register_ai_tool(id, working_dir: Option<String>)` [`registerAiTool(id, workingDir)`]
  - `unregister_ai_tool(id, working_dir)`, `register_db_server(id, config, working_dir)`, `unregister_db_server(id, working_dir)`
  - `pub fn sync_commands(disabled: &[String], working_dir: Option<&str>)`

- [ ] **Step 1: Write the failing unit tests**

In the `#[cfg(test)] mod tests` at the bottom of `v2/src-tauri/src/commands/ai_tools.rs`, change the two existing tests to pass a scope and add one:

```rust
        let args = mcp_add_args(&server, "project");
```
(in `mcp_add_puts_the_name_before_the_env_pairs`), and

```rust
    #[test]
    fn mcp_add_without_env_is_name_then_command() {
        let server = McpServer {
            name: "tcm-testcases".to_string(),
            command: "v2.exe".to_string(),
            args: vec!["--mcp".to_string()],
            env: Default::default(),
        };
        assert_eq!(
            mcp_add_args(&server, "user"),
            vec!["mcp", "add", "--scope", "user", "tcm-testcases", "--", "v2.exe", "--mcp"]
        );
    }

    /// A repository registration is `--scope project`, which the CLI keys
    /// on its cwd - the caller runs it inside the repo (see
    /// `run_claude_mcp_add`).
    #[test]
    fn a_repo_registration_asks_for_project_scope() {
        let server = McpServer {
            name: "tcm-testcases".to_string(),
            command: "v2.exe".to_string(),
            args: vec!["--mcp".to_string()],
            env: Default::default(),
        };
        let args = mcp_add_args(&server, "project");
        assert_eq!(&args[2..4], ["--scope", "project"]);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --lib commands::ai_tools`
Expected: compile error - `mcp_add_args` takes 1 argument.

- [ ] **Step 3: Rewrite the registration half**

Replace the `use crate::ai_tools::{...}` import at the top of `v2/src-tauri/src/commands/ai_tools.rs` with:

```rust
use crate::ai_tools::{
    atomic_write, command_dir, command_files_in, config_for, detect_in, is_installed,
    legacy_command_path, merge_entry, project_command_dir, remove_entry, tcm_server,
    DetectedTool, McpServer, ToolSpec, COMMAND_MARKER, DB_SERVER, TCM_SERVER, TOOL_SPECS,
};
```

Replace `detect_ai_tools`, `register_ai_tool`, `register_db_server`, `unregister_db_server`, `register_server`, `unregister_ai_tool`, `unregister_server`, `write_command`, `sync_commands`, `write_commands_for`, `remove_command`, `unregister_claude_code`, `unregister_claude_code_via_config`, `run_claude_mcp_remove`, `register_claude_code_via_config`, `register_claude_code`, `mcp_add_args` and `run_claude_mcp_add` with the following (keep `home_dir`, `appdata_dir`, `is_on_path`, `DbServerConfig` + its `impl`, `db_server_defaults`, `db_server_presets`, `claude_cli`, `DbPresetOut` exactly as they are):

```rust
/// A trimmed, non-empty working directory, or None.
fn root_of(working_dir: Option<&str>) -> Option<&str> {
    working_dir.map(str::trim).filter(|s| !s.is_empty())
}

#[tauri::command]
#[specta::specta]
pub fn detect_ai_tools(working_dir: Option<String>) -> Vec<DetectedTool> {
    detect_in(&home_dir(), &appdata_dir(), &is_on_path, root_of(working_dir.as_deref()))
}

#[tauri::command]
#[specta::specta]
pub fn register_ai_tool(id: String, working_dir: Option<String>) -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("failed to resolve current exe: {e}"))?
        .to_string_lossy()
        .to_string();
    register_server(&id, &tcm_server(&exe), working_dir.as_deref())
}

#[tauri::command]
#[specta::specta]
pub fn register_db_server(id: String, config: DbServerConfig, working_dir: Option<String>) -> Result<(), String> {
    register_server(&id, &config.to_server()?, working_dir.as_deref())
}

#[tauri::command]
#[specta::specta]
pub fn unregister_db_server(id: String, working_dir: Option<String>) -> Result<(), String> {
    unregister_server(&id, DB_SERVER, working_dir.as_deref())
}

#[tauri::command]
#[specta::specta]
pub fn unregister_ai_tool(id: String, working_dir: Option<String>) -> Result<(), String> {
    unregister_server(&id, TCM_SERVER, working_dir.as_deref())
}

/// The repository a registration for `spec` targets: a tool with a project
/// config needs one and refuses without (the UI never offers that - the
/// AI Bridge tab is gated on the repository); a tool without one ignores it.
fn project_root<'a>(spec: &ToolSpec, working_dir: Option<&'a str>) -> Result<Option<&'a str>, String> {
    match (spec.project_config, root_of(working_dir)) {
        (Some(_), Some(r)) => Ok(Some(r)),
        (Some(_), None) => Err(format!(
            "pick a working repository first - {} registers per repository",
            spec.name
        )),
        (None, _) => Ok(None),
    }
}

/// Merge `server` into the JSON config at `path` (created if absent).
fn merge_into_file(path: &std::path::Path, key: &str, server: &McpServer) -> Result<(), String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(e) => return Err(format!("failed to read {}: {e}", path.display())),
    };
    let merged = merge_entry(&existing, key, server)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    atomic_write(path, &merged)
}

/// Remove `name` from the JSON config at `path`. Missing file or entry is
/// a clean no-op - the state the caller asked for.
fn remove_from_file(path: &std::path::Path, key: &str, name: &str) -> Result<(), String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("failed to read {}: {e}", path.display())),
    };
    match remove_entry(&existing, key, name)? {
        Some(updated) => atomic_write(path, &updated),
        None => Ok(()),
    }
}

/// Shared by both servers: refuse a tool that isn't installed, pick the
/// repository or global target, then either shell out to the claude CLI
/// or merge into the tool's JSON config. A repository registration also
/// retires the app's own global copies - a user-scope server of the same
/// name would shadow the project one, and `/tcm:*` twice in the picker is
/// exactly the confusion per-repo scoping removes.
fn register_server(id: &str, server: &McpServer, working_dir: Option<&str>) -> Result<(), String> {
    let spec = TOOL_SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("unknown AI tool id: {id}"))?;

    if !is_installed(spec, &home_dir(), &appdata_dir(), &is_on_path) {
        return Err(format!("{} is not installed", spec.name));
    }
    let root = project_root(spec, working_dir)?;

    if spec.id == "claude-code" {
        // `project_root` guarantees Some for a tool with a project config.
        let r = root.ok_or_else(|| "pick a working repository first".to_string())?;
        register_claude_code_in(r, server)?;
        if server.name == TCM_SERVER {
            // Best-effort, and deliberately after the server is in: a
            // command pointing at tools that are not registered would be
            // worse than no command. A failure here does not undo a
            // registration that worked.
            if let Err(e) = write_commands_in(&project_command_dir(r), &[]) {
                crate::applog::warn(format!("could not write the Claude Code commands: {e}"));
            }
        }
        if server.name == DB_SERVER {
            // The connection string is in .mcp.json now; keep that file out
            // of `git status` for this checkout (owner's decision - the
            // repo's .gitignore is not ours to edit).
            if let Err(e) = crate::workspace::exclude_locally(std::path::Path::new(r), ".mcp.json") {
                crate::applog::warn(format!("could not exclude .mcp.json locally: {e}"));
            }
        }
        retire_global(spec, &server.name);
        return Ok(());
    }

    let (config_path, key, _scope) = config_for(spec, &home_dir(), &appdata_dir(), root);
    merge_into_file(&config_path, key, server)?;
    if root.is_some() {
        retire_global(spec, &server.name);
    }
    Ok(())
}

/// Take the app's OWN global copy away once the repository carries it.
/// Only managed names ever reach here, and command files are removed only
/// when they carry our marker. Best-effort: the repository registration
/// has already succeeded, and a global leftover is a nuisance, not a fault.
fn retire_global(spec: &ToolSpec, server_name: &str) {
    let result = if spec.id == "claude-code" {
        if server_name == TCM_SERVER {
            let _ = remove_commands_in(&command_dir(&home_dir()));
        }
        unregister_claude_code(server_name)
    } else {
        let (path, key, _) = config_for(spec, &home_dir(), &appdata_dir(), None);
        remove_from_file(&path, key, server_name)
    };
    if let Err(e) = result {
        crate::applog::warn(format!("could not retire the global {server_name} registration: {e}"));
    }
}

/// Removes a server from the repository's config when one is set and the
/// tool has such a config, else from the global one. No installed-guard:
/// if a config still carries an entry after the tool was uninstalled,
/// removing it is exactly what the user wants.
fn unregister_server(id: &str, server_name: &str, working_dir: Option<&str>) -> Result<(), String> {
    let spec = TOOL_SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("unknown AI tool id: {id}"))?;
    let root = match (spec.project_config, root_of(working_dir)) {
        (Some(_), Some(r)) => Some(r),
        _ => None,
    };

    if spec.id == "claude-code" {
        return match root {
            Some(r) => {
                if server_name == TCM_SERVER {
                    let _ = remove_commands_in(&project_command_dir(r));
                }
                unregister_claude_code_in(r, server_name)
            }
            None => {
                if server_name == TCM_SERVER {
                    let _ = remove_commands_in(&command_dir(&home_dir()));
                }
                unregister_claude_code(server_name)
            }
        };
    }

    let (config_path, key, _) = config_for(spec, &home_dir(), &appdata_dir(), root);
    remove_from_file(&config_path, key, server_name)
}

/// Bring a command directory into line with which tools are on: the
/// repository's when one is set, else the global one. A no-op unless the
/// directory already exists - somebody who never registered should not
/// acquire a command set because they changed an unrelated setting.
///
/// Public so `set_bridge_context` can call it when the AI Bridge tab's
/// toggles move.
pub fn sync_commands(disabled: &[String], working_dir: Option<&str>) {
    let dir = match root_of(working_dir) {
        Some(r) => project_command_dir(r),
        None => command_dir(&home_dir()),
    };
    if !dir.is_dir() {
        return;
    }
    if let Err(e) = write_commands_in(&dir, disabled) {
        crate::applog::warn(format!("could not sync the Claude Code commands: {e}"));
    }
}

/// Write the command set into `dir`, dropping ours for any tool that is
/// switched off. Refuses to overwrite a file this app did not write - the
/// paths are predictable and shared with whatever else the user keeps
/// there. One failure does not abandon the rest, but the first reason is
/// reported.
fn write_commands_in(dir: &std::path::Path, disabled: &[String]) -> Result<(), String> {
    // An earlier version wrote one top-level GLOBAL file. Leaving it would
    // put `/tcm-testcases` in the picker beside the namespaced set. Only
    // ours, and only when writing the global set.
    if dir == command_dir(&home_dir()) {
        let legacy = legacy_command_path(&home_dir());
        if matches!(std::fs::read_to_string(&legacy), Ok(t) if t.contains(COMMAND_MARKER)) {
            let _ = std::fs::remove_file(&legacy);
        }
    }

    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;

    // A tool switched off loses its command; switched back on, it returns.
    let wanted = command_files_in(dir, disabled);
    for (path, _) in command_files_in(dir, &[]) {
        let keep = wanted.iter().any(|(p, _)| *p == path);
        if !keep && matches!(std::fs::read_to_string(&path), Ok(t) if t.contains(COMMAND_MARKER)) {
            let _ = std::fs::remove_file(&path);
        }
    }

    let mut first_error: Option<String> = None;
    for (path, contents) in wanted {
        if matches!(std::fs::read_to_string(&path), Ok(t) if !t.contains(COMMAND_MARKER)) {
            let msg = format!(
                "{} already exists and was not written by this app - left alone",
                path.display()
            );
            first_error.get_or_insert(msg);
            continue;
        }
        if let Err(e) = atomic_write(&path, &contents) {
            first_error.get_or_insert(e);
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Take a command set away with its registration - a command pointing at
/// tools that are no longer connected is worse than none. Ours only.
fn remove_commands_in(dir: &std::path::Path) -> Result<(), String> {
    let mut paths: Vec<PathBuf> = command_files_in(dir, &[]).into_iter().map(|(p, _)| p).collect();
    if dir == command_dir(&home_dir()) {
        paths.push(legacy_command_path(&home_dir()));
    }
    for path in paths {
        if matches!(std::fs::read_to_string(&path), Ok(t) if t.contains(COMMAND_MARKER)) {
            let _ = std::fs::remove_file(&path);
        }
    }
    // Only if empty - `remove_dir` refuses otherwise, which is exactly the
    // guard wanted when the user has put something of their own in there.
    let _ = std::fs::remove_dir(dir);
    Ok(())
}

// ------------------------------------------------------------ claude code

/// User scope, as before per-repo scoping: still used to retire the app's
/// old global entry. CLI first, PATH second, the config file last.
fn unregister_claude_code(server_name: &str) -> Result<(), String> {
    match claude_cli() {
        Some(cli) => run_claude_mcp_remove(&cli, server_name, "user", None),
        None => match run_claude_mcp_remove(&PathBuf::from("claude"), server_name, "user", None) {
            Ok(()) => Ok(()),
            Err(_) => remove_from_file(
                &PathBuf::from(home_dir()).join(".claude.json"),
                "mcpServers",
                server_name,
            ),
        },
    }
}

/// Project scope: `claude mcp remove --scope project` run INSIDE the repo
/// (the CLI keys project scope on its cwd), falling back to editing
/// `<repo>/.mcp.json` - the file that command would have edited.
fn unregister_claude_code_in(root: &str, server_name: &str) -> Result<(), String> {
    let cwd = std::path::Path::new(root);
    let via_file = || remove_from_file(&cwd.join(".mcp.json"), "mcpServers", server_name);
    match claude_cli() {
        Some(cli) => run_claude_mcp_remove(&cli, server_name, "project", Some(cwd)).or_else(|_| via_file()),
        None => match run_claude_mcp_remove(&PathBuf::from("claude"), server_name, "project", Some(cwd)) {
            Ok(()) => Ok(()),
            Err(_) => via_file(),
        },
    }
}

fn run_claude_mcp_remove(
    cli: &std::path::Path,
    server_name: &str,
    scope: &str,
    cwd: Option<&std::path::Path>,
) -> Result<(), String> {
    let mut command = Command::new("cmd");
    command.arg("/C");
    command.arg(cli);
    command.args(["mcp", "remove", "--scope", scope, server_name]);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| format!("failed to run `claude mcp remove`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();
        // Already gone = the state the user asked for.
        if msg.contains("not found") || msg.contains("No MCP server") {
            return Ok(());
        }
        return Err(format!("`claude mcp remove` failed: {msg}"));
    }
    Ok(())
}

/// Project scope: `claude mcp add --scope project` run INSIDE the repo -
/// the CLI writes `<cwd>/.mcp.json`. Prefer the CLI (it owns the schema),
/// try PATH when it is not where the installers put it, and edit
/// `<repo>/.mcp.json` ourselves as the last resort, with the same
/// `merge_entry` every other tool registers through.
fn register_claude_code_in(root: &str, server: &McpServer) -> Result<(), String> {
    let cwd = std::path::Path::new(root);
    let via_file = || merge_into_file(&cwd.join(".mcp.json"), "mcpServers", server);
    if let Some(cli) = claude_cli() {
        return run_claude_mcp_add(&cli, server, "project", Some(cwd)).or_else(|_| via_file());
    }
    match run_claude_mcp_add(&PathBuf::from("claude"), server, "project", Some(cwd)) {
        Ok(()) => Ok(()),
        Err(_) => via_file(),
    }
}

/// The argument order for `claude mcp add`, exactly as the CLI's docs
/// show it: NAME first, then the `-e` pairs, then `--` and the command.
///
/// The order is load-bearing, not style. The CLI's `-e/--env` option is
/// VARIADIC - it keeps consuming arguments until something option-like or
/// `--` stops it - so env pairs placed before the name swallowed the name
/// too, and the CLI then bound the server binary to `name` and reported
/// `missing required argument 'commandOrUrl'`. Only the database server
/// sends env pairs, which is why registering it was the first to break.
fn mcp_add_args(server: &McpServer, scope: &str) -> Vec<String> {
    let mut args = vec![
        "mcp".into(),
        "add".into(),
        "--scope".into(),
        scope.into(),
        server.name.clone(),
    ];
    for (k, v) in &server.env {
        args.push("-e".into());
        args.push(format!("{k}={v}"));
    }
    args.push("--".into());
    args.push(server.command.clone());
    args.extend(server.args.iter().cloned());
    args
}

fn run_claude_mcp_add(
    cli: &std::path::Path,
    server: &McpServer,
    scope: &str,
    cwd: Option<&std::path::Path>,
) -> Result<(), String> {
    // Still via `cmd /C`: the npm install is a `.cmd` shim, which cannot be
    // executed directly.
    let mut command = Command::new("cmd");
    command.arg("/C");
    command.arg(cli);
    command.args(mcp_add_args(server, scope));
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| format!("failed to run `claude mcp add`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`claude mcp add` failed: {}", stderr.trim()));
    }
    Ok(())
}
```

- [ ] **Step 4: Point the bridge's sync at the repository**

In `v2/src-tauri/src/commands/ai_bridge.rs` change the last block of `set_bridge_context` to:

```rust
    static LAST: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    if last.as_deref() != Some(disabled_tools.as_slice()) {
        *last = Some(disabled_tools.clone());
        crate::commands::ai_tools::sync_commands(&disabled_tools, working_dir.as_deref());
    }
```

- [ ] **Step 5: Build, regenerate bindings, run the Rust gates**

Run (from `v2/src-tauri`): `cargo test --lib commands::ai_tools` → 3 passed. Then `cargo test --test bindings` (regenerates `bindings.ts`; check `detectAiTools(workingDir` and `registerAiTool(id, workingDir` appear). Then `cargo test` (full) → all `ok`. Fix any unused-import warnings the compiler reports in `commands/ai_tools.rs`.

- [ ] **Step 6: Commit**

```bash
cd D:/azure-devops-test-case-creator && git add v2/src-tauri v2/src/bindings.ts && git commit -F - <<'EOF'
feat(v2): AI tools register into the working repository

Claude Code, Cursor and VS Code register into <repo>/.mcp.json,
.cursor/mcp.json and .vscode/mcp.json (Claude Code via `claude mcp add
--scope project` run inside the repo, falling back to editing the file);
the /tcm:* skills go to <repo>/.claude/commands/tcm; the DB server's
.mcp.json is excluded locally via .git/info/exclude. Claude Desktop and
Windsurf have no project config and stay global. A repository
registration retires the app's own global copies so nothing is doubled.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
```

---

### Task 7: Frontend store for the working repository

**Files:**
- Create: `v2/src/lib/workingDir.ts`
- Test: `v2/src/lib/workingDir.test.ts`

**Interfaces:**
- Produces: `CASES_DIR = ".test-cases"`, `loadWorkingDir(): string` (`""` = unset), `saveWorkingDir(path: string): void` (empty clears), `subscribeWorkingDir(cb): () => void`, `workingDirSnapshot(): string`, `casesDir(root: string): string`, `isInsideCasesDir(root: string, path: string): boolean`.

- [ ] **Step 1: Write the failing tests**

Create `v2/src/lib/workingDir.test.ts`:

```ts
import { afterEach, expect, test, vi } from "vitest";
import {
  CASES_DIR,
  casesDir,
  isInsideCasesDir,
  loadWorkingDir,
  saveWorkingDir,
  subscribeWorkingDir,
  workingDirSnapshot,
} from "./workingDir";

afterEach(() => localStorage.clear());

test("unset reads as empty, and saving persists and notifies", () => {
  expect(loadWorkingDir()).toBe("");
  const heard = vi.fn();
  const off = subscribeWorkingDir(heard);
  saveWorkingDir("  D:\\repo  ");
  expect(loadWorkingDir()).toBe("D:\\repo");
  expect(workingDirSnapshot()).toBe("D:\\repo");
  expect(heard).toHaveBeenCalledTimes(1);
  off();
  saveWorkingDir("");
  expect(loadWorkingDir()).toBe("");
  expect(heard).toHaveBeenCalledTimes(1);
});

test("the cases folder sits directly under the repo", () => {
  expect(casesDir("D:\\repo")).toBe(`D:\\repo\\${CASES_DIR}`);
  expect(casesDir("D:/repo/")).toBe(`D:/repo\\${CASES_DIR}`);
});

test("inside-check ignores case and slash style, and is exact about the prefix", () => {
  expect(isInsideCasesDir("D:\\repo", "D:\\repo\\.test-cases\\login.json")).toBe(true);
  expect(isInsideCasesDir("D:\\repo", "d:/REPO/.test-cases/login.json")).toBe(true);
  expect(isInsideCasesDir("D:\\repo", "D:\\repo\\login.json")).toBe(false);
  expect(isInsideCasesDir("D:\\repo", "D:\\repo\\.test-cases-old\\login.json")).toBe(false);
});
```

- [ ] **Step 2: Run to verify it fails**

Run (from `v2`): `npx vitest run src/lib/workingDir.test.ts`
Expected: FAIL - cannot resolve `./workingDir`.

- [ ] **Step 3: Write the module**

Create `v2/src/lib/workingDir.ts`:

```ts
// The working repository: the one folder per-repo test-case files, skills
// and MCP registrations belong to. One choice for the whole app (a repo
// can serve several org/projects), persisted so it survives restarts, and
// observable so App re-pushes the bridge context the moment it changes.

const KEY = "tcm-v2-working-dir";

/** Mirrors `workspace::CASES_DIR` on the Rust side. */
export const CASES_DIR = ".test-cases";

export function loadWorkingDir(): string {
  try {
    return (localStorage.getItem(KEY) ?? "").trim();
  } catch {
    return "";
  }
}

const listeners = new Set<() => void>();

export function saveWorkingDir(path: string): void {
  const trimmed = path.trim();
  try {
    if (trimmed) localStorage.setItem(KEY, trimmed);
    else localStorage.removeItem(KEY);
  } catch {
    // storage unavailable -> the choice lasts for this session only
  }
  for (const l of listeners) l();
}

export function subscribeWorkingDir(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Strings compare by value, so a fresh read is a stable snapshot. */
export function workingDirSnapshot(): string {
  return loadWorkingDir();
}

export function casesDir(root: string): string {
  return `${root.replace(/[\\/]+$/, "")}\\${CASES_DIR}`;
}

const norm = (p: string) => p.replace(/\//g, "\\").replace(/\\+$/, "").toLowerCase();

/** Is `path` inside `<root>/.test-cases`? A sibling folder that merely
 * shares the prefix is outside. Matches `workspace::is_inside` in spirit;
 * the Rust side is authoritative where a file is actually written. */
export function isInsideCasesDir(root: string, path: string): boolean {
  const d = norm(casesDir(root));
  const p = norm(path);
  return p === d || p.startsWith(`${d}\\`);
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `npx vitest run src/lib/workingDir.test.ts` → 3 passed.

- [ ] **Step 5: Commit**

```bash
cd D:/azure-devops-test-case-creator && git add v2/src/lib/workingDir.ts v2/src/lib/workingDir.test.ts && git commit -F - <<'EOF'
feat(v2): working-repository store

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
```

---

### Task 8: App pushes the working repository to the bridge

**Files:**
- Modify: `v2/src/App.tsx` (imports; the `setBridgeContext` effect ~:389-414)
- Test: `v2/src/App.test.tsx` (the test containing `set_bridge_context`, ~:160-190)

**Interfaces:**
- Consumes: Task 7's `subscribeWorkingDir`, `workingDirSnapshot`; Task 3's sixth argument.

- [ ] **Step 1: Extend the existing test**

In `v2/src/App.test.tsx`, in the test that collects `pushes` from `set_bridge_context`, add before `renderApp();`:

```ts
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
```

and change the final `toMatchObject` to:

```ts
  expect(pushes[pushes.length - 1]).toMatchObject({
    organization: "acme",
    project: "Web",
    workingDir: "D:\\repo",
  });
```

- [ ] **Step 2: Run to verify it fails**

Run: `npx vitest run src/App.test.tsx -t "bridge"`
Expected: FAIL - `workingDir` is `null`.

- [ ] **Step 3: Wire it**

In `v2/src/App.tsx` add the import:

```ts
import { subscribeWorkingDir, workingDirSnapshot } from "./lib/workingDir";
```

and change the effect:

```tsx
  const disabledTools = useSyncExternalStore(subscribeDisabledTools, disabledToolsSnapshot);
  // The working repository decides where a writing job's file goes, so the
  // bridge learns of a change the moment the AI Bridge tab makes it.
  const workingDir = useSyncExternalStore(subscribeWorkingDir, workingDirSnapshot);
  useEffect(() => {
    if (!signedIn || !org || !project) return;
    commands
      .bridgeStatus()
      .then(() =>
        commands.setBridgeContext(
          org,
          project,
          bridgePrefs.moduleRef,
          bridgePrefs.preconditionsRef,
          disabledTools,
          workingDir || null,
        ),
      )
      .catch(() => {});
  }, [
    signedIn,
    org,
    project,
    bridgePrefs.moduleRef,
    bridgePrefs.preconditionsRef,
    disabledTools,
    workingDir,
  ]);
```

- [ ] **Step 4: Run to verify it passes**

Run: `npx vitest run src/App.test.tsx` → all pass.

- [ ] **Step 5: Commit**

```bash
cd D:/azure-devops-test-case-creator && git add v2/src/App.tsx v2/src/App.test.tsx && git commit -F - <<'EOF'
feat(v2): the bridge is told the working repository

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
```

---

### Task 9: AI Bridge tab - repository card, gate, per-repo detection and labels

**Files:**
- Modify: `v2/src/screens/AiBridge.tsx` (imports :1-30; queries/mutations :50-80; the return :150-265; the "Other tools" snippet :268-300)
- Test: `v2/src/screens/AiBridge.test.tsx`

**Interfaces:**
- Consumes: Task 7's store; Task 6's command signatures (`detectAiTools(workingDir)`, `registerAiTool(id, workingDir)`, `unregisterAiTool(id, workingDir)`, `registerDbServer(id, db, workingDir)`, `unregisterDbServer(id, workingDir)`); `DetectedTool.scope`.

- [ ] **Step 1: Write the failing tests**

In `v2/src/screens/AiBridge.test.tsx`, add a `beforeEach` that sets the repository for every EXISTING test (they assume the tab is usable), and add the new tests. Put this right after the clipboard `beforeEach`:

```ts
// Every existing test assumes the tab is usable, which now needs a working
// repository. The gating tests below clear it themselves.
beforeEach(() => {
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
});
```

Then append:

```ts
test("without a working repository only the picker is offered", async () => {
  localStorage.removeItem("tcm-v2-working-dir");
  let detected = 0;
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools") {
      detected += 1;
      return [{ id: "claude-code", name: "Claude Code", installed: true, registered_servers: [], scope: "global" }];
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  expect(await screen.findByText("Working repository")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /pick repository/i })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Register" })).not.toBeInTheDocument();
  expect(screen.queryByText("Connect your AI tools")).not.toBeInTheDocument();
  expect(detected).toBe(0);
});

test("picking a folder unlocks the tab and detection runs against it", async () => {
  localStorage.removeItem("tcm-v2-working-dir");
  const detectArgs: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "plugin:dialog|open") return "D:\\repo";
    if (cmd === "detect_ai_tools") {
      detectArgs.push(args);
      return [{ id: "claude-code", name: "Claude Code", installed: true, registered_servers: [], scope: "project" }];
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  fireEvent.click(await screen.findByRole("button", { name: /pick repository/i }));
  expect(await screen.findByText("Claude Code")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-working-dir")).toBe("D:\\repo");
  expect(detectArgs[0]).toMatchObject({ workingDir: "D:\\repo" });
  expect(screen.getByText("in this repo")).toBeInTheDocument();
});

test("Register passes the working repository along", async () => {
  let seen: unknown;
  mockIPC((cmd, args) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [{ id: "cursor", name: "Cursor", installed: true, registered_servers: [], scope: "project" }];
    if (cmd === "register_ai_tool") {
      seen = args;
      return null;
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  fireEvent.click(await screen.findByRole("button", { name: "Register" }));
  await waitFor(() => expect(seen).toMatchObject({ id: "cursor", workingDir: "D:\\repo" }));
});

test("a tool with no project config is labelled global", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
    if (cmd === "detect_ai_tools")
      return [{ id: "windsurf", name: "Windsurf", installed: true, registered_servers: ["tcm-testcases"], scope: "global" }];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderBridge(qc);

  expect(await screen.findByText("Windsurf")).toBeInTheDocument();
  expect(screen.getByText("global")).toBeInTheDocument();
});
```

Also add `scope: "global"` to every `detect_ai_tools` mock object in the existing tests (search the file for `registered_servers:`).

- [ ] **Step 2: Run to verify they fail**

Run: `npx vitest run src/screens/AiBridge.test.tsx`
Expected: the four new tests fail (no "Working repository" text; `workingDir` undefined in args).

- [ ] **Step 3: Implement the card, the gate and the arguments**

In `v2/src/screens/AiBridge.tsx`:

(a) Imports - change the lucide line and add the store:

```ts
import { Database, FolderOpen } from "lucide-react";
import { useEffect, useState, useSyncExternalStore } from "react";
```
```ts
import { saveWorkingDir, subscribeWorkingDir, workingDirSnapshot } from "../lib/workingDir";
```

(b) Inside the component, before `const bridge = useQuery(...)`:

```tsx
  // The repository everything on this tab is scoped to. Read through the
  // store so App's bridge push and this tab agree the moment it changes.
  const workingDir = useSyncExternalStore(subscribeWorkingDir, workingDirSnapshot);
  const pickWorkingDir = () => {
    open({ multiple: false, directory: true })
      .then((path) => {
        if (typeof path === "string") {
          saveWorkingDir(path);
          toast.success(`Working repository set to ${path}`);
        }
      })
      .catch(() => toast.error("Could not open the folder picker."));
  };
```

(c) Key detection on the repository and pass it to every registration call:

```tsx
  const tools = useQuery({
    queryKey: ["ai-tools", workingDir],
    queryFn: () => commands.detectAiTools(workingDir || null),
    enabled: Boolean(workingDir),
  });

  const register = useMutation({
    mutationFn: (id: string) => unwrapStr(commands.registerAiTool(id, workingDir || null)),
```
```tsx
  const unregister = useMutation({
    mutationFn: (id: string) => unwrapStr(commands.unregisterAiTool(id, workingDir || null)),
```
```tsx
  const registerDb = useMutation({
    mutationFn: (id: string) => unwrapStr(commands.registerDbServer(id, db, workingDir || null)),
```
```tsx
  const unregisterDb = useMutation({
    mutationFn: (id: string) => unwrapStr(commands.unregisterDbServer(id, workingDir || null)),
```

(the `onSuccess` invalidations of `["ai-tools"]` keep working - prefix match.)

(d) The card, defined right before the `return`:

```tsx
  const repoCard = (
    <section className="space-y-3 rounded-md border border-border bg-surface p-4">
      <div className="flex items-center gap-2">
        <FolderOpen size={14} className="shrink-0 text-muted" />
        <h2 className="text-sm font-semibold text-text">Working repository</h2>
      </div>
      {workingDir ? (
        <p className="id-mono break-all text-xs text-text">{workingDir}</p>
      ) : (
        <p className="text-xs text-muted">
          Not set. Pick the repository these test cases belong to: its{" "}
          <span className="id-mono">.test-cases</span> folder is where written and imported
          files go, and the AI tools below register into it rather than machine-wide.
        </p>
      )}
      <Button size="sm" variant="outline" onClick={pickWorkingDir}>
        <FolderOpen aria-hidden />
        {workingDir ? "Change" : "Pick repository"}
      </Button>
    </section>
  );

  // Nothing else on this tab means anything until there is a repository -
  // registration would land in a global config, and a writing job would
  // have nowhere agreed to put its file. The rest of the app is unaffected.
  if (!workingDir) {
    return <div className="max-w-lg">{repoCard}</div>;
  }
```

and render `{repoCard}` as the first child of the left column `<div className="space-y-6">`, above the Status section.

(e) In the tool row, after `<span className="text-text">{t.name}</span>` add the scope label:

```tsx
                <span className="flex-1 text-xs text-faint">
                  {t.scope === "project" ? "in this repo" : "global"}
                </span>
```

(f) In the "Other tools" details, the manual command becomes project-scoped - replace both occurrences of the `--scope user` string:

```tsx
                <p className="mb-1 text-faint">Command-line registration (run inside the repository):</p>
                <div className="flex items-center gap-2">
                  <code className="id-mono flex-1 truncate rounded bg-surface-2 px-2 py-1 text-xs text-text">
                    claude mcp add --scope project tcm-testcases -- "{exe}" --mcp
                  </code>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() =>
                      copy(`claude mcp add --scope project tcm-testcases -- "${exe}" --mcp`, "Command")
                    }
                  >
```

- [ ] **Step 4: Run the tab's tests**

Run: `npx vitest run src/screens/AiBridge.test.tsx` → all pass (existing + 4 new). Then `npx tsc --noEmit` → clean.

- [ ] **Step 5: Look at it in the dev app**

Start the dev server (`.claude/launch.json` entry `tcm-v2-dev`), enable demo data (`localStorage.setItem("tcm-v2-dev-demo","on")`, reload), open the AI Bridge tab: only the Working repository card shows; pick a folder; the rest of the tab appears with "in this repo"/"global" labels.

- [ ] **Step 6: Commit**

```bash
cd D:/azure-devops-test-case-creator && git add v2/src/screens/AiBridge.tsx v2/src/screens/AiBridge.test.tsx && git commit -F - <<'EOF'
feat(v2): the AI Bridge tab is scoped to a working repository

A Working repository card leads the tab and gates the rest of it;
detection is keyed on the repo so changing it re-checks what is
registered there; every Register/Unregister passes the repo along; each
tool row says whether it registers in this repo or globally.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
```

---

### Task 10: Import File copies a picked file into `.test-cases`

**Files:**
- Modify: `v2/src/screens/ImportFile.tsx` (imports; the `importFile` mutation ~:356-400)
- Test: `v2/src/screens/ImportFile.test.tsx`

**Interfaces:**
- Consumes: Task 2's `commands.copyIntoCases(root, source)`; Task 7's `loadWorkingDir`, `isInsideCasesDir`, `CASES_DIR`.

- [ ] **Step 1: Write the failing tests**

Append to `v2/src/screens/ImportFile.test.tsx`:

```ts
const oneCase = {
  title: "Copied case", steps: [{ action: "A", expected: "" }], tags: "",
  automation_status: "Not Automated", module_value: "", preconditions: "", update_id: null,
};

/// With a working repository set, a picked file is copied into its
/// .test-cases folder and THAT copy is what gets parsed and watched - the
/// repo, not wherever the file happened to be, is the source of truth.
test("a picked file is copied into the repo's .test-cases and imported from there", async () => {
  localStorage.setItem("tcm-v2-working-dir", "D:\\repo");
  let copyArgs: unknown;
  const parsed: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "plugin:dialog|open") return "C:\\Downloads\\cases.json";
    if (cmd === "copy_into_cases") {
      copyArgs = args;
      return "D:\\repo\\.test-cases\\cases.json";
    }
    if (cmd === "parse_import_file") {
      parsed.push((args as { path: string }).path);
      return { cases: [oneCase], warnings: [] };
    }
    if (cmd === "file_stamp") return "abc";
    if (cmd === "read_general_comment") return "";
    if (cmd === "watch_file") return null;
    if (cmd === "unwatch_all_files") return null;
    return [];
  });
  renderScreen();

  fireEvent.click(await screen.findByRole("button", { name: /import/i }));
  await screen.findByText("Copied case");
  expect(copyArgs).toMatchObject({ root: "D:\\repo", source: "C:\\Downloads\\cases.json" });
  expect(parsed).toEqual(["D:\\repo\\.test-cases\\cases.json"]);
  const watches = JSON.parse(localStorage.getItem("tcm-v2-watch:acme/42") as string);
  expect(watches[0].path).toBe("D:\\repo\\.test-cases\\cases.json");
});

test("without a working repository the picked file is imported where it is", async () => {
  let copied = false;
  const parsed: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    if (cmd === "plugin:dialog|open") return "C:\\Downloads\\cases.json";
    if (cmd === "copy_into_cases") {
      copied = true;
      return "never";
    }
    if (cmd === "parse_import_file") {
      parsed.push((args as { path: string }).path);
      return { cases: [oneCase], warnings: [] };
    }
    if (cmd === "file_stamp") return "abc";
    if (cmd === "read_general_comment") return "";
    if (cmd === "watch_file") return null;
    if (cmd === "unwatch_all_files") return null;
    return [];
  });
  renderScreen();

  fireEvent.click(await screen.findByRole("button", { name: /import/i }));
  await screen.findByText("Copied case");
  expect(copied).toBe(false);
  expect(parsed).toEqual(["C:\\Downloads\\cases.json"]);
});
```

(If the Import button's accessible name differs, use the one the existing tests click - search the file for `getByRole("button", { name:` near `plugin:dialog|open`.)

- [ ] **Step 2: Run to verify it fails**

Run: `npx vitest run src/screens/ImportFile.test.tsx`
Expected: the first new test fails - `copyArgs` undefined, `parsed` is the Downloads path.

- [ ] **Step 3: Implement the copy**

In `v2/src/screens/ImportFile.tsx` add the import:

```ts
import { CASES_DIR, isInsideCasesDir, loadWorkingDir } from "../lib/workingDir";
```

and change the `importFile` mutation's function and success handler:

```tsx
    mutationFn: async (givenPath: string | undefined) => {
      // JSON is the import format (the AI round-trip file exports produce).
      const picked =
        givenPath ??
        (await open({
          multiple: false,
          filters: [{ name: "Test cases (JSON)", extensions: ["json"] }],
        }));
      if (typeof picked !== "string") return null;
      // With a working repository set, the repo's .test-cases folder is
      // where files live: a pick from anywhere else is copied in, and the
      // COPY is what gets parsed, remembered and watched - so an assistant
      // editing "the file" edits the one the app is following.
      const root = loadWorkingDir();
      let path = picked;
      let copied = false;
      if (root && !isInsideCasesDir(root, picked)) {
        const c = await commands.copyIntoCases(root, picked);
        if (c.status === "error") throw new Error(c.error);
        path = c.data;
        copied = path !== picked;
      }
      const r = await commands.parseImportFile(path);
      if (r.status === "error") throw new Error(r.error);
      return {
        path,
        copied,
        stamp: await commands.fileStamp(path),
        data: r.data,
        // Whatever the file already says about the set as a whole - very
        // often written by whoever generated it.
        comment: await commands.readGeneralComment(path),
      };
    },
    onSuccess: (res) => {
      if (!res) return;
      const { path, copied, stamp, data, comment } = res;
      setQueue((q) => [...q, ...data.cases]);
      setWarnings(data.warnings);
      setReport(null);
      setRecents(recordRecentImport(path));
      // From here on, edits to this file land in the queue by themselves.
      // Re-importing the same file replaces its entry rather than adding a
      // second watch on it.
      if (stamp)
        setWatches((prev) => upsertWatch(prev, { path, stamp, snapshot: data.cases, comment }));
      toast.success(
        `Imported ${data.cases.length} case${data.cases.length === 1 ? "" : "s"}` +
          (data.warnings.length ? ` with ${data.warnings.length} warning(s)` : "") +
          (copied ? ` - copied into ${CASES_DIR}` : ""),
      );
```

(the rest of `onSuccess` and `onError` unchanged.)

- [ ] **Step 4: Run the screen's tests**

Run: `npx vitest run src/screens/ImportFile.test.tsx` → all pass. `npx tsc --noEmit` → clean.

- [ ] **Step 5: Commit**

```bash
cd D:/azure-devops-test-case-creator && git add v2/src/screens/ImportFile.tsx v2/src/screens/ImportFile.test.tsx && git commit -F - <<'EOF'
feat(v2): Import File copies a picked JSON into the repo's .test-cases

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
```

---

### Task 11: Copy that describes the new rules, changelog, version, full gates

**Files:**
- Modify: `v2/src-tauri/src/ai_tools.rs:169-184` (the `write` command body)
- Modify: `v2/src-tauri/src/ai_bridge.rs` ~:1362 (the writing guide's Workflow step 0)
- Modify: `v2/src/lib/changelog.ts` (new top entry), `v2/src-tauri/tauri.conf.json`, `v2/src-tauri/Cargo.toml`, `v2/src-tauri/Cargo.lock`

- [ ] **Step 1: Update the assistant-facing copy**

In `v2/src-tauri/src/ai_tools.rs` the `write` command body becomes:

```rust
        body: &[
            "Start a test-case writing job for: $ARGUMENTS",
            "",
            "Call `begin_test_case_writing` FIRST and put its questions to the developer -",
            "what the file is called (it goes in the repository's .test-cases folder),",
            "which specs are authoritative, what is out of scope. Those are theirs to",
            "answer, not yours to assume. The app watches that path, so what you write",
            "imports itself.",
            "",
            "Then call `get_writing_guide` and follow it. It is generated live from the org",
            "and project currently open, so it - not memory - is the instruction.",
        ],
```

In `v2/src-tauri/src/ai_bridge.rs` change the guide's step 0 to:

```rust
        0. Call `begin_test_case_writing` FIRST and put its questions to the\n\
        developer. What the file is called (it lives in the repository's\n\
        .test-cases folder), which specs are authoritative and what is out of\n\
        scope are theirs to decide, not yours to assume.\n\
```

Run `cargo test --test ai_tools --test tcm_mcp --test ai_bridge` → ok (the command-body test forbids copying guide rules, not this wording).

- [ ] **Step 2: Version and changelog**

Run `gh release list --repo AvinAlwis/azure-devops-test-case-manager-v2-releases --limit 1` and `git fetch origin` - the new version is one patch above the newest of (latest release, `origin/master`'s `tauri.conf.json`). Set it in `v2/src-tauri/tauri.conf.json` (`"version"`) and `v2/src-tauri/Cargo.toml` (`version`), then `cargo update -p v2 --offline` (from `v2/src-tauri`) to move `Cargo.lock`. Add the top entry to `CHANGELOG` in `v2/src/lib/changelog.ts` (date = the day it lands):

```ts
  {
    version: "<X.Y.Z>",
    date: "<YYYY-MM-DD>",
    items: [
      "The AI side of the app now works per repository. Pick a working repository on the AI Bridge tab (the tab asks for one before showing anything else); everything else in the app works as before without one.",
      "Test-case files live in the repository's .test-cases folder: a file picked in Import File is copied there and imported from the copy, and a writing job started through an assistant creates the folder, names the file inside it, and refuses to write anywhere else - the app watches it there.",
      "The /tcm:* skills and the MCP registrations for Claude Code, Cursor and VS Code go into the repository (.claude/commands/tcm, .mcp.json, .cursor/mcp.json, .vscode/mcp.json) instead of your user profile; the app removes its own old global copies when you register a repository. Claude Desktop and Windsurf have no per-repository config and stay global, and say so. The database server's .mcp.json is kept out of git status for that checkout.",
      "Changing the working repository re-checks what is registered there, so Register is offered again wherever it is needed.",
    ],
  },
```

- [ ] **Step 3: Full gates**

From `v2/src-tauri`: `cargo test` → every suite `ok` (run `cargo test; echo $?` in bash to read the real exit code).
From `v2`: `npm run build` → `✓ built`; then `npm test` → all pass (re-run any timed-out file alone; do not overlap with cargo).

- [ ] **Step 4: Commit and push**

```bash
cd D:/azure-devops-test-case-creator && git add v2 && git commit -F - <<'EOF'
feat(v2): per-repo workspace - .test-cases, repo-scoped skills and MCP registration - <X.Y.Z>

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
EOF
git push origin HEAD
```

(Do not build or publish a release unless asked - the owner ships from whichever machine is current.)

---

## Self-review

**Spec coverage.** (1) `.test-cases` per repo → Tasks 1, 2, 4, 10. (2) Import copies in → Task 10. (3) Writing job creates the folder, lands the file there, watched and imported from there → Task 4 (folder + resolved path announced to the existing watcher; Task 8 makes sure the bridge knows the repo). (4) Prompt for the directory, rest of app usable, AI Bridge gated → Tasks 7, 9. (5) Skills and DB registration into the repo → Tasks 5, 6. (6) Directory change re-detects and re-enables Register → Task 9 (query keyed on `workingDir`, `enabled` only with one). Owner decisions: commands dir not CLAUDE.md (Task 6 `project_command_dir`), global-only tools stay global with a label (Tasks 5, 9), global copies retired (Task 6 `retire_global`), `.mcp.json` excluded locally (Task 6 + Task 1 `exclude_locally`).

**Placeholders.** The only bracketed values are the release version/date in Task 11, which are determined at execution time by the stated procedure.

**Type consistency.** `config_for` returns a 3-tuple `(PathBuf, &str key, &str scope)` and is destructured that way in Tasks 5 and 6. `DetectedTool.scope: String` (Rust) → `scope: string` (TS) is read as `t.scope === "project"` in Task 9 and mocked as `scope: "global"|"project"` in its tests. `detectAiTools(workingDir: string | null)` / `registerAiTool(id, workingDir)` match the Rust `Option<String>` parameters named `working_dir`, which the Tauri mocks see as `workingDir` (Tasks 8, 9, 10 assert that name). `commands.copyIntoCases(root, source)` returns the `{status, data|error}` result shape the frontend unwraps in Task 10, matching the `Result<String, String>` in Task 2.
