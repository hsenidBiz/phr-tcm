"""_parse_step_rows — clipboard text -> step (action, expected) pairs.

Qt-free: the parser is a module-level pure function, so it runs under plain
pytest without constructing any widgets.
"""

from app.utils.steps_paste import parse_step_rows as _parse_step_rows


def test_single_value_is_left_alone():
    # A plain one-cell paste must NOT be split into steps.
    assert _parse_step_rows("just some text") == []
    assert _parse_step_rows("") == []
    assert _parse_step_rows("   ") == []


def test_multiline_becomes_actions():
    assert _parse_step_rows("open app\nlogin\nlogout") == [
        ("open app", ""), ("login", ""), ("logout", "")]


def test_tab_splits_action_and_expected():
    text = "Click Save\tRecord is saved\nReopen\tData persists"
    assert _parse_step_rows(text) == [
        ("Click Save", "Record is saved"),
        ("Reopen", "Data persists")]


def test_single_line_with_tab_splits():
    assert _parse_step_rows("Action A\tExpected A") == [("Action A", "Expected A")]


def test_blank_lines_and_crlf_are_handled():
    text = "step 1\r\n\r\nstep 2\r\n"
    assert _parse_step_rows(text) == [("step 1", ""), ("step 2", "")]


def test_extra_columns_past_two_are_ignored():
    assert _parse_step_rows("a\tb\tc\td") == [("a", "b")]
