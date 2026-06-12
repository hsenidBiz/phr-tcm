import threading

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton,
    QProgressBar, QTextEdit, QMessageBox
)
from PyQt5.QtCore import Qt, pyqtSignal, QThread, QObject, QThreadPool
from PyQt5.QtGui import QFont, QTextCursor, QCursor

from app.api.devops_client import TokenExpiredError, RateLimitError
from app.utils.anim import Spinner
from app.utils.worker import Worker


class CreationWorker(QObject):
    """Runs in a QThread. Emits signals to update the UI."""

    progress = pyqtSignal(int, str, str)   # (index, "success"/"error", message)
    token_needed = pyqtSignal()            # emitted when 401 is received
    finished = pyqtSignal()

    def __init__(self, client, queue, pbi_id, module_ref, area_path="", iteration_path="", preconditions_ref=None):
        super().__init__()
        self.client = client
        self.queue = queue
        self.pbi_id = pbi_id
        self.module_ref = module_ref
        self.area_path = area_path
        self.iteration_path = iteration_path
        self.preconditions_ref = preconditions_ref
        self._token_event = threading.Event()
        self._abort_event = threading.Event()

    @property
    def _abort(self) -> bool:
        return self._abort_event.is_set()

    def provide_token(self):
        """Called from the main thread after the user re-signs in."""
        self._token_event.set()

    def abort(self):
        self._abort_event.set()
        self._token_event.set()

    def run(self):
        for i, tc in enumerate(self.queue):
            if self._abort:
                break
            # tc_id is set once creation succeeds, so token/rate-limit retries
            # only repeat the link step — never a second (duplicate) create.
            tc_id = None
            while True:
                if self._abort:
                    break
                try:
                    if tc.update_id:
                        self.client.update_test_case_from_model(
                            tc.update_id, tc, self.module_ref, self.preconditions_ref,
                        )
                        self.progress.emit(i, "success", f"✎ Updated #{tc.update_id}: {tc.title}")
                    else:
                        if tc_id is None:
                            tc_id = self.client.create_test_case(
                                tc, self.module_ref,
                                self.area_path, self.iteration_path,
                                self.preconditions_ref,
                            )
                            self._abort_event.wait(0.5)  # pacing between create and link
                        self.client.link_to_pbi(tc_id, self.pbi_id)
                        self.progress.emit(i, "success", f"✓ Created #{tc_id}: {tc.title}")
                    break
                except TokenExpiredError:
                    self.token_needed.emit()
                    self._token_event.wait()
                    self._token_event.clear()
                    # Retry with the new token (client uses token_manager which was updated)
                except RateLimitError as exc:
                    self.progress.emit(
                        i, "warning",
                        f"⏳ Rate limited — waiting {exc.retry_after}s before retrying '{tc.title}'…"
                    )
                    self._abort_event.wait(exc.retry_after)
                except Exception as exc:
                    if tc_id is not None:
                        # "partial": the work item exists in DevOps, so this case
                        # must NOT be retried from the queue (it would duplicate).
                        self.progress.emit(
                            i, "partial",
                            f"✗ Created #{tc_id} but failed to link '{tc.title}' to the PBI: {exc}. "
                            f"Link it manually in Azure DevOps."
                        )
                    else:
                        self.progress.emit(i, "error", f"✗ Failed '{tc.title}': {exc}")
                    break

        self.finished.emit()


