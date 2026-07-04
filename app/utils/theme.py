from PyQt5.QtWidgets import QApplication
from PyQt5.QtGui import QPalette, QColor

from app.utils.settings import load_settings, save_settings

_dark: bool = False

# ------------------------------------------------------------------ #
#  Token dictionaries                                                  #
# ------------------------------------------------------------------ #

_LIGHT: dict = {
    "surface":            "#f9f9f9",
    "surface2":           "#f0f0f0",
    "border":             "#dddddd",
    "text":               "#333333",
    "text_dim":           "#555555",
    "text_dim2":          "#888888",
    "accent":             "#0078d4",
    "accent_hover":       "#106ebe",
    "btn_bg":             "#f0f0f0",
    "btn_border":         "#cccccc",
    "btn_hover":          "#e0e0e0",
    "warn_bg":            "#fffbe6",
    "warn_border":        "#ffe58f",
    "warn_text":          "#555555",
    "review_warn_bg":     "#fff3cd",
    "review_warn_border": "#ffc107",
    "review_warn_text":   "#856404",
    "tmpl_bg":            "#e8f4fb",
    "tmpl_border":        "#b3d9f5",
    "green_btn_bg":       "#e8f4e8",
    "green_btn_border":   "#8bc48b",
    "green_btn_hover":    "#d0ebd0",
    "red_btn_bg":         "#fde8e8",
    "red_btn_border":     "#e88b8b",
    "red_btn_hover":      "#f8d0d0",
    "tag_inner_bg":       "#ffffff",
    "tag_scroll_border":  "#cccccc",
    "tag_unsel_bg":       "#f0f0f0",
    "tag_unsel_text":     "#333333",
    "tag_unsel_border":   "#cccccc",
    "tag_unsel_hover":    "#dddddd",
    "detail_row_bg":      "#eef2f7",
    "tree_title":         "#003366",
    "tree_meta":          "#666666",
    "header_bg":          "#f0f0f0",
    "footer_bg":          "#f9f9f9",
    "file_lbl_color":     "#666666",
    "count_lbl_color":    "#555555",
    "ok":                 "#008000",
    "error":              "#cc0000",
    "warn_fg":            "#e67e00",
    "btn_disabled_bg":    "#aaaaaa",
    "btn_disabled_fg":    "#eeeeee",
    "scroll_handle":      "#c4c4c4",
    "scroll_handle_hover":"#a6a6a6",
}

_DARK: dict = {
    "surface":            "#252526",
    "surface2":           "#2d2d30",
    "border":             "#3e3e42",
    "text":               "#cccccc",
    "text_dim":           "#999999",
    "text_dim2":          "#777777",
    "accent":             "#0078d4",
    "accent_hover":       "#106ebe",
    "btn_bg":             "#3a3a3a",
    "btn_border":         "#555555",
    "btn_hover":          "#4a4a4a",
    "warn_bg":            "#3a3200",
    "warn_border":        "#665200",
    "warn_text":          "#cccccc",
    "review_warn_bg":     "#332700",
    "review_warn_border": "#665200",
    "review_warn_text":   "#d4b800",
    "tmpl_bg":            "#1a2d3a",
    "tmpl_border":        "#2a4d6a",
    "green_btn_bg":       "#1a3a1a",
    "green_btn_border":   "#3a6a3a",
    "green_btn_hover":    "#1f4a1f",
    "red_btn_bg":         "#3a1a1a",
    "red_btn_border":     "#6a3a3a",
    "red_btn_hover":      "#4a2020",
    "tag_inner_bg":       "#3c3c3c",
    "tag_scroll_border":  "#555555",
    "tag_unsel_bg":       "#4a4a4a",
    "tag_unsel_text":     "#cccccc",
    "tag_unsel_border":   "#666666",
    "tag_unsel_hover":    "#5a5a5a",
    "detail_row_bg":      "#1a2535",
    "tree_title":         "#5ba3d4",
    "tree_meta":          "#888888",
    "header_bg":          "#2d2d30",
    "footer_bg":          "#252526",
    "file_lbl_color":     "#aaaaaa",
    "count_lbl_color":    "#aaaaaa",
    "ok":                 "#4ec94e",
    "error":              "#f14c4c",
    "warn_fg":            "#e5c07b",
    "btn_disabled_bg":    "#3a3a3a",
    "btn_disabled_fg":    "#777777",
    "scroll_handle":      "#4a4a4a",
    "scroll_handle_hover":"#5e5e5e",
}


# ------------------------------------------------------------------ #
#  Public API                                                          #
# ------------------------------------------------------------------ #

def is_dark() -> bool:
    return _dark


def tokens() -> dict:
    return _DARK if _dark else _LIGHT


def apply(dark: bool) -> None:
    global _dark
    _dark = dark
    save_settings({"dark_mode": dark})
    _set_palette(dark)
    try:
        from app.utils import icons
        icons.clear_cache()   # re-tint icons for the new theme
    except Exception:
        pass


def load_saved() -> None:
    global _dark
    _dark = bool(load_settings().get("dark_mode", False))
    _set_palette(_dark)


# ------------------------------------------------------------------ #
#  Shared button style builders                                        #
# ------------------------------------------------------------------ #
# Use these for any solid-colour QPushButton so the static style and the
# screen's refresh_theme() can never drift apart. `extra` is appended to
# the base rule for per-site padding / font-size / border-radius tweaks.

_RADIUS = "border-radius: 4px;"  # baked into every builder so a custom `extra`
                                 # (padding/font-size) can never drop the rounding.


def btn_primary_qss(extra: str = "") -> str:
    """Solid accent-blue action button with theme-aware disabled state."""
    t = tokens()
    return (
        f"QPushButton {{ background: {t['accent']}; color: white; {_RADIUS} {extra} }}"
        f"QPushButton:hover {{ background: {t['accent_hover']}; }}"
        f"QPushButton:disabled {{ background: {t['btn_disabled_bg']}; color: {t['btn_disabled_fg']}; }}"
    )


def btn_danger_qss(extra: str = "") -> str:
    """Solid red destructive-action button with theme-aware disabled state."""
    t = tokens()
    return (
        f"QPushButton {{ background: #c42b1c; color: white; {_RADIUS} {extra} }}"
        f"QPushButton:hover {{ background: #a4261a; }}"
        f"QPushButton:disabled {{ background: {t['btn_disabled_bg']}; color: {t['btn_disabled_fg']}; }}"
    )


def btn_neutral_qss(extra: str = "padding: 5px 14px;") -> str:
    """Standard neutral button (theme surface colours)."""
    t = tokens()
    return (
        f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
        f"color: {t['text']}; {_RADIUS} {extra} }}"
        f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
    )


def btn_ghost_qss(extra: str = "padding: 5px 12px;") -> str:
    """Quiet borderless button — for toolbar / low-emphasis actions."""
    t = tokens()
    return (
        f"QPushButton {{ background: transparent; border: none; color: {t['text_dim']}; "
        f"{_RADIUS} {extra} }}"
        f"QPushButton:hover {{ background: {t['btn_hover']}; color: {t['text']}; }}"
        f"QPushButton:disabled {{ color: {t['text_dim2']}; }}"
    )


def btn_pill_accent_qss(extra: str = "padding: 5px 14px;") -> str:
    """An outlined accent 'pill' chip — a distinct, clearly-clickable control for
    things like a mode switch. Accent text/border on a transparent fill, with a
    soft accent wash on hover and a solid accent fill when pressed. Pair with an
    accent-tinted icon so it stays legible in every state."""
    t = tokens()
    return (
        f"QPushButton {{ background: transparent; border: 1px solid {t['accent']}; "
        f"color: {t['accent']}; border-radius: 12px; font-weight: 600; {extra} }}"
        f"QPushButton:hover {{ background: rgba(0, 120, 212, 0.14); }}"       # accent @ 14%
        f"QPushButton:pressed {{ background: rgba(0, 120, 212, 0.22); }}"
        f"QPushButton:disabled {{ border-color: {t['btn_disabled_bg']}; "
        f"color: {t['btn_disabled_fg']}; }}"
    )


# ------------------------------------------------------------------ #
#  Design-system helpers — spacing, type, surfaces                     #
# ------------------------------------------------------------------ #
# Palette-based. Do NOT install a global QApplication stylesheet: it would
# switch item views to QStyleSheetStyle and silently drop item background/
# foreground roles (see run_screen's status-colour delegate).

SPACE_XS, SPACE_SM, SPACE_MD, SPACE_LG, SPACE_XL = 4, 8, 16, 24, 32


def page_title_qss() -> str:
    return f"color: {tokens()['text']}; font-size: 20px; font-weight: 600;"


def section_label_qss() -> str:
    """Small muted section label that sits above a group of controls."""
    return f"color: {tokens()['text_dim']}; font-size: 11px; font-weight: 600;"


def caption_qss() -> str:
    return f"color: {tokens()['text_dim2']}; font-size: 11px;"


def card_qss(extra: str = "") -> str:
    """Subtle surface card style (apply to an objectName-scoped QFrame)."""
    t = tokens()
    return (f"background: {t['surface']}; border: 1px solid {t['border']}; "
            f"border-radius: 8px; {extra}")


