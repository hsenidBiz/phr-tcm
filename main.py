import sys
from PyQt5.QtWidgets import QApplication
from PyQt5.QtGui import QFont

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

        # Pending test cases waiting to be created
        self.queue: list[TestCase] = []

        # Module values discovered from the loaded PBI (populated by EditScreen)
        self.known_module_values: list[str] = []

        # Existing Test Cases on the current PBI — shared by the Edit and Import
        # tabs for duplicate detection and update-on-import. Each entry is a field
        # dict with an '_id' key. existing_cases_pbi records which PBI they belong to.
        self.existing_cases: list[dict] = []
        self.existing_cases_pbi: int | None = None

        # Team members: in-memory cache + shared in-flight fetcher (set by members_cache)
        self.cached_team_members: list | None = None
        self._team_members_fetcher = None  # TeamMemberFetcher | None


def main():
    app = QApplication(sys.argv)
    app.setApplicationName("Azure DevOps Test Case Creator")
    app.setOrganizationName("Internal Tool")

    # Set a clean default font
    font = QFont("Segoe UI", 10)
    app.setFont(font)

    # Light style polish
    app.setStyle("Fusion")

    state = AppState()
    window = MainWindow(state)
    window.show()

    sys.exit(app.exec_())


if __name__ == "__main__":
    main()
