import json
from pathlib import Path

_SETTINGS_PATH = Path.home() / ".devops_tc_creator" / "settings.json"
_DRAFT_PATH = Path.home() / ".devops_tc_creator" / "draft_queue.json"

# Keys that are safe to persist. The Bearer token is intentionally excluded.
_ALLOWED_KEYS = {
    "org_url", "project", "preconditions", "recent_pbis", "dark_mode",
    "mine_only_filter", "status_filter", "module_filter", "templates",
    "execution_notes",
}


def load_settings() -> dict:
    try:
        data = json.loads(_SETTINGS_PATH.read_text(encoding="utf-8"))
        return {k: v for k, v in data.items() if k in _ALLOWED_KEYS}
    except Exception:
        return {}


def save_settings(data: dict):
    existing = load_settings()
    existing.update({k: v for k, v in data.items() if k in _ALLOWED_KEYS})
    _SETTINGS_PATH.parent.mkdir(parents=True, exist_ok=True)
    _SETTINGS_PATH.write_text(json.dumps(existing, indent=2), encoding="utf-8")


# ------------------------------------------------------------------ #
#  Per-test-case local notes (kept on disk, never sent to ADO)        #
# ------------------------------------------------------------------ #

def load_execution_notes() -> dict:
    """Map of {str(test_case_id): notes_text} for the test runner."""
    notes = load_settings().get("execution_notes", {})
    return notes if isinstance(notes, dict) else {}


def get_execution_note(tc_id) -> str:
    return load_execution_notes().get(str(tc_id), "")


def save_execution_note(tc_id, text: str):
    """Store (or clear, when blank) the local note for a test case."""
    notes = load_execution_notes()
    key = str(tc_id)
    if text.strip():
        notes[key] = text
    else:
        notes.pop(key, None)
    save_settings({"execution_notes": notes})


def save_recent_pbi(pbi_id: int, title: str, project: str = "", max_items: int = 10):
    """Prepend a PBI entry to the recent list, deduplicating by ID.
    `project` scopes the entry so recents from other projects are not
    auto-selected after a project switch."""
    existing = load_settings()
    recent = [r for r in existing.get("recent_pbis", []) if r.get("id") != pbi_id]
    recent.insert(0, {"id": pbi_id, "title": title, "project": project})
    save_settings({"recent_pbis": recent[:max_items]})


def recent_pbis_for_project(project: str) -> list:
    """Recent PBI entries for the given project (legacy entries without a
    project key are included for backwards compatibility)."""
    recent = load_settings().get("recent_pbis", [])
    return [r for r in recent if r.get("project", project) == project]


def remove_recent_pbi(pbi_id: int):
    """Remove a PBI entry from the recent list by ID."""
    existing = load_settings()
    recent = [r for r in existing.get("recent_pbis", []) if r.get("id") != pbi_id]
    save_settings({"recent_pbis": recent})


# ------------------------------------------------------------------ #
#  Draft queue persistence                                            #
# ------------------------------------------------------------------ #

def save_draft_queue(queue: list):
    """Persist the queue to disk; each TestCase serialized via dataclasses.asdict."""
    import dataclasses
    data = [dataclasses.asdict(tc) for tc in queue]
    _DRAFT_PATH.parent.mkdir(parents=True, exist_ok=True)
    _DRAFT_PATH.write_text(json.dumps(data, indent=2), encoding="utf-8")


def load_draft_queue() -> list:
    """Load and return a list of TestCase from the draft file, or [] if absent/invalid."""
    from app.models.test_case import TestCase, Step
    try:
        data = json.loads(_DRAFT_PATH.read_text(encoding="utf-8"))
        result = []
        for d in data:
            d["steps"] = [Step(**s) for s in d.get("steps", [])]
            result.append(TestCase(**d))
        return result
    except Exception:
        return []


def clear_draft_queue():
    """Delete the draft file if it exists."""
    try:
        _DRAFT_PATH.unlink(missing_ok=True)
    except Exception:
        pass
