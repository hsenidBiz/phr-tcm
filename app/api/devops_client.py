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

    def validate_project(self) -> str:
        """
        GET the project to confirm the token and URL are valid.
        Returns the project display name. Safe — read only.
        """
        url = f"{self.tm.org_url}/_apis/projects/{self.tm.project}?api-version={API_VERSION}"
        resp = requests.get(url, headers=self.tm.get_json_headers(), timeout=15)
        data = self._handle(resp)
        return data.get("name", self.tm.project)

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
            "System.CreatedBy",
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
