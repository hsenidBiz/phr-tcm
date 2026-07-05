"""Parse pasted clipboard text into test-case steps. Kept Qt-free (no PyQt5
import) so it can be unit-tested under CI, which installs only the pure-logic
dependencies."""


def parse_step_rows(text: str) -> list:
    """Parse clipboard text into ``[(action, expected), …]`` when it looks like
    multiple steps — multi-line and/or tab-separated (e.g. two columns pasted
    from Excel). Returns ``[]`` for a plain single value so an ordinary
    single-cell paste is left to the cell editor untouched."""
    if not text:
        return []
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    if "\n" not in text.strip() and "\t" not in text:
        return []
    rows = []
    for line in text.split("\n"):
        if not line.strip():
            continue
        parts = line.split("\t")
        action = parts[0].strip()
        expected = parts[1].strip() if len(parts) > 1 else ""
        if action or expected:
            rows.append((action, expected))
    return rows
