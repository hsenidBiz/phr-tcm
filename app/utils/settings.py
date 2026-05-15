import json
from pathlib import Path

_SETTINGS_PATH = Path.home() / ".devops_tc_creator" / "settings.json"

# Keys that are safe to persist. The Bearer token is intentionally excluded.
_ALLOWED_KEYS = {"org_url", "project", "preconditions", "recent_pbis", "dark_mode", "mine_only_filter"}


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


def save_recent_pbi(pbi_id: int, title: str, max_items: int = 10):
    """Prepend a PBI entry to the recent list, deduplicating by ID."""
    existing = load_settings()
    recent = [r for r in existing.get("recent_pbis", []) if r.get("id") != pbi_id]
    recent.insert(0, {"id": pbi_id, "title": title})
    save_settings({"recent_pbis": recent[:max_items]})