def input_qss(extra: str = "") -> str:
    """Modern rounded text-input styling (QLineEdit / QPlainTextEdit / QTextEdit).
    Combos have their own builder (combo_qss)."""
    t = tokens()
    return (
        f"QLineEdit, QPlainTextEdit, QTextEdit {{ background: {t['surface2']}; "
        f"border: 1px solid {t['border']}; border-radius: 6px; padding: 6px 8px; "
        f"color: {t['text']}; {extra} }}"
        f"QLineEdit:focus, QPlainTextEdit:focus, QTextEdit:focus {{ border: 1px solid {t['accent']}; }}"
        f"QLineEdit:disabled, QPlainTextEdit:disabled, QTextEdit:disabled {{ color: {t['text_dim']}; }}"
    )


def style_inputs(root) -> None:
    """Give every PLAIN text input under `root` (QLineEdit / QPlainTextEdit /
    QTextEdit) the modern rounded `input_qss` look. Inputs that already carry a
    bespoke stylesheet — borderless search boxes, read-only fields, etc. — are
    left untouched. Inputs we style are tagged (`_autoInput`) so they re-theme on
    a theme toggle instead of being skipped (a styled widget is no longer
    "plain"). Per-widget (not a global app stylesheet), so item views are never
    switched to QStyleSheetStyle. Call on init and on every theme refresh.

    An editable QComboBox exposes its internal editor as a child QLineEdit;
    stamping input_qss onto it would draw a second bordered, padded box inside
    the already-padded combo and clip the text — so those are skipped and left
    to combo_qss, which styles the embedded editor itself."""
    from PyQt5.QtWidgets import QLineEdit, QPlainTextEdit, QTextEdit, QComboBox
    qss = input_qss()
    for w in root.findChildren((QLineEdit, QPlainTextEdit, QTextEdit)):
        if isinstance(w.parent(), QComboBox):
            continue
        if w.property("_autoInput") or not w.styleSheet():
            w.setProperty("_autoInput", True)
            w.setStyleSheet(qss)


def header_qss() -> str:
    """Modern flat table/tree header. Apply to the *header view* — a child of the
    item view, e.g. ``table.horizontalHeader()`` / ``tree.header()``. Styling the
    header child never switches the item view itself to QStyleSheetStyle, so item
    setBackground / setForeground / alternating rows are all preserved."""
    t = tokens()
    return (
        "QHeaderView { border: none; background: transparent; }"
        f"QHeaderView::section {{ background: {t['header_bg']}; color: {t['text_dim']}; "
        f"padding: 7px 12px; border: none; border-bottom: 1px solid {t['border']}; "
        "font-weight: 600; }"
        f"QHeaderView::section:hover {{ color: {t['text']}; }}"
        "QHeaderView::section:vertical { border-bottom: none; }"
    )


def style_item_view(view) -> None:
    """Apply the modern flat look (flat header, no gridlines, no sunken frame) to
    a QTableWidget / QTreeWidget. Only the header *child* is stylesheet-styled and
    the grid/frame are toggled via methods — item rendering stays native, so every
    setBackground / setForeground / alternating-row colour is preserved. Safe to
    call again on theme toggle to re-tint the header."""
    view.setFrameShape(view.NoFrame)
    qss = header_qss()
    for getter in ("horizontalHeader", "verticalHeader", "header"):
        fn = getattr(view, getter, None)
        hdr = fn() if callable(fn) else None
        if hdr is not None:
            hdr.setStyleSheet(qss)
            hdr.setHighlightSections(False)
    if hasattr(view, "setShowGrid"):
        view.setShowGrid(False)


_chevron_paths = {}


def _down_arrow_url(color: str) -> str:
    """Render chevron-down tinted to *color* to a cached PNG and return a
    forward-slashed path for QComboBox::down-arrow's `image: url(...)`. (Qt's
    QSS does NOT support the CSS border-triangle trick for ::down-arrow — it
    draws a box — so a real image is the reliable way to get an arrow.)"""
    cached = _chevron_paths.get(color)
    import os
    if cached and os.path.exists(cached):
        return cached.replace("\\", "/")
    try:
        import tempfile
        from app.utils import icons
        pm = icons.pixmap("chevron-down", color=color, size=18)
        d = os.path.join(tempfile.gettempdir(), "adotcm_chevrons")
        os.makedirs(d, exist_ok=True)
        fp = os.path.join(d, "chevron-%s.png" % color.lstrip("#"))
        pm.save(fp, "PNG")
        _chevron_paths[color] = fp
        return fp.replace("\\", "/")
    except Exception:
        return ""


