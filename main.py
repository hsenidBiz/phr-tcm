import os
import sys
from PyQt5.QtWidgets import QApplication
from PyQt5.QtGui import QFont, QIcon
from PyQt5.QtCore import Qt

from app.auth.token_manager import TokenManager
from app.api.devops_client import DevOpsClient
from app.gui.main_window import MainWindow
from app.models.test_case import TestCase


class AppState:
    """
    Central application state shared across all screens.
    Nothing in this object is written to disk.
    """

    def __init__(self):
        self.token_manager = TokenManager()
        self.client = DevOpsClient(self.token_manager)

        self.pbi_id: int | None = None
        self.pbi_title: str = ""
        self.module_ref: str | None = None
        self.preconditions_ref: str | None = None
        self.area_path: str = ""
        self.iteration_path: str = ""

        # Test plan / requirement-based suite for the selected PBI. Resolved for
        # display on the Config/Review screens and (re)created by the progress
        # worker so created test cases show on the board. test_plan_pbi records
        # which PBI these were resolved for.
        self.test_plan_id: int | None = None
        self.test_plan_name: str = ""
        self.suite_id: int | None = None
        self.test_plan_pbi: int | None = None
        # True while the (cold) test plan/suite discovery is running for the
        # current PBI; test_plan_progress is (current, total) plan-scan progress
        # so screens can show a loading indicator while it resolves.
        self.test_plan_detecting: bool = False
        self.test_plan_progress: tuple | None = None

        # Pending test cases waiting to be created
        self.queue: list[TestCase] = []

        # Module values discovered from the loaded PBI (populated by EditScreen)
        self.known_module_values: list[str] = []

        # Existing Test Cases on the current PBI — shared by the Edit and Import
        # tabs for duplicate detection and update-on-import. Each entry is a field
        # dict with an '_id' key. existing_cases_pbi records which PBI they belong to.
        self.existing_cases: list[dict] = []
        self.existing_cases_pbi: int | None = None

        # Session cache of a suite's test points (last outcome + result ids),
        # keyed by (test_plan_id, suite_id). Pre-fetched by the Run Tests tab so
        # the runner can show previous outcomes instantly; invalidated after a
        # run is submitted so re-opening reflects the just-recorded results.
        self.test_points_by_suite: dict = {}

        # Team members: in-memory cache + shared in-flight fetcher (set by members_cache)
        self.cached_team_members: list | None = None
        self._team_members_fetcher = None  # TeamMemberFetcher | None


def _enable_high_dpi():
    """Enable Qt high-DPI scaling. Must run BEFORE the QApplication is created.

    Qt 5 does not scale the UI by monitor DPI by default, so on 4K / display-
    scaled monitors — and when the window moves between monitors with different
    scale factors — text and widgets render at the wrong physical size. These
    attributes make Qt scale the whole UI (including px-based stylesheet font
    sizes and fixed widget sizes) by each screen's scale factor.
    """
    if hasattr(Qt, "AA_EnableHighDpiScaling"):
        QApplication.setAttribute(Qt.AA_EnableHighDpiScaling, True)
    if hasattr(Qt, "AA_UseHighDpiPixmaps"):
        QApplication.setAttribute(Qt.AA_UseHighDpiPixmaps, True)
    # Qt 5.14+: keep fractional scale factors (e.g. 125%, 150%) instead of
    # rounding them to whole numbers, so per-monitor scaling stays accurate.
    try:
        QApplication.setHighDpiScaleFactorRoundingPolicy(
            Qt.HighDpiScaleFactorRoundingPolicy.PassThrough
        )
    except (AttributeError, TypeError):
        pass


def _run_velopack_startup():
    """Velopack lifecycle hook — handles install / update / uninstall events for
    the packaged build. Must run before any GUI work (it may relaunch or exit the
    process during those events). Only the frozen (installed) build participates;
    running from source returns immediately to avoid the native 'not installed'
    noise.
    """
    if not getattr(sys, "frozen", False):
        return
    try:
        import velopack
        velopack.App().run()
    except Exception:
        pass


def _resource_path(rel: str) -> str:
    """Resolve a bundled resource path for both source runs and the frozen build."""
    base = getattr(sys, "_MEIPASS", None) or os.path.dirname(os.path.abspath(__file__))
    return os.path.join(base, rel)


def main():
    _run_velopack_startup()
    _enable_high_dpi()

    # Silence the benign Qt clipboard-contention warnings ("qt.qpa.mime: Retrying
    # to obtain clipboard" / "Unable to obtain clipboard"). Qt's clipboard monitor
    # races another process (e.g. Windows Clipboard History / a clipboard manager)
    # for the Windows clipboard lock; it retries and the app works fine — it's
    # just console noise. (The old token clipboard auto-detect was removed when
    # MSAL sign-in became the only auth, so this is not from our code.)
    _rules = os.environ.get("QT_LOGGING_RULES", "")
    os.environ["QT_LOGGING_RULES"] = (_rules + ";" if _rules else "") + "qt.qpa.mime=false"

    app = QApplication(sys.argv)
    app.setApplicationName("Azure DevOps Test Case Manager")
    app.setOrganizationName("Internal Tool")

    # App icon for the taskbar / alt-tab / windows. The frozen .exe + installer
    # icon come from the same resources/icon.ico (spec icon= / build.ps1 --icon).
    _icon = _resource_path(os.path.join("resources", "icon.ico"))
    if os.path.exists(_icon):
        app.setWindowIcon(QIcon(_icon))

    # Native, highly-readable Windows UI font. 11pt (up from 10) gives more
    # comfortable body/table/form text; chrome with explicit pixel sizes (tabs,
    # captions, the title bar) is unaffected.
    font = QFont("Segoe UI", 11)
    font.setStyleStrategy(QFont.PreferAntialias)
    app.setFont(font)

    # Light style polish
    app.setStyle("Fusion")

    state = AppState()
    window = MainWindow(state)
    window.show()

    sys.exit(app.exec_())


if __name__ == "__main__":
    main()
