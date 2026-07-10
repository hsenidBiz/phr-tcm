"""Demo mode — a fully offline stand-in for DevOpsClient + TokenManager.

When demo mode is enabled in Settings the app swaps its real Azure DevOps client
and token manager for these, so every screen is populated with plausible fake
data and NOTHING ever touches the network. No sign-in is required. The demo
client subclasses DevOpsClient (so `isinstance`/`.tm`/pure helpers keep working)
but overrides every method that would otherwise make an HTTP call, returning
canned data in exactly the shapes the real client produces.

Writes (create/update/link/run-submit) are accepted and no-op'd with fake ids, so
the create and test-run flows complete end-to-end without leaving the machine."""
from datetime import datetime, timedelta

from app.api.devops_client import DevOpsClient
from app.auth.token_manager import TokenManager
from app.models.test_case import Step
from app.utils.xml_builder import build_steps_xml

DEMO_ORG_URL = "https://dev.azure.com/contoso-demo"
DEMO_ORG_NAME = "contoso-demo"
DEMO_PROJECT = "Contoso Web"
DEMO_UPN = "demo.user@contoso.com"
DEMO_NAME = "Demo User"

DEMO_PBI_ID = 1042
DEMO_PBI_TITLE = "Checkout flow improvements"
DEMO_AREA = "Contoso Web\\Checkout"
DEMO_ITERATION = "Contoso Web\\Sprint 24"
DEMO_MODULE_REF = "Custom.ModuleValue"
DEMO_PRECOND_REF = "Custom.Preconditions"
DEMO_PLAN_ID = 200
DEMO_PLAN_NAME = "Checkout - Test Plan"
DEMO_ROOT_SUITE_ID = 201
DEMO_SUITE_ID = 210


def _ident(name, upn):
    return {"displayName": name, "uniqueName": upn, "id": upn, "imageUrl": ""}


_DEMO_USER = _ident(DEMO_NAME, DEMO_UPN)
_PRIYA = _ident("Priya Shah", "priya.shah@contoso.com")
_MARCO = _ident("Marco Rossi", "marco.rossi@contoso.com")


def _iso(days_ago):
    return (datetime(2026, 7, 1, 9, 0, 0) - timedelta(days=days_ago)).strftime(
        "%Y-%m-%dT%H:%M:%SZ")


class DemoTokenManager(TokenManager):
    """A token manager that always looks signed-in, with no real token."""

    def __init__(self):
        super().__init__()
        self.set_org_project(DEMO_ORG_URL, DEMO_PROJECT)

    def auto_refresh_active(self) -> bool:
        return True

    def is_expired(self) -> bool:
        return False

    def is_likely_expired(self) -> bool:
        return False

    def get_current_upn(self):
        return DEMO_UPN

    def get_expiry_display(self) -> str:
        return "Demo mode — no sign-in"

    def get_json_headers(self) -> dict:
        return {"Accept": "application/json"}

    def get_patch_headers(self) -> dict:
        return {"Accept": "application/json"}


# Test cases linked to the demo PBI (id, title, tags, automation, steps, outcome).
_TC_SPECS = [
    (5001, "Guest can add an item to the cart", "checkout;smoke", "Not Automated",
     [("Open the product page", "Product details are shown"),
      ("Click 'Add to cart'", "Item appears in the mini-cart with qty 1")], "Passed"),
    (5002, "Apply a valid discount code at checkout", "checkout;regression", "Planned",
     [("Add an item and go to checkout", "Checkout summary is shown"),
      ("Enter code SAVE10 and apply", "10% discount is deducted from the total")], "Failed"),
    (5003, "Reject an expired discount code", "checkout;regression", "Not Automated",
     [("Go to checkout with an item", "Order summary is shown"),
      ("Enter an expired code", "An 'code expired' error is shown, total unchanged")], "Passed"),
    (5004, "Pay with a saved credit card", "checkout;payments", "Not Automated",
     [("Proceed to payment", "Saved cards are listed"),
      ("Select a saved card and confirm", "Order is placed and confirmation shown")], "Blocked"),
    (5005, "Empty cart shows the empty state", "cart", "Not Automated",
     [("Remove all items from the cart", "Cart shows the empty-state message")], ""),
    (5006, "Checkout is blocked when out of stock", "checkout;inventory", "Planned",
     [("Add an out-of-stock item", "Item is flagged out of stock"),
      ("Attempt to checkout", "Checkout is blocked with an explanatory message")], "Passed"),
]

