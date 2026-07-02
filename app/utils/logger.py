"""Rotating file log for field diagnostics.

The app deliberately never crashes on background failures (settings writes,
cache reads, update checks, broker fallbacks) — but until now those errors
were swallowed with nothing to inspect. This module gives every best-effort
``except`` a place to record what happened: ``~/.devops_tc_creator/logs/app.log``,
rotated at ~1 MB with 3 backups, so a user can attach the file to a bug report.

Never log token material: no Authorization headers, no JWT contents. URLs and
HTTP status codes are fine (Bearer tokens travel in headers, not URLs).

Usage::

    from app.utils.logger import get_logger
    log = get_logger(__name__)
    ...
    except Exception:
        log.warning("Could not refresh members cache", exc_info=True)
"""

import logging
import logging.handlers
from pathlib import Path

_LOG_DIR = Path.home() / ".devops_tc_creator" / "logs"
_LOG_FILE = _LOG_DIR / "app.log"

# Everything in this codebase lives under the "app" package, so configuring
# the "app" logger once covers every get_logger(__name__) child.
_ROOT_NAME = "app"

_configured = False


def setup() -> None:
    """Attach the rotating file handler to the app's logger hierarchy.

    Idempotent, and never raises — a logging failure (read-only home dir,
    locked file) must not stop the app; loggers then simply discard records.
    """
    global _configured
    if _configured:
        return
    try:
        _LOG_DIR.mkdir(parents=True, exist_ok=True)
        handler = logging.handlers.RotatingFileHandler(
            _LOG_FILE, maxBytes=1_000_000, backupCount=3, encoding="utf-8"
        )
        handler.setFormatter(
            logging.Formatter("%(asctime)s %(levelname)-7s %(name)s: %(message)s")
        )
        root = logging.getLogger(_ROOT_NAME)
        root.setLevel(logging.INFO)
        root.addHandler(handler)
        root.propagate = False  # keep app records out of any host root logger
        _configured = True
    except Exception:
        pass


def get_logger(name: str) -> logging.Logger:
    """A logger under the app hierarchy; sets up the file handler on first use."""
    setup()
    if name != _ROOT_NAME and not name.startswith(_ROOT_NAME + "."):
        name = f"{_ROOT_NAME}.{name}"
    return logging.getLogger(name)
