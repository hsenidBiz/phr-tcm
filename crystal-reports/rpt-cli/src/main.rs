//! `rpt-tools` — the rpt-core tooling CLI.
//!
//! Subcommands:
//!
//! - `ir <in.rpt> [out.json]` — project the report to the `crpt-ir/1` JSON IR.
//! - `edit <in.rpt> <ops.json> <out.rpt>` — apply a JSON array of edit ops, write the result.
//! - `apply <in.rpt> <plan.json> <out.rpt>` — apply a VibeyReports LayoutPlan JSON.
//! - `diff <a.rpt> <b.rpt>` — semantic model diff (which JSON paths changed).
//! - `roundtrip <in.rpt>` — decode → re-encode → re-open → compare record bytes; exit 1 on drift.
//! - `sweep <dir>` — run ir + roundtrip over every .rpt under a directory; summary report.
//! - `check <in.rpt>` — offline load-path validation against the engine's open-file invariants.
//! - `new <out.rpt>` — create a blank .rpt from scratch.
//! - `render <in.rpt> <out.pdf>` — render a PDF preview (saved data when the report has it).

use std::io::Write as _;
use std::path::Path;

use rpt_core::edit::EditOp;
use rpt_reader::StreamId;

fn main() {
    let code = run();
    std::process::exit(code);
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("ir") => cmd_ir(&args[1..]),
        Some("check") => cmd_check(&args[1..]),
        Some("new") => cmd_new(&args[1..]),
        Some("apply") => cmd_apply(&args[1..]),
        Some("apply-all") => cmd_apply_all(&args[1..]),
        Some("edit") => cmd_edit(&args[1..]),
        Some("diff") => cmd_diff(&args[1..]),
        Some("roundtrip") => cmd_roundtrip(&args[1..]),
        Some("sweep") => cmd_sweep(&args[1..]),
        Some("render") => cmd_render(&args[1..]),
        _ => {
            eprintln!(
                "rpt-tools — Crystal Reports .rpt tooling (rpt-core)\n\n\
                 USAGE:\n\
                 \x20 rpt-tools ir <in.rpt> [out.json]\n\
                 \x20 rpt-tools edit <in.rpt> <ops.json> <out.rpt>\n\
                 \x20 rpt-tools apply-all <dir> <plan.json>\n\
                 \x20 rpt-tools apply <in.rpt> <plan.json> <out.rpt>\n\
                 \x20 rpt-tools diff <a.rpt> <b.rpt>\n\
                 \x20 rpt-tools roundtrip <in.rpt>\n\
                 \x20 rpt-tools sweep <dir>\n\
                 \x20 rpt-tools check <in.rpt>\n\
                 \x20 rpt-tools new <out.rpt> [--title T] [--author A] [--orientation o] [--paper N]\n\
                 \x20 rpt-tools render <in.rpt> <out.pdf> [--db] [-p Name=Value]\n\n\
                 ops.json: a JSON array of {{\"op\": ...}} objects — see docs/ir-schema.md"
            );
            2
        }
    }
}

