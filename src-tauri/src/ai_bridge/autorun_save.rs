//! Saving Auto Run scripts: parsing a save request, checking it and writing it.

use super::*;

/// The body keys `save_autorun_script` reads. Anything else is named
/// back rather than dropped - a misspelled "edits" that was silently
/// ignored would let an undeclared repair through as if it were a new
/// script.
pub(crate) const SAVE_BODY_KEYS: [&str; 3] = ["scripts", "edits", "dry_run"];

pub(crate) struct SaveRequest {
    pub(crate) scripts: Vec<crate::autorun::CaseScript>,
    pub(crate) edits: Vec<crate::autorun::edits::Edit>,
    /// Run every check and say what would happen, writing nothing.
    pub(crate) dry_run: bool,
}

pub(crate) fn bad_scripts(e: serde_json::Error) -> String {
    format!(
        "that is not a list of action scripts: {e}. Expected an array of {{ case_id, title, steps: [{{ step_number, actions }}] }} - call get_guide (topic autorun) for the format."
    )
}

pub(crate) fn bad_edits(e: serde_json::Error) -> String {
    format!(
        "that is not a list of declared edits: {e}. Each is {{ case_id, steps: [number], why, area (optional: true when the area changed), quirk (optional), precondition (optional: {{ op: \"add\" | \"remove\" | \"change\", flow, stage, value (only when two share the flow and stage), to (for add and change), reason }}) }}."
    )
}

/// Either shape: the bare array a new bundle has always been, or the
/// object that carries the declarations a repair needs alongside it.
///
/// A declaration may also travel INSIDE its script, as `"edits"` on the
/// script object (one entry or a list, its `case_id` taken from the
/// script when left out). `CaseScript` ignores keys it does not know, so
/// before this was read here a nested declaration was dropped without a
/// word and the repair was refused as undeclared - the shape behind the
/// owner's "save_autorun_script drops the edits list". `"edits": null` is
/// no declaration, the same as leaving it out.
pub(crate) fn parse_save_request(
    body: &str,
    load: &dyn Fn(i32) -> Result<Option<crate::autorun::CaseScript>, String>,
) -> Result<SaveRequest, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(bad_scripts)?;
    let mut dry_run = false;
    let (mut scripts_value, edits_value) = match v {
        serde_json::Value::Array(_) => (v, None),
        serde_json::Value::Object(mut map) => {
            let unknown: Vec<String> = map
                .keys()
                .filter(|k| !SAVE_BODY_KEYS.contains(&k.as_str()))
                .map(|k| format!("\"{k}\""))
                .collect();
            if !unknown.is_empty() {
                return Err(format!(
                    "this body carries {} save_autorun_script does not read: {}. It reads \"scripts\", \"edits\" and \"dry_run\".",
                    if unknown.len() == 1 { "a key" } else { "keys" },
                    unknown.join(", ")
                ));
            }
            dry_run = match map.remove("dry_run") {
                None | Some(serde_json::Value::Null) => false,
                Some(serde_json::Value::Bool(b)) => b,
                Some(_) => return Err("\"dry_run\" is true or false.".to_string()),
            };
            let scripts = map.remove("scripts").ok_or_else(|| {
                "this body has no \"scripts\". Send { \"scripts\": [...], \"edits\": [...] }.".to_string()
            })?;
            (scripts, map.remove("edits").filter(|e| !e.is_null()))
        }
        _ => {
            return Err(
                "that is not a list of action scripts. Send the array, or { \"scripts\": [...], \"edits\": [...] }."
                    .to_string(),
            )
        }
    };
    let mut edit_values: Vec<serde_json::Value> = match edits_value {
        None => vec![],
        Some(serde_json::Value::Array(list)) => list,
        // One entry on its own is a one-entry list, the same as one
        // nested in a script.
        Some(one @ serde_json::Value::Object(_)) => vec![one],
        // Anything else: refused in serde's own words for what it is.
        Some(other) => {
            return Err(match serde_json::from_value::<Vec<crate::autorun::edits::Edit>>(other) {
                Err(e) => bad_edits(e),
                Ok(_) => "that is not a list of declared edits.".to_string(),
            })
        }
    };
    // A `dry_run` inside a script is refused rather than dropped: dropped,
    // the save would be real, and a misplaced dry run must never save.
    if let serde_json::Value::Array(items) = &scripts_value {
        if items.iter().any(|s| s.get("dry_run").is_some()) {
            return Err(DRY_RUN_IN_A_SCRIPT.to_string());
        }
    }
    // A repair that sends only the steps it changes: merged into the saved
    // script here, so every gate below reads the whole script.
    if let serde_json::Value::Array(items) = &mut scripts_value {
        apply_patch_steps(items, load)?;
    }
    if let serde_json::Value::Array(items) = &mut scripts_value {
        for script in items.iter_mut().filter_map(|s| s.as_object_mut()) {
            let Some(nested) = script.remove("edits") else { continue };
            let case_id = script.get("case_id").cloned();
            let list = match nested {
                serde_json::Value::Null => vec![],
                serde_json::Value::Array(list) => list,
                one => vec![one],
            };
            for mut edit in list {
                if let (Some(fields), Some(id)) = (edit.as_object_mut(), case_id.as_ref()) {
                    fields.entry("case_id").or_insert_with(|| id.clone());
                }
                edit_values.push(edit);
            }
        }
    }
    if let serde_json::Value::Array(items) = &scripts_value {
        for script in items {
            if let Some(r) = script.get("steps").and_then(crate::autorun::forbidden_fixes::forbidden_in) {
                return Err(match script.get("case_id") {
                    Some(id) => format!("case {id}: {r}"),
                    None => r,
                });
            }
        }
    }
    // What arrives is held to the action vocabulary before it is read as a
    // script: serde drops a key an action does not have (`repeat` on a
    // `press_key`) without a word, and has no `screenshot`. Not applied to a
    // script read from disk.
    if let serde_json::Value::Array(items) = &scripts_value {
        for script in items {
            let actions = script
                .get("steps")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|step| step.get("actions").and_then(serde_json::Value::as_array))
                .flatten();
            for action in actions {
                if let Err(why) = crate::autorun::action_help::refuse_raw_saved_action(action) {
                    return Err(match script.get("case_id") {
                        Some(id) => format!("case {id}: {why}"),
                        None => why,
                    });
                }
            }
        }
    }
    let mut scripts: Vec<crate::autorun::CaseScript> = serde_json::from_value(scripts_value).map_err(bad_scripts)?;
    // The same declaration sent twice (nested and at the top) is one; two
    // DIFFERENT declarations for one case are refused, since the gate
    // could only ever read one of them.
    let mut edits: Vec<crate::autorun::edits::Edit> = Vec::with_capacity(edit_values.len());
    for mut value in edit_values {
        // A precondition edit is one object: a list of them, or anything
        // else, is refused in words rather than serde's.
        if let Some(fields) = value.as_object_mut() {
            let case = fields.get("case_id").map(|c| format!("case {c}: ")).unwrap_or_default();
            match fields.get("precondition") {
                None | Some(serde_json::Value::Null | serde_json::Value::Object(_)) => {}
                Some(serde_json::Value::Array(list)) if list.len() > 1 => {
                    return Err(format!("{case}{}", crate::autorun::edits::ONE_PRECONDITION_EDIT))
                }
                Some(_) => return Err(format!("{case}{}", crate::autorun::edits::PRECONDITION_ONE_OBJECT)),
            }
        }
        let edit: crate::autorun::edits::Edit = serde_json::from_value(value).map_err(bad_edits)?;
        if edits.contains(&edit) {
            continue;
        }
        if edits.iter().any(|e| e.case_id == edit.case_id) {
            return Err(format!(
                "case {} is declared twice in \"edits\", with different contents - send one entry per case",
                edit.case_id
            ));
        }
        edits.push(edit);
    }
    // A precondition edit for a script sent with its preconditions as
    // saved - whole, or through `patch_steps` - is applied here, so every
    // gate below reads the script as it will be saved. A script sent with
    // the change already made is left as it is, and the gate checks it is
    // exactly the declared change.
    for edit in &edits {
        let Some(precondition) = edit.precondition.as_ref() else { continue };
        let Some(script) = scripts.iter_mut().find(|s| s.case_id == edit.case_id) else { continue };
        let Some(saved) = load(edit.case_id)? else { continue };
        if crate::autorun::edits::same_preconditions(&script.preconditions, &saved.preconditions) {
            let (applied, _) = crate::autorun::edits::apply_precondition_edit(&saved.preconditions, precondition)
                .map_err(|why| format!("case {}: {why}", edit.case_id))?;
            script.preconditions = applied;
        }
    }
    Ok(SaveRequest { scripts, edits, dry_run })
}