# My Work board items: (id, type, title, state, assignee, priority, remaining, completed, tags)
_WI_SPECS = [
    (3001, "Bug", "Discount code field loses focus on mobile", "Active", _DEMO_USER, 1, 3.0, 2.0, "checkout;mobile"),
    (3002, "Task", "Add telemetry for failed payments", "Active", _DEMO_USER, 2, 5.0, 1.0, ""),
    (3003, "Task", "Write regression tests for the cart", "New", _DEMO_USER, 2, 8.0, 0.0, "testing"),
    (3004, "Bug", "Totals mismatch with multiple discounts", "New", _DEMO_USER, 1, 4.0, 0.0, "checkout"),
    (3005, "User Story", "Remember the last used payment method", "Resolved", _DEMO_USER, 3, 0.0, 6.0, ""),
    (3006, "Task", "Refactor the checkout state machine", "Closed", _DEMO_USER, 3, 0.0, 12.0, "tech-debt"),
    (3007, "Bug", "Slow cart load on large baskets", "Active", _PRIYA, 2, 6.0, 3.0, "performance"),
    (3008, "Task", "Update the payments SDK", "New", _MARCO, 2, 4.0, 0.0, "dependencies"),
]

# ADO state -> category per work-item type (drives the My Work board columns).
_STATES = {
    "Bug": [("New", "#b2b2b2", "Proposed"), ("Active", "#007acc", "InProgress"),
            ("Resolved", "#ff9d00", "Resolved"), ("Closed", "#339933", "Completed")],
    "Task": [("New", "#b2b2b2", "Proposed"), ("Active", "#007acc", "InProgress"),
             ("Closed", "#339933", "Completed")],
    "User Story": [("New", "#b2b2b2", "Proposed"), ("Active", "#007acc", "InProgress"),
                   ("Resolved", "#ff9d00", "Resolved"), ("Closed", "#339933", "Completed")],
    "Product Backlog Item": [("New", "#b2b2b2", "Proposed"), ("Approved", "#b2b2b2", "Proposed"),
                             ("Committed", "#007acc", "InProgress"), ("Done", "#339933", "Completed")],
    "Issue": [("Active", "#007acc", "InProgress"), ("Closed", "#339933", "Completed")],
}