/// Render a report to a PDF preview. Saved data drives the detail rows when the report carries
/// it; a report saved without data renders its static bands (that is the preview, not an error).
///
/// `--db` switches to live SQL Server data: the connection string is read from the
/// `RPT_TOOLS_DB` environment variable (ADO/OLE DB form — the application's own shape), never
/// from the command line, so the password cannot leak into process listings or shell history.
/// `-p Name=Value` (repeatable) supplies report parameters, coerced against the report's
/// declared kinds.
fn cmd_render(args: &[String]) -> i32 {
    let mut positional: Vec<&str> = Vec::new();
    let mut live = false;
    let mut params: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => live = true,
            "-p" => {
                let Some(kv) = args.get(i + 1) else {
                    eprintln!("error: -p expects Name=Value");
                    return 2;
                };
                match kv.split_once('=') {
                    Some((k, v)) => params.push((k.to_string(), v.to_string())),
                    None => {
                        eprintln!("error: -p expects Name=Value, got `{kv}`");
                        return 2;
                    }
                }
                i += 1;
            }
            other => positional.push(other),
        }
        i += 1;
    }
    if positional.len() != 2 {
        eprintln!("usage: rpt-tools render <in.rpt> <out.pdf> [--db] [-p Name=Value …]");
        return 2;
    }
    let (input, output) = (positional[0], positional[1]);
    let rpt = match rpt_core::open(input) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let result = if live {
        let conn = match std::env::var("RPT_TOOLS_DB") {
            Ok(c) => c,
            Err(_) => {
                eprintln!("error: --db needs the connection string in the RPT_TOOLS_DB environment variable (never the command line)");
                return 2;
            }
        };
        let db = rpt_core::preview::LiveDb { connection_string: conn, params };
        rpt_core::preview::preview_live_report(&rpt, &db)
    } else {
        rpt_core::preview::preview_report(&rpt)
    };
    match result {
        Ok(out) => match std::fs::write(output, &out.pdf) {
            Ok(()) => {
                eprintln!(
                    "render: {input} -> {output} ({} pages, {} bytes{})",
                    out.page_count,
                    out.pdf.len(),
                    if live { ", live data" } else { "" }
                );
                for d in &out.diagnostics {
                    eprintln!("  note: {d}");
                }
                0
            }
            Err(e) => {
                eprintln!("error: cannot write `{output}`: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn cmd_ir(args: &[String]) -> i32 {
    let Some(input) = args.first() else {
        eprintln!("usage: rpt-tools ir <in.rpt> [out.json]");
        return 2;
    };
    let rpt = match rpt_core::open(input) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let located = rpt_core::locate::locate(&rpt);
    let ir = rpt_core::ir::build_ir(&rpt, &located, input);
    let json = serde_json::to_string_pretty(&ir).unwrap_or_default();
    match args.get(1) {
        Some(out) => {
            let written = match std::fs::write(out, json.clone() + "\n") {
                Ok(()) => json.len(),
                Err(e) => {
                    eprintln!("error: cannot write `{out}`: {e}");
                    return 1;
                }
            };
            eprintln!(
                "ir: {input} -> {out} ({written} bytes, {} objects, {} warnings)",
                ir.objects.len(),
                ir.warnings.len()
            );
            for w in &ir.warnings {
                eprintln!("  warning: {w}");
            }
            0
        }
        None => {
            println!("{json}");
            0
        }
    }
}

/// Create a blank .rpt from scratch (no donor file) with optional title/author/margins.
fn cmd_new(args: &[String]) -> i32 {
    let mut opts = rpt_core::blank::NewReportOptions::default();
    let mut out = String::new();
    let mut i = 0;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    while i < args.len() {
        match args[i] {
            "--title" if i + 1 < args.len() => { opts.title = Some(args[i + 1].to_string()); i += 2; }
            "--author" if i + 1 < args.len() => { opts.author = Some(args[i + 1].to_string()); i += 2; }
            "--orientation" if i + 1 < args.len() => { opts.orientation = Some(args[i + 1].to_string()); i += 2; }
            "--paper" if i + 1 < args.len() => {
                opts.paper_size = args[i + 1].parse().ok(); i += 2;
            }
            other => { out = other.to_string(); i += 1; }
        }
    }
    if out.is_empty() {
        eprintln!("usage: rpt-tools new <out.rpt> [--title T] [--author A] [--orientation o] [--paper N]");
        return 2;
    }
    match rpt_core::blank::create_report(&opts) {
        Ok(bytes) => match std::fs::write(&out, bytes) {
            Ok(()) => {
                eprintln!("new: created {out} ({} bytes)", std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0));
                0
            }
            Err(e) => { eprintln!("error: cannot write `{out}`: {e}"); 1 }
        },
        Err(e) => { eprintln!("error: {e}"); 1 }
    }
}

/// Offline load-path validation: run the engine's open-file invariants against the report.
/// Exit 0 = no Error findings; exit 1 = findings (printed); exit 2 = usage/open failure.
fn cmd_check(args: &[String]) -> i32 {
    let Some(input) = args.first() else {
        eprintln!("usage: rpt-tools check <in.rpt>");
        return 2;
    };
    let rpt = match rpt_core::open(input) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    let report = rpt_core::validate::validate(&rpt);
    let errors = report.errors().count();
    let warnings = report.warnings().count();
    for f in &report.findings {
        let tag = match f.severity {
            rpt_core::validate::Severity::Error => "[ERR]",
            rpt_core::validate::Severity::Warn => "[WARN]",
        };
        println!("{tag} {}: {} (engine: {})", f.check, f.message, f.engine);
    }
    println!(
        "{}: {errors} error(s), {warnings} warning(s)",
        if errors == 0 { "PASS" } else { "FAIL" }
    );
    if errors == 0 { 0 } else { 1 }
}

/// Apply one plan to every .rpt under a directory (recursively), backing each modified
/// file up to <file>.bak beside it. Per-file failures do not stop the run.
fn cmd_apply_all(args: &[String]) -> i32 {
    if args.len() != 2 {
        eprintln!("usage: rpt-tools apply-all <dir> <plan.json>");
        return 2;
    }
    let (dir, plan_path) = (&args[0], &args[1]);
    let plan = match std::fs::read_to_string(plan_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read `{plan_path}`: {e}");
            return 1;
        }
    };
    let summary = match rpt_core::batch::apply_plan_all(std::path::Path::new(dir), &plan) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    for f in &summary.files {
        let label = f.path.to_string_lossy();
        match &f.outcome {
            rpt_core::batch::BatchOutcome::Applied(n) => eprintln!("  applied   {label} ({n} ops; .bak saved)"),
            rpt_core::batch::BatchOutcome::Unchanged => eprintln!("  unchanged {label}"),
            rpt_core::batch::BatchOutcome::Skipped => eprintln!("  skipped   {label}"),
            rpt_core::batch::BatchOutcome::Failed(e) => eprintln!("  FAILED    {label}: {e}"),
        }
    }
    let unchanged = summary
        .files
        .iter()
        .filter(|f| matches!(f.outcome, rpt_core::batch::BatchOutcome::Unchanged))
        .count();
    println!(
        "apply-all: {} applied, {unchanged} unchanged, {} failed, {} total",
        summary.applied(),
        summary.failed(),
        summary.files.len()
    );
    if summary.failed() > 0 { 1 } else { 0 }
}

/// Apply a VibeyReports LayoutPlan JSON (camelCase, action names as in VibeyReports.Contracts).
fn cmd_apply(args: &[String]) -> i32 {
    if args.len() != 3 {
        eprintln!("usage: rpt-tools apply <in.rpt> <plan.json> <out.rpt>");
        return 2;
    }
    let (input, plan_path, output) = (&args[0], &args[1], &args[2]);
    let plan = match std::fs::read_to_string(plan_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read `{plan_path}`: {e}");
            return 1;
        }
    };
    match rpt_core::plan::apply_plan_json(input, &plan) {
        Ok((bytes, outcome)) => match std::fs::write(output, bytes) {
            Ok(()) => {
                eprintln!(
                    "apply: {} + {} ops -> {} (removed objects: {:?}, removed tables: {:?})",
                    input, outcome.operations_applied, output, outcome.removed_objects, outcome.removed_tables
                );
                0
            }
            Err(e) => {
                eprintln!("error: cannot write `{output}`: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn cmd_edit(args: &[String]) -> i32 {
    if args.len() != 3 {
        eprintln!("usage: rpt-tools edit <in.rpt> <ops.json> <out.rpt>");
        return 2;
    }
    let (input, ops_path, output) = (&args[0], &args[1], &args[2]);
    let ops_raw = match std::fs::read_to_string(ops_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read `{ops_path}`: {e}");
            return 1;
        }
    };
    let ops: Vec<EditOp> = match serde_json::from_str(&ops_raw) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: bad ops file `{ops_path}`: {e}");
            return 1;
        }
    };
    match rpt_core::edit::apply_edits(input, &ops) {
        Ok(bytes) => match std::fs::write(output, bytes) {
            Ok(()) => {
                eprintln!("edit: {input} + {} ops -> {output}", ops.len());
                0
            }
            Err(e) => {
                eprintln!("error: cannot write `{output}`: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

/// The decoded model as a JSON value, for diffing.
fn model_json(path: &str) -> Result<serde_json::Value, String> {
    let rpt = rpt_core::open(path)?;
    serde_json::to_value(rpt.report()).map_err(|e| e.to_string())
}

fn cmd_diff(args: &[String]) -> i32 {
    if args.len() != 2 {
        eprintln!("usage: rpt-tools diff <a.rpt> <b.rpt>");
        return 2;
    }
    let a = match model_json(&args[0]) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {}: {e}", args[0]);
            return 1;
        }
    };
    let b = match model_json(&args[1]) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {}: {e}", args[1]);
            return 1;
        }
    };
    let mut diffs = Vec::new();
    diff_values(&a, &b, "$", &mut diffs);
    if diffs.is_empty() {
        println!("identical: decoded models match");
        return 0;
    }
    println!("{} difference(s):", diffs.len());
    for d in &diffs {
        println!("  {d}");
    }
    0
}

/// Recursive JSON diff producing `path: a -> b` lines.
fn diff_values(a: &serde_json::Value, b: &serde_json::Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (serde_json::Value::Object(ma), serde_json::Value::Object(mb)) => {
            for (k, va) in ma {
                let p = format!("{path}.{k}");
                match mb.get(k) {
                    Some(vb) => diff_values(va, vb, &p, out),
                    None => out.push(format!("{p}: removed")),
                }
            }
            for k in mb.keys() {
                if !ma.contains_key(k) {
                    out.push(format!("{path}.{k}: added"));
                }
            }
        }
        (serde_json::Value::Array(va), serde_json::Value::Array(vb)) => {
            if va.len() != vb.len() {
                out.push(format!("{path}: length {} -> {}", va.len(), vb.len()));
            }
            for (i, (xa, xb)) in va.iter().zip(vb.iter()).enumerate() {
                diff_values(xa, xb, &format!("{path}[{i}]"), out);
            }
        }
        _ => {
            if a != b {
                out.push(format!("{path}: {} -> {}", brief(a), brief(b)));
            }
        }
    }
}

fn brief(v: &serde_json::Value) -> String {
    let s = v.to_string();
    if s.len() > 90 {
        format!("{}…", &s[..88])
    } else {
        s
    }
}

/// Round-trip one file: open → reencode → reopen → compare Contents record serialization.
fn roundtrip(path: &Path) -> Result<(), String> {
    let rpt = rpt_core::open(&path.to_string_lossy())?;
    let bytes = rpt.reencode().map_err(|e| format!("reencode: {e}"))?;
    let reopened = rpt_core::open_bytes(&bytes)?;
    let a = rpt
        .stream(&StreamId::Contents)
        .ok_or("no Contents stream")?
        .serialize_tree();
    let b = reopened
        .stream(&StreamId::Contents)
        .ok_or("no Contents stream in reopened file")?
        .serialize_tree();
    if a != b {
        return Err("record bytes drifted across the round-trip".to_string());
    }
    Ok(())
}

fn cmd_roundtrip(args: &[String]) -> i32 {
    let Some(input) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: rpt-tools roundtrip <in.rpt>");
        return 2;
    };
    match roundtrip(Path::new(input)) {
        Ok(()) => {
            println!("roundtrip ok: {input}");
            0
        }
        Err(e) => {
            eprintln!("roundtrip FAILED: {input}: {e}");
            1
        }
    }
}

