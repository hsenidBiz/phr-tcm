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

    def is_valid(self) -> tuple:
        """Returns (bool, error_message). True if ready to submit."""
        if not self.title.strip():
            return False, "Title is required."
        if not self.steps:
            return False, "At least one step is required."
        for i, step in enumerate(self.steps):
            if not step.action.strip():
                return False, f"Step {i + 1} action is empty."
        if self.automation_status not in ("Not Automated", "Planned"):
            return False, f"Invalid automation status: {self.automation_status!r}"
        return True, ""
