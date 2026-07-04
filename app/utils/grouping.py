"""Smart grouping of test cases by title patterns (Qt-free, unit-tested).

Given a list of test-case titles, :func:`group_indices` clusters them into named
"folders" the way a Test Suite tree would, so the Edit and Run Tests screens can
show an expandable folder view over the flat list.

Two passes, in order (the "smart" strategy):

1. **Delimiter prefix.** If a title has a category separator (`" - "`, `":"`,
   `"|"`, `"/"`, `">>"`, …), the text before the first one is its group key.
   e.g. ``"Login - valid creds"`` and ``"Login - locked out"`` → folder ``Login``.
2. **Common word prefix.** Titles with no delimiter are bucketed by their first
   word, and any bucket of two or more becomes a folder named after the longest
   run of shared leading words. e.g. ``"User can login"`` / ``"User can logout"``
   → folder ``User can``.

Anything left alone (a unique prefix, a lone first word) lands in a final
``""`` (Ungrouped) bucket. A folder always has at least two members, so grouping
never manufactures single-item folders.
"""
from __future__ import annotations

# Category separators, matched at their earliest occurrence in a title. Padded
# forms come first but selection is purely by earliest index, so the
# structurally-first split always wins. A bare "-" is deliberately excluded — it
# is too common inside ordinary words ("sign-in", "e-mail") to be a safe split.
_DELIMITERS = (" - ", " – ", " — ", ": ", " : ", " | ", " > ", " >> ", " / ",
               "/", ":", "|")


def _delimiter_prefix(title: str) -> str | None:
    """The category text before the first delimiter, or None if there isn't one
    (or the prefix/remainder would be empty)."""
    best_i: int | None = None
    best_prefix: str | None = None
    for d in _DELIMITERS:
        i = title.find(d)
        if i > 0 and title[i + len(d):].strip():
            if best_i is None or i < best_i:
                best_i = i
                best_prefix = title[:i].strip()
    return best_prefix or None


def _common_word_prefix(titles: list[str]) -> str:
    """Longest run of shared leading words across `titles` (case-insensitive),
    displayed in the first title's casing. Always returns at least one word."""
    split = [t.split() for t in titles]
    first = split[0]
    common: list[str] = []
    for pos in range(len(first)):
        w = first[pos]
        if all(pos < len(ws) and ws[pos].lower() == w.lower() for ws in split):
            common.append(w)
        else:
            break
    return " ".join(common) if common else first[0]


def group_indices(titles: list[str]) -> list[tuple[str, list[int]]]:
    """Cluster `titles` into ``(group_name, [indices])`` folders.

    Every index appears in exactly one group. Named folders (each with ≥2
    members) come first, sorted case-insensitively by name; a final group named
    ``""`` holds everything ungrouped (present only when non-empty). Member index
    lists are sorted ascending, so callers get a stable order.
    """
    delim: dict[int, tuple[str, str]] = {}   # index -> (norm_key, display)
    no_delim: list[int] = []
    for i, raw in enumerate(titles):
        t = (raw or "").strip()
        prefix = _delimiter_prefix(t) if t else None
        if prefix:
            delim[i] = (prefix.lower(), prefix)
        else:
            no_delim.append(i)

    groups: list[tuple[str, list[int]]] = []
    ungrouped: list[int] = []

    # Pass 1: delimiter-prefix buckets.
    buckets: dict[str, dict] = {}
    for i, (key, disp) in delim.items():
        b = buckets.setdefault(key, {"name": disp, "idx": []})
        b["idx"].append(i)
    for b in buckets.values():
        if len(b["idx"]) >= 2:
            groups.append((b["name"], b["idx"]))
        else:
            ungrouped.extend(b["idx"])

    # Pass 2: common-word-prefix clustering for the delimiter-free titles.
    word_buckets: dict[str, list[int]] = {}
    for i in no_delim:
        words = titles[i].strip().split()
        first_word = words[0].lower() if words else ""
        word_buckets.setdefault(first_word, []).append(i)
    for first_word, idxs in word_buckets.items():
        if first_word and len(idxs) >= 2:
            name = _common_word_prefix([titles[i].strip() for i in idxs])
            groups.append((name, idxs))
        else:
            ungrouped.extend(idxs)

    groups.sort(key=lambda g: g[0].lower())
    result: list[tuple[str, list[int]]] = [(name, sorted(idxs))
                                           for name, idxs in groups]
    if ungrouped:
        result.append(("", sorted(ungrouped)))
    return result
