import threading

from PyQt5.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QPushButton,
    QProgressBar, QTextEdit, QDialog, QLineEdit, QDialogButtonBox,
    QMessageBox, QFrame
)
from PyQt5.QtCore import Qt, pyqtSignal, QThread, QObject
from PyQt5.QtGui import QFont, QColor, QTextCursor, QCursor

from app.api.devops_client import TokenExpiredError, RateLimitError


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
        self._abort = False

    def provide_token(self):
        """Called from the main thread after a new token is pasted."""
        self._token_event.set()

    def abort(self):
        self._abort = True
        self._token_event.set()

    def run(self):
        for i, tc in enumerate(self.queue):
            if self._abort:
                break
            while True:
                try:
                    tc_id = self.client.create_and_link(
                        tc, self.pbi_id, self.module_ref,
                        self.area_path, self.iteration_path,
                        self.preconditions_ref,
                    )
                    self.progress.emit(i, "success", f"✓ Created #{tc_id}: {tc.title}")
                    break
                except TokenExpiredError:
                    self.token_needed.emit()
                    self._token_event.wait()
                    self._token_event.clear()
                    if self._abort:
                        break
                    # Retry with the new token (client uses token_manager which was updated)
                except RateLimitError as exc:
                    import time
                    self.progress.emit(
                        i, "warning",
                        f"⏳ Rate limited — waiting {exc.retry_after}s before retrying '{tc.title}'…"
                    )
                    time.sleep(exc.retry_after)
                except Exception as exc:
                    self.progress.emit(i, "error", f"✗ Failed '{tc.title}': {exc}")
                    break

        self.finished.emit()


class TokenRefreshDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Token Expired")
        self.setModal(True)
        self.setMinimumWidth(500)
        layout = QVBoxLayout(self)
        layout.setSpacing(12)

        layout.addWidget(QLabel(
            "<b>Your Bearer token has expired.</b><br><br>"
            "Go to Azure DevOps in your browser, press <b>F12</b>, open the Network tab, "
            "click any request, and copy the value after <code>Bearer </code> in the "
            "Authorization header. Paste it below to continue."
        ))
        self.token_edit = QLineEdit()
        self.token_edit.setPlaceholderText("Paste new Bearer token here…")
        self.token_edit.setEchoMode(QLineEdit.Password)
        layout.addWidget(self.token_edit)

        btns = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        btns.accepted.connect(self._on_accept)
        btns.rejected.connect(self.reject)
        layout.addWidget(btns)

    def _on_accept(self):
        if not self.token_edit.text().strip():
            QMessageBox.warning(self, "Empty Token", "Please paste a token before clicking OK.")
            return
        self.accept()

    def get_token(self) -> str:
        return self.token_edit.text().strip()


class ProgressScreen(QWidget):
    all_done = pyqtSignal()  # emitted when creation is finished

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

        title = QLabel("Creating Test Cases…")
        font = QFont()
        font.setPointSize(16)
        font.setBold(True)
        title.setFont(font)
        self.title_label = title
        layout.addWidget(title)

        self.status_label = QLabel("Initialising…")
        layout.addWidget(self.status_label)

        self.progress_bar = QProgressBar()
        self.progress_bar.setMinimum(0)
        self.progress_bar.setTextVisible(True)
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
        self.done_btn = QPushButton("Done — Create Another Batch")
        self.done_btn.setFixedHeight(38)
        self.done_btn.setEnabled(False)
        self.done_btn.setStyleSheet(
            "QPushButton { background: #0078d4; color: white; border-radius: 4px; "
            "font-size: 13px; padding: 0 20px; }"
            "QPushButton:hover { background: #106ebe; }"
            "QPushButton:disabled { background: #aaa; }"
        )
        self.done_btn.clicked.connect(self.all_done)
        btn_row.addWidget(self.done_btn)
        layout.addLayout(btn_row)

    def refresh_theme(self):
        pass  # terminal log stays dark in both modes; palette handles other elements

    def start(self):
        """Begin the creation process. Called when this screen becomes active."""
        queue = list(self.app_state.queue)
        n = len(queue)

        self.title_label.setText("Creating Test Cases…")
        self.progress_bar.setMaximum(n)
        self.progress_bar.setValue(0)
        self.log.clear()
        self.result_label.setText("")
        self.done_btn.setEnabled(False)
        self.status_label.setText(f"Creating {n} test case{'s' if n != 1 else ''}…")

        self._success_count = 0
        self._error_count = 0
        self._total = n

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
        self._thread.start()

    def _on_progress(self, index: int, status: str, message: str):
        self.progress_bar.setValue(index + 1)
        self.status_label.setText(f"Processing {index + 1} / {self._total}…")

        colors = {"success": "#4ec94e", "error": "#f14c4c", "warning": "#e5c07b"}
        color = colors.get(status, "#d4d4d4")

        cursor = self.log.textCursor()
        cursor.movePosition(QTextCursor.End)
        self.log.setTextCursor(cursor)
        self.log.append(f'<span style="color:{color};">{message}</span>')

        if status == "success":
            self._success_count += 1
        elif status == "error":
            self._error_count += 1

    def _on_token_needed(self):
        """Pause the worker and ask user for a fresh token."""
        dlg = TokenRefreshDialog(self)
        if dlg.exec_() == TokenRefreshDialog.Accepted:
            new_token = dlg.get_token()
            self.app_state.token_manager.update_token(new_token)
            self._worker.provide_token()
        else:
            # User cancelled — abort remaining items
            self._worker.abort()
            self.log.append('<span style="color:#f14c4c;">⚠ Creation aborted by user (token refresh cancelled).</span>')

    def _on_finished(self):
        n_ok = self._success_count
        n_err = self._error_count
        n_skip = self._total - n_ok - n_err

        self.title_label.setText("Done")
        self.progress_bar.setValue(self._total)
        self.done_btn.setEnabled(True)

        if n_err == 0:
            self.status_label.setStyleSheet("color: #080;")
            self.status_label.setText(f"All {n_ok} test case{'s' if n_ok != 1 else ''} created successfully.")
        else:
            self.status_label.setStyleSheet("color: #c00;")
            self.status_label.setText(
                f"{n_ok} created, {n_err} failed"
                + (f", {n_skip} skipped" if n_skip else "") + "."
            )

        self.result_label.setText(
            f"Check Azure DevOps to verify that the test cases appear under PBI #{self.app_state.pbi_id}. "
            "Open the PBI and look for the 'Tests' / 'Tested By' links section."
        )
        # Clear queue now that we're done
        self.app_state.queue.clear()
