"""TestCase.is_valid() rules — the queue-time gate for manual entry."""

from app.models.test_case import Step, TestCase


def _case(**overrides):
    base = dict(title="Login works", steps=[Step(action="Open the page", expected="Shows")])
    base.update(overrides)
    return TestCase(**base)


def test_valid_case_passes():
    ok, err = _case().is_valid()
    assert ok and err == ""


def test_title_required():
    ok, err = _case(title="   ").is_valid()
    assert not ok
    assert "Title" in err


def test_title_over_255_rejected():
    ok, err = _case(title="X" * 256).is_valid()
    assert not ok
    assert "255" in err


def test_title_exactly_255_allowed():
    ok, _ = _case(title="X" * 255).is_valid()
    assert ok


def test_steps_required():
    ok, err = _case(steps=[]).is_valid()
    assert not ok
    assert "step" in err.lower()


def test_empty_step_action_rejected():
    ok, err = _case(steps=[Step(action="ok"), Step(action="   ")]).is_valid()
    assert not ok
    assert "Step 2" in err


def test_automation_status_constrained():
    ok, err = _case(automation_status="Automated").is_valid()
    assert not ok
    assert "automation" in err.lower()
    assert _case(automation_status="Planned").is_valid()[0]


def test_comma_tags_rejected():
    ok, err = _case(tags="smoke, regression").is_valid()
    assert not ok
    assert "semicolon" in err.lower()


def test_semicolon_tags_allowed():
    ok, _ = _case(tags="smoke; regression").is_valid()
    assert ok