fn cmd_sweep(args: &[String]) -> i32 {
    let Some(dir) = args.first() else {
        eprintln!("usage: rpt-tools sweep <dir>");
        return 2;
    };
    let mut files = Vec::new();
    collect_rpts(Path::new(dir), &mut files);
    files.sort();
    eprintln!("sweeping {} file(s) under {dir}", files.len());
    let (mut ok, mut ir_warn, mut fail) = (0usize, 0usize, 0usize);
    for f in &files {
        let p = f.to_string_lossy().to_string();
        let label = rpt_core::open(&p).and_then(|rpt| {
            let located = rpt_core::locate::locate(&rpt);
            let ir = rpt_core::ir::build_ir(&rpt, &located, &p);
            roundtrip(f).map(|()| (ir.objects.len(), ir.warnings))
        });
        match label {
            Ok((objects, warnings)) => {
                if warnings.is_empty() {
                    ok += 1;
                    eprintln!("  ok        {p} ({objects} objects)");
                } else {
                    ir_warn += 1;
                    eprintln!("  warn      {p} ({objects} objects, {} warnings)", warnings.len());
                    for w in warnings.iter().take(3) {
                        eprintln!("            - {w}");
                    }
                }
            }
            Err(e) => {
                fail += 1;
                eprintln!("  FAIL      {p}: {e}");
            }
        }
        let _ = std::io::stderr().flush();
    }
    println!("\nsweep: {ok} clean, {ir_warn} with warnings, {fail} failed, {} total", files.len());
    if fail > 0 {
        1
    } else {
        0
    }
}

fn collect_rpts(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rpts(&p, out);
        } else if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("rpt")).unwrap_or(false) {
            out.push(p);
        }
    }
}
