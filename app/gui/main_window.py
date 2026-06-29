from PyQt5.QtWidgets import (
    QMainWindow, QStackedWidget, QWidget, QVBoxLayout,
    QHBoxLayout, QLabel, QTabWidget, QStatusBar, QFrame, QPushButton,
    QMessageBox, QShortcut
)
from PyQt5.QtCore import Qt, QTimer, QThreadPool
from PyQt5.QtGui import QCursor, QKeySequence

from app.utils.anim import fade_in
from app.utils.worker import Worker

from app.gui.auth_screen import AuthScreen
from app.gui.config_screen import ConfigScreen
from app.gui.manual_entry import ManualEntryWidget
from app.gui.import_screen import ImportWidget
from app.gui.edit_screen import EditScreen
from app.gui.run_screen import RunScreen
from app.gui.review_screen import ReviewScreen
from app.gui.progress_screen import ProgressScreen
from app.gui import frameless
from app.utils import theme

# Page indices in the QStackedWidget
PAGE_AUTH = 0
PAGE_CONFIG = 1
PAGE_MAIN = 2
PAGE_REVIEW = 3
PAGE_PROGRESS = 4


class MainWindow(frameless.FramelessMixin, QMainWindow):
    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        from app.version import VERSION
        self.setWindowTitle(f"Azure DevOps Test Case Manager  v{VERSION}")
        self.setMinimumSize(860, 640)
        self.resize(980, 720)

        self.stack = QStackedWidget()
        self.setCentralWidget(self.stack)

        self._build_auth_page()
        self._build_config_page()
        self._build_main_page()
        self._build_review_page()
        self._build_progress_page()

        # Custom dark title bar + 1px border in place of the native OS chrome.
        self._title_bar = self.init_frameless(
            f"Azure DevOps Test Case Manager  v{VERSION}")

        self._status_bar = QStatusBar()
        self.setStatusBar(self._status_bar)

        # Bottom-left "Checking for updates" indicator — shown only while a
        # background update check is running, removed as soon as it finishes.
        from app.utils.anim import Spinner
        self._update_check_box = QWidget()
        _uc = QHBoxLayout(self._update_check_box)
        _uc.setContentsMargins(6, 0, 0, 0)
        _uc.setSpacing(6)
        self._update_spinner = Spinner(size=14, line_width=2)
        self._update_spinner.setVisible(False)
        _uc.addWidget(self._update_spinner)
        self._update_check_label = QLabel("Checking for updates")
        self._update_check_label.setStyleSheet("font-size: 11px;")
        _uc.addWidget(self._update_check_label)
        self._status_bar.addWidget(self._update_check_box)
        self._update_check_box.setVisible(False)

        # Update button (hidden until a newer version is found on GitHub)
        self._update_btn = QPushButton()
        self._update_btn.setVisible(False)
        self._update_btn.setFlat(True)
        self._update_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self._update_btn.setStyleSheet(
            "QPushButton { border: none; color: #0078d4; padding: 2px 8px; "
            "background: transparent; font-size: 12px; font-weight: bold; }"
            "QPushButton:hover { color: #106ebe; }"
        )
        self._update_btn.clicked.connect(self._on_update_clicked)
        self._status_bar.addPermanentWidget(self._update_btn)

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
        self._expiry_tick = QTimer(self)
        self._expiry_tick.setInterval(1000)
        self._expiry_tick.timeout.connect(self._tick_expiry)
        self._expiry_tick.start()

        self.stack.setCurrentIndex(PAGE_AUTH)

        # Modern scrollbars + dropdowns across every screen (re-applied on toggle).
        theme.style_scrollbars(self)
        theme.style_combos(self)

        # Minimum window size = enough to show the whole Configuration screen.
        self._apply_min_size_for_config()

        # Restore draft queue after window is shown
        QTimer.singleShot(300, self._check_draft_restore)

        # Offer to resume an interrupted test run
        QTimer.singleShot(600, self._check_run_restore)

        # Check GitHub for a newer version in the background
        QTimer.singleShot(1500, self._check_for_update)

    def _apply_min_size_for_config(self):
        """Make the window's minimum size large enough to show the whole
        Configuration screen (its tallest state) without scrolling — capped to
        the available screen so the window stays placeable on small displays
        (the config's scroll area covers that rare case)."""
        from PyQt5.QtWidgets import QApplication
        try:
            content_w, content_h = self.config_screen.required_min_size()
        except Exception:
            return
        chrome = 8  # 1px border ring + a little slack
        if getattr(self, "_title_bar", None) is not None:
            chrome += self._title_bar.sizeHint().height()
        if getattr(self, "_status_bar", None) is not None:
            chrome += self._status_bar.sizeHint().height()
        min_w = max(860, int(content_w))
        min_h = int(content_h) + chrome
        scr = QApplication.primaryScreen()
        if scr is not None:
            avail = scr.availableGeometry()
            min_w = min(min_w, avail.width() - 40)
            min_h = min(min_h, avail.height() - 60)
        self.setMinimumSize(min_w, min_h)
        # Open at least at the new minimum (with a little breathing room).
        self.resize(max(self.width(), min_w), max(self.height(), min_h))

    # ------------------------------------------------------------------ #
    #  Page builders                                                       #
    # ------------------------------------------------------------------ #

    def _build_auth_page(self):
        self.auth_screen = AuthScreen(self.app_state)
        self.auth_screen.connected.connect(self._go_config)
        self.stack.addWidget(self.auth_screen)

    def _build_config_page(self):
        self.config_screen = ConfigScreen(self.app_state)
        self.config_screen.configured.connect(lambda: self._go_main(land_on_import=True))
        self.config_screen.back_requested.connect(self._go_auth)
        self.stack.addWidget(self.config_screen)

    def _build_main_page(self):
        container = QWidget()
        v = QVBoxLayout(container)
        v.setContentsMargins(0, 0, 0, 0)
        v.setSpacing(0)

        # Header bar
        self._header_frame = QFrame()
        self._header_frame.setObjectName("headerFrame")
        self._header_frame.setStyleSheet(
            "#headerFrame { background: #f0f0f0; border-bottom: 1px solid #ddd; }"
        )
        h_layout = QHBoxLayout(self._header_frame)
        h_layout.setContentsMargins(20, 8, 20, 8)
        self.main_header_label = QLabel("")
        self.main_header_label.setStyleSheet("color: #555; font-size: 12px;")
        h_layout.addWidget(self.main_header_label)
        h_layout.addStretch()
        self.queue_count_label = QLabel("0 queued")
        self.queue_count_label.setAlignment(Qt.AlignCenter)
        self.queue_count_label.setFixedHeight(22)   # radius = half height -> true pill
        self.queue_count_label.setStyleSheet(
            "QLabel { background: #aaa; color: white; border-radius: 11px; "
            "padding: 0px 14px; font-weight: bold; font-size: 11px; }"
        )
        h_layout.addWidget(self.queue_count_label)
        v.addWidget(self._header_frame)

        # Tab widget
        self.tabs = QTabWidget()
        self.tabs.setStyleSheet(
            "QTabWidget::pane { border: none; border-top: 1px solid #ddd; } "
            "QTabBar::tab { padding: 8px 18px 14px; min-width: 140px; border: none; "
            "border-bottom: 2px solid transparent; color: #888; font-size: 17px; background: transparent; } "
            "QTabBar::tab:selected { color: #0078d4; font-weight: bold; border-bottom: 2px solid #0078d4; } "
            "QTabBar::tab:hover:!selected { color: #444; border-bottom: 2px solid #ccc; } "
        )
        # Match the tab bar's real font to the QSS font-size (17px). The QSS only
        # changes the *rendered* size; QTabBar still measures each tab with the
        # widget font, so without this it reserves too little height and clips
        # descenders (p, y). Bold is the widest state (selected) — size for it.
        from PyQt5.QtGui import QFont
        _tab_font = QFont("Segoe UI")
        _tab_font.setPixelSize(17)
        self.tabs.tabBar().setFont(_tab_font)

        # Tab order: Import → Edit → Manual → Run (land on Import first).
        self.import_widget = ImportWidget(self.app_state)
        self.import_widget.test_cases_queued.connect(self._on_test_cases_queued)
        self.tabs.addTab(self.import_widget, "Import File")

        self.edit_widget = EditScreen(self.app_state)
        self.edit_widget.test_case_queued.connect(self._on_test_case_queued)
        self.tabs.addTab(self.edit_widget, "Edit Test Cases")

        self.manual_widget = ManualEntryWidget(self.app_state)
        self.manual_widget.test_case_queued.connect(self._on_test_case_queued)
        self.tabs.addTab(self.manual_widget, "Manual Entry")

        self.run_widget = RunScreen(self.app_state)
        self.tabs.addTab(self.run_widget, "Run Tests")

        self.tabs.currentChanged.connect(self._on_tab_changed)

        v.addWidget(self.tabs, 1)

        # Footer bar with Review button
        self._footer_frame = QFrame()
        self._footer_frame.setObjectName("footerFrame")
        self._footer_frame.setStyleSheet(
            "#footerFrame { background: #f9f9f9; border-top: 1px solid #ddd; }"
        )
        f_layout = QHBoxLayout(self._footer_frame)
        f_layout.setContentsMargins(20, 10, 20, 10)

        from app.utils import theme as _theme, icons as _icons
        self.main_back_btn = QPushButton("Back to config")
        self.main_back_btn.setIcon(_icons.icon("arrow-left", size=15))
        self.main_back_btn.setFixedHeight(36)
        self.main_back_btn.setStyleSheet(_theme.btn_neutral_qss("font-size: 13px; padding: 0 16px;"))
        self.main_back_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.main_back_btn.clicked.connect(self._go_config)
        f_layout.addWidget(self.main_back_btn)
        f_layout.addStretch()

        # Import-tab widgets live in the footer so they sit level with Review & Create
        self._import_count_label = self.import_widget.count_label
        self._import_queue_btn = self.import_widget.queue_btn
        f_layout.addWidget(self._import_count_label)
        f_layout.addSpacing(12)
        f_layout.addWidget(self._import_queue_btn)
        f_layout.addSpacing(12)
        self._import_count_label.setVisible(False)
        self._import_queue_btn.setVisible(False)

        from app.utils import theme, icons
        self.review_btn = QPushButton("Review && create")
        self.review_btn.setIcon(icons.icon("arrow-right", color="white", size=15))
        self.review_btn.setLayoutDirection(Qt.RightToLeft)
        self.review_btn.setFixedHeight(36)
        self.review_btn.setEnabled(False)
        self.review_btn.setStyleSheet(
            theme.btn_primary_qss("border-radius: 4px; font-size: 13px; padding: 0 20px;")
        )
        self.review_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.review_btn.clicked.connect(self._go_review)
        f_layout.addWidget(self.review_btn)
        v.addWidget(self._footer_frame)

        self.stack.addWidget(container)

        # Keyboard shortcut: Ctrl+Shift+R = Review & Create
        QShortcut(QKeySequence("Ctrl+Shift+R"), self).activated.connect(self._go_review)

    def _build_review_page(self):
        self.review_screen = ReviewScreen(self.app_state)
        self.review_screen.confirmed.connect(self._go_progress)
        self.review_screen.back_requested.connect(lambda: self._go_main(land_on_import=False))
        self.review_screen.queue_changed.connect(self._update_queue_label)
        self.stack.addWidget(self.review_screen)

    def _build_progress_page(self):
        self.progress_screen = ProgressScreen(self.app_state)
        self.progress_screen.all_done.connect(self._go_config)
        self.stack.addWidget(self.progress_screen)

    # ------------------------------------------------------------------ #
    #  Navigation                                                          #
    # ------------------------------------------------------------------ #

    def _go_to(self, index: int):
        self.stack.setCurrentIndex(index)
        fade_in(self.stack.currentWidget())

    def _go_auth(self):
        self._go_to(PAGE_AUTH)
        self._status("Returned to authentication screen.")

    def _go_config(self):
        self.config_screen.on_enter()
        self._go_to(PAGE_CONFIG)
        self._status("Connected. Configure your PBI and module field.")

    def _go_main(self, land_on_import: bool = True):
        self._refresh_main_expiry()
        self._update_queue_label()
        # Warm-load the PBI's existing cases so module autocomplete and
        # duplicate-title detection work before the Edit tab is ever opened.
        self.edit_widget.ensure_loaded()
        # Fresh arrival from Config lands on Import File; returning via "Back"
        # from Review keeps whatever tab the user last had open.
        if land_on_import:
            self.tabs.setCurrentWidget(self.import_widget)
        # Sync the import-only footer widgets to whichever tab is current
        # (setCurrentWidget fires no signal when the tab is already current).
        self._on_tab_changed(self.tabs.currentIndex())
        self._go_to(PAGE_MAIN)

    def _on_tab_changed(self, index: int):
        # Identity check (not a fixed index) so the import-only footer widgets
        # stay correct if the tab order ever changes.
        on_import = (self.tabs.widget(index) is self.import_widget)
        self._import_count_label.setVisible(on_import)
        self._import_queue_btn.setVisible(on_import)

    def _refresh_main_expiry(self):
        tm = self.app_state.token_manager
        session = (
            "Signed in" if tm.auto_refresh_active() else tm.get_expiry_display()
        )
        t = theme.tokens()
        title = self.app_state.pbi_title or ""
        if len(title) > 80:
            title = title[:79] + "…"
        # PBI leads (prominent); project + session sit quietly behind it.
        self.main_header_label.setText(
            f"<span style='color:{t['text']}; font-weight:600;'>PBI #{self.app_state.pbi_id}</span>"
            f"<span style='color:{t['text_dim']};'>&nbsp;&nbsp;{title}</span>"
            f"<span style='color:{t['text_dim2']};'>"
            f"&nbsp;&nbsp;·&nbsp;&nbsp;{tm.project}&nbsp;&nbsp;·&nbsp;&nbsp;{session}</span>"
        )

    def _sync_review_btn(self, n: int | None = None):
        """Enable/disable the Review && Create button based on queue size and token state."""
        if n is None:
            n = len(self.app_state.queue)
        expired = self.app_state.token_manager.is_expired()
        self.review_btn.setEnabled(n > 0 and not expired)
        if expired:
            self.review_btn.setToolTip("Session has expired — sign in again to continue")
        else:
            self.review_btn.setToolTip("")

    def _tick_expiry(self):
        current = self.stack.currentIndex()
        if current == PAGE_CONFIG:
            self.config_screen.refresh_expiry()
        elif current == PAGE_MAIN:
            self._refresh_main_expiry()
            self._sync_review_btn()
        elif current == PAGE_REVIEW:
            self.review_screen.refresh_expiry_state()

    def _go_review(self):
        from app.gui.helpers import warn_if_token_expired
        if warn_if_token_expired(self, self.app_state.token_manager):
            return
        if not self.app_state.queue:
            QMessageBox.information(
                self, "Empty Queue",
                "Add at least one test case before reviewing."
            )
            return
        self.review_screen.on_enter()
        self._go_to(PAGE_REVIEW)

    def _go_progress(self):
        self._go_to(PAGE_PROGRESS)
        self.progress_screen.start()

    # ------------------------------------------------------------------ #
    #  Queue management                                                    #
    # ------------------------------------------------------------------ #

    def _on_test_case_queued(self, tc):
        proceed, _prompted = self._check_duplicate_titles([tc])
        if not proceed:
            return
        self.app_state.queue.append(tc)
        self._update_queue_label()
        self._status(f"Added '{tc.title}' to queue ({len(self.app_state.queue)} total).")
        self._notify_queue_accepted()

    def _on_test_cases_queued(self, cases):
        proceed, prompted = self._check_duplicate_titles(cases)
        if not proceed:
            return
        # If some cases are updates (e.g. matched by TestCaseID on re-import) and
        # the duplicate check didn't already prompt, confirm the create/update
        # breakdown before queuing so updates are never added silently.
        if not prompted and any(tc.update_id for tc in cases):
            if not self._confirm_import_summary(cases):
                return
        self.app_state.queue.extend(cases)
        self._update_queue_label()
        self._status(
            f"Added {len(cases)} test case(s) from file ({len(self.app_state.queue)} total)."
        )
        self._notify_queue_accepted()

    def _confirm_import_summary(self, cases: list) -> bool:
        """Summarise how many imported cases will be created vs update existing
        ones, and ask the user to Continue or Cancel before queuing."""
        n_updates = sum(1 for tc in cases if tc.update_id)
        n_creates = len(cases) - n_updates
        lines = []
        if n_creates:
            lines.append(f"• {n_creates} new test case{'s' if n_creates != 1 else ''} will be created")
        if n_updates:
            lines.append(f"• {n_updates} existing test case{'s' if n_updates != 1 else ''} will be updated")

        box = QMessageBox(self)
        box.setIcon(QMessageBox.Question)
        box.setWindowTitle("Confirm Import")
        box.setText(
            "Some imported test cases match existing ones:\n\n"
            + "\n".join(lines)
            + "\n\nUpdates overwrite the matching test case's steps and fields. "
            "Add these to the queue?"
        )
        continue_btn = box.addButton("Continue", QMessageBox.AcceptRole)
        box.addButton("Cancel", QMessageBox.RejectRole)
        box.setDefaultButton(continue_btn)
        box.exec_()
        return box.clickedButton() is continue_btn

    def _notify_queue_accepted(self):
        """Tell the emitting widget its cases were accepted so it can clear its inputs."""
        sender = self.sender()
        callback = getattr(sender, "on_queue_accepted", None)
        if callable(callback):
            callback()

    def _existing_cases_for_pbi(self) -> list:
        """Existing ADO Test Cases for the current PBI. Prefers the shared
        app_state cache (kept fresh by the Import and Edit tabs) and falls back
        to whatever the Edit tab has loaded."""
        if (self.app_state.existing_cases
                and self.app_state.existing_cases_pbi == self.app_state.pbi_id):
            return self.app_state.existing_cases
        return self.edit_widget._cases

    def _check_duplicate_titles(self, incoming: list) -> tuple:
        """Returns (proceed, prompted). `proceed` is True if it is safe to add
        `incoming` to the queue; `prompted` is True if a dialog was shown (so the
        caller can avoid stacking a second confirmation on top).

        Cases with a TestCaseID are deliberate, reliable updates and pass straight
        through. For the rest, if a title already exists on the PBI we warn that
        importing will create duplicates — we never update by title (only the
        TestCaseID can target the exact work item). Titles that only collide with
        the current session queue get a simple add-anyway confirmation."""
        # Cases that already carry an explicit work-item ID (e.g. re-imported from
        # a spreadsheet exported for bulk update) are deliberate updates — never
        # treat them as duplicates or re-match them by title.
        incoming = [tc for tc in incoming if not getattr(tc, "update_id", None)]
        if not incoming:
            return True, False

        # Titles that already exist on the PBI in Azure DevOps.
        ado_titles = {
            (c.get("System.Title", "") or "").lower()
            for c in self._existing_cases_for_pbi()
            if c.get("System.Title")
        }

        # Titles already sitting in the queue this session
        queue_titles = {tc.title.lower() for tc in self.app_state.queue}

        ado_dupes = [tc for tc in incoming if tc.title.lower() in ado_titles]
        queue_dupes = [tc.title for tc in incoming if tc.title.lower() in queue_titles]

        if not ado_dupes and not queue_dupes:
            return True, False

        def _fmt(titles: list) -> str:
            lines = "\n".join(f"  • {t}" for t in titles[:10])
            if len(titles) > 10:
                lines += f"\n  … and {len(titles) - 10} more"
            return lines

        # Case 1 — some titles already exist on the PBI. Without a TestCaseID we
        # can't tell which work item is meant, so we never update by title — we
        # only warn that importing will create duplicates.
        if ado_dupes:
            parts = [
                f"Already exist on PBI #{self.app_state.pbi_id} in Azure DevOps:\n"
                f"{_fmt([tc.title for tc in ado_dupes])}"
            ]
            if queue_dupes:
                parts.append(f"Already in your current queue:\n{_fmt(queue_dupes)}")

            box = QMessageBox(self)
            box.setIcon(QMessageBox.Warning)
            box.setWindowTitle("Duplicate Titles Found")
            box.setText(
                "The following test case title(s) already exist:\n\n"
                + "\n\n".join(parts)
                + "\n\nThese will be added as NEW test cases, creating duplicates. "
                "To update an existing test case instead, export it from the "
                "Edit Test Cases tab, change it, and re-import — its TestCaseID "
                "targets the exact work item.\n\nContinue?"
            )
            continue_btn = box.addButton("Continue", QMessageBox.AcceptRole)
            box.addButton("Cancel", QMessageBox.RejectRole)
            box.setDefaultButton(continue_btn)
            box.exec_()
            return (box.clickedButton() is continue_btn), True

        # Case 2 — duplicates only within the current session queue.
        reply = QMessageBox.question(
            self, "Duplicate Titles",
            "The following test case title(s) are already in your current queue:\n\n"
            + _fmt(queue_dupes)
            + "\n\nAdd to queue anyway?",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No,
        )
        return reply == QMessageBox.Yes, True

    def _update_queue_label(self):
        n = len(self.app_state.queue)
        self.queue_count_label.setText(f"{n} queued")
        color = "#0078d4" if n > 0 else "#aaa"
        self.queue_count_label.setStyleSheet(
            f"QLabel {{ background: {color}; color: white; border-radius: 11px; "
            f"padding: 0px 14px; font-weight: bold; font-size: 11px; }}"
        )
        self._sync_review_btn(n)

    def _status(self, msg: str):
        self._status_bar.showMessage(msg, 5000)

    # ------------------------------------------------------------------ #
    #  Draft queue save / restore                                          #
    # ------------------------------------------------------------------ #

    def _check_draft_restore(self):
        from app.utils.settings import load_draft_queue, clear_draft_queue
        draft = load_draft_queue()
        if not draft:
            return
        reply = QMessageBox.question(
            self, "Restore Draft",
            f"Restore {len(draft)} test case(s) from your last session?",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.Yes,
        )
        if reply == QMessageBox.Yes:
            self.app_state.queue.extend(draft)
            self._update_queue_label()
        clear_draft_queue()

    def _check_run_restore(self):
        from app.utils.settings import load_run_session, clear_run_session
        saved = load_run_session()
        if not saved or not saved.get("cases"):
            return
        cases = saved.get("cases", [])
        n = len(cases)
        marked = sum(1 for s in saved.get("state", []) if s.get("outcome"))
        reply = QMessageBox.question(
            self, "Resume Test Run",
            f"Resume your last test run?\n\n{n} test case{'s' if n != 1 else ''}, "
            f"{marked} already marked.",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.Yes,
        )
        if reply != QMessageBox.Yes:
            clear_run_session()
            return
        from app.gui.test_runner import TestRunner
        runner = TestRunner(self.app_state, cases, restore=saved)
        self.run_widget._open_runners.append(runner)
        runner.show()
        runner.raise_()
        runner.activateWindow()

    def closeEvent(self, event):
        if self.progress_screen.is_running():
            reply = QMessageBox.question(
                self, "Creation In Progress",
                "Test cases are still being created. Closing now will stop the "
                "remaining items.\n\nClose anyway?",
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No,
            )
            if reply != QMessageBox.Yes:
                event.ignore()
                return
            self.progress_screen.shutdown()
        from app.utils.settings import save_draft_queue, clear_draft_queue
        if self.app_state.queue:
            save_draft_queue(self.app_state.queue)
        else:
            clear_draft_queue()
        super().closeEvent(event)

    # ------------------------------------------------------------------ #
    #  Theme                                                               #
    # ------------------------------------------------------------------ #

    # ------------------------------------------------------------------ #
    #  Auto-update (Velopack + public GitHub Releases repo)                #
    # ------------------------------------------------------------------ #

    def _check_for_update(self):
        from app.utils import updater
        if not updater.update_supported():
            return
        self._show_update_check(True)
        worker = Worker(updater.check_for_update)
        worker.signals.result.connect(self._on_update_check_result)
        # Check failures (offline, VPN, …) are silent — just drop the indicator.
        worker.signals.error.connect(self._on_update_check_error)
        QThreadPool.globalInstance().start(worker)

    def _show_update_check(self, on: bool):
        """Show/hide the bottom-left 'Checking for updates' spinner + label."""
        if on:
            t = theme.tokens()
            self._update_spinner.set_color(t["accent"])
            self._update_check_label.setStyleSheet(
                f"color: {t['text_dim']}; font-size: 11px;")
            self._update_check_box.setVisible(True)
            self._update_spinner.start()
        else:
            self._update_spinner.stop()
            self._update_check_box.setVisible(False)

    def _on_update_check_error(self, _exc):
        self._show_update_check(False)

    def _on_update_check_result(self, info):
        self._show_update_check(False)
        if not info:
            return
        self._update_info = info
        version = info["version"]
        from app.utils import icons
        self._update_btn.setIcon(icons.icon("arrow-up", color=theme.tokens()["accent"], size=15))
        self._update_btn.setText(f"Update available (v{version})")
        self._update_btn.setVisible(True)
        notes = (info.get("notes") or "").strip()
        notes_block = f"\n\nWhat's new:\n{notes}" if notes else ""
        reply = QMessageBox.question(
            self, "Update Available",
            f"A newer version (v{version}) is available."
            + notes_block
            + "\n\nDownload and restart now? Any queued test cases are saved as a "
            "draft and restored after the restart.",
            QMessageBox.Yes | QMessageBox.No,
        )
        if reply == QMessageBox.Yes:
            self._apply_update()

    def _on_update_clicked(self):
        reply = QMessageBox.question(
            self, "Update and Restart",
            "Update to the latest version and restart the app?\n\n"
            "Any queued test cases are saved as a draft and restored "
            "after the restart.",
            QMessageBox.Yes | QMessageBox.No,
        )
        if reply == QMessageBox.Yes:
            self._apply_update()

    def _apply_update(self):
        from app.utils import updater
        from app.utils.settings import save_draft_queue, clear_draft_queue
        self._update_btn.setEnabled(False)
        self._update_btn.setText("Updating…")
        self._status("Downloading update…")
        # Velopack replaces the app files and restarts the process, which can
        # bypass closeEvent — persist the queue now so it's restored afterwards.
        if self.app_state.queue:
            save_draft_queue(self.app_state.queue)
        else:
            clear_draft_queue()
        # download_and_apply restarts into the new version on success (this
        # process is terminated by Velopack), so only the failure path returns.
        worker = Worker(updater.download_and_apply, self._update_info)
        worker.signals.error.connect(self._on_update_failed)
        QThreadPool.globalInstance().start(worker)

    def _on_update_failed(self, exc: Exception):
        version = self._update_info.get("version", "")
        self._update_btn.setEnabled(True)
        self._update_btn.setText(f"Update available (v{version})")
        QMessageBox.warning(
            self, "Update Failed",
            f"The update could not be downloaded or applied:\n\n{exc}\n\n"
            "You can keep using this version and try again later."
        )

    def _toggle_theme(self):
        theme.apply(not theme.is_dark())
        self._update_theme_btn()
        self._refresh_all_themes()

    def _update_theme_btn(self):
        from app.utils import icons
        dark = theme.is_dark()
        t = theme.tokens()
        # Icon + label both reflect the CURRENT mode; clicking anywhere on the
        # button (icon or text) toggles via the _toggle_theme connection.
        self._theme_btn.setText("Dark Mode" if dark else "Light Mode")
        self._theme_btn.setIcon(icons.icon("moon" if dark else "sun", color=t["text_dim"], size=16))
        self._theme_btn.setToolTip("Switch to light mode" if dark else "Switch to dark mode")
        self._theme_btn.setStyleSheet(
            f"QPushButton {{ border: none; background: transparent; padding: 3px 8px; "
            f"color: {t['text_dim']}; font-size: 12px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; border-radius: 4px; color: {t['text']}; }}"
        )

    def _refresh_all_themes(self):
        self._refresh_self_theme()
        self.refresh_frameless_theme()     # title bar + repaint the border
        self.auth_screen.refresh_theme()
        self.config_screen.refresh_theme()
        self.manual_widget.refresh_theme()
        self.import_widget.refresh_theme()
        self.edit_widget.refresh_theme()
        self.run_widget.refresh_theme()
        self.review_screen.refresh_theme()
        self.progress_screen.refresh_theme()
        theme.style_scrollbars(self)  # re-tint scrollbar handles for the theme
        theme.style_combos(self)      # re-tint dropdowns for the theme

    def _refresh_self_theme(self):
        t = theme.tokens()
        self._header_frame.setStyleSheet(
            f"#headerFrame {{ background: {t['header_bg']}; border-bottom: 1px solid {t['border']}; }}"
        )
        self._footer_frame.setStyleSheet(
            f"#footerFrame {{ background: {t['footer_bg']}; border-top: 1px solid {t['border']}; }}"
        )
        self.main_header_label.setStyleSheet(f"color: {t['text_dim']}; font-size: 12px;")
        self.tabs.setStyleSheet(
            f"QTabWidget::pane {{ border: none; border-top: 1px solid {t['border']}; }} "
            f"QTabBar::tab {{ padding: 8px 18px 14px; min-width: 140px; border: none;"
            f"border-bottom: 2px solid transparent; color: {t['text_dim2']}; "
            f"font-size: 17px; background: transparent; }} "
            f"QTabBar::tab:selected {{ color: {t['accent']}; font-weight: bold; "
            f"border-bottom: 2px solid {t['accent']}; }} "
            f"QTabBar::tab:hover:!selected {{ color: {t['text']}; "
            f"border-bottom: 2px solid {t['border']}; }} "
        )
        from app.utils import icons as _icons
        self.main_back_btn.setStyleSheet(theme.btn_neutral_qss("font-size: 13px; padding: 0 16px;"))
        self.main_back_btn.setIcon(_icons.icon("arrow-left", size=15))
        self.review_btn.setStyleSheet(
            theme.btn_primary_qss("border-radius: 4px; font-size: 13px; padding: 0 20px;")
        )
        self._import_count_label.setStyleSheet(f"color: {t['count_lbl_color']};")
