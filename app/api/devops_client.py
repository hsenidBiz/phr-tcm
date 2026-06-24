import time
import requests
from app.auth.token_manager import TokenManager
from app.models.test_case import TestCase
from app.utils.xml_builder import build_steps_xml

API_VERSION = "7.1"


class TokenExpiredError(Exception):
    pass


class RateLimitError(Exception):
    def __init__(self, message: str, retry_after: int = 30):
        super().__init__(message)
        self.retry_after = retry_after


class DevOpsClient:
    """
    All Azure DevOps REST API interactions.
    Only POST (create work item) and PATCH (add relation) are ever called.
    No DELETE calls exist anywhere in this class.
    """

    def __init__(self, token_manager: TokenManager):
        self.tm = token_manager
        # Session cache of the project's test plans (they rarely change mid-
        # session). Keyed by (org_url, project); invalidated when a plan is
        # created so a freshly-created plan is never missed.
        self._plans_cache = None
        self._plans_cache_key = None

    def _base(self) -> str:
        return f"{self.tm.org_url}/{self.tm.project}/_apis"

    def _handle(self, response: requests.Response) -> dict:
        if response.status_code == 401:
            raise TokenExpiredError("Token expired or invalid (401). Please sign in again.")
        if response.status_code == 403:
            raise PermissionError(
                f"Access denied (403). Your account may not have permission to create Test Cases "
                f"in project '{self.tm.project}'."
            )
        if response.status_code == 404:
            raise LookupError(f"Resource not found (404): {response.url}")
        if response.status_code == 429:
            retry_after = int(response.headers.get("Retry-After", "30"))
            raise RateLimitError(
                f"Rate limited by Azure DevOps. Will retry after {retry_after} seconds.",
                retry_after,
            )
        try:
            response.raise_for_status()
        except requests.HTTPError as exc:
            msg = ""
            try:
                msg = response.json().get("message", "")
            except Exception:
                pass
            raise RuntimeError(f"HTTP {response.status_code}: {msg or response.text[:200]}") from exc
        return response.json()

    # ------------------------------------------------------------------ #
    #  Safe read-only calls                                               #
    # ------------------------------------------------------------------ #

    def get_organizations(self) -> list:
        """
        Discover the Azure DevOps organisations the signed-in user belongs to.
        Returns list of {"name": str, "url": str}. Safe — read only.
        """
        vssps = "https://app.vssps.visualstudio.com/_apis"
        resp = requests.get(
            f"{vssps}/profile/profiles/me?api-version=6.0",
            headers=self.tm.get_json_headers(), timeout=15,
        )
        member_id = self._handle(resp)["id"]
        resp = requests.get(
            f"{vssps}/accounts?memberId={member_id}&api-version=6.0",
            headers=self.tm.get_json_headers(), timeout=15,
        )
        data = self._handle(resp)
        return [
            {"name": a["accountName"], "url": f"https://dev.azure.com/{a['accountName']}"}
            for a in data.get("value", [])
            if a.get("accountName")
        ]

    def get_projects(self, org_url: str) -> list:
        """
        All project names in the organisation (paginated). Safe — read only.
        """
        names = []
        continuation = None
        while True:
            url = f"{org_url}/_apis/projects?$top=200&api-version={API_VERSION}"
            if continuation:
                url += f"&continuationToken={continuation}"
            resp = requests.get(url, headers=self.tm.get_json_headers(), timeout=15)
            data = self._handle(resp)
            names.extend(p["name"] for p in data.get("value", []))
            continuation = resp.headers.get("x-ms-continuationtoken")
            if not continuation:
                break
        return sorted(names, key=str.lower)

    def search_work_items(self, text: str, top: int = 20) -> list:
        """
        Search work items in the current project by title substring (and by
        exact ID when the text is numeric) via a WIQL query. The POST here is
        query-only — it creates and modifies nothing. Test artifacts and tasks
        are excluded. Returns list of {"id", "title", "type"}, most recently
        changed first. Safe — read only.
        """
        safe = text.strip().replace("'", "''")
        clause = f"[System.Title] CONTAINS '{safe}'"
        if text.strip().isdigit():
            clause = f"([System.Id] = {int(text)} OR {clause})"
        wiql = (
            "SELECT [System.Id] FROM workitems "
            f"WHERE [System.TeamProject] = @project AND {clause} "
            "AND [System.WorkItemType] NOT IN "
            "('Test Case', 'Test Suite', 'Test Plan', 'Shared Steps', 'Task') "
            "ORDER BY [System.ChangedDate] DESC"
        )
        url = f"{self._base()}/wit/wiql?$top={top}&api-version={API_VERSION}"
        resp = requests.post(
            url, json={"query": wiql},
            headers=self.tm.get_json_headers(), timeout=15,
        )
        ids = [w["id"] for w in self._handle(resp).get("workItems", [])]
        if not ids:
            return []

        ids_csv = ",".join(str(i) for i in ids)
        url = (
            f"{self._base()}/wit/workitems?ids={ids_csv}"
            f"&fields=System.Title,System.WorkItemType&api-version={API_VERSION}"
        )
        resp = requests.get(url, headers=self.tm.get_json_headers(), timeout=15)
        by_id = {
            w["id"]: w.get("fields", {})
            for w in self._handle(resp).get("value", [])
        }
        return [
            {
                "id": i,
                "title": by_id[i].get("System.Title", ""),
                "type": by_id[i].get("System.WorkItemType", ""),
            }
            for i in ids if i in by_id
        ]

    def get_work_item(self, work_item_id: int) -> dict:
        """
        GET a single work item's fields. Safe — read only.
        Returns the fields dict including AreaPath and IterationPath.
        """
        select = (
            "System.Title,System.WorkItemType,"
            "System.AreaPath,System.IterationPath"
        )
        url = (
            f"{self._base()}/wit/workitems/{work_item_id}"
            f"?api-version={API_VERSION}&$select={select}"
        )
        resp = requests.get(url, headers=self.tm.get_json_headers(), timeout=15)
        data = self._handle(resp)
        return data.get("fields", {})

    def get_work_item_title(self, work_item_id: int) -> str:
        """Returns a display string like 'My Feature (Product Backlog Item)'."""
        fields = self.get_work_item(work_item_id)
        title = fields.get("System.Title", "Unknown")
        wtype = fields.get("System.WorkItemType", "")
        return f"{title} ({wtype})"

    def get_tags(self) -> list:
        """
        GET all work-item tags defined in the project.
        Returns list of {"id": str, "name": str}.
        Safe — read only.
        """
        url = f"{self._base()}/wit/tags?api-version={API_VERSION}"
        resp = requests.get(url, headers=self.tm.get_json_headers(), timeout=15)
        data = self._handle(resp)
        return data.get("value", [])

    def get_test_case_fields(self) -> list:
        """
        GET all fields available on the Test Case work item type.
        Returns list of {"name": str, "referenceName": str}.
        Filters out read-only and hidden system fields.
        Safe — read only.
        """
        url = (
            f"{self._base()}/wit/workitemtypes/Test%20Case/fields"
            f"?api-version={API_VERSION}"
        )
        resp = requests.get(url, headers=self.tm.get_json_headers(), timeout=15)
        data = self._handle(resp)

        fields = []
        for f in data.get("value", []):
            ref = f.get("referenceName", "")
            name = f.get("name", ref)
            read_only = f.get("readOnly", False)
            # Exclude built-in system fields and read-only fields
            if read_only:
                continue
            if ref.startswith("System.") and ref not in (
                "System.Title", "System.Tags", "System.Description"
            ):
                continue
            fields.append({"name": name, "referenceName": ref})

        return sorted(fields, key=lambda x: x["name"])

    def get_team_members(self) -> list[dict]:
        """
        GET all team members in the project.
        Returns list of {\"id\": str, \"displayName\": str, \"uniqueName\": str}.
        Safe — read only.
        """
        url = f"{self.tm.org_url}/_apis/projects/{self.tm.project}/teams?api-version={API_VERSION}"
        resp = requests.get(url, headers=self.tm.get_json_headers(), timeout=15)
        data = self._handle(resp)
        
        members = []
        for team in data.get("value", []):
            team_id = team.get("id")
            # Get members of each team
            members_url = f"{self.tm.org_url}/_apis/projects/{self.tm.project}/teams/{team_id}/members?api-version={API_VERSION}"
            members_resp = requests.get(members_url, headers=self.tm.get_json_headers(), timeout=15)
            members_data = self._handle(members_resp)
            
            for member in members_data.get("value", []):
                identity = member.get("identity", {})
                members.append({
                    "id": identity.get("id"),
                    "displayName": identity.get("displayName"),
                    "uniqueName": identity.get("uniqueName"),
                })
        
        # Remove duplicates and sort by displayName
        unique_members = {}
        for m in members:
            key = m.get("uniqueName", m.get("id"))
            if key:
                unique_members[key] = m
        
        return sorted(unique_members.values(), key=lambda x: x.get("displayName", ""))

    # Azure DevOps caps the workitems batch-GET (?ids=) endpoint at 200 IDs
    # per request, so larger PBIs must be fetched in successive batches.
    WORKITEM_BATCH_SIZE = 200

    def get_test_cases_for_pbi(self, pbi_id: int, extra_fields: list = None) -> tuple:
        """
        GET all Test Case work items linked to a PBI via TestedBy relations.
        Returns (list of field dicts, total_count).
        Fetches in batches of WORKITEM_BATCH_SIZE so there is no overall cap —
        all linked Test Cases are returned regardless of count.
        Each field dict has an '_id' key for the work item ID.
        Safe — read only.
        """
        url = (
            f"{self._base()}/wit/workitems/{pbi_id}"
            f"?$expand=relations&api-version={API_VERSION}"
        )
        resp = requests.get(url, headers=self.tm.get_json_headers(), timeout=15)
        data = self._handle(resp)

        tc_ids = []
        for rel in data.get("relations", []):
            if "testedby" in rel.get("rel", "").lower():
                try:
                    tc_ids.append(int(rel["url"].split("/")[-1]))
                except (KeyError, ValueError):
                    pass

        total = len(tc_ids)
        if not tc_ids:
            return [], 0

        base_fields = [
            "System.Id", "System.Title", "System.Tags",
            "Microsoft.VSTS.TCM.AutomationStatus",
            "Microsoft.VSTS.TCM.Steps",
            "System.CreatedBy", "System.AssignedTo",
        ]
        if extra_fields:
            base_fields.extend(f for f in extra_fields if f not in base_fields)
        fields_str = ",".join(base_fields)

        result = []
        for start in range(0, total, self.WORKITEM_BATCH_SIZE):
            batch = tc_ids[start:start + self.WORKITEM_BATCH_SIZE]
            ids_str = ",".join(str(i) for i in batch)
            url = (
                f"{self._base()}/wit/workitems"
                f"?ids={ids_str}&fields={fields_str}&api-version={API_VERSION}"
            )
            resp = requests.get(url, headers=self.tm.get_json_headers(), timeout=30)
            data = self._handle(resp)

            for item in data.get("value", []):
                fields = item.get("fields", {})
                fields["_id"] = item["id"]
                result.append(fields)

        return result, total

    def update_test_case_fields(self, tc_id: int, fields: dict):
        """
        PATCH a Test Case work item to update the specified fields.
        fields: {reference_name: value}
        Uses 'add' op which creates-or-replaces. No DELETE operations.
        """
        patch = [
            {"op": "add", "path": f"/fields/{ref}", "value": value}
            for ref, value in fields.items()
        ]
        url = f"{self._base()}/wit/workitems/{tc_id}?api-version={API_VERSION}"
        resp = requests.patch(url, json=patch, headers=self.tm.get_patch_headers(), timeout=30)
        self._handle(resp)

    def update_test_case_from_model(
        self,
        tc_id: int,
        test_case: TestCase,
        module_ref: str | None,
        preconditions_ref: str | None = None,
    ):
        """
        PATCH an existing Test Case work item with the values from a TestCase model.
        Used when an imported case matches an existing work item by title.

        Always overwrites Steps and AutomationStatus. Overwrites Tags / module /
        Preconditions only when the imported case provides a value, so a blank
        column in the spreadsheet never wipes existing data. Mirrors the field
        construction in create_test_case. Never creates, links, or DELETEs.
        """
        fields: dict = {
            "Microsoft.VSTS.TCM.Steps": build_steps_xml(test_case.steps),
            "Microsoft.VSTS.TCM.AutomationStatus": test_case.automation_status,
        }
        if test_case.tags:
            fields["System.Tags"] = test_case.tags
        if module_ref and test_case.module_value:
            fields[module_ref] = test_case.module_value
        if preconditions_ref and test_case.preconditions:
            fields[preconditions_ref] = f"<div>{test_case.preconditions}</div>"

        self.update_test_case_fields(tc_id, fields)

    # ------------------------------------------------------------------ #
    #  Write calls (POST / PATCH only)                                    #
    # ------------------------------------------------------------------ #

    def create_test_case(
        self,
        test_case: TestCase,
        module_ref: str | None,
        area_path: str = "",
        iteration_path: str = "",
        preconditions_ref: str | None = None,
    ) -> int:
        """
        POST a new Test Case work item.
        Returns the new work item ID.
        Only creates work items of type 'Test Case' — never any other type.
        """
        steps_xml = build_steps_xml(test_case.steps)

        patch = [
            {"op": "add", "path": "/fields/System.Title", "value": test_case.title},
            {"op": "add", "path": "/fields/Microsoft.VSTS.TCM.Steps", "value": steps_xml},
            {
                "op": "add",
                "path": "/fields/Microsoft.VSTS.TCM.AutomationStatus",
                "value": test_case.automation_status,
            },
        ]

        if area_path:
            patch.append(
                {"op": "add", "path": "/fields/System.AreaPath", "value": area_path}
            )

        if iteration_path:
            patch.append(
                {"op": "add", "path": "/fields/System.IterationPath", "value": iteration_path}
            )

        if test_case.tags:
            patch.append(
                {"op": "add", "path": "/fields/System.Tags", "value": test_case.tags}
            )

        if module_ref and test_case.module_value:
            patch.append(
                {"op": "add", "path": f"/fields/{module_ref}", "value": test_case.module_value}
            )

        if preconditions_ref and test_case.preconditions:
            patch.append(
                {
                    "op": "add",
                    "path": f"/fields/{preconditions_ref}",
                    "value": f"<div>{test_case.preconditions}</div>",
                }
            )

        if test_case.created_by:
            patch.append(
                {"op": "add", "path": "/fields/System.CreatedBy", "value": test_case.created_by}
            )

        url = f"{self._base()}/wit/workitems/$Test%20Case?api-version={API_VERSION}"
        resp = requests.post(url, json=patch, headers=self.tm.get_patch_headers(), timeout=30)
        data = self._handle(resp)
        return data["id"]

    def link_to_pbi(self, test_case_id: int, pbi_id: int):
        """
        PATCH the newly created Test Case to add a 'Tests' relation to the PBI.
        Uses Microsoft.VSTS.Common.TestedBy-Reverse which creates:
          - 'Tests' link on the Test Case side
          - 'Tested By' link on the PBI side
        This mirrors what the UI 'Add Test' button does.
        The PBI itself is never modified directly.
        """
        pbi_url = f"{self._base()}/wit/workitems/{pbi_id}"

        patch = [
            {
                "op": "add",
                "path": "/relations/-",
                "value": {
                    "rel": "Microsoft.VSTS.Common.TestedBy-Reverse",
                    "url": pbi_url,
                    "attributes": {"comment": "Linked by DevOps Test Case Creator"},
                },
            }
        ]

        url = f"{self._base()}/wit/workitems/{test_case_id}?api-version={API_VERSION}"
        resp = requests.patch(url, json=patch, headers=self.tm.get_patch_headers(), timeout=30)
        self._handle(resp)

    def create_and_link(
        self,
        test_case: TestCase,
        pbi_id: int,
        module_ref: str | None,
        area_path: str = "",
        iteration_path: str = "",
        preconditions_ref: str | None = None,
    ) -> int:
        """
        Convenience: create a Test Case then immediately link it to the PBI.
        Returns the new work item ID.
        Includes a 0.5-second delay after creation to respect rate limits.
        """
        tc_id = self.create_test_case(test_case, module_ref, area_path, iteration_path, preconditions_ref)
        time.sleep(0.5)
        self.link_to_pbi(tc_id, pbi_id)
        return tc_id

    # ------------------------------------------------------------------ #
    #  Test Plans & Suites (board visibility)                            #
    #                                                                     #
    #  A requirement-based test suite bound to a PBI automatically        #
    #  includes every Test Case linked to that PBI via "Tested By" — the  #
    #  link this client already creates. Ensuring such a suite exists is  #
    #  what makes the created tests show on the board's test count, the   #
    #  same way the manual "Add test" flow does. These calls only ever    #
    #  GET (read) or POST (create plans/suites). No DELETE anywhere.      #
    # ------------------------------------------------------------------ #

    @staticmethod
    def _area_matches(plan_area: str, pbi_area: str) -> bool:
        """True if the plan's area path equals or is an ancestor of the PBI's
        area path. ADO area paths use backslashes; tolerate slashes too."""
        if not plan_area or not pbi_area:
            return False
        pa = plan_area.strip().lower().replace("/", "\\")
        ba = pbi_area.strip().lower().replace("/", "\\")
        return ba == pa or ba.startswith(pa + "\\")

    @staticmethod
    def _default_plan_name(area_path: str) -> str:
        leaf = ""
        if area_path:
            leaf = area_path.replace("/", "\\").split("\\")[-1].strip()
        return f"{leaf} - Test Plan" if leaf else "Test Plan"

    def get_test_plans(self, use_cache: bool = False) -> list:
        """All test plans in the project (paginated). Returns list of
        {id, name, areaPath}. With use_cache=True, returns a session-cached list
        for the current org/project so repeated PBI selections don't re-list
        every plan. Safe — read only."""
        key = (self.tm.org_url, self.tm.project)
        if use_cache and self._plans_cache is not None and self._plans_cache_key == key:
            return self._plans_cache
        plans = []
        continuation = None
        while True:
            url = f"{self._base()}/testplan/plans?api-version={API_VERSION}"
            if continuation:
                url += f"&continuationToken={continuation}"
            resp = requests.get(url, headers=self.tm.get_json_headers(), timeout=20)
            data = self._handle(resp)
            for p in data.get("value", []):
                plans.append({
                    "id": p.get("id"),
                    "name": p.get("name", ""),
                    "areaPath": p.get("areaPath", "") or "",
                })
            continuation = resp.headers.get("x-ms-continuationtoken")
            if not continuation:
                break
        self._plans_cache = plans
        self._plans_cache_key = key
        return plans

    def get_test_plan(self, plan_id: int) -> dict:
        """A single test plan including its root suite id. Safe — read only."""
        url = f"{self._base()}/testplan/plans/{plan_id}?api-version={API_VERSION}"
        resp = requests.get(url, headers=self.tm.get_json_headers(), timeout=20)
        data = self._handle(resp)
        return {
            "id": data.get("id"),
            "name": data.get("name", ""),
            "areaPath": data.get("areaPath", "") or "",
            "rootSuiteId": (data.get("rootSuite") or {}).get("id"),
        }

    def find_requirement_suite(self, plan_id: int, pbi_id: int) -> dict | None:
        """The requirement-based suite bound to `pbi_id` within `plan_id`, or
        None. Pages through the plan's suites and returns as soon as it matches,
        so a plan with many suites doesn't pay to fetch every page once the suite
        is found. Safe — read only."""
        continuation = None
        while True:
            url = f"{self._base()}/testplan/Plans/{plan_id}/suites?api-version={API_VERSION}"
            if continuation:
                url += f"&continuationToken={continuation}"
            resp = requests.get(url, headers=self.tm.get_json_headers(), timeout=20)
            data = self._handle(resp)
            for s in data.get("value", []):
                if (s.get("requirementId") == pbi_id
                        and s.get("suiteType") == "requirementTestSuite"):
                    return {
                        "id": s.get("id"),
                        "name": s.get("name", ""),
                        "suiteType": s.get("suiteType", ""),
                        "requirementId": s.get("requirementId"),
                    }
            continuation = resp.headers.get("x-ms-continuationtoken")
            if not continuation:
                return None

    def find_existing_suite_for_pbi(self, pbi_id: int, area_path: str = "",
                                    plans: list | None = None) -> tuple:
        """Scan the project's test plans for a requirement-based suite bound to
        this PBI. The PBI's area-matched plan is checked first (manual default
        plans are area-scoped), so the common case returns after one or two
        calls. A plan whose suites can't be listed (e.g. a permission-restricted
        plan, or a transient error) is skipped rather than aborting the whole
        search. Pass `plans` to reuse an already-fetched plan list and avoid a
        second round-trip. Returns (plan_dict, suite_dict) or (None, None).
        Safe — read only."""
        if plans is None:
            plans = self.get_test_plans()
        ordered = sorted(
            plans, key=lambda p: 0 if self._area_matches(p.get("areaPath", ""), area_path) else 1
        )
        for plan in ordered:
            try:
                suite = self.find_requirement_suite(plan["id"], pbi_id)
            except (PermissionError, LookupError, RuntimeError):
                # Can't read this plan's suites — keep searching the others.
                # Token/rate-limit errors are deliberately NOT caught here so the
                # caller can handle re-auth / back-off.
                continue
            if suite:
                return plan, suite
        return None, None

    def find_plan_for_pbi_area(self, area_path: str, plans: list | None = None) -> dict | None:
        """An existing plan whose area path equals or is an ancestor of the PBI's
        area path (the natural home for this PBI's suite). Pass `plans` to reuse
        an already-fetched list. Returns the plan dict with rootSuiteId resolved,
        or None. Safe — read only."""
        if plans is None:
            plans = self.get_test_plans()
        best = None
        for p in plans:
            if self._area_matches(p.get("areaPath", ""), area_path):
                # Prefer the most specific (longest) matching area path.
                if best is None or len(p.get("areaPath", "")) > len(best.get("areaPath", "")):
                    best = p
        if best and not best.get("rootSuiteId"):
            best = self.get_test_plan(best["id"])
        return best

    def create_test_plan(self, name: str, area_path: str = "", iteration: str = "") -> dict:
        """POST a new test plan. Returns {id, name, areaPath, rootSuiteId}.
        Creating a plan also creates its root suite (returned as rootSuite)."""
        body = {"name": name}
        if area_path:
            body["areaPath"] = area_path
        if iteration:
            body["iteration"] = iteration
        url = f"{self._base()}/testplan/plans?api-version={API_VERSION}"
        resp = requests.post(url, json=body, headers=self.tm.get_json_headers(), timeout=30)
        data = self._handle(resp)
        self._plans_cache = None  # a new plan now exists — drop the cached list
        return {
            "id": data.get("id"),
            "name": data.get("name", name),
            "areaPath": data.get("areaPath", area_path) or "",
            "rootSuiteId": (data.get("rootSuite") or {}).get("id"),
        }

    def create_requirement_suite(self, plan_id: int, root_suite_id: int, pbi_id: int) -> int:
        """POST a requirement-based test suite bound to the PBI, under the plan's
        root suite. ADO names it after the requirement and auto-populates it from
        the PBI's 'Tested By'-linked test cases. Returns the new suite id."""
        body = {
            "suiteType": "requirementTestSuite",
            "requirementId": pbi_id,
            "parentSuite": {"id": root_suite_id},
        }
        url = f"{self._base()}/testplan/Plans/{plan_id}/suites?api-version={API_VERSION}"
        resp = requests.post(url, json=body, headers=self.tm.get_json_headers(), timeout=30)
        data = self._handle(resp)
        return data.get("id")

    def ensure_requirement_suite(self, pbi_id: int, area_path: str = "",
                                 iteration: str = "") -> tuple:
        """Find-or-create the shared test plan for the PBI's area and the
        requirement-based suite bound to the PBI. Reuses any existing suite
        (created here or by the manual 'Add test' flow) so nothing is duplicated.
        Returns (plan_id, plan_name, suite_id). May POST (create plan / suite);
        never DELETEs."""
        plans = self.get_test_plans()
        plan, suite = self.find_existing_suite_for_pbi(pbi_id, area_path, plans=plans)
        if suite:
            return plan["id"], plan.get("name", ""), suite["id"]

        plan = self.find_plan_for_pbi_area(area_path, plans=plans)
        if plan is None:
            plan = self.create_test_plan(
                self._default_plan_name(area_path), area_path, iteration
            )
        suite_id = self.create_requirement_suite(plan["id"], plan["rootSuiteId"], pbi_id)
        return plan["id"], plan.get("name", ""), suite_id
