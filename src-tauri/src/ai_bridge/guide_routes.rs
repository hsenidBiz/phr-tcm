//! The writing guide, the tag list and the modules the guide covers.

use super::*;

/// Run a draft through the app's REAL importer to get `TestCase`s, so
/// these tools accept exactly what the Import Test Cases tab accepts (bare
/// array, `test_cases` wrapper, the lot).
/// The project's existing tag names, for suggesting tags that match what
/// the team already uses instead of inventing near-duplicates.
///
/// Reads the shared reference cache the app fills (cache/mod.rs) - the
/// whole point is that an assistant asking for tags does NOT repeat a
/// request the app has already made. Only a completely cold cache (the AI
/// asked before the developer opened a tag field) fetches, and it stores
/// the result so the app doesn't pay for it either.
pub(crate) async fn tags(
    caller: &crate::autorun::agents::Caller,
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
    target: &str,
) -> (u16, String) {
    let project = tags_project(caller, ctx, client).await;
    let key = crate::cache::keys::tags(&ctx.org, &project);
    let (values, source) = match crate::cache::get::<Vec<String>>(&key) {
        Some(v) => (v, "cache"),
        None => match client {
            Some(c) => match c.get_tags(&ctx.org, &project).await {
                Ok(v) => {
                    crate::cache::put(&key, &v);
                    (v, "fetched")
                }
                Err(e) => return (502, format!("could not read tags: {e}")),
            },
            None => return (503, "sign in to Test Case Manager first".into()),
        },
    };
    // A project can carry thousands of tags; ?query= filters to a
    // case-insensitive substring match so the common call stays cheap.
    let total = values.len();
    let (values, capped): (Vec<String>, bool) =
        match q(target, "query").filter(|f| !f.trim().is_empty()) {
            Some(f) => {
                let f = f.to_lowercase();
                (
                    values.into_iter().filter(|t| t.to_lowercase().contains(&f)).collect(),
                    false,
                )
            }
            // No query on a 2,000-tag project used to dump the whole list
            // (~30 KB) into the assistant's context (round 8 dogfooding) -
            // capped, with the cap saying how to get the rest.
            None if values.len() > NO_QUERY_TAG_LIMIT => {
                (values.into_iter().take(NO_QUERY_TAG_LIMIT).collect(), true)
            }
            None => (values, false),
        };
    let note = if capped {
        format!(
            "Showing {NO_QUERY_TAG_LIMIT} of {total} tags - pass ?query= (a substring) to \
             search all of them. Prefer an existing tag over a new one. Tags are \
             semicolon-separated in the import JSON, never commas."
        )
    } else {
        "Prefer an existing tag over a new one. Tags are semicolon-separated in the import \
         JSON, never commas."
            .to_string()
    };
    let mut out = serde_json::json!({
        "tags": values,
        "count": values.len(),
        "total": total,
        "source": source,
        "note": note,
    });
    // Tags are per project, so while combined the answer says whose they
    // are. Off, the answer is exactly what it always was.
    if ctx.all_projects {
        out["project"] = serde_json::json!(project);
    }
    (200, out.to_string())
}

/// Who a writing job belongs to: one assistant (its proxy id and, for a named
/// subagent, its name), or `None` for the default caller (an older proxy, a
/// test, the person).
pub(crate) type JobOwner = Option<crate::autorun::agents::AgentKey>;

pub(crate) fn job_owner(caller: &crate::autorun::agents::Caller) -> JobOwner {
    match caller {
        crate::autorun::agents::Caller::Default => None,
        crate::autorun::agents::Caller::Agent(key) => Some(key.clone()),
    }
}

/// The PBI each assistant's latest writing job named as its reference (the
/// intake's `examples_pbi`), per organization, kept so `get_tags` can read
/// the tags of the project that job's cases belong to. Process coordination,
/// not cached data: it holds only what the intake named, nothing fetched.
/// In memory only. A later job by the same assistant in the same organization
/// replaces its entry (a job naming no PBI removes it), an assistant's
/// goodbye drops its entries, a different account signing in drops all of
/// them, and it is gone when the app closes.
pub(crate) static WRITING_JOB_PBIS: StdMutex<std::collections::BTreeMap<(JobOwner, String), i32>> =
    StdMutex::new(std::collections::BTreeMap::new());

