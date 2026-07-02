from dataclasses import dataclass, field


@dataclass
class Step:
    action: str
    expected: str = ""


@dataclass
class TestCase:
    title: str
    steps: list[Step] = field(default_factory=list)
    tags: str = ""                              # semicolon-separated
    automation_status: str = "Not Automated"   # "Not Automated" or "Planned"
    module_value: str = ""
    preconditions: str = ""
    created_by: str = ""                        # user identifier (e.g. "user@domain.com")
    update_id: int | None = None                # when set, update this existing work item instead of creating a new one

    # Azure DevOps rejects System.Title values longer than 255 characters.
    MAX_TITLE_LEN = 255

    def is_valid(self) -> tuple:
        """Returns (bool, error_message). True if ready to submit."""
        title = self.title.strip()
        if not title:
            return False, "Title is required."
        if len(title) > self.MAX_TITLE_LEN:
            return False, (
                f"Title is {len(title)} characters — Azure DevOps allows at most "
                f"{self.MAX_TITLE_LEN}."
            )
        if not self.steps:
            return False, "At least one step is required."
        for i, step in enumerate(self.steps):
            if not step.action.strip():
                return False, f"Step {i + 1} action is empty."
        if self.automation_status not in ("Not Automated", "Planned"):
            return False, f"Invalid automation status: {self.automation_status!r}"
        if "," in self.tags:
            return False, (
                "Tags must be separated with semicolons — Azure DevOps does not "
                "allow commas in tag names."
            )
        return True, ""