/// A script entry that carries `patch_steps` in place of `steps`: a repair
/// that sends only the steps it changes, each `{ step, actions }` (any
/// other field of a step, such as `unchecked`, may come with it). Each
/// replaces those fields of that step of the case's SAVED script, every
/// other step is kept as saved, and any other key of the entry (`title`,
/// `area`, ...) replaces the saved one. The result is the whole script, as
/// if it had been sent whole, so the repair's `edits` must still declare
/// every patched step. Refused: an entry with both `steps` and
/// `patch_steps`, one with no `case_id`, a case with no saved script, a
/// clean-up patch not after the case's last step, and a step patched
/// twice. A step the saved script lacks is ADDED in step order; a patch
/// with `"cleanup": true` replaces the saved clean-up step (whatever its
/// number) or adds one after the last case step.
pub fn apply_patch_steps(
    items: &mut [serde_json::Value],
    load: &dyn Fn(i32) -> Result<Option<crate::autorun::CaseScript>, String>,
) -> Result<(), String> {
    for item in items.iter_mut() {
        let Some(map) = item.as_object_mut() else { continue };
        let Some(patches) = map.remove("patch_steps").filter(|p| !p.is_null()) else { continue };
        let case_id = map
            .get("case_id")
            .and_then(serde_json::Value::as_i64)
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| "a script with \"patch_steps\" needs its \"case_id\"".to_string())?;
        if map.contains_key("steps") {
            return Err(format!("case {case_id}: send \"steps\" (the whole script) or \"patch_steps\" (only the changed steps), not both"));
        }
        let stored = load(case_id)?.ok_or_else(|| {
            format!("case {case_id} has no saved script to patch - send its whole script with \"steps\"")
        })?;
        let mut whole = serde_json::to_value(&stored).map_err(|e| e.to_string())?;
        let patches = match patches {
            serde_json::Value::Array(list) => list,
            one @ serde_json::Value::Object(_) => vec![one],
            _ => return Err(format!("case {case_id}: \"patch_steps\" is a list of {{ step, actions }}")),
        };
        let Some(steps) = whole.get_mut("steps").and_then(serde_json::Value::as_array_mut) else {
            return Err(format!("case {case_id}: its saved script has no steps to patch"));
        };
        let mut patched: Vec<i64> = Vec::new();
        let mut cleanup_patched = false;
        for patch in patches {
            let Some(fields) = patch.as_object() else {
                return Err(format!("case {case_id}: each of \"patch_steps\" is {{ step, actions }}"));
            };
            let Some(n) = fields.get("step").or_else(|| fields.get("step_number")).and_then(serde_json::Value::as_i64) else {
                return Err(format!("case {case_id}: each of \"patch_steps\" names its \"step\" by number"));
            };
            if !fields.contains_key("actions") {
                return Err(format!("case {case_id}: patch_steps step {n} has no \"actions\""));
            }
            if patched.contains(&n) {
                return Err(format!("case {case_id}: patch_steps names step {n} twice"));
            }
            patched.push(n);
            let is_cleanup = fields.get("cleanup").and_then(serde_json::Value::as_bool) == Some(true);
            let number_of = |s: &serde_json::Value| s.get("step_number").and_then(serde_json::Value::as_i64);
            let is_cleanup_step = |s: &serde_json::Value| s.get("cleanup").and_then(serde_json::Value::as_bool) == Some(true);
            let others = fields.iter().filter(|(k, _)| !matches!(k.as_str(), "step" | "step_number"));
            if is_cleanup {
                // The clean-up step: the patch merges into the saved one
                // (fields it does not send are kept, its number is the
                // patch's), or is added after the last case step when the
                // script has none.
                if cleanup_patched {
                    return Err(format!("case {case_id}: patch_steps holds two clean-up patches; a case has one clean-up step"));
                }
                cleanup_patched = true;
                let last = steps.iter().filter(|s| !is_cleanup_step(s)).filter_map(number_of).max();
                if let Some(last) = last.filter(|l| n <= *l) {
                    return Err(format!("case {case_id}: a clean-up step comes after the case's last step ({last})"));
                }
                match steps.iter().position(is_cleanup_step) {
                    Some(at) => {
                        steps[at]["step_number"] = serde_json::json!(n);
                        for (k, v) in others {
                            steps[at][k.as_str()] = v.clone();
                        }
                    }
                    None => {
                        let mut step = serde_json::Map::new();
                        step.insert("step_number".to_string(), serde_json::json!(n));
                        for (k, v) in others {
                            step.insert(k.clone(), v.clone());
                        }
                        steps.push(serde_json::Value::Object(step));
                    }
                }
                continue;
            }
            if let Some(step) = steps.iter_mut().find(|s| number_of(s) == Some(n)) {
                for (k, v) in others {
                    step[k.as_str()] = v.clone();
                }
            } else {
                // A step the saved script lacks: added in step order among
                // the case steps (a clean-up step stays last).
                let mut step = serde_json::Map::new();
                step.insert("step_number".to_string(), serde_json::json!(n));
                for (k, v) in others {
                    step.insert(k.clone(), v.clone());
                }
                let at = steps
                    .iter()
                    .position(|s| is_cleanup_step(s) || number_of(s).is_some_and(|m| m > n))
                    .unwrap_or(steps.len());
                steps.insert(at, serde_json::Value::Object(step));
            }
        }
        // The entry's own keys win over the saved ones.
        for (k, v) in map.iter() {
            whole[k.as_str()] = v.clone();
        }
        *item = whole;
    }
    Ok(())
}

/// The case and steps a repair's quirk is about, with the class of the
/// failure that led to it (read from the newest run of that case).
pub(crate) fn repair_source(
    root: &std::path::Path,
    old: &crate::autorun::CaseScript,
    edit: &crate::autorun::edits::Edit,
) -> crate::autorun::quirks::QuirkSource {
    let run = crate::autorun::failures::latest_run(root, Some(edit.case_id));
    crate::autorun::quirks::source_for_repair(run.as_ref(), old, edit.case_id, &edit.steps)
}

