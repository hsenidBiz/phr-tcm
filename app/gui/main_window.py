from PyQt5.QtWidgets import (
    QMainWindow, QStackedWidget, QWidget, QVBoxLayout,
    QHBoxLayout, QLabel, QTabWidget, QStatusBar, QFrame, QPushButton
)
from PyQt5.QtCore import Qt
from PyQt5.QtGui import QFont, QCursor

from app.gui.auth_screen import AuthScreen
from app.gui.config_screen import ConfigScreen
from app.gui.manual_entry import ManualEntryWidget
from app.gui.import_screen import ImportWidget
from app.gui.edit_screen import EditScreen
from app.gui.review_screen import ReviewScreen
from app.gui.progress_screen import ProgressScreen
from app.utils import theme

# Page indices in the QStackedWidget
PAGE_AUTH = 0
PAGE_CONFIG = 1
PAGE_MAIN = 2
PAGE_REVIEW = 3
PAGE_PROGRESS = 4


class MainWindow(QMainWindow):
    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self.setWindowTitle("Azure DevOps Test Case Creator")
        self.setMinimumSize(860, 640)
        self.resize(980, 720)

        self.stack = QStackedWidget()
        self.setCentralWidget(self.stack)

        self._build_auth_page()
        self._build_config_page()
        self._build_main_page()
        self._build_review_page()
        self._build_progress_page()

        self._status_bar = QStatusBar()
        self.setStatusBar(self._status_bar)

        # Theme toggle button (always visible in status bar)
        self._theme_btn = QPushButton()
        self._theme_btn.setFlat(True)
        self._theme_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._theme_btn.clicked.connect(self._toggle_theme)
        self._status_bar.addPermanentWidget(self._theme_btn)

        # Apply saved theme; refresh if dark (screens were built in light mode)
        theme.load_saved()
        self._update_theme_btn()
        if theme.is_dark():
            self._refresh_all_themes()

        # Keep expiry countdown ticking on every page that shows it
        from PyQt5.QtCore import QTimer
        self._expiry_tick = QTimer(self)
        self._expiry_tick.setInterval(1000)
        self._expiry_tick.timeout.connect(self._tick_expiry)
        self._expiry_tick.start()

        self.stack.setCurrentIndex(PAGE_AUTH)

    # ------------------------------------------------------------------ #
    #  Page builders                                                       #
    # ------------------------------------------------------------------ #

    def _build_auth_page(self):
        self.auth_screen = AuthScreen(self.app_state)
        self.auth_screen.connected.connect(self._go_config)
        self.stack.addWidget(self.auth_screen)

    def _build_config_page(self):
        self.config_screen = ConfigScreen(self.app_state)
        self.config_screen.configured.connect(self._go_main)
        self.config_screen.back_requested.connect(self._go_auth)
        self.stack.addWidget(self.config_screen)

    def _build_main_page(self):
        container = QWidget()
        v = QVBoxLayout(container)
        v.setContentsMargins(0, 0, 0, 0)
        v.setSpacing(0)

        # Header bar
        self._header_frame = QFrame()
        self._header_frame.setStyleSheet(
            "QFrame { background: #f0f0f0; border-bottom: 1px solid #ddd; }"
        )
        h_layout = QHBoxLayout(self._header_frame)
        h_layout.setContentsMargins(20, 8, 20, 8)
        self.main_header_label = QLabel("")
        self.main_header_label.setStyleSheet("color: #555; font-size: 12px;")
        h_layout.addWidget(self.main_header_label)
        h_layout.addStretch()
        self.queue_count_label = QLabel("Queue: 0 test cases")
        self.queue_count_label.setStyleSheet("color: #0078d4; font-weight: bold;")
        h_layout.addWidget(self.queue_count_label)
        v.addWidget(self._header_frame)

        # Tab widget
        self.tabs = QTabWidget()
        self.tabs.setStyleSheet(
            "QTabBar::tab { padding: 8px 20px; min-width: 160px; }"
            "QTabBar::tab:selected { font-weight: bold; color: #0078d4; }"
        )

        self.manual_widget = ManualEntryWidget(self.app_state)
        self.manual_widget.test_case_queued.connect(self._on_test_case_queued)
        self.tabs.addTab(self.manual_widget, "Manual Entry")

        self.import_widget = ImportWidget(self.app_state)
        self.import_widget.test_cases_queued.connect(self._on_test_cases_queued)
        self.tabs.addTab(self.import_widget, "Import File")

        self.edit_widget = EditScreen(self.app_state)
        self.tabs.addTab(self.edit_widget, "Edit Test Cases")

        v.addWidget(self.tabs, 1)

        # Footer bar with Review button
        self._footer_frame = QFrame()
        self._footer_frame.setStyleSheet(
            "QFrame { background: #f9f9f9; border-top: 1px solid #ddd; }"
        )
        f_layout = QHBoxLayout(self._footer_frame)
        f_layout.setContentsMargins(20, 10, 20, 10)

        self.main_back_btn = QPushButton("← Back to Config")
        self.main_back_btn.setFixedHeight(36)
        self.main_back_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; "
            "border-radius: 4px; font-size: 13px; padding: 0 16px; }"
            "QPushButton:hover { background: #e0e0e0; }"
        )
        self.main_back_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.main_back_btn.clicked.connect(self._go_config)
        f_layout.addWidget(self.main_back_btn)
        f_layout.addStretch()

        self.review_btn = QPushButton("Review & Create →")
        self.review_btn.setFixedHeight(36)
        self.review_btn.setEnabled(False)
        self.review_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; "
            "font-size: 13px; padding: 0 20px; }"
            "QPushButton:hover { background: #106ebe; }"
            "QPushButton:disabled { background: #aaa; }"
        )
        self.review_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.review_btn.clicked.connect(self._go_review)
        f_layout.addWidget(self.review_btn)
        v.addWidget(self._footer_frame)

        self.stack.addWidget(container)

    def _build_review_page(self):
        self.review_screen = ReviewScreen(self.app_state)
        self.review_screen.confirmed.connect(self._go_progress)
        self.review_screen.back_requested.connect(self._go_main)
        self.stack.addWidget(self.review_screen)

    def _build_progress_page(self):
        self.progress_screen = ProgressScreen(self.app_state)
        self.progress_screen.all_done.connect(self._go_config)
        self.stack.addWidget(self.progress_screen)

    # ------------------------------------------------------------------ #
    #  Navigation                                                          #
    # ------------------------------------------------------------------ #

    def _go_auth(self):
        self.stack.setCurrentIndex(PAGE_AUTH)
        self._status("Returned to authentication screen.")

    def _go_config(self):
        self.config_screen.on_enter()
        self.stack.setCurrentIndex(PAGE_CONFIG)
        self._status("Connected. Configure your PBI and module field.")

    def _go_main(self):
        self._refresh_main_expiry()
        self._update_queue_label()
        self.stack.setCurrentIndex(PAGE_MAIN)

    def _refresh_main_expiry(self):
        tm = self.app_state.token_manager
        self.main_header_label.setText(
            f"{tm.org_url}/{tm.project}  |  PBI #{self.app_state.pbi_id}: "
            f"{self.app_state.pbi_title}  |  {tm.get_expiry_display()}"
        )

    def _tick_expiry(self):
        """Called every second to refresh the expiry countdown on the active page."""
        current = self.stack.currentIndex()
        if current == PAGE_CONFIG:
            self.config_screen.refresh_expiry()
        elif current == PAGE_MAIN:
            self._refresh_main_expiry()

    def _go_review(self):
        if not self.app_state.queue:
            from PyQt5.QtWidgets import QMessageBox
            QMessageBox.information(
                self, "Empty Queue",
                "Add at least one test case before reviewing."
            )
            return
        self.review_screen.on_enter()
        self.stack.setCurrentIndex(PAGE_REVIEW)

    def _go_progress(self):
        self.stack.setCurrentIndex(PAGE_PROGRESS)
        self.progress_screen.start()

    # ------------------------------------------------------------------ #
    #  Queue management                                                    #
    # ------------------------------------------------------------------ #

    def _on_test_case_queued(self, tc):
        self.app_state.queue.append(tc)
        self._update_queue_label()
        self._status(f"Added '{tc.title}' to queue ({len(self.app_state.queue)} total).")

    def _on_test_cases_queued(self, cases):
        self.app_state.queue.extend(cases)
        self._update_queue_label()
        self._status(
            f"Added {len(cases)} test case(s) from file ({len(self.app_state.queue)} total)."
        )

    def _update_queue_label(self):
        n = len(self.app_state.queue)
        self.queue_count_label.setText(f"Queue: {n} test case{'s' if n != 1 else ''}")
        self.review_btn.setEnabled(n > 0)

    def _status(self, msg: str):
        self._status_bar.showMessage(msg, 5000)

    # ------------------------------------------------------------------ #
    #  Theme                                                               #
    # ------------------------------------------------------------------ #

    def _toggle_theme(self):
        theme.apply(not theme.is_dark())
        self._update_theme_btn()
        self._refresh_all_themes()

    def _update_theme_btn(self):
        if theme.is_dark():
            self._theme_btn.setText("☀  Light")
            self._theme_btn.setStyleSheet(
                "QPushButton { border: none; color: #aaa; padding: 2px 8px; "
                "background: transparent; font-size: 12px; }"
                "QPushButton:hover { color: #ddd; }"
            )
        else:
            self._theme_btn.setText("🌙  Dark")
            self._theme_btn.setStyleSheet(
                "QPushButton { border: none; color: #666; padding: 2px 8px; "
                "background: transparent; font-size: 12px; }"
                "QPushButton:hover { color: #333; }"
            )

    def _refresh_all_themes(self):
        self._refresh_self_theme()
        self.auth_screen.refresh_theme()
        self.config_screen.refresh_theme()
        self.manual_widget.refresh_theme()
        self.import_widget.refresh_theme()
        self.edit_widget.refresh_theme()
        self.review_screen.refresh_theme()
        self.progress_screen.refresh_theme()

    def _refresh_self_theme(self):
        t = theme.tokens()
        self._header_frame.setStyleSheet(
            f"QFrame {{ background: {t['header_bg']}; border-bottom: 1px solid {t['border']}; }}"
        )
        self._footer_frame.setStyleSheet(
            f"QFrame {{ background: {t['footer_bg']}; border-top: 1px solid {t['border']}; }}"
        )
        self.main_header_label.setStyleSheet(f"color: {t['text_dim']}; font-size: 12px;")
        self.tabs.setStyleSheet(
            f"QTabBar::tab {{ padding: 8px 20px; min-width: 160px; }}"
            f"QTabBar::tab:selected {{ font-weight: bold; color: {t['accent']}; }}"
        )
        self.main_back_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; font-size: 13px; padding: 0 16px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )
