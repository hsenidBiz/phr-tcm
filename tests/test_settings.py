"""Settings persistence: the _ALLOWED_KEYS whitelist and draft-queue round-trip.
All paths are monkeypatched into tmp_path so the real ~/.devops_tc_creator is
never touched."""

import json

from app.models.test_case import Step, TestCase
from app.utils import settings


def _redirect(monkeypatch, tmp_path):
    monkeypatch.setattr(settings, "_SETTINGS_PATH", tmp_path / "settings.json")
    monkeypatch.setattr(settings, "_DRAFT_PATH", tmp_path / "draft_queue.json")


def test_only_whitelisted_keys_are_saved(monkeypatch, tmp_path):
    _redirect(monkeypatch, tmp_path)
    settings.save_settings({"org_url": "https://dev.azure.com/x", "bearer_token": "SECRET"})
    on_disk = json.loads((tmp_path / "settings.json").read_text(encoding="utf-8"))
    assert on_disk["org_url"] == "https://dev.azure.com/x"
    assert "bearer_token" not in on_disk


def test_load_filters_unknown_keys(monkeypatch, tmp_path):
    _redirect(monkeypatch, tmp_path)
    (tmp_path / "settings.json").write_text(
        json.dumps({"project": "Proj", "rogue_key": 1}), encoding="utf-8"
    )
    loaded = settings.load_settings()
    assert loaded == {"project": "Proj"}


def test_corrupt_settings_returns_empty(monkeypatch, tmp_path):
    _redirect(monkeypatch, tmp_path)
    (tmp_path / "settings.json").write_text("{not json", encoding="utf-8")
    assert settings.load_settings() == {}


def test_save_merges_with_existing(monkeypatch, tmp_path):
    _redirect(monkeypatch, tmp_path)
    settings.save_settings({"org_url": "a"})
    settings.save_settings({"project": "b"})
    loaded = settings.load_settings()
    assert loaded["org_url"] == "a"
    assert loaded["project"] == "b"


def test_geometry_keys_are_whitelisted():
    assert "window_geometry" in settings._ALLOWED_KEYS
    assert "edit_splitter_sizes" in settings._ALLOWED_KEYS


def test_draft_queue_round_trip(monkeypatch, tmp_path):
    _redirect(monkeypatch, tmp_path)
    queue = [
        TestCase(
            title="Draft",
            steps=[Step(action="One", expected="A")],
            tags="t1; t2",
            update_id=7,
        )
    ]
    settings.save_draft_queue(queue)
    loaded = settings.load_draft_queue()
    assert len(loaded) == 1
    assert loaded[0].title == "Draft"
    assert loaded[0].update_id == 7
    assert loaded[0].steps == [Step(action="One", expected="A")]


def test_missing_draft_returns_empty(monkeypatch, tmp_path):
    _redirect(monkeypatch, tmp_path)
    assert settings.load_draft_queue() == []
    settings.clear_draft_queue()  # no-op on a missing file, must not raise


def test_hidden_work_items_round_trip_per_org(monkeypatch, tmp_path):
    _redirect(monkeypatch, tmp_path)
    org_a = "https://dev.azure.com/a"
    org_b = "https://dev.azure.com/b"
    settings.save_hidden_work_items(org_a, {3, 1, 2})
    settings.save_hidden_work_items(org_b, {9})
    assert settings.load_hidden_work_items(org_a) == {1, 2, 3}
    assert settings.load_hidden_work_items(org_b) == {9}
    assert settings.load_hidden_work_items("https://dev.azure.com/none") == set()


def test_hidden_work_items_empty_drops_entry(monkeypatch, tmp_path):
    _redirect(monkeypatch, tmp_path)
    org = "https://dev.azure.com/a"
    settings.save_hidden_work_items(org, {5})
    settings.save_hidden_work_items(org, set())   # unhid everything
    on_disk = json.loads((tmp_path / "settings.json").read_text(encoding="utf-8"))
    assert org not in on_disk.get("hidden_work_items", {})
    assert settings.load_hidden_work_items(org) == set()
