#!/usr/bin/env python3
"""Shape-sync check: the TS types must mirror the Rust serde shapes exactly.

(a) Ir fields: every field of `pub struct Ir` in ../rpt-core/src/ir.rs must appear (camelCased)
    on the TS `Ir` interface in src/types.ts, and vice versa.
(b) EditOp variants: every variant of `pub enum EditOp` in ../rpt-core/src/edit.rs must have a
    `| { op: "<snake_case>"` member in the TS `EditOp` union, and vice versa.

Exit code 1 with a mismatch listing on any difference. `--ir-only` checks (a) only.
"""
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CORE = ROOT.parent / "rpt-core" / "src"


def snake_to_camel(name: str) -> str:
    parts = name.split("_")
    return parts[0] + "".join(p.title() for p in parts[1:])


def rust_struct_fields(path: Path, struct: str) -> list[str]:
    src = path.read_text(encoding="utf-8")
    m = re.search(rf"pub struct {struct}\s*\{{(.*?)\n\}}", src, re.S)
    if not m:
        raise SystemExit(f"cannot find `pub struct {struct}` in {path}")
    return re.findall(r"^\s*pub (\w+):", m.group(1), re.M)


def enum_variants(path: Path, enum: str) -> list[str]:
    src = path.read_text(encoding="utf-8")
    m = re.search(rf"pub enum {enum}\s*\{{(.*?)\n\}}", src, re.S)
    if not m:
        raise SystemExit(f"cannot find `pub enum {enum}` in {path}")
    return re.findall(r"^\s{4}([A-Z][A-Za-z0-9]*)\s*\{", m.group(1), re.M)


def ts_interface_keys(path: Path, iface: str) -> set[str]:
    src = path.read_text(encoding="utf-8")
    m = re.search(rf"export interface {iface}\s*\{{(.*?)\n\}}", src, re.S)
    if not m:
        raise SystemExit(f"cannot find `export interface {iface}` in {path}")
    return set(re.findall(r"^\s{2}(\w+)[?]?:", m.group(1), re.M))


def ts_op_tags(path: Path) -> set[str]:
    src = path.read_text(encoding="utf-8")
    return set(re.findall(r'\|\s*\{\s*op:\s*"([a-z0-9_]+)"', src))


def main() -> int:
    ir_only = "--ir-only" in sys.argv
    failures: list[str] = []

    # (a) Ir fields
    rust_ir = set(rust_struct_fields(CORE / "ir.rs", "Ir"))
    ts_ir = ts_interface_keys(ROOT / "src" / "types.ts", "Ir")
    for missing in sorted(rust_ir - ts_ir):
        failures.append(f"Ir: Rust field `{missing}` missing from TS interface")
    for extra in sorted(ts_ir - rust_ir):
        failures.append(f"Ir: TS field `{extra}` has no Rust counterpart")

    if not ir_only:
        # (b) EditOp variants
        variants = enum_variants(CORE / "edit.rs", "EditOp")
        rust_tags = {re.sub(r"(?<!^)(?=[A-Z])", "_", v).lower() for v in variants}
        ts_tags = ts_op_tags(ROOT / "src" / "types.ts")
        for missing in sorted(rust_tags - ts_tags):
            failures.append(f"EditOp: variant `{missing}` missing from TS union")
        for extra in sorted(ts_tags - rust_tags):
            failures.append(f"EditOp: TS op `{extra}` has no Rust variant")

    if failures:
        print(f"SHAPE MISMATCH ({len(failures)}):")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("shapes in sync")
    return 0


if __name__ == "__main__":
    sys.exit(main())
