import json
import threading
from datetime import datetime, timedelta
from pathlib import Path

from PyQt5.QtCore import QObject, pyqtSignal

_CACHE_TTL = timedelta(hours=24)

_CACHE_PATH = Path.home() / ".devops_tc_creator" / "team_members_cache.json"
_write_lock = threading.Lock()


def _cache_key(org_url: str, project: str) -> str:
    return f"{org_url.rstrip('/').lower()}::{project.strip().lower()}"


def load_cached(org_url: str, project: str) -> list | None:
    """Return the persisted member list for this project, or None if absent or stale (>24 h)."""
    try:
        data = json.loads(_CACHE_PATH.read_text(encoding="utf-8"))
        entry = data.get(_cache_key(org_url, project))
        if entry and isinstance(entry.get("members"), list):
            updated_at = entry.get("updated_at")
            if updated_at:
                age = datetime.now() - datetime.fromisoformat(updated_at)
                if age > _CACHE_TTL:
                    return None
            return entry["members"]
    except Exception:
        pass
    return None


def save_to_disk(org_url: str, project: str, members: list):
    """Persist the member list for this project, keyed by org+project."""
    with _write_lock:
        try:
            try:
                data = json.loads(_CACHE_PATH.read_text(encoding="utf-8"))
            except Exception:
                data = {}
            data[_cache_key(org_url, project)] = {
                "members": members,
                "updated_at": datetime.now().isoformat(timespec="seconds"),
            }
            _CACHE_PATH.parent.mkdir(parents=True, exist_ok=True)
            _CACHE_PATH.write_text(json.dumps(data, indent=2), encoding="utf-8")
        except Exception:
            pass


class TeamMemberFetcher(QObject):
    """
    Fetches team members in a daemon background thread.
    Emits done(members) on the Qt main thread when complete (queued connection).
    Shared via AppState._team_members_fetcher so multiple widgets attach to the
    same in-flight request rather than issuing duplicate API calls.
    """
    done = pyqtSignal(list)

    def __init__(self, client):
        super().__init__()
        self._client = client

    def start(self):
        threading.Thread(target=self._run, daemon=True).start()

    def _run(self):
        try:
            members = self._client.get_team_members()
        except Exception:
            members = []
        self.done.emit(members)