/// Is the script sent word for word the one already on disk?
///
/// Compared the way the declared-edit gate compares steps - by
/// `step_signature`, so JSON formatting does not count as a change -
/// plus the fields outside the steps a save can carry, `title`,
/// `account`, `no_save`, `preconditions`, `setup` and `area` (a blank area is no
/// area; case does not tell two area names apart). `no_save` counts: a re-send that leaves it
/// out of a script marked Must not save is a repair turning it off, which
/// the gate refuses - never a quiet "unchanged". Positional rather than keyed by step number, so a bundle
/// that merely REORDERS the same steps counts as a change and goes
/// through the gate rather than around it. `repairs` is deliberately
/// not compared: it is never the sender's to set. Nor are the marks
/// (`changes`, `needs_unchanged`, `needs_state`): they affect order, not safety, so
/// marking a saved script is no repair and does not add to its count. The
/// sent marks are what is saved.
pub(crate) fn unchanged_script(old: &crate::autorun::CaseScript, sent: &crate::autorun::CaseScript) -> bool {
    old.title == sent.title
        && old.account == sent.account
        && old.no_save == sent.no_save
        && old.preconditions == sent.preconditions
        && old.needs_records.iter().all(|n| sent.needs_records.contains(n))
        && old.setup == sent.setup
        && old.area_name().map(crate::autorun::nav::module_key) == sent.area_name().map(crate::autorun::nav::module_key)
        && old.steps.len() == sent.steps.len()
        && old.steps.iter().zip(&sent.steps).all(|(a, b)| {
            a.step_number == b.step_number
                && crate::autorun::edits::step_signature(a)
                    == crate::autorun::edits::step_signature(b)
        })
}

/// What a dry run that passed every check answers. It opens with
/// `DRY_RUN_NOTHING_SAVED` and never says "saved N script(s)", so it can
/// never be read as the save's own answer.
pub fn dry_run_answer(count: usize, lines: &[String]) -> String {
    format!("{DRY_RUN_NOTHING_SAVED} It would save {count} script(s): {}", lines.join(", "))
}

/// Said to a save whose `dry_run` is inside a script rather than beside
/// `"scripts"`: nothing is checked or saved.
pub const DRY_RUN_IN_A_SCRIPT: &str = "\"dry_run\" goes beside \"scripts\", not inside a script - nothing was saved. Send { \"scripts\": [...], \"dry_run\": true }.";

/// How a script save's dry run answer begins.
pub const DRY_RUN_NOTHING_SAVED: &str = "Dry run: nothing was saved.";

/// Save one or many Auto Run action scripts, as an assistant writes them.
///
/// A BUNDLE by design: the body is an array, so a whole PBI's worth of
/// cases lands in one call - and the same shape is what the Auto Run
/// screen's Import button reads from a file. One case is a bundle of one.
///
/// ALL OR NOTHING, for real. Every gate below runs for every script
/// before a single byte is written: the body parses, each change to a
/// script that already exists is declared, and each script meets its test case's expected results. Only
/// then does `store::save_scripts_checked` run - and it re-checks, under the
/// scripts lock, that every script the gates read is still the one they
/// read (409 if not), then validates and
/// serialises every entry before writing any of them, so a bad case id
/// or a filesystem error on entry 16 of 30 can never leave the other 29
/// half-applied. An unknown action `kind` fails here rather than
/// mid-run, with the browser already open in front of them.
///
/// `repairs` NEVER comes from the body. It is the count of changes made
/// without a person looking, shown to the person in the script editor, and
/// only a person's own save sets it back to zero. A script that comes back
/// word for word unchanged does not add to it: re-saving a whole PBI after
/// fixing one case is exactly that, for every other case in the bundle.
pub(crate) async fn save_autorun_scripts(
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    body: &str,
    asker: Option<SlotNo>,
) -> (u16, String) {
    // What the caller's discovery carried from the areas it left, while it goes on.
    let carried = carried_of(asker).await;
    save_autorun_scripts_carrying(ctx, client, body, asker, carried).await
}

