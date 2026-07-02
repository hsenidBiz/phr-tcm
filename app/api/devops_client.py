import time
import requests
from app.auth.token_manager import TokenManager
from app.models.test_case import TestCase
from app.utils.logger import get_logger
from app.utils.xml_builder import build_steps_xml

log = get_logger(__name__)

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
        # One pooled HTTPS connection (keep-alive), reused for every call, so the
        # many sequential round-trips (plan/suite discovery, batched work-item
        # fetches, result paging) don't each pay a fresh TCP + TLS handshake.
        # The Bearer header is still passed per-call (it can refresh mid-session).
        self._session = requests.Session()
        # Session cache of the project's test plans (they rarely change mid-
        # session). Keyed by (org_url, project); invalidated when a plan is
        # created so a freshly-created plan is never missed.
        self._plans_cache = None
        self._plans_cache_key = None

    def _base(self) -> str:
        return f"{self.tm.org_url}/{self.tm.project}/_apis"

    def _handle(self, response: requests.Response) -> dict:
        # Status + URL only — Bearer tokens travel in headers, never logged.
        # 403/404 are routinely provoked and handled by the find-or-create
        # discovery flows (stale suite-cache probes, permission-restricted
        # plans), so they log at debug to keep the file useful for real faults.
        if response.status_code in (403, 404):
            log.debug("HTTP %s from %s", response.status_code, response.url)
        elif response.status_code >= 400:
            log.warning("HTTP %s from %s", response.status_code, response.url)
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
        resp = self._session.get(
            f"{vssps}/profile/profiles/me?api-version=6.0",
            headers=self.tm.get_json_headers(), timeout=15,
        )
        member_id = self._handle(resp)["id"]
        resp = self._session.get(
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
            resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=15)
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
        resp = self._session.post(
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
        resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=15)
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
        resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=15)
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
        resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=15)
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
        resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=15)
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

    _TEAM_PAGE_SIZE = 200

    def get_team_members(self) -> list[dict]:
        """
        GET all team members in the project.
        Returns list of {\"id\": str, \"displayName\": str, \"uniqueName\": str}.
        The team list and each team's member list are paged ($top/$skip) so
        large orgs aren't truncated, and the per-team member fetches run on a
        small thread pool instead of one-by-one (requests.Session is safe for
        concurrent use; the Bearer header is fetched per call and TokenManager's
        refresh is lock-protected). Safe — read only.
        """
        page = self._TEAM_PAGE_SIZE

        def _paged(url_base: str) -> list:
            out, skip = [], 0
            while True:
                url = f"{url_base}?$top={page}&$skip={skip}&api-version={API_VERSION}"
                resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=15)
                batch = self._handle(resp).get("value", [])
                out.extend(batch)
                if len(batch) < page:
                    return out
                skip += page

        teams = _paged(f"{self.tm.org_url}/_apis/projects/{self.tm.project}/teams")
        team_ids = [t.get("id") for t in teams if t.get("id")]

        members = []
        if team_ids:
            from concurrent.futures import ThreadPoolExecutor
            with ThreadPoolExecutor(max_workers=min(4, len(team_ids))) as pool:
                per_team = pool.map(
                    lambda tid: _paged(
                        f"{self.tm.org_url}/_apis/projects/{self.tm.project}/teams/{tid}/members"
                    ),
                    team_ids,
                )
                for team_members in per_team:
                    for member in team_members:
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
        resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=15)
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
            "System.CreatedBy", "System.CreatedDate", "System.AssignedTo",
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
            resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=30)
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
        resp = self._session.patch(url, json=patch, headers=self.tm.get_patch_headers(), timeout=30)
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
        resp = self._session.post(url, json=patch, headers=self.tm.get_patch_headers(), timeout=30)
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
                    "attributes": {"comment": "Linked by DevOps Test Case Manager"},
                },
            }
        ]

        url = f"{self._base()}/wit/workitems/{test_case_id}?api-version={API_VERSION}"
        resp = self._session.patch(url, json=patch, headers=self.tm.get_patch_headers(), timeout=30)
        self._handle(resp)

    def work_item_url(self, wi_id: int) -> str:
        """The REST URL of a work item, for use as a relation target."""
        return f"{self._base()}/wit/workitems/{wi_id}"

    def detect_bug_type(self) -> dict:
        """Resolve which work item type to file bugs as on this project's process,
        and which field holds the repro/description. Prefers 'Bug' (Agile/Scrum/
        CMMI → ReproSteps); falls back to 'Issue' (Basic → System.Description).
        Cached per session. Safe — read only."""
        if getattr(self, "_bug_type_cache", None):
            return self._bug_type_cache
        names = set()
        try:
            url = f"{self._base()}/wit/workitemtypes?api-version={API_VERSION}"
            resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=20)
            names = {wt.get("name", "") for wt in self._handle(resp).get("value", [])}
        except Exception:
            log.warning("Could not enumerate work item types — assuming 'Bug'",
                        exc_info=True)
        if "Bug" in names or not names:
            info = {"type": "Bug", "repro_field": "Microsoft.VSTS.TCM.ReproSteps",
                    "has_severity": True}
        elif "Issue" in names:
            info = {"type": "Issue", "repro_field": "System.Description",
                    "has_severity": False}
        else:
            info = {"type": "Bug", "repro_field": "Microsoft.VSTS.TCM.ReproSteps",
                    "has_severity": True}
        self._bug_type_cache = info
        return info

    def create_work_item(self, wi_type: str, fields: dict, relations: list = None) -> dict:
        """POST a new work item of `wi_type` with fields and optional relations.
        Returns {id, url(web)}. Only POST — never DELETEs."""
        from urllib.parse import quote
        patch = [{"op": "add", "path": f"/fields/{ref}", "value": val}
                 for ref, val in fields.items()]
        for rel in (relations or []):
            patch.append({"op": "add", "path": "/relations/-", "value": rel})
        url = f"{self._base()}/wit/workitems/${quote(wi_type)}?api-version={API_VERSION}"
        resp = self._session.post(url, json=patch, headers=self.tm.get_patch_headers(), timeout=30)
        data = self._handle(resp)
        return {"id": data.get("id"),
                "url": ((data.get("_links") or {}).get("html") or {}).get("href", "")}

    def add_workitem_attachment(self, content: bytes, file_name: str) -> str:
        """Upload binary content as a work-item attachment; returns its URL (added
        afterwards as an AttachedFile relation). Creates an attachment, no DELETE."""
        from urllib.parse import quote
        headers = dict(self.tm.get_json_headers())
        headers["Content-Type"] = "application/octet-stream"
        url = (f"{self._base()}/wit/attachments"
               f"?fileName={quote(file_name)}&api-version={API_VERSION}")
        resp = self._session.post(url, data=content, headers=headers, timeout=60)
        return self._handle(resp).get("url", "")

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
            resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=20)
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
        resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=20)
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
            resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=20)
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
                                    plans: list | None = None, progress_cb=None) -> tuple:
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
        total = len(ordered)
        for i, plan in enumerate(ordered):
            if progress_cb:
                try:
                    progress_cb(i + 1, total)
                except Exception:
                    pass
            try:
                suite = self.find_requirement_suite(plan["id"], pbi_id)
            except (PermissionError, LookupError, RuntimeError):
                # Can't read this plan's suites — keep searching the others.
                # Token/rate-limit errors are deliberately NOT caught here so the
                # caller can handle re-auth / back-off.
                log.info("Skipping unreadable test plan %s during suite search",
                         plan.get("id"))
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

    def get_suite_by_id(self, plan_id: int, suite_id: int) -> dict:
        """A single test suite by id — a direct GET used to cheaply confirm a
        cached requirement suite still exists, avoiding a scan of every suite in
        the plan. Raises LookupError (404) if it no longer exists. Read only."""
        url = (f"{self._base()}/testplan/Plans/{plan_id}/suites/{suite_id}"
               f"?api-version={API_VERSION}")
        resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=20)
        data = self._handle(resp)
        return {
            "id": data.get("id"),
            "name": data.get("name", ""),
            "suiteType": data.get("suiteType", ""),
            "requirementId": data.get("requirementId"),
        }

    def create_test_plan(self, name: str, area_path: str = "", iteration: str = "") -> dict:
        """POST a new test plan. Returns {id, name, areaPath, rootSuiteId}.
        Creating a plan also creates its root suite (returned as rootSuite)."""
        body = {"name": name}
        if area_path:
            body["areaPath"] = area_path
        if iteration:
            body["iteration"] = iteration
        url = f"{self._base()}/testplan/plans?api-version={API_VERSION}"
        resp = self._session.post(url, json=body, headers=self.tm.get_json_headers(), timeout=30)
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
        resp = self._session.post(url, json=body, headers=self.tm.get_json_headers(), timeout=30)
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

    # ------------------------------------------------------------------ #
    #  Test execution — runs, results, outcomes, attachments             #
    #                                                                     #
    #  Records manual test outcomes against the test points in the PBI's  #
    #  requirement-based suite. Only GET / POST / PATCH — no DELETE. The   #
    #  GUI gates the actual submission behind an explicit confirm.        #
    # ------------------------------------------------------------------ #

    def get_test_points(self, plan_id: int, suite_id: int,
                        test_case_ids: list | None = None) -> list:
        """All test points in a plan+suite (paginated). Returns a list of
        {point_id, test_case_id, config_id, config_name, last_outcome}. The
        `test_case_id` (work item id) maps a test case to its point. Safe — read
        only."""
        tc_filter = ""
        if test_case_ids:
            tc_filter = "&testCaseId=" + ",".join(str(i) for i in test_case_ids)
        points = []
        continuation = None
        while True:
            url = (
                f"{self._base()}/testplan/Plans/{plan_id}/Suites/{suite_id}/TestPoint"
                f"?api-version={API_VERSION}{tc_filter}"
            )
            if continuation:
                url += f"&continuationToken={continuation}"
            resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=20)
            data = self._handle(resp)
            for p in data.get("value", []):
                tcref = p.get("testCaseReference") or {}
                cfg = p.get("configuration") or {}
                results = p.get("results") or {}
                points.append({
                    "point_id": p.get("id"),
                    "test_case_id": tcref.get("id"),
                    "config_id": cfg.get("id"),
                    "config_name": cfg.get("name", ""),
                    "last_outcome": results.get("outcome", "") or "",
                    "last_run_id": results.get("lastTestRunId"),
                    "last_result_id": results.get("lastResultId"),
                })
            continuation = resp.headers.get("x-ms-continuationtoken")
            if not continuation:
                break
        return points

    def get_result(self, run_id: int, result_id: int) -> dict:
        """A single test result's outcome + comment (used to pre-load the runner
        with the last recorded values). Safe — read only."""
        url = (f"{self._base()}/test/Runs/{run_id}/Results/{result_id}"
               f"?api-version={API_VERSION}")
        resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=20)
        data = self._handle(resp)
        return {"outcome": data.get("outcome", "") or "",
                "comment": data.get("comment", "") or ""}

    def get_result_attachments(self, run_id: int, result_id: int) -> list:
        """List a test result's attachments. Returns a list of
        {id, file_name, comment}. Safe — read only."""
        url = (f"{self._base()}/test/Runs/{run_id}/Results/{result_id}/attachments"
               f"?api-version={API_VERSION}")
        resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=20)
        data = self._handle(resp)
        return [{"id": a.get("id"),
                 "file_name": a.get("fileName", "") or "",
                 "comment": a.get("comment", "") or ""}
                for a in data.get("value", [])]

    def download_result_attachment(self, run_id: int, result_id: int,
                                   attachment_id: int) -> bytes:
        """Download one test-result attachment's raw bytes. Safe — read only."""
        url = (f"{self._base()}/test/Runs/{run_id}/Results/{result_id}"
               f"/attachments/{attachment_id}?api-version={API_VERSION}")
        headers = dict(self.tm.get_json_headers())
        headers["Accept"] = "application/octet-stream"
        resp = self._session.get(url, headers=headers, timeout=60)
        if resp.status_code == 401:
            raise TokenExpiredError("Token expired or invalid (401). Please sign in again.")
        resp.raise_for_status()
        return resp.content

    def get_result_screenshots(self, run_id: int, result_id: int) -> list:
        """List + download a result's *image* attachments in one (worker-thread)
        call. Returns a list of {file_name, data: bytes}, skipping non-images and
        any download that fails. Safe — read only."""
        image_exts = (".png", ".jpg", ".jpeg", ".bmp", ".gif")
        out = []
        for att in self.get_result_attachments(run_id, result_id):
            if not (att.get("file_name") or "").lower().endswith(image_exts):
                continue
            try:
                data = self.download_result_attachment(run_id, result_id, att["id"])
            except Exception:
                continue
            if data:
                out.append({"file_name": att.get("file_name", ""), "data": data})
        return out

    def create_test_run(self, plan_id: int, name: str, point_ids: list) -> dict:
        """POST a manual test run seeded from the given test point ids. Azure
        DevOps creates one result per point and the run starts InProgress.
        Returns {run_id, web_url}."""
        body = {
            "name": name,
            "plan": {"id": str(plan_id)},
            "pointIds": [int(p) for p in point_ids],
            "automated": False,
        }
        url = f"{self._base()}/test/runs?api-version={API_VERSION}"
        resp = self._session.post(url, json=body, headers=self.tm.get_json_headers(), timeout=30)
        data = self._handle(resp)
        return {"run_id": data.get("id"), "web_url": data.get("webAccessUrl", "")}

    def get_run_results(self, run_id: int) -> list:
        """The results auto-created for a run. Returns a list of
        {result_id, test_case_id, point_id} so the caller can map each result
        back to the test case it belongs to. Safe — read only."""
        url = f"{self._base()}/test/Runs/{run_id}/results?api-version={API_VERSION}"
        resp = self._session.get(url, headers=self.tm.get_json_headers(), timeout=30)
        data = self._handle(resp)
        out = []
        for r in data.get("value", []):
            tc = r.get("testCase") or {}
            tp = r.get("testPoint") or {}
            out.append({
                "result_id": r.get("id"),
                "test_case_id": int(tc["id"]) if tc.get("id") else None,
                "point_id": int(tp["id"]) if tp.get("id") else None,
            })
        return out

    def update_run_results(self, run_id: int, results: list):
        """PATCH outcomes onto a run's results. Each item: {id, outcome, comment,
        duration_ms}. `outcome` must be an ADO value (Passed/Failed/Blocked/
        NotApplicable). Marks each result Completed. Plain-JSON PATCH (not
        json-patch)."""
        body = []
        for r in results:
            item = {"id": r["id"], "outcome": r["outcome"], "state": "Completed"}
            if r.get("comment"):
                item["comment"] = r["comment"][:1000]
            if r.get("duration_ms"):
                item["durationInMs"] = r["duration_ms"]
            if r.get("bug_ids"):
                item["associatedBugs"] = [{"id": b} for b in r["bug_ids"]]
            body.append(item)
        url = f"{self._base()}/test/Runs/{run_id}/results?api-version={API_VERSION}"
        resp = self._session.patch(url, json=body, headers=self.tm.get_json_headers(), timeout=30)
        self._handle(resp)

    def update_result_steps(self, run_id: int, result_id: int, iteration_details: list):
        """Attach per-step (iteration) results to a single test result so the ADO
        step-by-step view reflects which steps passed/failed. Additive and
        best-effort — the overall outcome is recorded by update_run_results.
        Plain-JSON PATCH, no DELETE."""
        body = [{"id": result_id, "iterationDetails": iteration_details}]
        url = f"{self._base()}/test/Runs/{run_id}/results?api-version={API_VERSION}"
        resp = self._session.patch(url, json=body, headers=self.tm.get_json_headers(), timeout=30)
        self._handle(resp)

    def add_result_attachment(self, run_id: int, result_id: int, b64: str,
                            file_name: str, comment: str = ""):
        """POST a base64-encoded attachment (e.g. a screenshot) to a test result."""
        body = {
            "stream": b64,
            "fileName": file_name,
            "attachmentType": "GeneralAttachment",
        }
        if comment:
            body["comment"] = comment[:1000]
        url = (
            f"{self._base()}/test/Runs/{run_id}/Results/{result_id}/attachments"
            f"?api-version={API_VERSION}"
        )
        resp = self._session.post(url, json=body, headers=self.tm.get_json_headers(), timeout=60)
        self._handle(resp)

    def complete_test_run(self, run_id: int):
        """PATCH the run to the Completed state (plain-JSON PATCH)."""
        url = f"{self._base()}/test/runs/{run_id}?api-version={API_VERSION}"
        resp = self._session.patch(
            url, json={"state": "Completed"},
            headers=self.tm.get_json_headers(), timeout=30,
        )
        self._handle(resp)
