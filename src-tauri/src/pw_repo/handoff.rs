//! Finish in the repo: the prompt a person pastes into Claude Code opened in
//! their clone, so the repo's own `/gen-test` finishes what an export began.
//!
//! An export does `/gen-test`'s first three steps' work (each case's raw
//! spec in `suites/_generated/`, its `index.json` entry and its section in
//! the feature's test-case file) but never runs a raw spec. Steps 4 (run
//! and heal the raw spec), 5 (refactor), 6 (verify) and 7 (run summary) are
//! the repo's, and `.claude/commands/gen-test.md` has no "start at step 4"
//! mode: a bare `/gen-test <id>` would fetch the ticket and generate a new
//! raw spec over the export. So the prompt names the steps and the files
//! instead, and the parts of steps 1-3 the export cannot do (navigation
//! capture, the shared-component checkpoint, the seed's user).
//!
//! One prompt per test-case file, `suites/<seg>/test-cases/<feature>.md`:
//! its cases go in `specs/<feature>.spec.ts` (`--spec <feature>`), which is
//! how the repo's linter pairs a spec with its test cases. A case some spec
//! already has (`// ADO <id>`) is left out and listed: that is `/edit-spec`'s
//! job. Every case of a prompt runs as one repo user, the spec's own if it
//! declares one.
//!
//! The prompt is fixed text plus values checked here only: the case ids,
//! `<seg>` folders and file stems (`layout::valid_name`), raw spec
//! names from `index.json`, and user keys from the clone's `users.json`.
//! The app only builds this text; it never starts Claude Code or anything
//! else for it. Read only.

use crate::pw_export::clone::ClonedRepo;
use crate::pw_export::test_case;
use crate::pw_repo::layout;
use crate::pw_repo::presence::{self, INDEX_REL};
use crate::pw_repo::sync::ExportRecord;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// One prompt: the cases of one test-case file, in the order they were asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct HandoffCard {
    /// The feature's folders under `suites/`, at least two, e.g. `<country>/<side>/<module>/[<group>/]<feature>`.
    pub seg: String,
    /// The test-case file's stem, and so the spec's: `--spec <stem>`.
    pub stem: String,
    /// The cases the prompt finishes.
    pub case_ids: Vec<i32>,
    /// Empty when `problem` is set or every case is in `already`.
    pub prompt: String,
    /// Why this file's prompt cannot be built; the other files still get theirs.
    pub problem: Option<String>,
    /// One sentence per case left out because a spec already has it.
    pub already: Vec<String>,
}

/// Whether `name` is a file stem the prompt may carry: the same rule as a
/// folder (`layout::valid_name`), since a test-case file is named for its feature.
pub fn valid_name(name: &str) -> bool {
    layout::valid_name(name)
}

/// `--spec`'s value as the command takes it: quoted when it has a space.
fn spec_arg(stem: &str) -> String {
    if stem.contains(' ') {
        format!("\"{stem}\"")
    } else {
        stem.to_string()
    }
}

/// The `navigation.json` key of a feature: the last two folders of `seg`.
fn nav_key_of(seg: &str) -> String {
    let folders: Vec<String> = seg.split('/').map(str::to_string).collect();
    layout::navigation_key(&folders)
}

/// "101", "101 and 102", "101, 102 and 103".
fn and_list(items: &[String]) -> String {
    match items.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => items.join(""),
    }
}

fn ticks(paths: &[String]) -> String {
    paths.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ")
}

/// What `prompt` is built from, every value already checked.
pub struct PromptInput<'a> {
    pub seg: &'a str,
    pub stem: &'a str,
    pub ids: &'a [i32],
    /// Relative to the clone, forward slashes, as `ids`.
    pub raw_specs: &'a [String],
    /// The test-case file, relative to the clone.
    pub section: &'a str,
    /// The repo user every raw spec runs as.
    pub user_key: &'a str,
    /// `src/navigation.json` has an entry for the feature's last two folders.
    pub navigation_captured: bool,
    /// A case of the prompt has a `**From TCM:**` block in the test-case file.
    pub has_from_tcm: bool,
    /// The lines of the file's `<!-- tcm:order -->` block; empty when it has none.
    pub order: Vec<String>,
    /// Every case of the prompt already has a `**Uses:**` line in the file.
    pub uses_written: bool,
}

