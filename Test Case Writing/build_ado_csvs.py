"""
Build Azure DevOps bulk-import CSVs for Test Case work items.

Target: Test Plans -> select suite -> Import test cases from CSV/XLSX.

Format (per https://learn.microsoft.com/en-us/azure/devops/test/bulk-import-export-test-cases):
- One ROW per test step. Title/ID/metadata repeat on every row for a case.
- Required columns: ID, Work Item Type, Title, Test Step, Step Action,
  Step Expected, Area Path, Assigned To, State.
- ID is empty for new test cases.
- Work Item Type = "Test Case" (exact spelling/casing).
- State = "Design" (required).
- Test Step is sequential: 1, 2, 3, ...
- UTF-8 encoding; double quotes around cells with commas or line breaks.

Option A mapping (user-confirmed): all markdown "Expected Results" bullets are
concatenated into the LAST step's Step Expected cell; earlier steps have an
empty Step Expected.

Extra non-required columns (the wizard's auto-mapping will offer them; user
verifies in the mapping review screen): Description (preconditions, HTML),
Priority (1/2/3), Module (custom field).
"""

import csv
import html
import os
import re

INPUT_DIR = r"D:\Test Case Writing"
OUTPUT_DIR = r"D:\Test Case Writing\ADO Import"

PRIORITY_MAP = {"High": 1, "Medium": 2, "Low": 3}
AREA_PATH = r"PeoplesHR\HRM\Boards\Work items"
ASSIGNED_TO = "avin.a@peopleshr.com"
MODULE = "Performance"
STATE = "Design"
WORK_ITEM_TYPE = "Test Case"

SOURCES = [
    ("Proficiency Profile UI - Manual Test Cases.md", "Proficiency Profile - ADO Import.csv"),
    ("Goal Groups UI - Manual Test Cases.md",         "Goal Groups - ADO Import.csv"),
    ("Preview UI - Manual Test Cases.md",             "Preview - ADO Import.csv"),
]

CASE_HEADER_RE = re.compile(r'^###\s+([A-Z]{2}-\d+)\s+—\s+(.+?)\s*$')
PRIORITY_RE    = re.compile(r'^-\s+Priority:\s+(High|Medium|Low)\s*$')
PRECOND_RE     = re.compile(r'^-\s+Preconditions:\s*(.+?)\s*$')
STEP_LINE_RE   = re.compile(r'^(\s*)(\d+)\.\s+(.+?)\s*$')
BULLET_LINE_RE = re.compile(r'^(\s*)-\s+(.+?)\s*$')


def parse_test_cases(md_path):
    with open(md_path, encoding="utf-8") as f:
        lines = f.read().splitlines()

    cases = []
    i = 0
    while i < len(lines):
        header_m = CASE_HEADER_RE.match(lines[i])
        if not header_m:
            i += 1
            continue

        case_id = header_m.group(1)
        title = header_m.group(2).strip()
        preconditions = ""
        steps = []
        expected = []  # list of (indent_level, text)
        priority = None

        section = None
        j = i + 1
        while j < len(lines):
            line = lines[j]
            if CASE_HEADER_RE.match(line):
                break

            pri_m = PRIORITY_RE.match(line)
            if pri_m:
                priority = pri_m.group(1)
                j += 1
                break

            pc_m = PRECOND_RE.match(line)
            if pc_m:
                preconditions = pc_m.group(1).strip()
                section = None
                j += 1
                continue

            if re.match(r'^-\s+Steps:\s*$', line):
                section = 'steps'
                j += 1
                continue
            if re.match(r'^-\s+Expected Results:\s*$', line):
                section = 'expected'
                j += 1
                continue

            if section == 'steps':
                sm = STEP_LINE_RE.match(line)
                if sm:
                    steps.append(sm.group(3).strip())
                    j += 1
                    continue
                if line.strip() == "":
                    j += 1
                    continue
                section = None
                continue

            if section == 'expected':
                bm = BULLET_LINE_RE.match(line)
                if bm:
                    indent_spaces = len(bm.group(1))
                    level = 0 if indent_spaces <= 4 else 1
                    expected.append((level, bm.group(2).strip()))
                    j += 1
                    continue
                if line.strip() == "":
                    j += 1
                    continue
                section = None
                continue

            j += 1

        if priority is None:
            raise ValueError(f"Case {case_id} missing Priority")

        cases.append({
            "id": case_id,
            "title": title,
            "preconditions": preconditions,
            "steps": steps,
            "expected": expected,
            "priority": priority,
        })
        i = j

    return cases


def format_expected_cell(expected_bullets):
    """
    Render the Expected Results list as a single plain-text cell.
    Top-level bullets get '• '; nested sub-bullets get '    ◦ '.
    Separator is a real newline — the csv module will quote and preserve it.
    """
    if not expected_bullets:
        return ""
    lines = []
    for level, text in expected_bullets:
        prefix = "• " if level == 0 else "    ◦ "
        lines.append(f"{prefix}{text}")
    return "\n".join(lines)


def description_html(preconditions):
    if not preconditions:
        return ""
    return f"<p><strong>Preconditions:</strong> {html.escape(preconditions)}</p>"


def write_csv(cases, csv_path):
    headers = [
        "ID",
        "Work Item Type",
        "Title",
        "Test Step",
        "Step Action",
        "Step Expected",
        "Area Path",
        "Assigned To",
        "State",
        "Priority",
        "Description",
        "Module",
    ]

    total_rows = 0
    with open(csv_path, "w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, quoting=csv.QUOTE_MINIMAL)
        writer.writerow(headers)
        for case in cases:
            title = f"{case['id']} — {case['title']}"
            priority_num = PRIORITY_MAP[case["priority"]]
            description = description_html(case["preconditions"])
            n = len(case["steps"])
            expected_last = format_expected_cell(case["expected"])
            for step_idx, step_text in enumerate(case["steps"], start=1):
                step_expected = expected_last if step_idx == n else ""
                writer.writerow([
                    "",                       # ID — empty for new
                    WORK_ITEM_TYPE,
                    title,
                    step_idx,
                    step_text,
                    step_expected,
                    AREA_PATH,
                    ASSIGNED_TO,
                    STATE,
                    priority_num,
                    description,
                    MODULE,
                ])
                total_rows += 1
    return total_rows


def main():
    os.makedirs(OUTPUT_DIR, exist_ok=True)
    grand_cases = 0
    grand_rows = 0
    for src, dst in SOURCES:
        cases = parse_test_cases(os.path.join(INPUT_DIR, src))
        rows = write_csv(cases, os.path.join(OUTPUT_DIR, dst))
        print(f"{src}: {len(cases)} cases -> {rows} step rows -> {dst}")
        grand_cases += len(cases)
        grand_rows += rows
    print(f"\nTotal: {grand_cases} cases, {grand_rows} step rows written to {OUTPUT_DIR}")


if __name__ == "__main__":
    main()
