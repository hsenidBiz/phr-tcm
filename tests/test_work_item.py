"""WorkItem.column() — how My Work groups items into board columns.

Qt-free logic, so it runs under plain pytest (no offscreen Qt needed).
"""

from app.models.work_item import WORK_ITEM_FIELDS, WorkItem, column_for_category


def test_original_estimate_and_activity_properties():
    wi = WorkItem({
        "_id": 1,
        "Microsoft.VSTS.Scheduling.OriginalEstimate": 8.0,
        "Microsoft.VSTS.Common.Activity": "Development",
    })
    assert wi.original_estimate == 8.0
    assert wi.activity == "Development"
    # Absent fields degrade cleanly.
    blank = WorkItem({"_id": 2})
    assert blank.original_estimate is None
    assert blank.activity == ""


def test_scheduling_fields_are_requested():
    assert "Microsoft.VSTS.Scheduling.OriginalEstimate" in WORK_ITEM_FIELDS
    assert "Microsoft.VSTS.Common.Activity" in WORK_ITEM_FIELDS

# A minimal process map: {type: {state: category}}. Real one comes from
# DevOpsClient.get_work_item_states(); the shape is all column() cares about.
STATES = {
    "Task": {
        "To Do": "Proposed",
        "Doing": "InProgress",
        "Done": "Completed",
        "Removed": "Removed",
        # A custom parked state whose process category is Proposed.
        "Later": "Proposed",
    },
    "Bug": {
        "New": "Proposed",
        "Active": "InProgress",
        "Resolved": "Resolved",
        "Closed": "Completed",
    },
}


def _wi(state, wtype="Task"):
    return WorkItem({"_id": 1, "System.WorkItemType": wtype, "System.State": state})


def test_category_mapping():
    assert _wi("To Do").column(STATES) == "To Do"
    assert _wi("Doing").column(STATES) == "Doing"
    assert _wi("Done").column(STATES) == "Done"


def test_resolved_category_lands_in_done():
    # Resolved and Completed both belong to Done (once resolved, off "Doing").
    assert _wi("Resolved", "Bug").column(STATES) == "Done"
    assert _wi("Closed", "Bug").column(STATES) == "Done"


def test_removed_is_hidden():
    assert _wi("Removed").column(STATES) is None


def test_later_state_forced_to_done_over_category():
    # "Later" is categorised Proposed in the process (would normally be To Do),
    # but a state literally named "Later" is parked with the finished work.
    assert column_for_category("Proposed") == "To Do"
    assert _wi("Later").column(STATES) == "Done"


def test_later_is_case_and_whitespace_insensitive():
    assert _wi("later").column(STATES) == "Done"
    assert _wi("  Later  ").column(STATES) == "Done"


def test_name_heuristic_when_state_absent_from_process_map():
    # Unknown type/state falls back to the name heuristic.
    unknown = WorkItem({"_id": 2, "System.WorkItemType": "Epic", "System.State": "New"})
    assert unknown.column(STATES) == "To Do"
    assert WorkItem(
        {"_id": 3, "System.WorkItemType": "Epic", "System.State": "Closed"}
    ).column(STATES) == "Done"
    assert WorkItem(
        {"_id": 4, "System.WorkItemType": "Epic", "System.State": "Later"}
    ).column(STATES) == "Done"
