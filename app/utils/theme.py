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
}

_DARK: dict = {
    "surface":            "#252526",
    "surface2":           "#2d2d30",
    "border":             "#3e3e42",
    "text":               "#cccccc",
    "text_dim":           "#999999",
    "text_dim2":          "#777777",
    "accent":             "#0078d4",
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


def load_saved() -> None:
    global _dark
    _dark = bool(load_settings().get("dark_mode", False))
    _set_palette(_dark)


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