/// [`save_autorun_scripts`] with the areas the caller's discovery carried
/// (`DiscoveryState::carried`) already read: the save's seen check lays them
/// over the saved map (`discovery_map::add_carried`).
pub async fn save_autorun_scripts_carrying(
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    body: &str,
    asker: Option<SlotNo>,
    carried: Vec<crate::autorun::discovery_map::AreaMap>,
) -> (u16, String) {
    // Parsing reads the saved script of a repair (to merge its patch onto
    // it); that read, not the later one at Gate 1, is the version the
    // bundle is built from, so it is the one the write checks against.
    let reads = crate::autorun::store::ScriptReads::new();
    // Resolved once for the whole save: a switch partway through then
    // answers with the switch sentence at the write, not a changed script.
    let resolved_root = autorun_root();
    let load = |id: i32| match &resolved_root {
        Ok(root) => reads.load(root, id),
        Err((_, why)) => Err(why.clone()),
    };
    let SaveRequest { scripts, edits, dry_run } = match parse_save_request(body, &load) {
        Ok(r) => r,
        Err(e) => return (400, e),
    };
    if scripts.is_empty() {
        return (400, "no scripts in the bundle".to_string());
    }
    // Capped before any lookup - disk or Azure DevOps - runs at all: the
    // floor's own lookup sends every id in the bundle through
    // `get_test_cases_by_ids` in one call, and `/test-cases` already caps
    // ITS ids list at the same number for the same reason.
    if scripts.len() > MAX_CASE_IDS {
        return (400, format!("a bundle can carry at most {MAX_CASE_IDS} scripts"));
    }
    // A declaration for a case this bundle is not saving changes nothing
    // and declares nothing - but it would still file its quirk, and it
    // usually means the wrong case id was typed into one of the two
    // lists. Named back before anything is read or written.
    let stray: Vec<String> = {
        let mut ids: Vec<i32> = edits
            .iter()
            .map(|e| e.case_id)
            .filter(|id| !scripts.iter().any(|s| s.case_id == *id))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter().map(|id| id.to_string()).collect()
    };
    if !stray.is_empty() {
        return (
            400,
            format!(
                "edits names {} {}, which {} not in this bundle",
                if stray.len() == 1 { "case" } else { "cases" },
                stray.join(", "),
                if stray.len() == 1 { "is" } else { "are" }
            ),
        );
    }
    let root = match resolved_root {
        // `set_root` runs exactly once, during app setup, and only when
        // `app_data_dir()` resolves - so a missing root is not something
        // this process will ever recover from on its own. Refuse rather
        // than invent a path: a script written somewhere the app does
        // not read would look saved and never appear.
        Err(refused) => return refused,
        Ok(r) => r,
    };

    // Gate 0: the project's own rules, before anything is read from disk
    // or Azure DevOps - a project whose runs start on the module screen
    // refuses a script that opens pages by address, and every project
    // refuses an `area` it has not recorded.
    if let Err(why) = crate::autorun::nav::check_project_rules(&root, &ctx.org, &ctx.project, &scripts) {
        return (400, why);
    }

    // Gate 1: what this bundle does to the scripts already on disk.
    let mut prepared: Vec<crate::autorun::CaseScript> = Vec::with_capacity(scripts.len());
    let mut lines: Vec<String> = Vec::with_capacity(scripts.len());
    let mut repair_sources: Vec<crate::autorun::quirks::QuirkSource> = Vec::new();
    // Cases whose repair changes the step their suspected-defect mark is
    // on: the assistant has decided that step was the script's fault after
    // all, so the mark goes once the save has landed.
    let mut repaired_marks: Vec<(i32, i32)> = Vec::new();
    // Marks a repair left alone because the step's checks did not change.
    let mut kept_marks: Vec<String> = Vec::new();
    // Which steps of each script the seen check reads, by case: `None`
    // for a new script (all of them), its declared steps for a repair. An
    // unchanged resave is not listed: it was checked when it was saved.
    let mut seen_scope: Vec<(i32, Option<Vec<i32>>)> = Vec::new();
    // The saved script of each repair, for the check floor and the values a
    // repair may not move away from the case (`case_values::check_repair_values`).
    let mut repair_olds: Vec<(i32, crate::autorun::CaseScript)> = Vec::new();
    // One line per check this save's repairs give up, said after the save's
    // first line (outside the notes cap).
    let mut given_up_lines: Vec<String> = Vec::new();
    // Whether scripts saved before they were stamped count here, read once.
    let legacy = crate::autorun::seen_check::legacy_scripts_count(&root, &ctx.org, &ctx.project);
    // Every case's refusal from this gate, said together: a bundle with two
    // bad repairs is told about both in one answer.
    let mut refused: Vec<String> = Vec::new();
    // What each repair's precondition edit did, one line per case, said
    // after the save's first line. The flows name the stages, read only
    // when a precondition edit is declared; without them the ids are said.
    let mut precondition_lines: Vec<String> = Vec::new();
    // Cases kept unchecked because a divergence of theirs waits for a ruling, said after those.
    let mut unchecked_lines: Vec<String> = Vec::new();
    let flows = if edits.iter().any(|e| e.precondition.is_some()) {
        crate::api_templates::flow_store::list(&root, &ctx.org, &ctx.project).unwrap_or_default()
    } else {
        Vec::new()
    };
    // The version of each case's script this bundle was built from (`None`:
    // there was none): the first read of it, else the one below. The write
    // re-checks these under the scripts lock, so a save that overlaps this
    // one cannot be overwritten unseen.
    let mut read: Vec<(i32, Option<String>)> = Vec::with_capacity(scripts.len());
    for sent in &scripts {
        let existing = match crate::autorun::store::load_script(&root, sent.case_id) {
            Ok(v) => v,
            Err(e) => return (500, e),
        };
        read.push((sent.case_id, reads.first_version(sent.case_id, existing.as_ref())));
        let declared = edits.iter().find(|e| e.case_id == sent.case_id);
        // An unchanged re-send never lowers the floor it was stored with.
        let mut stored_floor: u32 = 0;
        let mut script = sent.clone();
        // A new script is checked whole; an unchanged one and a repair keep
        // steps checked by an earlier save, so they vouch as it did.
        let vouched = existing
            .as_ref()
            .is_none_or(|old| crate::autorun::seen_check::vouch_carries_over(old, &ctx.org, &ctx.project, legacy));
        script.organization = Some(ctx.org.trim().to_string());
        script.project = Some(ctx.project.trim().to_string());
        let may_check = crate::autorun::proven::may_be_checked(&root, &ctx.org, &ctx.project, sent.case_id);
        script.checked = vouched && may_check;
        if vouched && !may_check {
            unchecked_lines.push(format!(
                "case {}: not marked checked: a divergence of the case waits for a person's ruling, and the script counts as checked once a run passes after the ruling",
                sent.case_id
            ));
        }
        // A needed record whose setup is gone is not one a repair must keep.
        let existing = existing.map(|old| crate::autorun::needs_records::forgive_stale(old, &root, &ctx.org, &ctx.project));
        // Never what was sent: the list a save leaves is the one it works out below, from the old one.
        script.unfired_guards = existing.as_ref().map(|old| old.unfired_guards.clone()).unwrap_or_default();
        match existing {
            // A re-send that changes nothing is not a repair. The count
            // is of changes made without a person looking, so an
            // identical bundle - which is what saving a whole PBI again
            // after fixing one case IS, for every other case in it -
            // must not raise it. A declaration
            // for such a case is still refused, by `check_edits`' own
            // rule 2: it names a step nothing happened to.
            Some(old) if unchanged_script(&old, sent) => {
                if declared.is_some() {
                    if let Err(why) = crate::autorun::edits::check_edits(&old, sent, declared) {
                        refused.push(why);
                        continue;
                    }
                }
                script.repairs = old.repairs;
                script.last_repair = old.last_repair.clone();
                script.precondition_repairs = old.precondition_repairs.clone();
                script.given_up = old.given_up.clone();
                stored_floor = old.checks;
                let marks_changed = old.changes != sent.changes
                    || old.needs_unchanged != sent.needs_unchanged
                    || old.needs_state != sent.needs_state;
                let said = if marks_changed { "marks updated" } else { "unchanged" };
                lines.push(format!("case {} ({said})", script.case_id));
            }
            Some(old) => {
                if let Err(why) = crate::autorun::edits::check_edits(&old, sent, declared) {
                    // A body with no declarations at all, refused for
                    // something a declaration would cover (the sentences
                    // that point at "edits"): say first that the list
                    // itself is missing, which is the actual fix. Only
                    // then: when the bundle declares OTHER cases, the list
                    // is there and this case's entry is what is missing,
                    // which `check_edits`' own "case N: ... not declared"
                    // sentence already says accurately.
                    if edits.is_empty() && why.contains("\"edits\"") {
                        refused.push(format!(
                            "case {} already has a script, so this save is a repair, and \"edits\" is missing. {why}",
                            sent.case_id
                        ));
                        continue;
                    }
                    refused.push(why);
                    continue;
                }
                script.repairs = old.repairs.saturating_add(1);
                // What was given up before stays; this repair's own entries
                // are added from its edit, never from what was sent.
                script.given_up = old.given_up.clone();
                if let Some(e) = declared {
                    let at = crate::applog::iso_stamp();
                    for g in &e.gives_up {
                        let entry = crate::autorun::GivenUp {
                            step: g.step,
                            kind: g.kind.trim().to_lowercase(),
                            why: g.why.trim().to_string(),
                            at: at.clone(),
                            // A timing field taken off a check that stays has nothing replacing it.
                            replaced_by: if crate::autorun::case_values::is_timing_field(&g.kind.trim().to_lowercase()) {
                                Vec::new()
                            } else {
                                crate::autorun::case_values::added_kinds(&old, &script, g.step)
                            },
                        };
                        let replaced = if entry.replaced_by.is_empty() {
                            String::new()
                        } else {
                            format!(" (replaced by {})", entry.replaced_by.join(", "))
                        };
                        given_up_lines.push(format!(
                            "case {}: check given up: step {} {} - {}{replaced}. The person sees it on the case; name it in your report.",
                            script.case_id, entry.step, entry.kind, entry.why
                        ));
                        script.given_up.push(entry);
                    }
                }
                repair_olds.push((sent.case_id, old.clone()));
                // `check_edits` above only returns Ok when a real change
                // was fully declared, so `declared` is always Some here -
                // there is no path where a script actually differs from
                // disk and this branch is reached with nothing declared.
                seen_scope.push((sent.case_id, crate::autorun::seen_check::steps_to_check(declared)));
                script.precondition_repairs = old.precondition_repairs.clone();
                if let Some(e) = declared {
                    let why = e.why.trim();
                    // `check_edits` above applied the same edit to the same
                    // saved preconditions, so it applies here too.
                    let precondition_line = e
                        .precondition
                        .as_ref()
                        .and_then(|p| crate::autorun::edits::apply_precondition_edit(&old.preconditions, p).ok())
                        .map(|(_, record)| {
                            let line = crate::autorun::preconditions::repair_line(&flows, &record);
                            script.precondition_repairs.push(record);
                            line
                        });
                    if !dry_run {
                        if !why.is_empty() {
                            crate::applog::info(format!(
                                "AI repaired case {} steps {:?}: {why}",
                                script.case_id, e.steps
                            ));
                        }
                        if let Some(line) = &precondition_line {
                            crate::applog::info(format!("AI repaired case {}: {line}", script.case_id));
                        }
                    }
                    script.last_repair = Some(match &precondition_line {
                        Some(line) if why.is_empty() => line.clone(),
                        Some(line) => format!("{why}; {line}"),
                        None => why.to_string(),
                    });
                    if let Some(line) = precondition_line {
                        precondition_lines.push(format!("case {}: {line}", script.case_id));
                    }
                    if let Some(d) = old.suspected_defect.as_ref().filter(|d| e.steps.contains(&d.step_number)) {
                        // Only a repair of the step's checks takes the mark:
                        // a moved locator is no decision that the step was
                        // wrong. A step missing on either side is changed.
                        let at = |s: &crate::autorun::CaseScript| s.steps.iter().find(|x| x.step_number == d.step_number).cloned();
                        let changed = match (at(&old), at(&script)) {
                            (Some(before), Some(after)) => crate::autorun::defects::checks_changed(&before, &after),
                            _ => true,
                        };
                        if changed {
                            repaired_marks.push((script.case_id, d.step_number));
                        } else {
                            kept_marks.push(crate::autorun::defects::kept_line(script.case_id, d.step_number));
                        }
                    }
                    // Read now, while the script on disk is still the
                    // one that ran: its targets classify the failure.
                    if e.quirk.as_deref().is_some_and(|t| !t.trim().is_empty()) {
                        repair_sources.push(repair_source(&root, &old, e));
                    }
                }
                lines.push(format!("case {} (repaired)", script.case_id));
            }
            None => {
                if declared.is_some() {
                    refused.push(format!(
                        "case {} has no script yet - \"edits\" is for changing one that exists",
                        sent.case_id
                    ));
                    continue;
                }
                script.repairs = 0;
                script.last_repair = None;
                script.precondition_repairs = Vec::new();
                script.given_up = Vec::new();
                seen_scope.push((sent.case_id, None));
                lines.push(format!("case {} (new)", script.case_id));
            }
        }
        script.checks = (crate::autorun::case_values::check_count(&script) as u32).max(stored_floor);
        prepared.push(script);
    }
    if !refused.is_empty() {
        return (400, refused.join("\n"));
    }

    // Gate 2: the expected-result floor. This is the one place the save
    // route reaches Azure DevOps, and only to READ the cases - a script
    // is judged against what its test case says should happen, never
    // against what the application happens to do.
    let Some(client) = client else {
        // 503, like every other route that needs a signed-in client - not
        // 400: nothing about the bundle itself is wrong, the app just has
        // nowhere to check it against yet.
        return (
            503,
            "sign in to Test Case Manager first - a script is checked against its test case before it is saved"
                .to_string(),
        );
    };
    let ids: Vec<i32> = prepared.iter().map(|s| s.case_id).collect();
    let cases = match client
        .get_test_cases_by_ids(
            &ctx.org,
            &ids,
            ctx.module_ref.as_deref(),
            ctx.preconditions_ref.as_deref(),
        )
        .await
    {
        Ok(cases) => cases,
        // Azure DevOps refuses the whole batch when any id is not a work
        // item, so it cannot say which - the ids are named back instead.
        Err(crate::ado::AdoError::NotFound) => {
            return (
                400,
                format!(
                    "Azure DevOps has no work item for at least one of {} - a script is saved against a real test case",
                    ids.iter().map(|i| format!("#{i}")).collect::<Vec<_>>().join(", ")
                ),
            )
        }
        Err(e) => return (502, format!("Azure DevOps error: {e:?}")),
    };
    // Every case's shortfalls, said together.
    let mut short: Vec<String> = Vec::new();
    for script in &prepared {
        let Some(case) = cases.iter().find(|c| c.id == script.case_id) else {
            short.push(format!("case {} is not a test case in this organization", script.case_id));
            continue;
        };
        let shortfalls =
            crate::autorun::floor::check_floor(script, &crate::autorun::floor::expected_of(&case.steps));
        if !shortfalls.is_empty() {
            short.push(format!("case {}: {}", script.case_id, shortfalls.join("; ")));
        }
    }
    if !short.is_empty() {
        return (400, short.join("\n"));
    }

    // The environment's test name prefix, for the note on a record a script
    // relies on without declaring it, and for the values a check may carry
    // that the case never held.
    let test_prefix = crate::environments::active(&root)
        .map(|e| e.test_prefix)
        .unwrap_or_else(|_| crate::environments::default_test_prefix());
    // The project's quirks, for the class a repair's quirk will actually have
    // and whether it fits (a file that cannot be read is judged as empty; the
    // record itself says what it could not do).
    let quirks_now = crate::autorun::quirks::load_quirks(&root, &ctx.org, &ctx.project).unwrap_or_default();

    // Gate 2b: a new or changed step's check values come from the case. A
    // value the case does not hold needs a "because"; a count the case
    // states differently is refused. Unchanged steps are not compared.
    let mut value_refusals: Vec<String> = Vec::new();
    let mut value_sources: Vec<(i32, Vec<crate::autorun::case_values::SourcedValue>)> = Vec::new();
    let mut value_notes: Vec<String> = Vec::new();
    for (case_id, only) in &seen_scope {
        let (Some(script), Some(case)) =
            (prepared.iter().find(|s| s.case_id == *case_id), cases.iter().find(|c| c.id == *case_id))
        else {
            continue;
        };
        let text = crate::autorun::case_text::CaseText::from_case(case);
        // A repair also keeps its checks and the case's values.
        if let (Some((_, old)), Some(edit)) =
            (repair_olds.iter().find(|(id, _)| id == case_id), edits.iter().find(|e| e.case_id == *case_id))
        {
            value_refusals.extend(
                crate::autorun::case_values::check_repair_values(old, script, &text, &edit.steps, &edit.gives_up, &test_prefix)
                    .into_iter()
                    .map(|r| format!("case {case_id}: {r}")),
            );
            // And it never lets the page decide whether a failure counts.
            value_refusals.extend(
                crate::autorun::forbidden_fixes::check_repair_absorbs_with(old, script, Some(edit), &quirks_now)
                    .into_iter()
                    .map(|r| format!("case {case_id}: {r}")),
            );
        }
        // A repair holds only the values new or changed in its steps.
        let old_script = repair_olds.iter().find(|(id, _)| id == case_id).map(|(_, old)| old);
        let found = crate::autorun::case_values::check_case_values_against(
            script,
            &text,
            only.as_deref(),
            &test_prefix,
            old_script,
        );
        value_refusals.extend(found.refusals.into_iter().map(|r| format!("case {case_id}: {r}")));
        value_sources.push((*case_id, found.sourced));
        value_notes.extend(found.notes.into_iter().map(|n| format!("case {case_id}: note: {n}")));
    }
    let value_listed = crate::autorun::case_values::value_source_lines(&value_sources);
    if !value_refusals.is_empty() {
        return (400, value_refusals.join("\n"));
    }

    // Gate 3: every locator a new or changed step names was seen on the
    // live app (`seen_check`), so no script is saved against a guess.
    // The components are read only when a checked script uses one, so a
    // damaged file never holds up a save that does not need it.
    let uses_components = seen_scope.iter().any(|(case_id, _)| {
        prepared
            .iter()
            .any(|s| s.case_id == *case_id && crate::autorun::seen_check::uses_components(s))
    });
    // The saved scripts, read once for every check of this save.
    let saved_scripts =
        if seen_scope.is_empty() { Vec::new() } else { crate::autorun::store::list_scripts(&root) };
    // The project's own save words, for what counts as saving a record.
    let save_words = crate::autorun::nav::load_nav(&root, &ctx.org, &ctx.project).map(|n| n.save_words).unwrap_or_default();
    let check_and_save = || -> Result<Vec<String>, SaveRefusal> {
        // What the save noticed without refusing: said after its answer.
        let mut notes: Vec<String> = Vec::new();
        // Each checked script's clean-up targets never seen, as the script will keep them.
        let mut unfired: Vec<(i32, Vec<String>)> = Vec::new();
        if !seen_scope.is_empty() {
            let (map, components, files) = seen_check_inputs(&root, ctx, uses_components, &saved_scripts, &carried)
                .map_err(|e| SaveRefusal::Other(400, e))?;
            // The placement of a timed or same_document check, again with each component expanded.
            if uses_components {
                for (case_id, only) in &seen_scope {
                    let Some(script) = prepared.iter().find(|s| s.case_id == *case_id) else { continue };
                    crate::autorun::store::check_expanded_response_timings(script, only.as_deref(), &components)
                        .map_err(|e| SaveRefusal::Other(400, e))?;
                }
            }
            // Every script is checked, and every locator never seen in any
            // of them is named at once; a refusal of any other kind is
            // answered alone, as it always was.
            let mut unseen: Vec<(i32, Vec<String>)> = Vec::new();
            // Locators only ever seen matching nothing, used to check that
            // something is absent: refused after the unseen ones, which a
            // discovery in progress may still record.
            let mut unproven: Vec<(i32, Vec<String>)> = Vec::new();
            // Absence checks let through on the case's own words: listed
            // first among the notes, so the cap never drops one before the
            // others.
            let mut from_case: Vec<String> = Vec::new();
            for (case_id, only) in &seen_scope {
                let (Some(script), Some(case)) = (
                    prepared.iter().find(|s| s.case_id == *case_id),
                    cases.iter().find(|c| c.id == *case_id),
                ) else {
                    return Err(SaveRefusal::Other(
                        400,
                        format!("case {case_id} could not be checked against the live app, so it was not saved"),
                    ));
                };
                use crate::autorun::seen_check::SeenVerdict;
                match crate::autorun::seen_check::seen_verdict(
                    &map,
                    &components,
                    script,
                    &case_step_text(case),
                    only.as_deref(),
                    &files,
                ) {
                    SeenVerdict::Passed => {
                        let review = crate::autorun::seen_check::absence_review(
                            &map,
                            script,
                            &case_step_text(case),
                            only.as_deref(),
                            saved_scripts.iter().find(|s| s.case_id == *case_id),
                        );
                        from_case.extend(review.from_case.into_iter().map(|n| format!("case {case_id}: note: {n}")));
                        let loops = crate::autorun::seen_check::loop_rows_review(&map, script, only.as_deref());
                        let mut refused = review.refusals;
                        refused.extend(loops.refusals);
                        if !refused.is_empty() {
                            unproven.push((*case_id, refused));
                        }
                        from_case.extend(loops.notes.into_iter().map(|n| format!("case {case_id}: note: {n}")));
                    }
                    SeenVerdict::Other(why) => return Err(SaveRefusal::Unseen(why)),
                    SeenVerdict::Unseen(lines) => unseen.push((*case_id, lines)),
                }
            }
            if !unseen.is_empty() {
                return Err(SaveRefusal::Unseen(unseen_in_bundle(&unseen)));
            }
            if !unproven.is_empty() {
                return Err(SaveRefusal::Other(400, unseen_in_bundle(&unproven)));
            }
            // Everything is seen. Notes, for the new and changed steps only.
            notes.extend(from_case);
            for (case_id, only) in &seen_scope {
                let Some(script) = prepared.iter().find(|s| s.case_id == *case_id) else { continue };
                // A clean-up guard's target nobody has seen yet is checked
                // when the guard first fires: said here, never a refusal.
                // A case the save does not hold is read as having no text: the
                // note is still said.
                let text = cases.iter().find(|c| c.id == *case_id).map(case_step_text).unwrap_or_default();
                let deferred = crate::autorun::seen_check::guarded_cleanup_deferred(&map, &components, script, &text, only.as_deref(), &files);
                unfired.push((*case_id, crate::autorun::seen_check::merged_unfired(&script.unfired_guards, only.as_deref(), script, &deferred)));
                let guarded = crate::autorun::seen_check::guarded_cleanup_notes(&map, &components, script, &text, only.as_deref(), &files);
                let sentences = guarded
                    .into_iter()
                    .chain(crate::autorun::seen_check::loose_name_notes(&map, &components, script, only.as_deref()))
                    .chain(crate::autorun::preconditions::undeclared_record_note(script, only.as_deref(), &test_prefix, &save_words));
                notes.extend(sentences.map(|n| format!("case {case_id}: note: {n}")));
            }
            // The value check's notes count toward the same cap.
            notes.extend(value_notes.iter().cloned());
            // The delete notes are style warnings: after the case-value
            // notes, so the cap drops them first.
            // The project's other scripts as they will stand after this save.
            let area_pool: Vec<crate::autorun::CaseScript> = saved_scripts
                .iter()
                .filter(|s| !prepared.iter().any(|p| p.case_id == s.case_id))
                .filter(|s| crate::autorun::seen_check::same_project(s, &ctx.org, &ctx.project))
                .cloned()
                .chain(prepared.iter().cloned())
                .collect();
            for (case_id, only) in &seen_scope {
                let Some(script) = prepared.iter().find(|s| s.case_id == *case_id) else { continue };
                let sentences = crate::autorun::script_notes::delete_after_check_notes(script, Some(&components), only.as_deref())
                    .into_iter()
                    .chain(crate::autorun::script_notes::positional_delete_notes(script, Some(&components), only.as_deref()))
                    .chain(
                        saved_scripts
                            .iter()
                            .find(|s| s.case_id == *case_id)
                            .map(|old| crate::autorun::script_notes::moved_with_check(old, script))
                            .unwrap_or_default(),
                    )
                    .chain(crate::autorun::script_notes::order_notes_with(script, &area_pool, Some(&components)));
                notes.extend(sentences.map(|n| format!("case {case_id}: note: {n}")));
            }
            if notes.len() > MAX_SAVE_NOTES {
                let more = notes.len() - MAX_SAVE_NOTES;
                notes.truncate(MAX_SAVE_NOTES);
                notes.push(format!("and {more} more notes"));
            }
        }
        // Everything has passed. A dry run checks the bundle as the write
        // would and stops there; a save writes it.
        if dry_run {
            return crate::autorun::store::check_scripts(&prepared)
                .map(|()| notes)
                .map_err(|e| SaveRefusal::Other(400, e));
        }
        let mut to_write = prepared.clone();
        for (case_id, list) in unfired {
            if let Some(script) = to_write.iter_mut().find(|s| s.case_id == case_id) {
                script.unfired_guards = list;
            }
        }
        match crate::autorun::store::save_scripts_checked(&root, &to_write, &read) {
            Ok(()) => Ok(notes),
            Err(crate::autorun::store::SaveScriptsError::Invalid(e)) => Err(SaveRefusal::Other(400, e)),
            Err(changed @ crate::autorun::store::SaveScriptsError::Changed { .. }) => {
                Err(SaveRefusal::Other(409, changed.to_string()))
            }
            Err(unread @ crate::autorun::store::SaveScriptsError::Unreadable { .. }) => {
                Err(SaveRefusal::Other(500, unread.to_string()))
            }
            Err(switched @ crate::autorun::store::SaveScriptsError::EnvironmentSwitched) => {
                Err(SaveRefusal::Other(409, switched.to_string()))
            }
            Err(crate::autorun::store::SaveScriptsError::Io(e)) => {
                Err(SaveRefusal::Other(500, format!("could not save the bundle: {e}")))
            }
        }
    };
    // With a component in use, the components load, the check and the
    // write hold the components lock, so a component save (which re-checks
    // the scripts that use it) cannot slip between them. Nothing in here
    // awaits or saves a component. A dry run writes nothing, so it does
    // not hold the lock.
    let check_and_save_locked = || {
        if uses_components && !dry_run {
            crate::autorun::components::with_components_locked(check_and_save)
        } else {
            check_and_save()
        }
    };
    let mut checked = check_and_save_locked();
    // Refused only for locators never seen, while a discovery is open: the
    // refused ones are checked on the discovery's current page, the ones
    // there are recorded, and the save is checked once more.
    // Never on a dry run: it records nothing.
    let mut recorded: Option<Vec<String>> = None;
    if !dry_run && matches!(checked, Err(SaveRefusal::Unseen(_))) {
        let targets = seen_check_inputs(&root, ctx, uses_components, &saved_scripts, &carried).ok().and_then(|(map, components, files)| {
            let mut all: Vec<crate::browser::locator::Target> = Vec::new();
            for (case_id, only) in &seen_scope {
                let script = prepared.iter().find(|s| s.case_id == *case_id)?;
                let case = cases.iter().find(|c| c.id == *case_id)?;
                all.extend(crate::autorun::seen_check::unseen_targets(
                    &map,
                    &components,
                    script,
                    &case_step_text(case),
                    only.as_deref(),
                    &files,
                )?);
            }
            Some(all)
        });
        // Each checked script's own area: a sighting is recorded under the
        // discovery's area, so it can only count when that is theirs.
        let areas: Vec<Option<&str>> = seen_scope
            .iter()
            .map(|(case_id, _)| prepared.iter().find(|s| s.case_id == *case_id).and_then(|s| s.area_name()))
            .collect();
        // On the page of the caller's own browser, if it has one.
        if let (Some(n), Some(targets)) = (asker, targets.filter(|t| !t.is_empty())) {
            if let Some(found) = record_refused_on_page(ctx, &root, &areas, &targets, n).await {
                if !found.is_empty() {
                    checked = check_and_save_locked();
                }
                recorded = Some(found);
            }
        }
    }
    let answer = |said: (u16, String)| match &recorded {
        Some(found) => after_recording(found, said),
        None => said,
    };
    let notes = match checked {
        Ok(notes) => notes,
        Err(refused) => return answer(refused.said()),
    };
    if dry_run {
        let blocked = blocked_by_divergence_lines(&root, ctx, &prepared);
        let said = std::iter::once(dry_run_answer(prepared.len(), &lines))
            .chain(precondition_lines)
            .chain(unchecked_lines)
            .chain(given_up_lines)
            .chain(value_listed)
            .chain(notes)
            .chain(blocked);
        return (200, said.collect::<Vec<_>>().join("\n"));
    }
    crate::applog::info(format!("AI saved {} auto-run script(s)", prepared.len()));

    let mut report = vec![format!("saved {} script(s): {}", prepared.len(), lines.join(", "))];
    report.extend(precondition_lines);
    report.extend(unchecked_lines);
    report.extend(given_up_lines);
    report.extend(value_listed);
    report.extend(notes);
    report.extend(kept_marks);
    for (case_id, step) in &repaired_marks {
        match crate::autorun::store::clear_suspected_defect_at(&root, *case_id, *step) {
            Ok(true) => {
                crate::applog::info(format!(
                    "AI repair of case {case_id} step {step} cleared its suspected defect"
                ));
                report.push(format!(
                    "case {case_id}: suspected defect at step {step} cleared - the step was repaired"
                ));
            }
            // The mark moved or went since the repair was read: not ours
            // to clear, and nothing to report.
            Ok(false) => {}
            Err(e) => report.push(format!(
                "case {case_id}: the suspected defect at step {step} could not be cleared: {e}"
            )),
        }
    }
    // A repair declared for the steps of a case the person is rewriting
    // closes the divergence at those steps: the script now follows the
    // rewritten case. A failure to close is said, never a refusal: the
    // scripts are already down.
    for edit in edits.iter().filter(|e| !e.steps.is_empty()) {
        let now = crate::autorun::sessions::now_ms();
        // A step the rewrite renumbered is found by its action text.
        let text = cases.iter().find(|c| c.id == edit.case_id).map(crate::autorun::case_text::CaseText::from_case);
        let closed = crate::autorun::divergences::update(&root, &ctx.org, &ctx.project, |list| {
            Ok(crate::autorun::divergences::close_repaired_with(list, edit.case_id, &edit.steps, now, &|step| {
                text.as_ref().and_then(|t| t.step_action_folded(step))
            }))
        });
        match closed {
            Ok(steps) => report.extend(steps.into_iter().map(|s| {
                format!(
                    "case {}: the divergence at step {s} is closed - the script now follows the rewritten case",
                    edit.case_id
                )
            })),
            Err(e) => {
                // Said only when something was waiting on this repair.
                let waiting = crate::autorun::divergences::load(&root, &ctx.org, &ctx.project)
                    .map(|l| {
                        l.iter().any(|d| {
                            d.case_id == edit.case_id
                                && d.status == crate::autorun::divergences::DivergenceStatus::AwaitingRepair
                                && edit.steps.contains(&d.step)
                        })
                    })
                    .unwrap_or(false);
                if waiting {
                    let at = edit.steps.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ");
                    report.push(format!(
                        "case {}: the divergence at step {at} could not be closed: {e}",
                        edit.case_id
                    ));
                }
            }
        }
    }
    report.extend(blocked_by_divergence_lines(&root, ctx, &prepared));
    // The quirks come last, after the scripts are safely down: a quirk
    // the list will not take (too long, or past the cap) is worth saying
    // so about, but it is not worth losing a good repair over. A refused
    // one is said first, below, so it is not lost at the end of a long
    // answer.
    // Each quirk keeps the case and steps of its repair, and the class of
    // the failure that led to it, so later runs can say whether it helped.
    // Each refused one: its quote, why, and whether it is the cap's refusal.
    let mut refused: Vec<(String, String, bool)> = Vec::new();
    for edit in &edits {
        let Some(text) = edit.quirk.as_deref().filter(|t| !t.trim().is_empty()) else {
            continue;
        };
        let sources: Vec<crate::autorun::quirks::QuirkSource> =
            repair_sources.iter().filter(|s| s.case_id == edit.case_id).cloned().collect();
        match crate::autorun::quirks::record_quirk_classed(
            &root,
            &ctx.org,
            &ctx.project,
            text,
            "assistant",
            crate::autorun::quirks::FROM_AUTORUN,
            sources,
            edit.quirk_class,
            crate::autorun::sessions::now_ms(),
        )
        .map(|(r, _)| r)
        {
            Ok(crate::autorun::quirks::Recorded::Added(id)) => {
                report.push(format!("quirk recorded as {id}: {}", text.trim()))
            }
            Ok(crate::autorun::quirks::Recorded::AlreadyKnown(id)) => {
                report.push(format!("quirk already known, as {id}: {}", text.trim()))
            }
            Ok(crate::autorun::quirks::Recorded::Reactivated(id, reason)) => {
                report.push(format!("quirk {}: {}", reactivated_reply(&id, reason.as_deref()), text.trim()))
            }
            Err(why) => {
                // Quote the quirk (its first line, spaces collapsed), so
                // the assistant can say it again; only a full list has a
                // retire-one-and-record-again answer, the rest keep their
                // own advice.
                let quoted: String = text.trim().lines().next().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
                let why = why.trim_end_matches('.').to_string();
                let cap = crate::autorun::quirks::is_cap_refusal(&why);
                refused.push((quoted, why, cap));
            }
        }
    }
    // With several refused at the cap, the retire advice and the list of
    // what to retire are the same for each: said once, after their lines.
    let several = refused.iter().filter(|(_, _, cap)| *cap).count() > 1;
    let mut refused_quirks: Vec<String> = Vec::new();
    let mut advice: Option<String> = None;
    for (quoted, why, cap) in &refused {
        let again = if *cap { " Retire one and record it again with record_quirk." } else { "" };
        match why.split_once(" - ").filter(|_| *cap && several) {
            Some((reason, rest)) => {
                advice.get_or_insert_with(|| {
                    let mut chars = rest.chars();
                    let first = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
                    format!("{first}{}.", chars.as_str())
                });
                refused_quirks.push(format!("quirk not recorded: \"{quoted}\" - {reason}.{again}"));
            }
            None => refused_quirks.push(format!("quirk not recorded: \"{quoted}\" - {why}.{again}")),
        }
    }
    refused_quirks.extend(advice);
    refused_quirks.extend(report);
    answer((200, refused_quirks.join("\n")))
}

