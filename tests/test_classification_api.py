"""DevOpsClient.get_classification_paths — the Area/Iteration dropdown source.

Qt-free: the client is built with a stub token manager and its HTTP session is
replaced with a fake that replays a canned classification-node tree. Verifies
the tree is flattened to the backslash path strings ADO stores in
System.AreaPath / System.IterationPath, that the result is cached (one GET), and
that the call is strictly read-only (GET only).
"""

import json

from app.api.devops_client import DevOpsClient


class _FakeTM:
    org_url = "https://dev.azure.com/org"
    project = "HRM"

    def get_json_headers(self):
        return {"Authorization": "Bearer fake"}


class _FakeResponse:
    def __init__(self, payload):
        self.status_code = 200
        self._payload = payload
        self.headers = {}
        self.text = json.dumps(payload)
        self.url = "https://dev.azure.com/org/HRM/_apis/wit/classificationnodes"

    def raise_for_status(self):
        pass

    def json(self):
        return self._payload


class _FakeSession:
    def __init__(self, responses):
        self._responses = list(responses)
        self.calls = []

    def get(self, url, headers=None, timeout=None):
        self.calls.append(("GET", url))
        return self._responses.pop(0)

    def __getattr__(self, name):
        raise AssertionError(f"classification lookup must only GET: {name}")


# A project "HRM" with a nested area tree; the node `path` fields deliberately
# carry the extra \Area segment that the flattened output must NOT include.
_AREA_TREE = {
    "name": "HRM", "path": "\\HRM\\Area",
    "children": [
        {"name": "Gamma Guardians", "path": "\\HRM\\Area\\Gamma Guardians",
         "children": [
             {"name": "Backend", "path": "\\HRM\\Area\\Gamma Guardians\\Backend"},
         ]},
        {"name": "Alpha", "path": "\\HRM\\Area\\Alpha"},
    ],
}


def _client(responses):
    client = DevOpsClient(_FakeTM())
    client._session = _FakeSession(responses)
    return client


def test_flattens_tree_to_stored_path_values():
    client = _client([_FakeResponse(_AREA_TREE)])
    paths = client.get_classification_paths("areas")
    assert paths == [
        "HRM",
        "HRM\\Gamma Guardians",
        "HRM\\Gamma Guardians\\Backend",
        "HRM\\Alpha",
    ]


def test_result_is_cached_single_get():
    client = _client([_FakeResponse(_AREA_TREE)])
    client.get_classification_paths("areas")
    client.get_classification_paths("areas")   # served from cache, no 2nd GET
    assert len(client._session.calls) == 1


def test_get_only_and_hits_the_right_endpoint():
    client = _client([_FakeResponse(_AREA_TREE)])
    client.get_classification_paths("iterations")
    (method, url), = client._session.calls
    assert method == "GET"
    assert "/wit/classificationnodes/iterations" in url