/// The prompt. Pure: the values are checked by `cards`.
pub fn prompt(p: &PromptInput) -> String {
    let id_list: Vec<String> = p.ids.iter().map(i32::to_string).collect();
    let several = p.ids.len() > 1;
    let (cases, whose, them) = if several { ("cases", "each case's", "them") } else { ("case", "the case's", "it") };
    let nav_key = nav_key_of(p.seg);
    let key = p.user_key;
    let seg = p.seg;
    let stem = p.stem;

    let mut out = vec![
        format!(
            "Run steps 4, 5, 6 and 7 of .claude/commands/gen-test.md for ADO {cases} {} in {seg}.",
            and_list(&id_list)
        ),
        format!(
            "Steps 1-3 are already done: the Test Case Manager exported {whose} raw spec ({}) and its test-case section (`{}`).",
            ticks(p.raw_specs),
            p.section
        ),
        format!(
            "Each raw spec runs as the seed's user: set `test.use({{ userKey: '{key}' }})` in `suites/_generated/seed.spec.ts` to {key} before running {them}."
        ),
        if several {
            "In step 4, run those raw specs. Classify each failure first, as step 4 says.".to_string()
        } else {
            "In step 4, run that raw spec. Classify a failure first, as step 4 says.".to_string()
        },
        "If it is a missing precondition, record it and stop. Otherwise heal it in place within the healer's bounds, and never regenerate it from the ticket. \
         Afterwards, put suites/_generated/seed.spec.ts back the way it was."
            .to_string(),
    ];
    if p.has_from_tcm {
        out.push(
            "Read each case's **From TCM:** block before running: it says what the case must not save, what it appears to write or publish, and the cleanup it relies on."
                .to_string(),
        );
    }
    out.push(
        "Read docs/tcm-protected-baseline.md and the entries tagged tcm: in docs/app-quirks.md first; never fix a test by editing or deleting a record named in the protected baseline."
            .to_string(),
    );
    out.push(
        "A failure on a step the From TCM block lists as a known product defect is a product defect, not something to heal."
            .to_string(),
    );
    if !p.order.is_empty() {
        // A line's own full stop would double the one this sentence ends its list with.
        let lines: Vec<&str> = p.order.iter().map(|l| l.trim_end_matches('.')).collect();
        out.push(format!("Order: {}. Keep publishing and destructive cases out of routine runs.", lines.join("; ")));
    }
    if !p.navigation_captured {
        out.push(format!(
            "src/navigation.json has no entry for {nav_key} yet: do step 2's navigation capture with me before step 5."
        ));
    }
    // TCM writes a case's `**Uses:**` lines itself where its script maps to a shared component.
    if !p.uses_written {
        out.push(
            "Before step 5, run step 2's shared-component checkpoint: if any steps belong to a component under suites/common-components, add its **Uses:** line to the test case."
                .to_string(),
        );
    }
    out.push(
        "The values in the raw specs come from each case's Test Case Manager script, which passed against the live app. \
         Tag them by the repo's own provenance rules: `from-ticket` where the test-case section states the value, otherwise `invented`, `assumed` or `derived` as usual."
            .to_string(),
    );
    out.push("Do not fetch the tickets and do not run git.".to_string());
    out.push(format!(
        "Put {them} in `suites/{seg}/specs/{stem}.spec.ts` with --spec {} (it creates the spec or appends to it; the --spec contract).",
        spec_arg(stem)
    ));
    out.push(if p.order.is_empty() {
        "The tests are independent (no --chain).".to_string()
    } else {
        "Run them in the order above.".to_string()
    });
    out.push("Stop and report on any stop-and-report condition the command defines.".to_string());
    out.join(" ")
}

/// Text the prompt can carry as it is: no backtick, quote, `..` or control character.
fn carriable(text: &str) -> bool {
    !text.is_empty() && !text.contains("..") && !text.chars().any(|c| c == '`' || c == '\'' || c.is_control())
}

/// A raw spec's name from `index.json`: a plain `<name>.spec.ts`, no folder.
fn plain_spec_name(name: &str) -> bool {
    carriable(name) && name.ends_with(".spec.ts") && !name.contains('/') && !name.contains('\\')
}

/// `(seg, stem)` of a test-case file `suites/<folders>/test-cases/<stem>.md`
/// with at least two folders (a feature is always at least two deep), every
/// folder `layout::valid_name` and the stem `valid_name`.
fn seg_of(md: &str) -> Option<(String, String)> {
    let rest = md.strip_prefix("suites/")?;
    let (seg, name) = rest.rsplit_once("/test-cases/")?;
    let stem = name.strip_suffix(".md")?;
    let parts: Vec<&str> = seg.split('/').collect();
    (parts.len() >= 2 && parts.iter().all(|p| layout::valid_name(p)) && valid_name(stem))
        .then(|| (seg.to_string(), stem.to_string()))
}