/// One line for each saved case that a divergence keeps from running: its
/// runs are Blocked until a person rules (or the repair lands). Read only;
/// a divergences file that cannot be read says nothing.
pub(crate) fn blocked_by_divergence_lines(
    root: &std::path::Path,
    ctx: &BridgeContext,
    saved: &[crate::autorun::CaseScript],
) -> Vec<String> {
    let Ok(list) = crate::autorun::divergences::load(root, &ctx.org, &ctx.project) else {
        return Vec::new();
    };
    saved
        .iter()
        .filter_map(|s| {
            let d = crate::autorun::divergences::blocking(&list, s.case_id)?;
            Some(format!(
                "case {}: {} - runs are Blocked until then",
                s.case_id,
                crate::autorun::divergences::run_sentence(d)
            ))
        })
        .collect()
}

/// The most notes one save answers with before saying how many more.
pub(crate) const MAX_SAVE_NOTES: usize = 20;

/// The refusal of a bundle whose scripts name locators never seen, from
/// each refused case's lines in bundle order: the lines alone when one
/// case is refused, each after "case <id>: " when more are; listed as
/// `seen_check::refusal_list` lists them.
pub(crate) fn unseen_in_bundle(unseen: &[(i32, Vec<String>)]) -> String {
    let lines: Vec<String> = match unseen {
        [(_, lines)] => lines.clone(),
        many => many
            .iter()
            .flat_map(|(case_id, lines)| lines.iter().map(move |l| format!("case {case_id}: {l}")))
            .collect(),
    };
    crate::autorun::seen_check::refusal_list(&lines)
}

/// Why a script save's last gate refused it: a locator never seen on the
/// live app (`Unseen`, the check's own sentence), or anything else.
pub(crate) enum SaveRefusal {
    Unseen(String),
    Other(u16, String),
}

impl SaveRefusal {
    pub(crate) fn said(self) -> (u16, String) {
        match self {
            SaveRefusal::Unseen(why) => (400, why),
            SaveRefusal::Other(status, why) => (status, why),
        }
    }
}

/// A test case's step actions and expected results, as the seen check
/// reads them: plain words, the Markdown taken out.
pub(crate) fn case_step_text(case: &crate::ado::TestCaseFull) -> Vec<String> {
    crate::steps_xml::plain_texts(&case.steps)
}

/// What a script save's seen check reads: the discovery map, the project's
/// components (only when a checked script uses one, so a damaged file
/// never holds up a save that does not need it) and its Test files, so the
/// size the app shows for one a script uploads passes as the script's own
/// data.
/// The areas the discovery in the asker's browser carried over from areas it
/// left (`DiscoveryState::carried`); none with no discovery or no browser, or
/// when the browser is busy.
pub(crate) async fn carried_of(asker: Option<SlotNo>) -> Vec<crate::autorun::discovery_map::AreaMap> {
    let Some(n) = asker else { return Vec::new() };
    let Ok(slot) = tokio::time::timeout(std::time::Duration::from_millis(500), crate::commands::autorun::session_slot(n).lock()).await
    else {
        return Vec::new();
    };
    slot.as_ref().and_then(|s| s.discovery.as_ref()).map(|d| d.carried.clone()).unwrap_or_default()
}

pub(crate) fn seen_check_inputs(
    root: &std::path::Path,
    ctx: &BridgeContext,
    uses_components: bool,
    saved_scripts: &[crate::autorun::CaseScript],
    carried: &[crate::autorun::discovery_map::AreaMap],
) -> Result<
    (crate::autorun::discovery_map::DiscoveryMap, crate::autorun::components::ComponentFile, Vec<crate::test_files::TestFile>),
    String,
> {
    let mut map = crate::autorun::seen_check::load_checked_map_with(root, &ctx.org, &ctx.project, saved_scripts)?;
    crate::autorun::discovery_map::add_carried(&mut map, carried);
    let components = if uses_components {
        crate::autorun::components::load_components(root, &ctx.org, &ctx.project)?
    } else {
        crate::autorun::components::ComponentFile::default()
    };
    let files = crate::test_files::list(&crate::test_files::folder(root, &ctx.org, &ctx.project)).unwrap_or_else(|e| {
        crate::applog::warn(format!("Auto Run save: the Test files could not be listed: {e}"));
        Vec::new()
    });
    Ok((map, components, files))
}

