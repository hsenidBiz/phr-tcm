//! Reporting a bug in THIS app, as a GitHub issue.
//!
//! Not Azure DevOps: bugs in the test cases go to ADO (that is what the
//! runner's Bug dialog is for), bugs in the tool go where the tool is
//! developed. The issue lands on the PUBLIC releases repo, because that is
//! the only place a colleague who installed the app - and has no access to
//! the private source repo - can file anything at all.
//!
//! Two consequences follow from "public", and both shape this module:
//!
//! 1. **The log is scrubbed.** It names the organization, the project, the
//!    work items and the person signed in. None of that belongs in a public
//!    issue, so it is replaced by placeholders before the text goes
//!    anywhere. `scrub` is the whole of that promise, and is tested as such.
//! 2. **Nothing is posted automatically.** The app opens a PREFILLED issue
//!    form and the reporter presses the button. That is not just courtesy:
//!    posting for them would need a token, and a token shipped inside an
//!    installer is readable by anyone who has the installer. There is no
//!    credential in this app and this feature does not add one.
//!
//! GitHub's issue URL carries a body but cannot carry an attachment, so the
//! full scrubbed log is written to a file the reporter can drag onto the
//! issue; the body holds as much of the tail as a URL can safely take.

/// Where issues go. The public releases repo - see the module note.
const ISSUES_URL: &str =
    "https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases/issues/new";

/// How much log to put in the URL itself.
///
/// Browsers and servers both give up on very long URLs somewhere past 8KB,
/// and the title and template take their share, so the body's log excerpt
/// is kept well inside that. The rest is what the attached file is for.
const URL_LOG_BUDGET: usize = 4000;

/// What the app hands the reporter.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct BugReport {
    /// The prefilled GitHub issue form. Nothing is submitted until the
    /// reporter presses the button on that page.
    pub url: String,
    /// The full scrubbed log, written to disk so it can be dragged onto
    /// the issue - GitHub has no way to attach a file from a URL.
    pub log_path: String,
    /// Whether the body had to drop older lines to fit the URL.
    pub truncated: bool,
}

/// An email address, roughly. Deliberately generous: over-matching costs a
/// placeholder, under-matching leaks somebody's address.
fn looks_like_email(word: &str) -> bool {
    let core = word.trim_matches(|c: char| !c.is_alphanumeric());
    match core.split_once('@') {
        Some((user, host)) => !user.is_empty() && host.contains('.'),
        None => false,
    }
}

/// Remove the things that identify the company from a log before it can be
/// pasted anywhere public.
///
/// Replaced: the organization and project by name (they appear in every
/// request URL), any email address, and any single-quoted value - which is
/// how the log writes work item titles. Ids are LEFT ALONE: a bare number
/// says nothing outside the organization, and without it the log stops
/// being traceable to the run it describes.
pub fn scrub(log: &str, org: &str, project: &str) -> String {
    let mut out = String::with_capacity(log.len());
    for (i, line) in log.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut line = line.to_string();
        // Longest first: a project named "Web" must not chew holes in a
        // longer name that contains it.
        let mut names: Vec<(&str, &str)> = vec![(org, "<org>"), (project, "<project>")];
        names.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));
        for (name, placeholder) in names {
            let name = name.trim();
            if name.len() >= 2 {
                line = line.replace(name, placeholder);
            }
        }
        // Quoted values are titles - "Submit failed for 'Login as admin'".
        line = replace_quoted(&line);
        line = line
            .split(' ')
            .map(|w| if looks_like_email(w) { "<email>" } else { w })
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&line);
    }
    out
}

/// Everything between single quotes becomes a placeholder.
fn replace_quoted(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('\'') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('\'') else { break };
        out.push_str(&rest[..open]);
        out.push_str("'<redacted>'");
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// The tail of the log that fits the URL budget, newest lines kept.
/// Returns (excerpt, whether anything was dropped).
pub fn excerpt(scrubbed: &str) -> (String, bool) {
    if scrubbed.len() <= URL_LOG_BUDGET {
        return (scrubbed.to_string(), false);
    }
    // Cut on a line boundary - half a log line helps nobody.
    let start = scrubbed.len() - URL_LOG_BUDGET;
    let cut = scrubbed[start..]
        .find('\n')
        .map(|i| start + i + 1)
        .unwrap_or(start);
    (scrubbed[cut..].to_string(), true)
}

/// The issue body: what the reporter said, what they were running, and the
/// end of the log.
pub fn body(
    description: &str,
    version: &str,
    os: &str,
    excerpt: &str,
    truncated: bool,
    log_path: &str,
) -> String {
    let described = if description.trim().is_empty() {
        "_(nothing written - please say what happened)_"
    } else {
        description.trim()
    };
    let note = if truncated {
        "\n_Older lines were dropped to fit. The attached file has all of them._"
    } else {
        ""
    };
    format!(
        "{described}\n\n\
         ---\n\n\
         | | |\n|---|---|\n| Version | {version} |\n| OS | {os} |\n\n\
         **Please drag `{log_path}` onto this issue before submitting** - it is \
         the full log, and it has already had your organization, project and \
         work item names removed.\n\n\
         <details><summary>End of the log</summary>\n\n\
         ```\n{excerpt}\n```\n{note}\n</details>\n"
    )
}

/// The prefilled issue form. Query values are percent-encoded; GitHub reads
/// `title` and `body` from them.
pub fn issue_url(title: &str, body: &str) -> String {
    format!(
        "{ISSUES_URL}?title={}&body={}&labels=bug",
        urlencoding::encode(title),
        urlencoding::encode(body)
    )
}

/// A one-line title from the reporter's own words, so the issue list is
/// readable without opening anything.
pub fn title(description: &str) -> String {
    let first = description
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Bug report");
    let trimmed: String = first.chars().take(80).collect();
    if trimmed.is_empty() {
        "Bug report".to_string()
    } else {
        trimmed
    }
}