/// The user key a spec declares, `test.use({ userKey: '<key>' })` at the
/// start of a line; the first one.
pub fn declared_user_key(spec: &str) -> Option<String> {
    spec.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("test.use(")?;
        let after = &rest[rest.find("userKey")? + "userKey".len()..];
        let after = after.trim_start().strip_prefix(':')?.trim_start();
        let quote = after.chars().next().filter(|c| *c == '\'' || *c == '"')?;
        let body = &after[1..];
        Some(body[..body.find(quote)?].to_string())
    })
}

/// One case, checked: its file, raw spec, and the spec that already has it.
struct Checked {
    id: i32,
    seg: String,
    stem: String,
    raw: String,
    section: String,
    /// The specs with its `// ADO <id>` marker, short (`specs/x.spec.ts`).
    in_specs: Vec<String>,
}

/// Case `id` is one this app exported (`written` by an export this run, or
/// on the Sync `record`) and the clone still has its raw spec (an
/// `index.json` entry whose file is there) and one section; else why not.
fn check(repo: &ClonedRepo, found: &presence::Presence, record: &ExportRecord, written: &BTreeMap<i32, String>, id: i32) -> Result<Checked, String> {
    if !written.contains_key(&id) && !record.cases.contains_key(&id) {
        return Err(format!("Case {id} was not exported by this app, so there is nothing to finish for it here."));
    }
    let key = id.to_string();
    let file = repo
        .index
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, f)| f.clone())
        .ok_or_else(|| format!("Case {id} has no entry in {INDEX_REL}. Export it again first."))?;
    if !plain_spec_name(&file) {
        return Err(format!("Case {id}'s entry in {INDEX_REL} is not a plain .spec.ts file name."));
    }
    if !repo.generated_files.contains(&file) {
        return Err(format!("Case {id}'s raw spec suites/_generated/{file} is not in the clone. Export it again first."));
    }
    if found.excluded(id) {
        return Err(format!("Case {id} is excluded in the repo, so it is not finished there."));
    }
    let mds: Vec<&String> = found.places(id).iter().filter(|p| p.contains("/test-cases/")).collect();
    let section = match mds.as_slice() {
        [] => return Err(format!("Case {id} has no section in a test-case file. Export it again first.")),
        [one] => (*one).clone(),
        several => {
            // The file this app wrote it to, when the record says which.
            let recorded = record.cases.get(&id).map(|e| e.files.as_slice()).unwrap_or(&[]);
            match several.iter().find(|p| recorded.contains(p)) {
                Some(p) => (*p).clone(),
                None => {
                    let names: Vec<String> = several.iter().map(|p| p.to_string()).collect();
                    return Err(format!("Case {id} has a section in more than one test-case file: {}.", names.join(", ")));
                }
            }
        }
    };
    let (seg, stem) = seg_of(&section).ok_or_else(|| {
        format!("Case {id}'s test-case file is not suites/<folders>/<feature>/test-cases/<name>.md with folder and file names the app can use (no _ or . at the start, and none of / \\ < > : \" | ? * ` ').")
    })?;
    let in_specs = found
        .places(id)
        .iter()
        .filter(|p| p.contains("/specs/"))
        .map(|p| format!("specs/{}", p.rsplit('/').next().unwrap_or(p)))
        .collect();
    Ok(Checked { id, seg, stem, raw: format!("suites/_generated/{file}"), section, in_specs })
}

/// The repo user key case `id`'s raw spec runs as, the one the export used:
/// what an export wrote this run (`written`), else what the Sync `record`
/// has, else what the export's mapping gives now (`mapped`).
pub fn user_key(record: &ExportRecord, written: &BTreeMap<i32, String>, mapped: &BTreeMap<i32, String>, id: i32) -> Option<String> {
    [written.get(&id), record.cases.get(&id).map(|e| &e.user_key), mapped.get(&id)]
        .into_iter()
        .flatten()
        .map(|k| k.trim())
        .find(|k| !k.is_empty())
        .map(str::to_string)
}

/// The sentence for a case with no user key.
pub fn no_user_key(id: i32) -> String {
    format!(
        "Case {id} needs a user key mapped in the export first: map its account to a repo user under Accounts in Export to Playwright, then export it again."
    )
}

/// The sentence for a case a spec already has.
pub fn already_in(id: i32, specs: &[String]) -> String {
    format!("#{id} is already in {}: change that test with /edit-spec in the repo.", and_list(specs))
}

