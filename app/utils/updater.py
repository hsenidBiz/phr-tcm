"""Auto-update via Velopack + a public GitHub Releases repo.

Only the installed (packaged) build auto-updates — running from source is a
no-op. Velopack downloads full or delta updates from the public releases repo
and applies them with a restart. There is no git and no embedded token: the
releases repo is public, so unauthenticated reads are enough.

The startup lifecycle hook (``velopack.App().run()``) lives in ``main.py`` and
must run before the GUI — this module only handles the in-app "is there a newer
version?" check and the download/apply.
"""

import sys

# Public, releases-only repo holding the Velopack installer + update packages.
# No source code lives here. Created separately from the private source repo.
# (Renamed from ...-creator-releases at v2.0.1; GitHub redirects the old URL so
# pre-2.0.1 installs still find this for their one update hop.)
RELEASES_REPO = "https://github.com/AvinAlwis/azure-devops-test-case-manager-releases"


def update_supported() -> bool:
    """True only for the installed Velopack build (a frozen PyInstaller exe).
    Source runs (``python main.py``) never auto-update."""
    return getattr(sys, "frozen", False)


def _update_manager():
    import velopack
    # access_token=None → unauthenticated (public repo); prerelease=False → stable only.
    return velopack.UpdateManager(velopack.GithubSource(RELEASES_REPO, None, False))


def check_for_update() -> dict | None:
    """Return update details when a newer release exists, else None.

    The returned dict carries the live Velopack manager + info so that a later
    ``download_and_apply`` reuses them:
        {"version": str, "notes": str, "_manager": UpdateManager, "_info": UpdateInfo}

    Silent (returns None) on any failure — including the "not installed"
    RuntimeError when run from source — so the update check never disturbs use.
    """
    if not update_supported():
        return None
    try:
        manager = _update_manager()
        info = manager.check_for_updates()
        if info is None:
            return None
        target = info.TargetFullRelease
        return {
            "version": target.Version,
            "notes": target.NotesMarkdown or "",
            "_manager": manager,
            "_info": info,
        }
    except Exception:
        return None


def download_and_apply(update: dict):
    """Download the pending update and restart into the new version.

    Velopack replaces the app files and relaunches the app, terminating this
    process as part of the restart — so callers must persist any state first.
    Raises on a download/apply failure (the current version keeps running)."""
    manager = update["_manager"]
    info = update["_info"]
    manager.download_updates(info)
    manager.apply_updates_and_restart(info)