def combo_qss(extra: str = "") -> str:
    """Modern QComboBox: flat rounded field, a real chevron-down arrow image, and
    a rounded themed popup list with padded items + accent selection."""
    t = tokens()
    arrow = _down_arrow_url(t["text_dim"])
    arrow_rule = (
        f'QComboBox::down-arrow {{ image: url("{arrow}"); width: 12px; '
        "height: 12px; margin-right: 9px; }" if arrow else "")
    rules = [
        # min-height guarantees vertical room for the text at any DPI/font size —
        # without it an editable combo's embedded QLineEdit gets squeezed by the
        # padding and clips the bottom of tall glyphs (Segoe UI descenders).
        f"QComboBox {{ background: {t['surface2']}; border: 1px solid {t['border']}; "
        f"border-radius: 6px; padding: 5px 10px; min-height: 20px; color: {t['text']}; {extra} }}",
        f"QComboBox:hover {{ border-color: {t['scroll_handle_hover']}; }}",
        f"QComboBox:focus, QComboBox:on {{ border-color: {t['accent']}; }}",
        f"QComboBox:disabled {{ color: {t['text_dim2']}; background: {t['surface']}; }}",
        # Editable combos render their text through an embedded QLineEdit; strip
        # its own frame/margins so it fills the padded content rect instead of
        # adding a second inset that clips the text.
        f"QComboBox QLineEdit {{ border: none; background: transparent; padding: 0; "
        f"margin: 0; color: {t['text']}; selection-background-color: {t['accent']}; "
        "selection-color: #ffffff; }",
        "QComboBox::drop-down { border: none; width: 26px; }",
        arrow_rule,
        f"QComboBox QAbstractItemView {{ background: {t['surface2']}; "
        f"border: 1px solid {t['border']}; border-radius: 6px; outline: none; padding: 4px; "
        f"selection-background-color: {t['accent']}; selection-color: #ffffff; }}",
        f"QComboBox QAbstractItemView::item {{ min-height: 22px; padding: 4px 8px; "
        f"border-radius: 4px; color: {t['text']}; }}",
        "QComboBox QAbstractItemView::item:hover { background: rgba(128, 128, 128, 0.18); }",
    ]
    return "".join(rules)


def list_qss(extra: str = "") -> str:
    """Modern QListWidget (search dropdowns / pickers): rounded themed surface,
    padded rounded items, subtle hover, accent selection."""
    t = tokens()
    return (
        f"QListWidget {{ background: {t['surface2']}; border: 1px solid {t['border']}; "
        f"border-radius: 8px; outline: none; padding: 4px; color: {t['text']}; {extra} }}"
        "QListWidget::item { padding: 8px 10px; border-radius: 6px; }"
        "QListWidget::item:hover { background: rgba(128, 128, 128, 0.16); }"
        f"QListWidget::item:selected {{ background: {t['accent']}; color: #ffffff; }}"
    )


def style_combos(root) -> None:
    """Apply combo_qss to every QComboBox under *root* (downward-only — never
    touches sibling tables/trees). Re-call on theme toggle to re-tint."""
    from PyQt5.QtWidgets import QComboBox
    qss = combo_qss()
    for c in root.findChildren(QComboBox):
        c.setStyleSheet(qss)


def status_dot_html(state: str) -> str:
    """A small coloured bullet for inline status text (ok / warn / error)."""
    t = tokens()
    col = {"ok": t["ok"], "warn": t["warn_fg"], "error": t["error"]}.get(state, t["text_dim2"])
    return f"<span style='color: {col};'>●</span>"


def scrollbar_qss() -> str:
    """Modern thin scrollbar — no arrow buttons, transparent track, rounded
    handle that darkens on hover.

    Apply ONLY to the QScrollBar widgets themselves (via `style_scrollbars()` /
    `verticalScrollBar().setStyleSheet()`), NEVER to a container or globally:
    a stylesheet on an item view (or its ancestor) switches it to
    QStyleSheetStyle, which drops model item backgrounds/foregrounds and
    alternating-row colours (import detail rows, rename diff, review tree, the
    run-screen status tints). Styling the scrollbar child is downward-safe."""
    t = tokens()
    h, hh = t["scroll_handle"], t["scroll_handle_hover"]
    return (
        "QScrollBar:vertical { background: transparent; width: 10px; margin: 0; }"
        f"QScrollBar::handle:vertical {{ background: {h}; min-height: 28px; border-radius: 5px; }}"
        f"QScrollBar::handle:vertical:hover {{ background: {hh}; }}"
        "QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical { height: 0; border: none; background: none; }"
        "QScrollBar::add-page:vertical, QScrollBar::sub-page:vertical { background: transparent; }"
        "QScrollBar:horizontal { background: transparent; height: 10px; margin: 0; }"
        f"QScrollBar::handle:horizontal {{ background: {h}; min-width: 28px; border-radius: 5px; }}"
        f"QScrollBar::handle:horizontal:hover {{ background: {hh}; }}"
        "QScrollBar::add-line:horizontal, QScrollBar::sub-line:horizontal { width: 0; border: none; background: none; }"
        "QScrollBar::add-page:horizontal, QScrollBar::sub-page:horizontal { background: transparent; }"
    )