/// The one user key the cases `ids` (with `keys`) run as, for the spec
/// `spec_rel`, or why there is none.
fn one_key(repo: &ClonedRepo, ids: &[i32], keys: &[Option<String>], spec_rel: &str) -> Result<String, String> {
    if let Some((id, _)) = ids.iter().zip(keys).find(|(_, k)| k.is_none()) {
        return Err(no_user_key(*id));
    }
    let keys: Vec<&String> = keys.iter().flatten().collect();
    for k in &keys {
        if !repo.user_keys.contains(k) {
            return Err(format!("Account {k} is not in the repo's users.json."));
        }
        if !carriable(k) {
            return Err(format!("Account {k} has a name the prompt cannot carry as it is."));
        }
    }
    let distinct: BTreeSet<&String> = keys.iter().copied().collect();
    if distinct.len() > 1 {
        let each: Vec<String> = ids.iter().zip(&keys).map(|(id, k)| format!("#{id} as {k}")).collect();
        return Err(format!(
            "One spec runs as one account, but these cases run as different ones ({}): map the cases to the same account, or export them in separate runs.",
            each.join(", ")
        ));
    }
    let key = keys[0].clone();
    if let Ok(text) = std::fs::read_to_string(repo.root.join(spec_rel)) {
        if let Some(declared) = declared_user_key(&text) {
            if declared != key {
                let short = spec_rel.rsplit('/').next().unwrap_or(spec_rel);
                return Err(format!(
                    "These cases run as {key}. Map these cases to {declared}, the account specs/{short} already runs as, then export them again."
                ));
            }
        }
    }
    Ok(key)
}

/// The prompts for `case_ids` in the clone `repo` (already opened), one per
/// test-case file in the order its first case was asked for. `written` is
/// each case an export wrote this run, with its user key; `mapped` each
/// case's user key by the export's mapping now. Refuses the whole lot, with
/// a sentence, if any case does not check out; a file whose cases cannot
/// share a spec's user gets a card with only its `problem`.
pub fn cards(
    repo: &ClonedRepo,
    record: &ExportRecord,
    written: &BTreeMap<i32, String>,
    mapped: &BTreeMap<i32, String>,
    case_ids: &[i32],
) -> Result<Vec<HandoffCard>, String> {
    if case_ids.is_empty() {
        return Err("There are no exported cases to finish.".into());
    }
    let found = presence::scan_repo(repo);
    let mut groups: Vec<(String, Vec<Checked>)> = Vec::new();
    let mut seen: BTreeSet<i32> = BTreeSet::new();
    for &id in case_ids {
        if !seen.insert(id) {
            continue;
        }
        let c = check(repo, &found, record, written, id)?;
        match groups.iter_mut().find(|(s, _)| *s == c.section) {
            Some((_, list)) => list.push(c),
            None => groups.push((c.section.clone(), vec![c])),
        }
    }

    let mut out = Vec::with_capacity(groups.len());
    for (section, list) in groups {
        let (seg, stem) = (list[0].seg.clone(), list[0].stem.clone());
        let already: Vec<String> = list.iter().filter(|c| !c.in_specs.is_empty()).map(|c| already_in(c.id, &c.in_specs)).collect();
        let todo: Vec<&Checked> = list.iter().filter(|c| c.in_specs.is_empty()).collect();
        let ids: Vec<i32> = todo.iter().map(|c| c.id).collect();
        let mut card = HandoffCard { seg: seg.clone(), stem: stem.clone(), case_ids: ids.clone(), prompt: String::new(), problem: None, already };
        if !ids.is_empty() {
            let keys: Vec<Option<String>> = ids.iter().map(|&id| user_key(record, written, mapped, id)).collect();
            let spec_rel = format!("suites/{seg}/specs/{stem}.spec.ts");
            match one_key(repo, &ids, &keys, &spec_rel) {
                Err(problem) => card.problem = Some(problem),
                Ok(key) => {
                    let raw: Vec<String> = todo.iter().map(|c| c.raw.clone()).collect();
                    // What the export wrote into the file; a file that cannot be read says nothing.
                    let md = std::fs::read_to_string(repo.root.join(&section)).unwrap_or_default();
                    card.prompt = prompt(&PromptInput {
                        seg: &seg,
                        stem: &stem,
                        ids: &ids,
                        raw_specs: &raw,
                        section: &section,
                        user_key: &key,
                        navigation_captured: repo.navigation_keys.contains(&nav_key_of(&seg)),
                        has_from_tcm: ids.iter().any(|&id| test_case::has_from_tcm(&md, id)),
                        order: test_case::order_lines(&md),
                        uses_written: ids.iter().all(|&id| test_case::has_uses(&md, id)),
                    });
                }
            }
        }
        out.push(card);
    }
    Ok(out)
}

/// The clone at `root`, opened, then `cards`.
pub fn cards_at(
    root: &Path,
    record: &ExportRecord,
    written: &BTreeMap<i32, String>,
    mapped: &BTreeMap<i32, String>,
    case_ids: &[i32],
) -> Result<Vec<HandoffCard>, String> {
    cards(&crate::pw_export::clone::open(root)?, record, written, mapped, case_ids)
}
