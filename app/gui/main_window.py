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

        # Restore draft queue after window is shown
        QTimer.singleShot(300, self._check_draft_restore)

        # Check GitHub for a newer version in the background
        QTimer.singleShot(1500, self._check_for_update)

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
        self.queue_count_label.setStyleSheet(
            "QLabel { background: #aaa; color: white; border-radius: 10px; "
            "padding: 2px 12px; font-weight: bold; font-size: 11px; }"
        )
        h_layout.addWidget(self.queue_count_label)
        v.addWidget(self._header_frame)

        # Tab widget
        self.tabs = QTabWidget()
        self.tabs.setStyleSheet(
            "QTabWidget::pane { border: none; border-top: 1px solid #ddd; } "
            "QTabBar::tab { padding: 8px 18px; min-width: 140px; border: none; "
            "border-bottom: 2px solid transparent; color: #888; font-size: 17px; background: transparent; } "
            "QTabBar::tab:selected { color: #0078d4; font-weight: bold; border-bottom: 2px solid #0078d4; } "
            "QTabBar::tab:hover:!selected { color: #444; border-bottom: 2px solid #ccc; } "
        )

        self.manual_widget = ManualEntryWidget(self.app_state)
        self.manual_widget.test_case_queued.connect(self._on_test_case_queued)
        self.tabs.addTab(self.manual_widget, "Manual Entry")

        self.import_widget = ImportWidget(self.app_state)
        self.import_widget.test_cases_queued.connect(self._on_test_cases_queued)
        self.tabs.addTab(self.import_widget, "Import File")
        self.tabs.currentChanged.connect(self._on_tab_changed)

        self.edit_widget = EditScreen(self.app_state)
        self.edit_widget.test_case_queued.connect(self._on_test_case_queued)
        self.tabs.addTab(self.edit_widget, "Edit Test Cases")

        v.addWidget(self.tabs, 1)

        # Footer bar with Review button
        self._footer_frame = QFrame()
        self._footer_frame.setObjectName("footerFrame")
        self._footer_frame.setStyleSheet(
            "#footerFrame { background: #f9f9f9; border-top: 1px solid #ddd; }"
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

        # Import-tab widgets live in the footer so they sit level with Review & Create
        self._import_count_label = self.import_widget.count_label
        self._import_queue_btn = self.import_widget.queue_btn
        f_layout.addWidget(self._import_count_label)
        f_layout.addSpacing(12)
        f_layout.addWidget(self._import_queue_btn)
        f_layout.addSpacing(12)
        self._import_count_label.setVisible(False)
        self._import_queue_btn.setVisible(False)

        from app.utils import theme
        self.review_btn = QPushButton("Review && Create →")
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
        self.review_screen.back_requested.connect(self._go_main)
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

    def _go_main(self):
        self._refresh_main_expiry()
        self._update_queue_label()
        # Warm-load the PBI's existing cases so module autocomplete and
        # duplicate-title detection work before the Edit tab is ever opened.
        self.edit_widget.ensure_loaded()
        self._go_to(PAGE_MAIN)

    def _on_tab_changed(self, index: int):
        on_import = (index == 1)
        self._import_count_label.setVisible(on_import)
        self._import_queue_btn.setVisible(on_import)

    def _refresh_main_expiry(self):
        tm = self.app_state.token_manager
        session = (
            "Signed in" if tm.auto_refresh_active() else tm.get_expiry_display()
        )
        self.main_header_label.setText(
            f"{tm.org_url}/{tm.project}  |  PBI #{self.app_state.pbi_id}: "
            f"{self.app_state.pbi_title}  |  {session}"
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
        if not self._check_duplicate_titles([tc]):
            return
        self.app_state.queue.append(tc)
        self._update_queue_label()
        self._status(f"Added '{tc.title}' to queue ({len(self.app_state.queue)} total).")
        self._notify_queue_accepted()

    def _on_test_cases_queued(self, cases):
        if not self._check_duplicate_titles(cases):
            return
        self.app_state.queue.extend(cases)
        self._update_queue_label()
        self._status(
            f"Added {len(cases)} test case(s) from file ({len(self.app_state.queue)} total)."
        )
        self._notify_queue_accepted()

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

    def _check_duplicate_titles(self, incoming: list) -> bool:
        """Return True if it is safe to add `incoming` to the queue.

        When an incoming title already exists on the PBI in Azure DevOps, offer to
        update that work item (matched cases get their update_id set) instead of
        creating a duplicate. Titles that only collide with the current session
        queue fall back to a simple add-anyway confirmation."""
        # Cases that already carry an explicit work-item ID (e.g. re-imported from
        # a spreadsheet exported for bulk update) are deliberate updates — never
        # treat them as duplicates or re-match them by title.
        incoming = [tc for tc in incoming if not getattr(tc, "update_id", None)]
        if not incoming:
            return True

        # Map existing ADO title -> work item id (first match wins on collisions)
        ado_by_title: dict = {}
        for c in self._existing_cases_for_pbi():
            title = (c.get("System.Title", "") or "").lower()
            wid = c.get("_id")
            if title and wid and title not in ado_by_title:
                ado_by_title[title] = wid

        # Titles already sitting in the queue this session
        queue_titles = {tc.title.lower() for tc in self.app_state.queue}

        ado_dupes = [tc for tc in incoming if tc.title.lower() in ado_by_title]
        queue_dupes = [tc.title for tc in incoming if tc.title.lower() in queue_titles]

        if not ado_dupes and not queue_dupes:
            return True

        def _fmt(titles: list) -> str:
            lines = "\n".join(f"  • {t}" for t in titles[:10])
            if len(titles) > 10:
                lines += f"\n  … and {len(titles) - 10} more"
            return lines

        # Case 1 — some titles already exist on the PBI: offer Update vs Add-as-new.
        if ado_dupes:
            parts = [
                f"Already exist on PBI #{self.app_state.pbi_id} in Azure DevOps:\n"
                f"{_fmt([tc.title for tc in ado_dupes])}"
            ]
            if queue_dupes:
                parts.append(f"Already in your current queue:\n{_fmt(queue_dupes)}")

            box = QMessageBox(self)
            box.setIcon(QMessageBox.Question)
            box.setWindowTitle("Duplicate Titles Found")
            box.setText(
                "The following test case title(s) already exist:\n\n"
                + "\n\n".join(parts)
                + "\n\nUpdate the existing work item(s) with the values from your "
                "file (steps, tags, preconditions, module, automation status), "
                "or add them as new copies?"
            )
            update_btn = box.addButton(
                f"Update {len(ado_dupes)} Existing", QMessageBox.AcceptRole
            )
            box.addButton("Add as New", QMessageBox.DestructiveRole)
            cancel_btn = box.addButton("Cancel", QMessageBox.RejectRole)
            box.setDefaultButton(update_btn)
            box.exec_()

            clicked = box.clickedButton()
            if clicked is cancel_btn:
                return False
            if clicked is update_btn:
                for tc in ado_dupes:
                    tc.update_id = ado_by_title[tc.title.lower()]
            return True

        # Case 2 — duplicates only within the current session queue.
        reply = QMessageBox.question(
            self, "Duplicate Titles",
            "The following test case title(s) are already in your current queue:\n\n"
            + _fmt(queue_dupes)
            + "\n\nAdd to queue anyway?",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No,
        )
        return reply == QMessageBox.Yes

    def _update_queue_label(self):
        n = len(self.app_state.queue)
        self.queue_count_label.setText(f"{n} queued")
        color = "#0078d4" if n > 0 else "#aaa"
        self.queue_count_label.setStyleSheet(
            f"QLabel {{ background: {color}; color: white; border-radius: 10px; "
            f"padding: 2px 12px; font-weight: bold; font-size: 11px; }}"
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
    #  Auto-update (git pull from the private GitHub clone)                #
    # ------------------------------------------------------------------ #

    def _check_for_update(self):
        from app.utils import updater
        if not updater.update_supported():
            return
        worker = Worker(updater.check_for_update)
        worker.signals.result.connect(self._on_update_check_result)
        # Check failures (offline, VPN, …) are silent — the update check
        # must never disturb normal use.
        QThreadPool.globalInstance().start(worker)

    def _on_update_check_result(self, info):
        if not info:
            return
        self._update_info = info
        n = info["commits"]
        plural = "s" if n != 1 else ""
        self._update_btn.setText(f"⬆  Update available ({n} commit{plural})")
        self._update_btn.setVisible(True)
        reply = QMessageBox.question(
            self, "Update Available",
            f"A newer version is available on GitHub "
            f"({n} new commit{plural}).\n\n"
            f"Latest change: {info['latest']}\n\n"
            "Update and restart now? Any queued test cases are saved as a "
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
        self._update_btn.setEnabled(False)
        self._update_btn.setText("Updating…")
        worker = Worker(updater.apply_update)
        worker.signals.result.connect(self._on_update_applied)
        worker.signals.error.connect(self._on_update_failed)
        QThreadPool.globalInstance().start(worker)

    def _on_update_applied(self, _output):
        from app.utils import updater
        updater.start_new_instance()
        self.close()  # closeEvent saves the draft queue on the way out

    def _on_update_failed(self, exc: Exception):
        n = self._update_info["commits"]
        plural = "s" if n != 1 else ""
        self._update_btn.setEnabled(True)
        self._update_btn.setText(f"⬆  Update available ({n} commit{plural})")
        QMessageBox.warning(
            self, "Update Failed",
            f"The update could not be applied:\n\n{exc}\n\n"
            "You can keep using this version and try again later."
        )

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
            f"#headerFrame {{ background: {t['header_bg']}; border-bottom: 1px solid {t['border']}; }}"
        )
        self._footer_frame.setStyleSheet(
            f"#footerFrame {{ background: {t['footer_bg']}; border-top: 1px solid {t['border']}; }}"
        )
        self.main_header_label.setStyleSheet(f"color: {t['text_dim']}; font-size: 12px;")
        self.tabs.setStyleSheet(
            f"QTabWidget::pane {{ border: none; border-top: 1px solid {t['border']}; }} "
            f"QTabBar::tab {{ padding: 8px 18px; min-width: 140px; border: none; "
            f"border-bottom: 2px solid transparent; color: {t['text_dim2']}; "
            f"font-size: 17px; background: transparent; }} "
            f"QTabBar::tab:selected {{ color: {t['accent']}; font-weight: bold; "
            f"border-bottom: 2px solid {t['accent']}; }} "
            f"QTabBar::tab:hover:!selected {{ color: {t['text']}; "
            f"border-bottom: 2px solid {t['border']}; }} "
        )
        self.main_back_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; font-size: 13px; padding: 0 16px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
        )
        self.review_btn.setStyleSheet(
            theme.btn_primary_qss("border-radius: 4px; font-size: 13px; padding: 0 20px;")
        )
        self._import_count_label.setStyleSheet(f"color: {t['count_lbl_color']};")