pub(crate) fn note_writing_job_pbi(caller: &crate::autorun::agents::Caller, org: &str, pbi: Option<i32>) {
    let mut jobs = WRITING_JOB_PBIS.lock().unwrap_or_else(|e| e.into_inner());
    let key = (job_owner(caller), org.to_string());
    match pbi {
        Some(p) => {
            jobs.insert(key, p);
        }
        None => {
            jobs.remove(&key);
        }
    }
}

/// An assistant said goodbye, or its session was found gone past its grace:
/// its jobs (and its named subagents') are over.
pub fn forget_writing_jobs_of(agent_id: &str) {
    WRITING_JOB_PBIS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(owner, _), _| owner.as_ref().is_none_or(|k| k.id != agent_id));
}

/// Every job's PBI is forgotten: called when a different account signs in,
/// as the caches are, so one person's job is never read as the next one's.
pub fn forget_writing_jobs() {
    WRITING_JOB_PBIS.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// Whose tags `get_tags` reads: the home project (where cases are written),
/// except that while combined the caller's own writing job naming a PBI
/// points at that PBI's project. A PBI whose project cannot be read leaves
/// it at home.
pub(crate) async fn tags_project(
    caller: &crate::autorun::agents::Caller,
    ctx: &BridgeContext,
    client: Option<&crate::ado::AdoClient>,
) -> String {
    if !ctx.all_projects {
        return ctx.project.clone();
    }
    let pbi = WRITING_JOB_PBIS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&(job_owner(caller), ctx.org.clone()))
        .copied();
    match (pbi, client) {
        (Some(pbi), Some(c)) => {
            work_item_project_cached(ctx, c, pbi).await.unwrap_or_else(|_| ctx.project.clone())
        }
        _ => ctx.project.clone(),
    }
}

/// The most tags a query-less get_tags answers with. High enough that a
/// small project's whole list still comes back in one call, low enough
/// that a 2,229-tag org cannot flood the context by accident.
pub(crate) const NO_QUERY_TAG_LIMIT: usize = 300;

/// The org's Module values: configured picklist first, observed values as
/// the fallback - the same discovery the app's own module picker uses.
/// What is known about the Module values a case may carry.
///
/// Three states, not one list. Collapsing them lost the only one that
/// matters: a failed lookup used to come back as an empty Vec, which every
/// caller read as "no constraint to check", so validate_cases answered a
/// dropped connection or a 403 with a clean bill of health.
pub enum Modules {
    /// This organization has no Module field at all.
    NotConfigured,
    /// The values a case may carry. Empty means the field is free text.
    Known(Vec<String>),
    /// The lookup failed, so nothing can be said about Module either way.
    Unavailable(String),
}

pub(crate) async fn allowed_modules(ctx: &BridgeContext, client: &crate::ado::AdoClient) -> Modules {
    let Some(fref) = &ctx.module_ref else {
        return Modules::NotConfigured;
    };
    match client
        .get_field_allowed_values(&ctx.org, &ctx.project, "Test Case", fref)
        .await
    {
        // A picklist with entries is the answer.
        Ok(list) if !list.is_empty() => Modules::Known(list),
        // No picklist: the field is free text, so fall back to what the
        // project actually uses. That call failing is still a failure.
        Ok(_) => match client.field_values_in_use(&ctx.org, &ctx.project, fref).await {
            Ok(used) => Modules::Known(used),
            Err(e) => Modules::Unavailable(e.to_string()),
        },
        Err(e) => Modules::Unavailable(e.to_string()),
    }
}

impl Modules {
    /// The values to check against, when there are any to check against.
    pub fn known(&self) -> &[String] {
        match self {
            Modules::Known(v) => v,
            _ => &[],
        }
    }
}

/// The writing guide's standard granularity section, used while the
/// person's own writing style is off.
pub const GRANULARITY: &str = "\
        ## Granularity - quality over quantity\n\
        Similar checks belong in ONE case, not several. Checking a\n\
        notification's title and checking its body is one case with two\n\
        steps (or one step with both in the expected result) - not two\n\
        cases. Split only when the checks need different setup or data, or\n\
        can fail independently in a way the tester must record separately.\n\
        A padded case count is not coverage; every extra case is another\n\
        row someone has to execute and maintain.\n\n\
";

