"""Shared test-outcome row styling (Run Tests tab + Test Suites browser).

One source of truth for the outcome→colour mapping so every view that tints
rows by a test point's last outcome stays in sync. The colours are applied as
a translucent tint over the list background so they read as dim hints rather
than bold blocks; OUTCOME_ALPHA (0–255) is the one knob: lower = dimmer.
"""

from PyQt5.QtGui import QColor

OUTCOME_BG = {
    "passed": "#1E5E1E",          # dark green
    "failed": "#7A2222",          # dark red
    "blocked": "#6E5A12",         # dark amber / yellow
    "paused": "#4A2A6E",          # dark purple
    "notapplicable": "#4A4A4A",   # dark neutral grey
}
OUTCOME_ACTIVE_BG = "#1E3F6E"     # dark blue — has a point but no result yet
OUTCOME_ALPHA = 150               # darker, richer tint (slightly translucent)

# Legend entries matching the row tints ("_active" = the not-yet-run tint).
LEGEND_ITEMS = [
    ("Passed", "passed"), ("Failed", "failed"), ("Blocked", "blocked"),
    ("Paused", "paused"), ("N/A", "notapplicable"), ("Not run", "_active"),
]


def outcome_label(oc: str) -> str:
    return {"passed": "Passed", "failed": "Failed", "blocked": "Blocked",
            "paused": "Paused",
            "notapplicable": "Not Applicable"}.get(oc, "Active (not run)")


def outcome_tint(oc: str) -> QColor:
    """The translucent QBrush colour a row with this (lowercased) outcome gets."""
    col = QColor(OUTCOME_BG.get(oc, OUTCOME_ACTIVE_BG))
    col.setAlpha(OUTCOME_ALPHA)
    return col


def legend_swatch_rgb(oc: str, base: QColor) -> tuple:
    """The opaque (r, g, b) a legend swatch shows: the outcome tint composited
    over the view's base colour — the SAME shade a tinted row renders at."""
    hexcol = OUTCOME_ACTIVE_BG if oc == "_active" else OUTCOME_BG[oc]
    tint = QColor(hexcol)
    a = OUTCOME_ALPHA / 255.0
    return (round(base.red() * (1 - a) + tint.red() * a),
            round(base.green() * (1 - a) + tint.green() * a),
            round(base.blue() * (1 - a) + tint.blue() * a))