def style_scrollbars(root) -> None:
    """Give every scroll area under `root` (and `root` itself if it is one) the
    modern scrollbar look, by styling each scrollbar widget directly so item
    views are never switched to QStyleSheetStyle. Call after a window's UI is
    built and again on theme toggle (handle colours are theme-aware)."""
    from PyQt5.QtWidgets import QAbstractScrollArea
    qss = scrollbar_qss()
    areas = list(root.findChildren(QAbstractScrollArea))
    if isinstance(root, QAbstractScrollArea):
        areas.append(root)
    for area in areas:
        area.verticalScrollBar().setStyleSheet(qss)
        area.horizontalScrollBar().setStyleSheet(qss)


# ------------------------------------------------------------------ #
#  Internal                                                            #
# ------------------------------------------------------------------ #

def _set_palette(dark: bool) -> None:
    app = QApplication.instance()
    if app is None:
        return
    app.setPalette(_make_palette(dark))


def _make_palette(dark: bool) -> QPalette:
    p = QPalette()
    if dark:
        p.setColor(QPalette.Window,          QColor("#1e1e1e"))
        p.setColor(QPalette.WindowText,      QColor("#cccccc"))
        p.setColor(QPalette.Base,            QColor("#3c3c3c"))
        p.setColor(QPalette.AlternateBase,   QColor("#252526"))
        p.setColor(QPalette.Text,            QColor("#cccccc"))
        p.setColor(QPalette.BrightText,      QColor("#ffffff"))
        p.setColor(QPalette.Button,          QColor("#3a3a3a"))
        p.setColor(QPalette.ButtonText,      QColor("#cccccc"))
        p.setColor(QPalette.Highlight,       QColor("#0078d4"))
        p.setColor(QPalette.HighlightedText, QColor("#ffffff"))
        p.setColor(QPalette.Link,            QColor("#4facde"))
        p.setColor(QPalette.ToolTipBase,     QColor("#2d2d30"))
        p.setColor(QPalette.ToolTipText,     QColor("#cccccc"))
        p.setColor(QPalette.Light,           QColor("#555555"))
        p.setColor(QPalette.Midlight,        QColor("#444444"))
        p.setColor(QPalette.Dark,            QColor("#1a1a1a"))
        p.setColor(QPalette.Mid,             QColor("#2a2a2a"))
        p.setColor(QPalette.Shadow,          QColor("#000000"))
        p.setColor(QPalette.Disabled, QPalette.Text,       QColor("#666666"))
        p.setColor(QPalette.Disabled, QPalette.ButtonText, QColor("#666666"))
        p.setColor(QPalette.Disabled, QPalette.WindowText, QColor("#666666"))
        p.setColor(QPalette.Disabled, QPalette.Base,       QColor("#2a2a2a"))
        p.setColor(QPalette.Disabled, QPalette.Button,     QColor("#2d2d2d"))
    else:
        p.setColor(QPalette.Window,          QColor("#efeff0"))
        p.setColor(QPalette.WindowText,      QColor("#333333"))
        p.setColor(QPalette.Base,            QColor("#ffffff"))
        p.setColor(QPalette.AlternateBase,   QColor("#f5f5f5"))
        p.setColor(QPalette.Text,            QColor("#333333"))
        p.setColor(QPalette.BrightText,      QColor("#000000"))
        p.setColor(QPalette.Button,          QColor("#efeff0"))
        p.setColor(QPalette.ButtonText,      QColor("#333333"))
        p.setColor(QPalette.Highlight,       QColor("#0078d4"))
        p.setColor(QPalette.HighlightedText, QColor("#ffffff"))
        p.setColor(QPalette.Link,            QColor("#0078d4"))
        p.setColor(QPalette.ToolTipBase,     QColor("#fffbe6"))
        p.setColor(QPalette.ToolTipText,     QColor("#333333"))
    return p
