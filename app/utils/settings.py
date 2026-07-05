import json
import os
import time
from pathlib import Path

from app.utils.logger import get_logger

log = get_logger(__name__)


def _write_json_atomic(path: Path, obj) -> None:
    """Write JSON via a temp file + atomic os.replace so an interrupted or
    concurrent write can never leave a truncated/corrupt file (a corrupt file
    makes load_settings() fall back to {} and silently drop saved prefs).

    On Windows os.replace can fail with PermissionError/[WinError 5] when the
    target is briefly locked — antivirus scanning the freshly-written .tmp, the
    search indexer, or a second running instance. So: retry a few times, then
    fall back to an in-place write, and NEVER raise — a settings/state save must
    not crash the app."""
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        data = json.dumps(obj, indent=2)
        tmp = path.with_name(path.name + ".tmp")
        tmp.write_text(data, encoding="utf-8")
        for delay in (0, 0.05, 0.1, 0.2):
            if delay:
                time.sleep(delay)
            try:
                os.replace(tmp, path)
                return
            except PermissionError:
                continue  # target momentarily locked — retry
        # Replace kept failing: write in place so the value still persists,
        # then drop the temp file.
        path.write_text(data, encoding="utf-8")
        try:
            tmp.unlink(missing_ok=True)
        except OSError:
            pass
    except Exception:
        # best-effort: a failed settings write must never crash the app
        log.warning("Failed to write %s", path.name, exc_info=True)

_SETTINGS_PATH = Path.home() / ".devops_tc_creator" / "settings.json"
_DRAFT_PATH = Path.home() / ".devops_tc_creator" / "draft_queue.json"
_RUN_SESSION_PATH = Path.home() / ".devops_tc_creator" / "run_session.json"
_RUN_SHOTS_DIR = Path.home() / ".devops_tc_creator" / "run_session_shots"

# Keys that are safe to persist. The Bearer token is intentionally excluded.
_ALLOWED_KEYS = {
    "org_url", "project", "preconditions", "recent_pbis", "dark_mode",
    "mine_only_filter", "status_filter", "module_filter", "templates",
    "execution_notes", "test_plan_cache", "always_on_top",
    "window_geometry", "edit_splitter_sizes", "suite_splitter_sizes",
    "visible_tabs", "demo_mode", "hidden_work_items",
}


def load_settings() -> dict:
    try:
        data = json.loads(_SETTINGS_PATH.read_text(encoding="utf-8"))
        return {k: v for k, v in data.items() if k in _ALLOWED_KEYS}
    except FileNotFoundError:
        return {}  # first run — nothing saved yet
    except Exception:
        log.warning("Could not read settings.json — using defaults", exc_info=True)
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
#  Work Manager — locally hidden work items (never sent to ADO)       #
#  Scoped per org so hiding an item in one org can't affect another.  #
# ------------------------------------------------------------------ #

def load_hidden_work_items(org_url: str) -> set:
    """The set of work-item ids the user has hidden on the Work Manager board
    for the given org."""
    data = load_settings().get("hidden_work_items", {})
    ids = data.get(org_url or "", []) if isinstance(data, dict) else []
    if not isinstance(ids, list):
        return set()
    out = set()
    for i in ids:
        try:
            out.add(int(i))
        except (TypeError, ValueError):
            continue
    return out


def save_hidden_work_items(org_url: str, ids) -> None:
    """Persist the hidden work-item ids for an org (replacing that org's list)."""
    data = load_settings().get("hidden_work_items", {})
    if not isinstance(data, dict):
        data = {}
    cleaned = sorted({int(i) for i in ids})
    if cleaned:
        data[org_url or ""] = cleaned
    else:
        data.pop(org_url or "", None)   # nothing hidden — drop the empty entry
    save_settings({"hidden_work_items": data})


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
    except FileNotFoundError:
        return []  # no draft saved
    except Exception:
        log.warning("Could not load the draft queue — starting empty", exc_info=True)
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
    except FileNotFoundError:
        return None  # no interrupted run
    except Exception:
        log.warning("Could not load the saved run session", exc_info=True)
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


# ------------------------------------------------------------------ #
#  My Work focus timer (resumable across restarts)                    #
# ------------------------------------------------------------------ #

_FOCUS_TIMER_PATH = Path.home() / ".devops_tc_creator" / "focus_timer.json"


def save_focus_timer(data: dict):
    """Persist the My Work focus timer ({id, title, accum seconds, …}) so a
    running timer survives an app restart (restored paused — offline time is
    never counted)."""
    _write_json_atomic(_FOCUS_TIMER_PATH, data)


def load_focus_timer() -> dict | None:
    """The saved focus timer, or None if absent/invalid."""
    try:
        return json.loads(_FOCUS_TIMER_PATH.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return None
    except Exception:
        log.warning("Could not load the saved focus timer", exc_info=True)
        return None


def clear_focus_timer():
    """Delete the saved focus timer (logged or discarded)."""
    try:
        _FOCUS_TIMER_PATH.unlink(missing_ok=True)
    except Exception:
        pass