/// The writing guide's standard edge-case section, used while the
/// person's own writing style is off.
pub const EDGE_CASES: &str = "\
        ## Edge cases worth writing\n\
        A set that only walks the happy path is not finished. For each\n\
        feature, add the edge cases a tester can run from the application\n\
        itself in a few minutes, each as its own case with the branch in\n\
        its title:\n\n\
        - Access: open the page's address without signing in, or as a role\n\
        that should not see it; the expected result is what the application\n\
        shows instead (the sign-in page, a permission message), named\n\
        exactly.\n\
        - Required and empty: submit with a required field blank, with only\n\
        spaces, at the field's maximum length, and one over it.\n\
        - Boundaries the form shows: the smallest and largest value a field\n\
        accepts, a date at the edge of the allowed range, zero and a\n\
        negative number where the field is numeric.\n\
        - State: the same action twice (double submit, refresh after\n\
        saving, back button after a save), and an item edited by someone\n\
        else in between when the application shows that.\n\
        - Absence: the list with nothing in it, a search with no matches, a\n\
        filter that removes everything; the expected result is the empty\n\
        state's own words.\n\n\
        Do NOT write cases that need developer tools, a modified request,\n\
        a database change, a disconnected network, or a clock change: a\n\
        tester cannot run them from the application, and a case nobody can\n\
        run is worse than none. If a spec names such a behaviour, put it in\n\
        `reviewer_notes` as a note for the developers instead.\n\n\
";

/// The person's writing style (`crate::writing_style`) as the guide
/// carries it: their text under its own heading, then the line that keeps
/// the format and import rules above it.
pub(crate) fn custom_style_section(text: &str) -> String {
    format!(
        "## Team test design style (set on this machine)\n{}\n\n\
        If anything here conflicts with the Format section or the import rules, \
        the Format section and the import rules win.\n\n",
        text.trim()
    )
}

/// The workflow line the guide carries while the person's writing style is
/// on: where the style's own steps, if it has any, fit in.
pub(crate) const CUSTOM_STYLE_STEP: &str = "\
        1.5. If the team test design style above adds steps (for example a scenario list to approve \
        before drafting, or a summary or review at the end), follow them at the point it says.\n\
";

/// The guide's custom style section: the person's saved style while the
/// Writing Style module is on (`on`), nothing while it is off - the guide is
/// then exactly the standard one, whatever is saved. `on` is passed in so
/// both answers are testable.
pub fn custom_style_when(on: bool) -> Option<String> {
    if !on {
        return None;
    }
    crate::writing_style::current_text().map(|t| custom_style_section(&t))
}

