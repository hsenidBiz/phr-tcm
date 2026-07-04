"""A thin view over an Azure DevOps work item's field dict.

Work items come back from the batch GET as `{reference_name: value, "_id": id}`
dicts (same shape the Edit tab uses for test cases). `WorkItem` wraps one of
those, exposing the fields My Work needs and keeping the raw dict for
round-tripping edits. Board columns are derived from each state's *category*
(discovered per work-item type via the process), never hardcoded state names.
"""

from dataclasses import dataclass

# Fields My Work asks the batch GET for.
WORK_ITEM_FIELDS = [
    "System.Id", "System.Title", "System.WorkItemType", "System.State",
    "System.AssignedTo", "System.CreatedBy", "System.ChangedDate",
    "System.AreaPath", "System.IterationPath", "System.Tags", "System.Rev",
    "Microsoft.VSTS.Common.Priority",
    "Microsoft.VSTS.Scheduling.RemainingWork",
    "Microsoft.VSTS.Scheduling.CompletedWork",
]

# The three board columns, and the ADO state *category* that lands in each.
# (Categories are process-independent: every process tags its states with one.)
COLUMNS = ["To Do", "Doing", "Done"]
_CATEGORY_COLUMN = {
    "Proposed": "To Do",
    "InProgress": "Doing",
    # Resolved and Completed both land in Done — once an item is resolved or
    # later, it's off the active "Doing" work.
    "Resolved": "Done",
    "Completed": "Done",
    # "Removed" -> None (hidden)
}


def column_for_category(category: str):
    """Board column for a state category, or None to hide it (e.g. Removed)."""
    return _CATEGORY_COLUMN.get(category or "")


@dataclass
class WorkItem:
    fields: dict

    @property
    def id(self):
        return self.fields.get("_id")

    @property
    def title(self) -> str:
        return self.fields.get("System.Title", "") or ""

    @property
    def type(self) -> str:
        return self.fields.get("System.WorkItemType", "") or ""

    @property
    def state(self) -> str:
        return self.fields.get("System.State", "") or ""

    @property
    def rev(self):
        return self.fields.get("System.Rev")

    @property
    def priority(self):
        return self.fields.get("Microsoft.VSTS.Common.Priority")

    @property
    def tags(self) -> str:
        return self.fields.get("System.Tags", "") or ""

    @property
    def area_path(self) -> str:
        return self.fields.get("System.AreaPath", "") or ""

    @property
    def iteration_path(self) -> str:
        return self.fields.get("System.IterationPath", "") or ""

    @property
    def changed_date(self) -> str:
        return self.fields.get("System.ChangedDate", "") or ""

    @property
    def remaining_work(self):
        return self.fields.get("Microsoft.VSTS.Scheduling.RemainingWork")

    @property
    def completed_work(self):
        return self.fields.get("Microsoft.VSTS.Scheduling.CompletedWork")

    def identity(self, ref: str):
        """(displayName, uniqueName) for an identity field, or ('', '')."""
        v = self.fields.get(ref)
        if isinstance(v, dict):
            return v.get("displayName", "") or "", v.get("uniqueName", "") or ""
        return "", ""

    @property
    def assigned_to(self) -> str:
        return self.identity("System.AssignedTo")[0]

    @property
    def assigned_to_unique(self) -> str:
        return self.identity("System.AssignedTo")[1]

    def column(self, states_by_type: dict):
        """Board column for this item, using a {type: {state: category}} map from
        the process. A state literally named "Later" always lands in Done —
        parked/deferred items sit with the finished work, overriding whatever
        category the process assigns it. Falls back to a name heuristic when the
        type/state isn't in the map (unknown/custom process). Returns None to
        hide (Removed)."""
        name = self.state.strip().lower()
        if name == "later":
            return "Done"
        cat = (states_by_type.get(self.type) or {}).get(self.state)
        if cat:
            return column_for_category(cat)
        if name in ("new", "to do", "proposed", "open", "approved", "design"):
            return "To Do"
        if name == "removed":
            return None
        if name in ("done", "closed", "completed", "resolved"):
            return "Done"
        return "Doing"