class ProgressScreen(QWidget):
    all_done = pyqtSignal()          # emitted when creation is finished

    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        self._worker = None
        self._thread = None
        self._build_ui()

    def _build_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(40, 30, 40, 30)
        layout.setSpacing(14)

        title_row = QHBoxLayout()
        title_row.setSpacing(10)
        self.title_label = QLabel("Creating Test Cases…")
        font = QFont()
        font.setPointSize(16)
        font.setBold(True)
        self.title_label.setFont(font)
        title_row.addWidget(self.title_label)
        self._spinner = Spinner(size=26)
        self._spinner.setVisible(False)
        title_row.addWidget(self._spinner)
        title_row.addStretch()
        layout.addLayout(title_row)

        self.status_label = QLabel("Initialising…")
        layout.addWidget(self.status_label)

        self.progress_bar = QProgressBar()
        self.progress_bar.setMinimum(0)
        self.progress_bar.setTextVisible(False)
        self.progress_bar.setStyleSheet(
            "QProgressBar { border: none; border-radius: 5px; background: #e5e5e5; "
            "min-height: 10px; max-height: 10px; } "
            "QProgressBar::chunk { border-radius: 5px; "
            "background: qlineargradient(x1:0, y1:0, x2:1, y2:0, "
            "stop:0 #0078d4, stop:1 #00b0ff); }"
        )
        layout.addWidget(self.progress_bar)

        # Log area
        self.log = QTextEdit()
        self.log.setReadOnly(True)
        self.log.setFont(QFont("Consolas", 10))
        self.log.setStyleSheet(
            "QTextEdit { background: #1e1e1e; color: #d4d4d4; border-radius: 4px; }"
        )
        layout.addWidget(self.log)

        # Result summary
        self.result_label = QLabel("")
        self.result_label.setWordWrap(True)
        self.result_label.setStyleSheet("font-size: 13px;")
        layout.addWidget(self.result_label)

        btn_row = QHBoxLayout()
        btn_row.addStretch()

        self.cancel_btn = QPushButton("Cancel")
        self.cancel_btn.setFixedHeight(38)
        self.cancel_btn.setEnabled(False)
        self.cancel_btn.setStyleSheet(
            "QPushButton { background: #f0f0f0; border: 1px solid #ccc; border-radius: 4px; "
            "font-size: 13px; padding: 0 16px; }"
            "QPushButton:hover { background: #e0e0e0; }"
            "QPushButton:disabled { color: #aaa; }"
        )
        self.cancel_btn.setCursor(QCursor(Qt.PointingHandCursor))
        self.cancel_btn.clicked.connect(self._on_cancel)
        btn_row.addWidget(self.cancel_btn)
        btn_row.addSpacing(8)

        from app.utils import theme
        self.done_btn = QPushButton("Done — Create Another Batch")
        self.done_btn.setFixedHeight(38)
        self.done_btn.setEnabled(False)
        self.done_btn.setStyleSheet(
            theme.btn_primary_qss("border-radius: 4px; font-size: 13px; padding: 0 20px;")
        )
        self.done_btn.clicked.connect(self.all_done)
        btn_row.addWidget(self.done_btn)
        layout.addLayout(btn_row)

    def refresh_theme(self):
        from app.utils import theme
        t = theme.tokens()
        track = "#3a3a3a" if theme.is_dark() else "#e5e5e5"
        self.progress_bar.setStyleSheet(
            f"QProgressBar {{ border: none; border-radius: 5px; background: {track}; "
            f"min-height: 10px; max-height: 10px; }} "
            f"QProgressBar::chunk {{ border-radius: 5px; "
            f"background: qlineargradient(x1:0, y1:0, x2:1, y2:0, "
            f"stop:0 #0078d4, stop:1 #00b0ff); }}"
        )
        self._spinner.set_color(t["accent"])
        self.cancel_btn.setStyleSheet(
            f"QPushButton {{ background: {t['btn_bg']}; border: 1px solid {t['btn_border']}; "
            f"border-radius: 4px; font-size: 13px; padding: 0 16px; }}"
            f"QPushButton:hover {{ background: {t['btn_hover']}; }}"
            f"QPushButton:disabled {{ color: {t['text_dim2']}; }}"
        )
        self.done_btn.setStyleSheet(
            theme.btn_primary_qss("border-radius: 4px; font-size: 13px; padding: 0 20px;")
        )

    @staticmethod
    def _progress_phrase(n_creates: int, n_updates: int) -> str:
        """Human phrasing for an in-progress batch of creates and/or updates."""
        def _s(k: int) -> str:
            return "s" if k != 1 else ""
        if n_updates and n_creates:
            return f"Creating {n_creates} and updating {n_updates} test case{_s(n_creates + n_updates)}"
        if n_updates:
            return f"Updating {n_updates} test case{_s(n_updates)}"
        return f"Creating {n_creates} test case{_s(n_creates)}"

    def start(self):
        """Begin the creation process. Called when this screen becomes active."""
        queue = list(self.app_state.queue)
        n = len(queue)
        self._n_updates = sum(1 for tc in queue if tc.update_id)
        self._n_creates = n - self._n_updates

        verb = "Updating" if self._n_updates and not self._n_creates else "Creating"
        self.title_label.setText(f"{verb} Test Cases…")
        self.progress_bar.setMaximum(n)
        self.progress_bar.setValue(0)
        self.log.clear()
        self.result_label.setText("")
        self.done_btn.setEnabled(False)
        self.status_label.setText(self._progress_phrase(self._n_creates, self._n_updates) + "…")
        self._spinner.start()

        self.cancel_btn.setEnabled(True)
        self.cancel_btn.setText("Cancel")

        self._success_count = 0
        self._error_count = 0
        self._total = n
        # Indices that must NOT stay in the queue afterwards: fully created,
        # or created-but-unlinked (retrying those would duplicate the work item).
        self._consumed_indices = set()

        self._worker = CreationWorker(
            self.app_state.client,
            queue,
            self.app_state.pbi_id,
            self.app_state.module_ref,
            self.app_state.area_path,
            self.app_state.iteration_path,
            self.app_state.preconditions_ref,
        )
        self._thread = QThread()
        self._worker.moveToThread(self._thread)
        self._thread.started.connect(self._worker.run)
        self._worker.progress.connect(self._on_progress)
        self._worker.token_needed.connect(self._on_token_needed)
        self._worker.finished.connect(self._on_finished)
        self._worker.finished.connect(self._thread.quit)
        self._worker.finished.connect(self._worker.deleteLater)
        self._thread.finished.connect(self._clear_thread_refs)
        self._thread.finished.connect(self._thread.deleteLater)
        self._thread.start()

    def _clear_thread_refs(self):
        self._worker = None
        self._thread = None

    def is_running(self) -> bool:
        """True while a creation batch is still in progress."""
        try:
            return self._thread is not None and self._thread.isRunning()
        except RuntimeError:  # C++ object already deleted via deleteLater
            return False

    def shutdown(self):
        """Abort any running batch and wait for the thread (called on app close)."""
        if self.is_running():
            if self._worker:
                self._worker.abort()
            self._thread.quit()
            self._thread.wait(5000)

    def _on_progress(self, index: int, status: str, message: str):
        self.progress_bar.setValue(index + 1)
        self.status_label.setText(f"Processing {index + 1} / {self._total}…")

        colors = {"success": "#4ec94e", "error": "#f14c4c", "partial": "#f14c4c", "warning": "#e5c07b"}
        color = colors.get(status, "#d4d4d4")

        cursor = self.log.textCursor()
        cursor.movePosition(QTextCursor.End)
        self.log.setTextCursor(cursor)
        self.log.append(f'<span style="color:{color};">{message}</span>')

        if status == "success":
            self._success_count += 1
            self._consumed_indices.add(index)
        elif status == "partial":
            self._error_count += 1
            self._consumed_indices.add(index)
        elif status == "error":
            self._error_count += 1

    def _on_cancel(self):
        if self._worker:
            self._worker.abort()
        self.cancel_btn.setEnabled(False)
        self.cancel_btn.setText("Cancelling…")

    def _on_token_needed(self):
        """Pause the worker and re-authenticate through Microsoft sign-in."""
        msal = self.app_state.token_manager.msal_authenticator
        reply = QMessageBox.question(
            self, "Session Expired",
            "Your Azure DevOps session has expired.\n\n"
            "Sign in again with Microsoft to continue creating the remaining test cases?",
            QMessageBox.Yes | QMessageBox.No,
        )
        if reply != QMessageBox.Yes or msal is None:
            self._worker.abort()
            self.log.append('<span style="color:#f14c4c;">⚠ Creation aborted (sign-in declined).</span>')
            return
        self.log.append('<span style="color:#e5c07b;">⏳ Waiting for browser sign-in…</span>')
        worker = Worker(msal.sign_in_interactive)
        worker.signals.result.connect(self._on_reauth_token)
        worker.signals.error.connect(self._on_reauth_error)
        QThreadPool.globalInstance().start(worker)

    def _on_reauth_token(self, token: str):
        self.app_state.token_manager.update_token(token)
        self.log.append('<span style="color:#3fb950;">✓ Signed in — resuming…</span>')
        self._worker.provide_token()

    def _on_reauth_error(self, exc: Exception):
        self._worker.abort()
        self.log.append(
            f'<span style="color:#f14c4c;">⚠ Sign-in failed ({exc}) — creation aborted. '
            f'Unprocessed cases stay in the queue.</span>'
        )

    def _on_finished(self):
        n_ok = self._success_count
        n_err = self._error_count
        n_skip = self._total - n_ok - n_err

        self._spinner.stop()
        self.cancel_btn.setEnabled(False)
        self.cancel_btn.setText("Cancel")
        self.title_label.setText("Done")
        self.progress_bar.setValue(self._total)
        self.done_btn.setEnabled(True)

        if self._n_updates and not self._n_creates:
            done_verb = "updated"
        elif self._n_updates:
            done_verb = "processed"
        else:
            done_verb = "created"

        from app.utils import theme
        t = theme.tokens()
        if n_err == 0 and n_skip == 0:
            self.status_label.setStyleSheet(f"color: {t['ok']};")
            self.status_label.setText(
                f"All {n_ok} test case{'s' if n_ok != 1 else ''} {done_verb} successfully."
            )
        else:
            self.status_label.setStyleSheet(f"color: {t['error']};")
            self.status_label.setText(
                f"{n_ok} {done_verb}, {n_err} failed"
                + (f", {n_skip} skipped" if n_skip else "") + "."
            )

        # Keep failed/skipped cases in the queue so they can be fixed and retried.
        # Successful and created-but-unlinked items are removed (re-running them
        # would create duplicates in Azure DevOps).
        remaining = [
            tc for i, tc in enumerate(self.app_state.queue)
            if i not in self._consumed_indices
        ]
        self.app_state.queue[:] = remaining

        from app.utils.settings import clear_draft_queue, save_draft_queue
        if remaining:
            save_draft_queue(remaining)
            self.done_btn.setText(f"Done — {len(remaining)} unprocessed case{'s' if len(remaining) != 1 else ''} kept in queue")
            self.result_label.setText(
                f"{len(remaining)} case{'s were' if len(remaining) != 1 else ' was'} not created and "
                "remain in the queue — review them and run Create again to retry. "
                f"Verify created test cases under PBI #{self.app_state.pbi_id} in Azure DevOps."
            )
        else:
            clear_draft_queue()
            self.done_btn.setText("Done — Create Another Batch")
            self.result_label.setText(
                f"Check Azure DevOps to verify that the test cases appear under PBI #{self.app_state.pbi_id}. "
                "Open the PBI and look for the 'Tests' / 'Tested By' links section."
            )

        # The existing-case cache is now stale (items were created/updated) —
        # force a fresh load next time the Import or Edit tab needs it so that
        # duplicate detection stays accurate across consecutive batches.
        self.app_state.existing_cases = []
        self.app_state.existing_cases_pbi = None
