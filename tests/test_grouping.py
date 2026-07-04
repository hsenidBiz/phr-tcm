"""Tests for app.utils.grouping.group_indices (Qt-free smart title grouping)."""
from app.utils.grouping import group_indices


def _named(result):
    """Turn the (name, idxs) result into {name: [idxs]} for easy assertions."""
    return {name: idxs for name, idxs in result}


def test_delimiter_prefix_groups_shared_category():
    titles = [
        "Login - valid credentials",
        "Login - locked out",
        "Checkout: empty cart",
    ]
    groups = _named(group_indices(titles))
    assert groups["Login"] == [0, 1]
    # "Checkout" is alone → ungrouped, not a single-item folder.
    assert "Checkout" not in groups
    assert groups[""] == [2]


def test_various_delimiters_recognised():
    titles = [
        "Auth | sign in",
        "Auth | sign out",
        "Billing / invoice",
        "Billing / refund",
    ]
    groups = _named(group_indices(titles))
    assert groups["Auth"] == [0, 1]
    assert groups["Billing"] == [2, 3]
    assert "" not in groups


def test_earliest_delimiter_wins():
    # " - " (index 5) is earlier than ": " (index 13), so the prefix is "Login".
    titles = ["Login - step: one", "Login - step: two"]
    groups = _named(group_indices(titles))
    assert groups["Login"] == [0, 1]


def test_word_prefix_fallback_when_no_delimiter():
    titles = ["User can login", "User can logout", "Admin dashboard loads"]
    groups = _named(group_indices(titles))
    assert groups["User can"] == [0, 1]
    assert groups[""] == [2]   # single "Admin…" → ungrouped


def test_word_prefix_uses_first_word_bucket_common_prefix():
    titles = ["Verify homepage loads", "Verify checkout works"]
    groups = _named(group_indices(titles))
    # Share only the first word → folder named "Verify".
    assert groups["Verify"] == [0, 1]


def test_case_insensitive_delimiter_grouping():
    titles = ["LOGIN - a", "login - b", "Login - c"]
    groups = _named(group_indices(titles))
    # All three collapse into one folder (display uses the first occurrence).
    assert list(groups.keys()) == ["LOGIN"]
    assert groups["LOGIN"] == [0, 1, 2]


def test_bare_hyphen_is_not_a_delimiter():
    # A hyphen with no surrounding spaces must not split ordinary words.
    titles = ["sign-in works", "sign-out works"]
    groups = _named(group_indices(titles))
    # Falls back to word prefix → both start "sign-in"/"sign-out", first word
    # differs so they bucket separately → ungrouped.
    assert groups.get("") == [0, 1]


def test_every_index_present_exactly_once():
    titles = [
        "Login - a", "Login - b", "Checkout: x", "Checkout: y",
        "Standalone title", "User can do a", "User can do b",
    ]
    result = group_indices(titles)
    seen = sorted(i for _, idxs in result for i in idxs)
    assert seen == list(range(len(titles)))


def test_groups_sorted_alphabetically_ungrouped_last():
    titles = ["Zebra - a", "Zebra - b", "Apple - a", "Apple - b", "lonely"]
    names = [name for name, _ in group_indices(titles)]
    assert names == ["Apple", "Zebra", ""]


def test_empty_and_blank_titles():
    titles = ["", "   ", "Login - a", "Login - b"]
    groups = _named(group_indices(titles))
    assert groups["Login"] == [2, 3]
    assert groups[""] == [0, 1]


def test_empty_input():
    assert group_indices([]) == []