/// Live writing guide: format rules from the importer's own constants +
/// the org's Module values, fetched fresh (no snapshot staleness). The
/// person's writing style is read from disk on every call, so a save takes
/// effect the next time an assistant reads the guide.
pub(crate) async fn guide(ctx: &BridgeContext, client: &crate::ado::AdoClient) -> String {
    let custom = custom_style_when(crate::modules::on(crate::modules::Module::WritingStyle));
    let statuses = crate::model::VALID_STATUSES
        .iter()
        .map(|v| format!("\"{v}\""))
        .collect::<Vec<_>>()
        .join(" or ");
    let module_lines = match allowed_modules(ctx, client).await {
        Modules::Known(v) if !v.is_empty() => {
            v.iter().map(|m| format!("- `{m}`")).collect::<Vec<_>>().join("\n")
        }
        Modules::Known(_) | Modules::NotConfigured => {
            "This organization has no Module values to choose from - leave it blank.".to_string()
        }
        // Deliberately not the same sentence as above: one says there is
        // nothing to pick, the other says we could not find out. An
        // assistant told the first will confidently leave Module blank.
        Modules::Unavailable(why) => format!(
            "Module values could not be read from Azure DevOps ({why}) - ask the developer \
             rather than guessing."
        ),
    };
    // Cache-only: the guide must not become another request. If nothing is
    // cached yet, `get_tags` will fill it on demand.
    let tag_lines = match crate::cache::get::<Vec<String>>(&crate::cache::keys::tags(&ctx.org, &ctx.project)) {
        Some(tags) if !tags.is_empty() => {
            let shown = tags
                .iter()
                .take(GUIDE_TAG_LIMIT)
                .map(|t| format!("`{t}`"))
                .collect::<Vec<_>>()
                .join(", ");
            if tags.len() > GUIDE_TAG_LIMIT {
                format!(
                    "{shown}\n\n(showing {GUIDE_TAG_LIMIT} of {} - call `get_tags` for all of them)",
                    tags.len()
                )
            } else {
                shown
            }
        }
        _ => "Call `get_tags` for the list this project already uses.".to_string(),
    };
    format!(
        "# Writing test cases for Test Case Manager ({org}/{project})\n\n\
        Produce a JSON array of test cases. The developer imports it via the\n\
        Import Test Cases tab, reviews, then creates - you never write to Azure DevOps.\n\n\
        ## Use these tools; do not rebuild them\n\
        Do NOT write your own script, generator or one-off parser to produce,\n\
        transform, validate or reformat test cases. Use the tools: they carry\n\
        THIS project's live rules - the allowed Module values, the tags\n\
        already in use, the exact field contract, and what a kept `id` means\n\
        on the way back in. A hand-rolled equivalent gets those subtly wrong,\n\
        and wrongly in a way nobody sees: the file still looks right.\n\n\
        This has already cost a real draft. A set too large for `validate_cases`\n\
        led to a workaround being written instead, and it reported a pass over\n\
        cases it had never checked.\n\n\
        If a tool genuinely cannot do what you need, STOP and say so. Name the\n\
        tool, what you needed it to do, and what it did instead - then ask the\n\
        developer whether a tool should be added for it. That is a decision for\n\
        them, and a missing capability they hear about gets fixed for everyone.\n\
        Routing around it silently fixes it for nobody and hides the gap.\n\n\
        - `db_lookup` and `db_query` check real data: which table a screen reads\n\
        from, and what a value is today. The expected result still comes from the\n\
        specification. The database only tells you the current state, never what\n\
        it should be.\n\n\
        ## Format\n\
        Each case: `title` (required, <=255 chars), `steps` (required, each\n\
        `{{\"action\", \"expected\"}}`, or `{{\"shared\": N}}` for a Shared Steps reference - keep those exactly as exported, never invent one),\n\
        a step may carry `attachments`: `{{\"path\": \"C:/shots/login.png\"}}` attaches a new file (a full local path on this machine, 25 MB at most,\n\
        never on a shared step), and an entry with an `id` is one already on the case, so keep those exactly as exported.\n\
        To take an existing one off, prefer `transform_cases` `remove_attachment` (it does both parts); by hand, delete its entry AND list its `id` in the case's `remove_attachments`.\n\
        `attach_file` needs a `where` naming the case. Existing attachments are not copied to a case without an `id`.\n\
        `tags` (semicolon-separated, never commas),\n\
        `automation_status` (exactly {statuses}),\n\
        `priority` (optional, a number 1-4: a new case without one is created with 2,\n\
        and an update without one leaves the work item's priority as it is), `module` (ONLY from the list\n\
        below), `preconditions` (state, not steps), and `comment` - the developer's own note, which round-trips\n\
        through the file and is never sent to Azure DevOps. You never write\n\
        `comment`: leave it exactly as you found it, and never add one.\n\
        Include `id` ONLY to update that exact work item;\n\
        omit it to create.\n\n\
        Reuse preconditions VERBATIM wherever the environment genuinely is\n\
        the same - do not reword the same setup per case. The run-sheet\n\
        ordering groups cases by shared preconditions so the tester changes\n\
        environment as little as possible; 192 bespoke wordings of the same\n\
        few setups leave it nothing to group, and the ordering buys nothing.\n\n\
        {granularity}\
        ## Writing style - sound like a tester, not a model\n\
        This applies to the TEST CASES THEMSELVES: every title, step,\n\
        expected result, precondition and reviewer note. What you say to the\n\
        developer in the conversation is not bound by it. Inside the cases,\n\
        FOLLOW THIS WRITING STYLE:\n\n\
        - SHOULD use clear, simple language.\n\
        - SHOULD be spartan and informative.\n\
        - SHOULD use short, impactful sentences.\n\
        - SHOULD use active voice; avoid passive voice.\n\
        - SHOULD focus on practical, actionable checks.\n\
        - SHOULD use data and examples to support claims when possible: the\n\
        exact value entered, the exact text expected.\n\
        - SHOULD use \"you\" and \"your\" to directly address the tester.\n\
        - AVOID em dashes (—) anywhere. Use only commas, periods, or other\n\
        standard punctuation. To connect ideas, use a period; never an em dash.\n\
        - AVOID constructions like \"...not just this, but also this\".\n\
        - AVOID metaphors and clichés.\n\
        - AVOID generalizations.\n\
        - AVOID setup language in any sentence: in conclusion, in closing, etc.\n\
        - AVOID warnings or notes to the reader inside a case; write the\n\
        content requested and nothing around it.\n\
        - AVOID unnecessary adjectives and adverbs.\n\
        - AVOID hashtags.\n\
        - AVOID semicolons.\n\
        - AVOID markdown and asterisks in titles (reviewer_notes is rendered as\n\
        markdown and needs none). Steps, expected results and preconditions\n\
        follow the Formatting section.\n\
        - AVOID these words: can, may, just, that, very, really, literally,\n\
        actually, certainly, probably, basically, could, maybe, delve, embark,\n\
        enlightening, esteemed, shed light, craft, crafting, imagine, realm,\n\
        game-changer, unlock, discover, skyrocket, abyss, not alone, in a world\n\
        where, revolutionize, disruptive, utilize, utilizing, dive deep,\n\
        tapestry, illuminate, unveil, pivotal, intricate, elucidate, hence,\n\
        furthermore, however, harness, exciting, groundbreaking, cutting-edge,\n\
        remarkable, it, remains to be seen, glimpse into, navigating,\n\
        landscape, stark, testament, in summary, in conclusion, moreover,\n\
        boost, skyrocketing, opened up, powerful, inquiries, ever-evolving.\n\
        - MUST put the name of anything the tester looks for on screen in\n\
        double quotation marks: the \"Save\" button, the \"Leave Requests\"\n\
        page, the \"Search employees\" placeholder, the \"Status\" column,\n\
        the \"Approved\" tab, the \"Your changes were saved\" message. The\n\
        name inside the quotes is the exact text on screen, capitalised as\n\
        the application shows it. Without quotes a tester cannot tell the\n\
        word \"save\" from the button \"Save\".\n\n\
        IMPORTANT: review every case before handing it back and make sure\n\
        there are no em dashes.\n\n\
        ## Formatting\n\
        Steps and preconditions are Markdown. Use formatting only where it helps\n\
        reading; never excessively. In a step, bold the one value or control it\n\
        turns on, such as the **Save** button. Use a list in the preconditions\n\
        for several conditions. Put any SQL or code in a fenced block: a lead-in\n\
        sentence such as \"Run the following query.\", then the query in a\n\
        ```sql fence. In a step its lines stay plain lines in Azure DevOps; in\n\
        the preconditions it shows as code. A step keeps bold, italic and line\n\
        breaks, a few bold words at most; never headings, tables, links,\n\
        images or emphasis on a whole sentence. Text Markdown would change\n\
        (`\\\\server`, a line starting with `>`, a `---` line, `__init__`) goes in\n\
        backticks.\n\n\
        ## area\n\
        Give every case an `area`: where it sits on the page or in the\n\
        feature, as a path with `/` between the levels, page or screen\n\
        first: `\"Manage Events / Create / Validation\"`. Reuse the same spelling\n\
        for the same place across the set, so its cases stack under one\n\
        node in the app's Test map instead of two. This is the app's own grouping path, not the work item's Area Path. Never sent to Azure DevOps. Set or move it in bulk with `transform_cases` (`set_area`, and `where.area_is` to pick an area).\n\n\
        ## specs\n\
        Put the documents these cases were written from in the file's\n\
        top-level `\"specs\"` list, beside `test_cases`: each entry a path to\n\
        a markdown file (relative to the JSON file, or absolute) or an Azure\n\
        DevOps wiki page URL as copied from the browser. These are the\n\
        documents named at intake. The developer reads them beside the cases\n\
        in the browser, and every `Spec:` citation in `reviewer_notes` links\n\
        to its heading there - so name the document in a citation the way its\n\
        file or wiki page is named.\n\n\
        Only `.md` files and Azure DevOps wiki links may be listed in\n\
        `specs`. Any other file - code such as `.cshtml` or `.cs`, text\n\
        files, PDFs - may be read and cited in `reviewer_notes`, but never\n\
        added to `specs`: it is refused if added, and the importer warns\n\
        about each one it drops.\n\n\
        ## Findings\n\
        When something you read is WRONG - a case that contradicts its spec,\n\
        a spec that contradicts itself, code that does what neither says -\n\
        put it in that case's `findings` list in the file: `{{\"kind\":\n\
        \"test_case\"|\"spec\"|\"code\", \"subject\": \"<spec section or code\n\
        symbol>\", \"title\": \"<one line>\", \"detail\": \"<markdown>\"}}`.\n\
        `kind` is one of test_case, spec or code. Write it into the file\n\
        directly, or with `transform_cases` and its `set_findings` op. A\n\
        finding about the spec or the code goes on the case it affects; if\n\
        several, on the first. The developer reads findings in the browser\n\
        page under each case. Do this on your own when it applies; nobody\n\
        will ask you to. And never write `comment` for this or for anything\n\
        else, and never put it in reviewer_notes: the first is the\n\
        developer's field, the second says where a case came from and\n\
        nothing more. Do not write a case around a defect as if the defect\n\
        were the requirement - record the finding and say so in the\n\
        conversation.\n\n\
        ## reviewer_notes\n\
        Optional, never sent to Azure DevOps, and the most useful thing you\n\
        can add. Two parts, in this order, and nothing else:\n\n\
        1. **What this case checks**, in one or two plain sentences that\n\
        someone who has not read the spec would understand. Not the steps\n\
        retold - the point, in ordinary words.\n\
        2. **Where the requirement lives**: `Spec: Step10-ManagePerformanceCycle.md\n\
        7.7 (AC-3)`, `Code: IndexModel.CanCopyFromPreviousCycle`, or both.\n\n\
        Add `Out of scope: SSO` only when THIS case deliberately leaves\n\
        something out. The `Spec:` pointer comes FIRST, with the quote\n\
        directly beneath it as `> \"<the source sentence>\"` - the checker\n\
        reads a quote only in that position, so a quote placed above the\n\
        pointer is reported as missing. Quote the source sentence\n\
        verbatim, or it must not be presented as a quote: when you cannot\n\
        quote (a table, a diagram, code), state the exemption in the\n\
        fixed form\n\
        `Spec: <file> <section> - no quotable text (<why>)`,\n\
        naming why as one of code-not-prose, absence,\n\
        table/diagram, or synthesis. Never rewrite inside quotation marks;\n\
        elide with an ellipsis (`...`) instead - check_spec_coverage\n\
        honours an elided quote by requiring every fragment verbatim, in\n\
        order.\n\n\
        Leave OUT, every time:\n\
        - Where the cases came from as a body of work - \"Source:\n\
        implementation (authority = app)\", \"written from the spec\". The\n\
        developer chose that at intake and already knows it; repeating it\n\
        per case is the same sentence on every case.\n\
        - The SET's scope. That was agreed once, in the plan. A note is\n\
        about ONE case.\n\
        - A walk through the steps. They are directly above the note.\n\
        - Anything WRONG that you noticed - a contradiction, a gap, a bug.\n\
        That is a finding: put it in the case's `findings` list and keep\n\
        the note to what the case checks and where its requirement lives.\n\
        - Your reasoning, or a decision argued at length. If a case really\n\
        needs an argument made, that belongs in the conversation, not here - and never in `comment`.\n\n\
        Cite; never paraphrase from memory. Rendered as MARKDOWN, so a wiki\n\
        link works - but two sentences and a citation need no formatting.\n\n\
        ## One branch per case\n\
        When the spec says a thing is shown ONLY when X, that is two test\n\
        cases, not one. Write the positive and the negative separately, each\n\
        with a title stating its own branch, and have each name its\n\
        counterpart in `reviewer_notes` so neither reads as a duplicate.\n\n\
        Never fold the negative in as an extra step of the positive - do not\n\
        turn a setting off half way through a case to check the other branch.\n\
        It is covered that way, but it is INVISIBLE: someone auditing titles\n\
        against a large backlog cannot see the negative was tested, and the\n\
        case leaves the environment different from how it found it.\n\
        `validate_cases` warns when a case looks like both branches at once.\n\n\
        Two things a merged case hides, both real: a title claiming a button\n\
        is hidden from the Manager AND the Reviewer when the case only ever\n\
        signs in as one of them, and a weak negative - one unrated goal out\n\
        of two, where four out of five would have caught an implementation\n\
        that passes on any rating.\n\n\
        {edge_cases}\
        ## Allowed Module values (live)\n{module_lines}\n\n\
        ## Tags this project already uses\n\
        Reuse these wherever one fits - a near-duplicate ('smoke-test' next to\n\
        an existing 'smoke') fragments the project's tags. A genuinely new tag\n\
        is allowed when nothing here matches.\n\n{tag_lines}\n\n\
        {transform_ops}\n\
        ## Workflow\n\
        0. The assistant the person talks to settles the PBI, if the job is for\n\
        one: call `get_current_pbi`. The developer named one that is TCM's: use\n\
        it, no search. A different one: ask which. None named and TCM has one:\n\
        say \"PBI <id>, <title>, is selected in TCM - is that the one?\" and wait\n\
        for their yes. TCM has none: `search_pbis`, or ask for the id; a job for\n\
        a specification alone needs no PBI. Pass the settled PBI id to every\n\
        slice subagent: a subagent never asks about the PBI, it uses the one it\n\
        was given.\n\
        Then call `begin_test_case_writing` and put its questions to the\n\
        developer. What the file is called (it lives in the repository's\n\
        .test-cases folder), which specs are authoritative and what is out of\n\
        scope are theirs to decide, not yours to assume.\n\
        1. Call `get_test_cases` for the PBI you're writing for and mimic\n\
        their style and granularity.\n\
        {scenario_step}\
        2. Draft your cases. Write your draft IN SPEC ORDER - cases walking\n\
        down the document, so a reviewer can scroll the spec and the file\n\
        together, and so `check_spec_coverage` (next) can reason about it\n\
        against the document in the order you wrote it.\n\
        2.5. Call `check_spec_coverage` with the draft and the plan's spec\n\
        paths, while it is still in spec order. Report `uncovered` to the\n\
        developer and account for every entry - \"out of scope for this batch\"\n\
        is a fine answer, silence is not.\n\
        3. Call `optimize_cases` with the JSON: it spells navigation out as\n\
        steps, trims expected results to the outcome, and reorders the cases so\n\
        the tester changes environment as few times as possible. Hand back the\n\
        JSON it returns. The optimizer then stamps every case with BOTH\n\
        orders: `spec_order` (the order you wrote) and `tester_order` (its\n\
        grouped run sequence). Keep those two fields exactly as it set them -\n\
        do not renumber them by hand, and do not strip them; the app uses\n\
        them to flip the queue between the two readings.\n\
        Its options: `dry_run: true` answers only the report (run it first and\n\
        read `expected_rewritten`, every expected result it would shorten,\n\
        before and after). `entry` is the first step of every preamble\n\
        (default \"Launch the application.\"); a non-launch entry, such as\n\
        opening a module, goes after the sign-in step. `reorder: false` keeps a\n\
        set meant to be read against the specification in document order -\n\
        navigation and expected results are still cleaned up. `trim_expected:\n\
        false` leaves every expected result exactly as written and still does\n\
        the navigation, ordering and dedupe work: use it when the value of the\n\
        set is in what each expected result ASSERTS (arithmetic, formulas,\n\
        quoted messages), or when a dry run reports that reordering saved 0\n\
        environment switches. For a large draft pass `path`, and `in_place:\n\
        true` to write the result back to that file (atomic) and get only the\n\
        report; an in-place write keeps the file's other fields (`specs`,\n\
        `comments`), and without it the response carries the file's `specs`\n\
        for you to write back.\n\
        4. Call `validate_cases` and fix every warning. For a large draft,\n\
        pass a local file via its `path` argument instead of inlining the\n\
        JSON.\n\
        5. For later edits - retagging, retitling, setting a module - call\n\
        `transform_cases` instead of rewriting the file yourself.\n",
        org = ctx.org,
        project = ctx.project,
        transform_ops = crate::transform::ops_reference(),
        granularity = custom.as_deref().unwrap_or(GRANULARITY),
        edge_cases = if custom.is_some() { "" } else { EDGE_CASES },
        scenario_step = if custom.is_some() { CUSTOM_STYLE_STEP } else { "" },
    )
}

