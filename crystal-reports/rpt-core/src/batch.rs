//! Batch apply — one plan against every `.rpt` under a directory.
//!
//! For report families maintained in lockstep (the PMSV10 set), the same layout plan often
//! applies to several files. Each file is applied INDEPENDENTLY: a failure on one file
//! leaves that file untouched and the run continues, and every modified file is backed up
//! to `<file>.bak` beside it before it is rewritten.

use std::path::{Path, PathBuf};

/// The result of one file's application.
#[derive(Debug, Clone)]
pub struct BatchFileResult {
    /// The file's path.
    pub path: PathBuf,
    /// What happened.
    pub outcome: BatchOutcome,
}

/// One file's outcome.
#[derive(Debug, Clone)]
pub enum BatchOutcome {
    /// Applied and written; the ops that landed.
    Applied(usize),
    /// The plan had nothing to change for this file (identical output not rewritten).
    Unchanged,
    /// Not a `.rpt` (skipped by the walker).
    Skipped,
    /// Application failed; the file was NOT modified. Carries the message.
    Failed(String),
}

/// The run's summary.
#[derive(Debug, Clone, Default)]
pub struct BatchSummary {
    /// Per-file results, in the walk's (sorted) order.
    pub files: Vec<BatchFileResult>,
}

impl BatchSummary {
    /// How many files were modified.
    pub fn applied(&self) -> usize {
        self.files.iter().filter(|f| matches!(f.outcome, BatchOutcome::Applied(_))).count()
    }
    /// How many failed.
    pub fn failed(&self) -> usize {
        self.files.iter().filter(|f| matches!(f.outcome, BatchOutcome::Failed(_))).count()
    }
}

/// Apply `plan_json` to every `.rpt` under `dir` (recursively), writing each modified file
/// in place after saving a `<file>.bak` backup beside it. `plan_json` is the same Vibey
/// plan JSON `rpt-tools apply` takes (including `subreport`-scoped actions).
///
/// # Errors
///
/// Only `dir` not being a readable directory. Per-file failures are reported in the
/// summary, never aborting the run.
pub fn apply_plan_all(dir: &Path, plan_json: &str) -> Result<BatchSummary, String> {
    if !dir.is_dir() {
        return Err(format!("`{}` is not a directory", dir.display()));
    }
    let mut files = Vec::new();
    collect_rpts(dir, &mut files);
    files.sort();
    let mut summary = BatchSummary::default();
    for path in files {
        let label = path.to_string_lossy().to_string();
        let outcome = match crate::plan::apply_plan_json(&label, plan_json) {
            Ok((bytes, res)) => {
                let original = std::fs::read(&path).map_err(|e| format!("read `{label}`: {e}"))?;
                if bytes == original {
                    BatchOutcome::Unchanged
                } else {
                    let bak = path.with_extension("rpt.bak");
                    std::fs::write(&bak, &original)
                        .map_err(|e| format!("backup `{}`: {e}", bak.display()))?;
                    std::fs::write(&path, &bytes)
                        .map_err(|e| format!("write `{label}`: {e}"))?;
                    BatchOutcome::Applied(res.operations_applied)
                }
            }
            Err(e) => BatchOutcome::Failed(e),
        };
        summary.files.push(BatchFileResult { path, outcome });
    }
    Ok(summary)
}

fn collect_rpts(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rpts(&p, out);
        } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("rpt")) {
            out.push(p);
        }
    }
}