class DemoClient(DevOpsClient):
    """Offline DevOpsClient returning canned data; never makes a network call."""

    def __init__(self, token_manager=None):
        super().__init__(token_manager or DemoTokenManager())
        self._next_id = 9000

        # Unified work-item store: {id: field-dict}. Holds both test cases and
        # My Work items so get_work_items serves either.
        self._wi = {}
        self._tc_ids = []
        self._outcomes = {}
        for tc_id, title, tags, auto, steps, outcome in _TC_SPECS:
            self._tc_ids.append(tc_id)
            self._outcomes[tc_id] = outcome
            self._wi[tc_id] = {
                "System.Id": tc_id,
                "System.Title": title,
                "System.Tags": tags,
                "Microsoft.VSTS.TCM.AutomationStatus": auto,
                "Microsoft.VSTS.TCM.Steps": build_steps_xml(
                    [Step(action=a, expected=e) for a, e in steps]),
                "System.CreatedBy": _DEMO_USER,
                "System.CreatedDate": _iso(tc_id - 5000),
                "System.AssignedTo": _DEMO_USER,
                "System.WorkItemType": "Test Case",
            }

        self._mywork_ids = []
        for (wid, wtype, title, state, who, prio, remaining, completed, tags) in _WI_SPECS:
            self._mywork_ids.append(wid)
            self._wi[wid] = {
                "System.Id": wid,
                "System.Title": title,
                "System.WorkItemType": wtype,
                "System.State": state,
                "System.AssignedTo": who,
                "System.CreatedBy": _DEMO_USER,
                "System.ChangedDate": _iso(wid - 3000),
                "System.AreaPath": DEMO_AREA,
                "System.IterationPath": DEMO_ITERATION,
                "System.Tags": tags,
                "System.Rev": 3,
                "Microsoft.VSTS.Common.Priority": prio,
                "Microsoft.VSTS.Scheduling.RemainingWork": remaining,
                "Microsoft.VSTS.Scheduling.CompletedWork": completed,
                "System.Description": "<div>Demo work item — no real data.</div>",
                "Microsoft.VSTS.TCM.ReproSteps": "<div>1. Do the thing.<br>2. See the bug.</div>",
            }

        # Test points for the requirement suite (one per test case).
        self._points = []
        for i, tc_id in enumerate(self._tc_ids):
            self._points.append({
                "point_id": 300 + i,
                "test_case_id": tc_id,
                "test_case_name": self._wi[tc_id]["System.Title"],
                "test_case_state": "Design",
                "config_id": 1,
                "config_name": "Windows 11 / Chrome",
                "tester": DEMO_NAME,
                "last_outcome": self._outcomes[tc_id],
                "last_run_id": 8000 if self._outcomes[tc_id] else None,
                "last_result_id": (100000 + tc_id) if self._outcomes[tc_id] else None,
            })
        self._last_run_points = []

    # ---- id helper --------------------------------------------------------
    def _mint_id(self):
        self._next_id += 1
        return self._next_id

    # ---- discovery / config ----------------------------------------------
    def get_organizations(self):
        return [{"name": DEMO_ORG_NAME, "url": DEMO_ORG_URL}]

    def get_projects(self, org_url):
        return [DEMO_PROJECT, "Contoso Mobile"]

    def search_work_items(self, text, top=20):
        # PBI-only, mirroring the real client's WIQL type filter.
        results = [{"id": DEMO_PBI_ID, "title": DEMO_PBI_TITLE, "type": "Product Backlog Item"},
                   {"id": 1080, "title": "Payments hardening", "type": "Product Backlog Item"}]
        t = (text or "").strip().lower()
        if not t:
            return results[:top]
        return [r for r in results
                if t in r["title"].lower() or t == str(r["id"])][:top]

    def get_work_item(self, work_item_id):
        if work_item_id in self._wi:
            f = self._wi[work_item_id]
            return {
                "System.Title": f.get("System.Title", ""),
                "System.WorkItemType": f.get("System.WorkItemType", ""),
                "System.AreaPath": f.get("System.AreaPath", DEMO_AREA),
                "System.IterationPath": f.get("System.IterationPath", DEMO_ITERATION),
            }
        return {"System.Title": DEMO_PBI_TITLE, "System.WorkItemType": "Product Backlog Item",
                "System.AreaPath": DEMO_AREA, "System.IterationPath": DEMO_ITERATION}

    def get_test_case_fields(self):
        return sorted([
            {"name": "Module", "referenceName": DEMO_MODULE_REF},
            {"name": "Preconditions", "referenceName": DEMO_PRECOND_REF},
            {"name": "Title", "referenceName": "System.Title"},
            {"name": "Tags", "referenceName": "System.Tags"},
        ], key=lambda x: x["name"])

    def get_tags(self):
        return [{"id": str(i), "name": n} for i, n in enumerate(
            ["checkout", "smoke", "regression", "payments", "cart", "mobile"], start=1)]

    # ---- test-case fetch --------------------------------------------------
    def get_work_items(self, ids, fields):
        return [dict(self._wi[i], _id=i) for i in ids if i in self._wi]

    def get_test_cases_by_ids(self, ids, extra_fields=None):
        return [dict(self._wi[i], _id=i) for i in ids if i in self._wi and i in self._tc_ids]

    def get_test_cases_for_pbi(self, pbi_id, extra_fields=None):
        cases = [dict(self._wi[i], _id=i) for i in self._tc_ids]
        return cases, len(cases)

    # ---- My Work ----------------------------------------------------------
    def query_work_items(self, wiql, top=500):
        if "@Me" in (wiql or ""):
            return [i for i in self._mywork_ids
                    if self._wi[i].get("System.AssignedTo", {}).get("uniqueName") == DEMO_UPN][:top]
        return list(self._mywork_ids)[:top]   # team scope: the whole board

    def get_teams(self):
        return [{"id": "t1", "name": "Checkout Team"}, {"id": "t2", "name": "Platform Team"}]

    def get_team_field_values(self, team):
        return {"field_ref": "System.AreaPath", "default": DEMO_AREA,
                "values": [{"value": DEMO_AREA, "includeChildren": True}]}

    def get_work_item_states(self, wi_type):
        return [{"name": n, "color": c, "category": cat}
                for (n, c, cat) in _STATES.get(wi_type, _STATES["Task"])]

    def detect_bug_type(self):
        return {"type": "Bug", "repro_field": "Microsoft.VSTS.TCM.ReproSteps",
                "has_severity": True}

    def create_work_item(self, wi_type, fields, relations=None):
        wid = self._mint_id()
        rec = {"System.Id": wid, "System.WorkItemType": wi_type,
               "System.AssignedTo": _DEMO_USER, "System.CreatedBy": _DEMO_USER,
               "System.ChangedDate": _iso(0), "System.AreaPath": DEMO_AREA,
               "System.IterationPath": DEMO_ITERATION, "System.Rev": 1,
               "System.State": "New"}
        rec.update(fields or {})
        self._wi[wid] = rec
        if wi_type not in ("Test Case",):
            self._mywork_ids.insert(0, wid)
        return {"id": wid, "url": f"{DEMO_ORG_URL}/{DEMO_PROJECT}/_workitems/edit/{wid}"}

    def update_work_item_fields(self, wi_id, fields):
        rec = self._wi.get(wi_id)
        if rec is not None:
            rec.update(fields or {})
        return rec or {}

    def update_test_case_fields(self, tc_id, fields):
        self.update_work_item_fields(tc_id, fields)

    def add_work_item_comment(self, wi_id, text):
        return {"id": self._mint_id(), "text": text}

    def get_work_item_comments(self, wi_id):
        return [{"id": 1, "text": "Looks good — merging after the demo.",
                 "created_by": DEMO_NAME, "created_date": _iso(1), "avatar_url": ""},
                {"id": 2, "text": "Reproduced on staging.",
                 "created_by": "Priya Shah", "created_date": _iso(2), "avatar_url": ""}]

    def get_avatar_image(self, url):
        return None

    # ---- plans / suites ---------------------------------------------------
    def get_test_plans(self, use_cache=False):
        return [
            {"id": DEMO_PLAN_ID, "name": DEMO_PLAN_NAME, "areaPath": DEMO_AREA},
            {"id": 220, "name": "Platform - Test Plan", "areaPath": "Contoso Web\\Platform"},
        ]

    def get_test_plan(self, plan_id):
        return {"id": plan_id, "name": DEMO_PLAN_NAME, "areaPath": DEMO_AREA,
                "rootSuiteId": DEMO_ROOT_SUITE_ID}

    def find_requirement_suite(self, plan_id, pbi_id):
        if plan_id == DEMO_PLAN_ID and pbi_id == DEMO_PBI_ID:
            return {"id": DEMO_SUITE_ID, "name": DEMO_PBI_TITLE,
                    "suiteType": "requirementTestSuite", "requirementId": DEMO_PBI_ID}
        return None

    def find_existing_suite_for_pbi(self, pbi_id, area_path="", plans=None, progress_cb=None):
        suite = self.find_requirement_suite(DEMO_PLAN_ID, pbi_id)
        if suite:
            return {"id": DEMO_PLAN_ID, "name": DEMO_PLAN_NAME, "areaPath": DEMO_AREA}, suite
        return None, None

    def find_plan_for_pbi_area(self, area_path, plans=None):
        return {"id": DEMO_PLAN_ID, "name": DEMO_PLAN_NAME, "areaPath": DEMO_AREA,
                "rootSuiteId": DEMO_ROOT_SUITE_ID}

    def get_suite_by_id(self, plan_id, suite_id):
        return {"id": suite_id, "name": DEMO_PBI_TITLE,
                "suiteType": "requirementTestSuite", "requirementId": DEMO_PBI_ID}

    def get_all_suites(self, plan_id):
        return [
            {"id": DEMO_ROOT_SUITE_ID, "name": DEMO_PLAN_NAME, "parent_id": None,
             "suite_type": "staticTestSuite", "requirement_id": None},
            {"id": DEMO_SUITE_ID, "name": DEMO_PBI_TITLE, "parent_id": DEMO_ROOT_SUITE_ID,
             "suite_type": "requirementTestSuite", "requirement_id": DEMO_PBI_ID},
            {"id": 202, "name": "Regression", "parent_id": DEMO_ROOT_SUITE_ID,
             "suite_type": "staticTestSuite", "requirement_id": None},
            {"id": 203, "name": "Smoke (query-based)", "parent_id": DEMO_ROOT_SUITE_ID,
             "suite_type": "dynamicTestSuite", "requirement_id": None},
        ]

    def create_test_plan(self, name, area_path="", iteration=""):
        return {"id": DEMO_PLAN_ID, "name": name or DEMO_PLAN_NAME,
                "areaPath": area_path or DEMO_AREA, "rootSuiteId": DEMO_ROOT_SUITE_ID}

    def create_requirement_suite(self, plan_id, root_suite_id, pbi_id):
        return DEMO_SUITE_ID

    def ensure_requirement_suite(self, pbi_id, area_path="", iteration=""):
        return DEMO_PLAN_ID, DEMO_PLAN_NAME, DEMO_SUITE_ID

    # ---- create / link (write no-ops) ------------------------------------
    def create_test_case(self, test_case, module_ref, area_path="", iteration_path="",
                         preconditions_ref=None):
        return self._mint_id()

    def update_test_case_from_model(self, tc_id, test_case, module_ref, preconditions_ref=None):
        return None

    def link_to_pbi(self, test_case_id, pbi_id):
        return None

    def add_workitem_attachment(self, content, file_name):
        return f"{DEMO_ORG_URL}/_apis/wit/attachments/demo-{file_name}"

    # ---- test execution ---------------------------------------------------
    def get_test_points(self, plan_id, suite_id, test_case_ids=None):
        pts = [dict(p) for p in self._points]
        if test_case_ids:
            wanted = set(test_case_ids)
            pts = [p for p in pts if p["test_case_id"] in wanted]
        return pts

    def create_test_run(self, plan_id, name, point_ids):
        self._last_run_points = [int(p) for p in point_ids]
        return {"run_id": self._mint_id(), "web_url": ""}

    def get_run_results(self, run_id):
        pid_to_tc = {p["point_id"]: p["test_case_id"] for p in self._points}
        return [{"result_id": 100000 + pid, "test_case_id": pid_to_tc.get(pid),
                 "point_id": pid} for pid in self._last_run_points]

    def update_run_results(self, run_id, results):
        return None

    def update_result_steps(self, run_id, result_id, iteration_details):
        return None

    def add_result_attachment(self, run_id, result_id, b64, file_name, comment=""):
        return None

    def complete_test_run(self, run_id):
        return None

    def get_result(self, run_id, result_id):
        return {"outcome": "", "comment": ""}

    def get_result_attachments(self, run_id, result_id):
        return []

    def get_result_screenshots(self, run_id, result_id):
        return []

    def download_result_attachment(self, run_id, result_id, attachment_id):
        return b""

    # ---- team members -----------------------------------------------------
    def get_team_members(self):
        return [
            {"id": DEMO_UPN, "displayName": DEMO_NAME, "uniqueName": DEMO_UPN},
            {"id": _PRIYA["uniqueName"], "displayName": "Priya Shah",
             "uniqueName": _PRIYA["uniqueName"]},
            {"id": _MARCO["uniqueName"], "displayName": "Marco Rossi",
             "uniqueName": _MARCO["uniqueName"]},
        ]
