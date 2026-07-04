"""DevOpsClient.get_all_suites — the Test Suites browser's one new API call.

Qt-free: the client is built with a stub token manager and its HTTP session is
replaced with a fake that replays canned pages, so these run under plain
pytest. Verifies pagination, the flat parent-link shape the tree is rebuilt
from, and that the call is strictly read-only (GET only).
"""

import json

from app.api.devops_client import DevOpsClient


class _FakeTM:
    org_url = "https://dev.azure.com/org"
    project = "Proj"

    def get_json_headers(self):
        return {"Authorization": "Bearer fake"}


class _FakeResponse:
    def __init__(self, payload, headers=None):
        self.status_code = 200
        self._payload = payload
        self.headers = headers or {}
        self.text = json.dumps(payload)
        self.url = "https://dev.azure.com/org/Proj/_apis/fake"

    def raise_for_status(self):
        pass   # always 200 in these tests

    def json(self):
        return self._payload


class _FakeSession:
    """Replays one canned response per GET, records every request made."""

    def __init__(self, responses):
        self._responses = list(responses)
        self.calls = []   # (method, url)

    def get(self, url, headers=None, timeout=None):
        self.calls.append(("GET", url))
        return self._responses.pop(0)

    def __getattr__(self, name):
        # Any non-GET verb (post/patch/delete/...) is a test failure.
        raise AssertionError(f"get_all_suites must only GET, attempted: {name}")


def _client(responses):
    client = DevOpsClient(_FakeTM())
    client._session = _FakeSession(responses)
    return client


def _suite(sid, name, parent=None, stype="staticTestSuite", req=None):
    s = {"id": sid, "name": name, "suiteType": stype}
    if parent is not None:
        s["parentSuite"] = {"id": parent}
    if req is not None:
        s["requirementId"] = req
    return s


def test_get_all_suites_flat_shape_and_parent_links():
    client = _client([_FakeResponse({"value": [
        _suite(1, "Root Plan Suite"),                    # root: no parentSuite
        _suite(2, "Regression", parent=1),
        _suite(3, "AI Insights", parent=2, stype="requirementTestSuite", req=777),
        _suite(4, "Smoke query", parent=1, stype="dynamicTestSuite"),
    ]})])
    suites = client.get_all_suites(42)

    assert [s["id"] for s in suites] == [1, 2, 3, 4]
    by_id = {s["id"]: s for s in suites}
    assert by_id[1]["parent_id"] is None            # root suite
    assert by_id[2]["parent_id"] == 1
    assert by_id[3]["parent_id"] == 2
    assert by_id[3]["suite_type"] == "requirementTestSuite"
    assert by_id[3]["requirement_id"] == 777
    assert by_id[4]["suite_type"] == "dynamicTestSuite"
    assert by_id[2]["requirement_id"] is None


def test_get_all_suites_follows_continuation_tokens():
    client = _client([
        _FakeResponse({"value": [_suite(1, "Root")]},
                      headers={"x-ms-continuationtoken": "page2"}),
        _FakeResponse({"value": [_suite(2, "Child", parent=1)]}),
    ])
    suites = client.get_all_suites(7)

    assert [s["id"] for s in suites] == [1, 2]
    session = client._session
    assert len(session.calls) == 2
    assert "continuationToken=page2" in session.calls[1][1]
    assert "continuationToken" not in session.calls[0][1]


def test_get_all_suites_is_get_only_and_scoped_to_plan():
    client = _client([_FakeResponse({"value": []})])
    assert client.get_all_suites(99) == []
    (method, url), = client._session.calls
    assert method == "GET"
    assert "/testplan/Plans/99/suites" in url
    assert url.startswith("https://dev.azure.com/org/Proj/_apis/")


def test_get_test_points_includes_browser_fields():
    """The points dict carries the case name/state and tester the browser shows."""
    client = _client([_FakeResponse({"value": [{
        "id": 5,
        "testCaseReference": {"id": 122514, "name": "TC_Verify AI Insights",
                              "state": "Design"},
        "configuration": {"id": 1, "name": "Windows 10"},
        "tester": {"displayName": "Hansani Gunasekara"},
        "results": {"outcome": "paused", "lastTestRunId": 3, "lastResultId": 9},
    }]})])
    (pt,) = client.get_test_points(1, 2)

    assert pt["test_case_name"] == "TC_Verify AI Insights"
    assert pt["test_case_state"] == "Design"
    assert pt["tester"] == "Hansani Gunasekara"
    assert pt["last_outcome"] == "paused"
    assert pt["config_name"] == "Windows 10"
