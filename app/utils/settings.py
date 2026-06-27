import json
import os
from pathlib import Path


def _write_json_atomic(path: Path, obj) -> None:
    """Write JSON via a temp file + atomic os.replace so an interrupted or
    concurrent write can never leave a truncated/corrupt file — a corrupt file
    makes load_settings() fall back to {} and silently drop saved prefs (theme,
    recent PBIs, filters)."""
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(json.dumps(obj, indent=2), encoding="utf-8")
    os.replace(tmp, path)

_SETTINGS_PATH = Path.home() / ".devops_tc_creator" / "settings.json"
_DRAFT_PATH = Path.home() / ".devops_tc_creator" / "draft_queue.json"
_RUN_SESSION_PATH = Path.home() / ".devops_tc_creator" / "run_session.json"
_RUN_SHOTS_DIR = Path.home() / ".devops_tc_creator" / "run_session_shots"

# Keys that are safe to persist. The Bearer token is intentionally excluded.
_ALLOWED_KEYS = {
    "org_url", "project", "preconditions", "recent_pbis", "dark_mode",
    "mine_only_filter", "status_filter", "module_filter", "templates",
    "execution_notes", "test_plan_cache", "always_on_top",
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
    _write_json_atomic(_SETTINGS_PATH, existing)


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


# ------------------------------------------------------------------ #
#  Persisted PBI -> test plan/suite resolution                        #
#  Lets the app skip the slow plan/suite discovery on re-launch.      #
# ------------------------------------------------------------------ #

def get_cached_test_plan(pbi_id) -> dict | None:
    """Persisted {plan_id, plan_name, suite_id} resolved for a PBI, or None."""
    cache = load_settings().get("test_plan_cache", {})
    entry = cache.get(str(pbi_id)) if isinstance(cache, dict) else None
    if isinstance(entry, dict) and entry.get("suite_id"):
        return entry
    return None


def save_cached_test_plan(pbi_id, plan_id, plan_name, suite_id):
    """Persist a PBI's resolved test plan + requirement suite. Positive results
    only — 'no suite yet' is never cached, since it changes once cases exist."""
    if not suite_id:
        return
    cache = load_settings().get("test_plan_cache", {})
    if not isinstance(cache, dict):
        cache = {}
    cache[str(pbi_id)] = {
        "plan_id": plan_id, "plan_name": plan_name or "", "suite_id": suite_id,
    }
    save_settings({"test_plan_cache": cache})


def clear_cached_test_plan(pbi_id):
    """Drop a stale cache entry (e.g. the suite no longer exists). Self-heals on
    the next launch by re-discovering."""
    cache = load_settings().get("test_plan_cache", {})
    if isinstance(cache, dict) and cache.pop(str(pbi_id), None) is not None:
        save_settings({"test_plan_cache": cache})


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
    _write_json_atomic(_DRAFT_PATH, data)


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


# ------------------------------------------------------------------ #
#  Test-runner session persistence (resume an interrupted run)        #
# ------------------------------------------------------------------ #

def save_run_session(data: dict):
    """Persist the active test-runner session (cases + per-case outcomes/comments/
    step results + screenshot filenames + PBI context) so it can be resumed."""
    _write_json_atomic(_RUN_SESSION_PATH, data)


def load_run_session() -> dict | None:
    """The saved test-runner session, or None if absent/invalid."""
    try:
        return json.loads(_RUN_SESSION_PATH.read_text(encoding="utf-8"))
    except Exception:
        return None


def run_shots_dir() -> Path:
    """Folder holding the resumable session's screenshot PNGs (created on demand)."""
    _RUN_SHOTS_DIR.mkdir(parents=True, exist_ok=True)
    return _RUN_SHOTS_DIR


def clear_run_session():
    """Delete the saved run session and its screenshot folder."""
    try:
        _RUN_SESSION_PATH.unlink(missing_ok=True)
    except Exception:
        pass
    try:
        import shutil
        shutil.rmtree(_RUN_SHOTS_DIR, ignore_errors=True)
    except Exception:
        pass
