"""Git-based auto-update.

The app is distributed as a clone of the private GitHub repo on each
machine, so updates arrive via the machine's existing git credentials —
no tokens are stored or embedded. Everything here is read-only except
the fast-forward merge the user explicitly confirms.
"""

import os
import subprocess
import sys
from pathlib import Path

_REPO_ROOT = Path(__file__).resolve().parents[2]
_NO_WINDOW = 0x08000000 if os.name == "nt" else 0  # CREATE_NO_WINDOW


class UpdateError(Exception):
    pass


def _git(*args: str, timeout: int = 30) -> str:
    try:
        result = subprocess.run(
            ["git", *args],
            cwd=_REPO_ROOT, capture_output=True, text=True,
            timeout=timeout, creationflags=_NO_WINDOW,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise UpdateError(str(exc)) from exc
    if result.returncode != 0:
        raise UpdateError((result.stderr or result.stdout).strip())
    return result.stdout.strip()


def update_supported() -> bool:
    """True when running from source inside a git checkout with git available."""
    if getattr(sys, "frozen", False):  # PyInstaller build — no clone to pull
        return False
    if not (_REPO_ROOT / ".git").exists():
        return False
    try:
        _git("--version", timeout=10)
        return True
    except UpdateError:
        return False


def check_for_update() -> dict | None:
    """Fetch the remote and report how far behind this clone is.

    Returns {"commits": int, "latest": str, "branch": str} when an update
    is available, None when up to date. Raises UpdateError on git or
    network failure.
    """
    branch = _git("rev-parse", "--abbrev-ref", "HEAD")
    _git("fetch", "origin", branch, timeout=60)
    behind = int(_git("rev-list", "--count", f"HEAD..origin/{branch}"))
    if behind == 0:
        return None
    latest = _git("log", "-1", "--format=%s", f"origin/{branch}")
    return {"commits": behind, "latest": latest, "branch": branch}


def apply_update() -> str:
    """Fast-forward this clone to the already-fetched remote branch.

    Refuses to touch a clone with local modifications or diverged history
    (--ff-only) so an update can never destroy local work.
    """
    if _git("status", "--porcelain"):
        raise UpdateError(
            "This copy has local file changes, so the update was skipped to "
            "avoid disturbing them. Commit, stash, or discard the changes "
            "and try again."
        )
    branch = _git("rev-parse", "--abbrev-ref", "HEAD")
    return _git("merge", "--ff-only", f"origin/{branch}", timeout=60)


def start_new_instance():
    """Launch a fresh copy of the app; the caller then closes this one."""
    subprocess.Popen([sys.executable, *sys.argv], cwd=os.getcwd())
