
// ---------------------------------------------------------- audit fixes

/// One typo in `where` used to leave an all-None filter, which matches
/// EVERY case - so a targeted retag silently rewrote the whole draft and
/// reported success.
#[test]
fn an_unknown_where_key_is_refused_rather_than_matching_everything() {
    let ops = serde_json::json!([
        { "op": "set_module", "value": "Payments",
          "where": { "title_contain": "login" } }
    ]);
    let err = v2_lib::transform::parse_ops(&ops).unwrap_err();
    assert!(err.contains("title_contain"), "got {err}");
    assert!(err.contains("unknown key"), "got {err}");
}

#[test]
fn a_where_clause_of_the_wrong_shape_is_refused() {
    for bad in [
        serde_json::json!([{ "op": "set_module", "value": "P", "where": "login" }]),
        serde_json::json!([{ "op": "set_module", "value": "P", "where": { "has_tag": 7 } }]),
    ] {
        assert!(v2_lib::transform::parse_ops(&bad).is_err(), "accepted {bad}");
    }
}

/// No `where` still means every case - that is documented behaviour and
/// must keep working.
#[test]
fn an_absent_where_still_means_every_case() {
    let ops = serde_json::json!([{ "op": "set_module", "value": "Payments" }]);
    assert_eq!(v2_lib::transform::parse_ops(&ops).unwrap().len(), 1);
}

/// `str_of` returned "" for an absent key, so a mistyped "vlaue" blanked
/// the field on every matched case and reported it applied.
#[test]
fn a_missing_value_is_refused_but_an_explicit_empty_one_still_clears() {
    let typo = serde_json::json!([{ "op": "set_tags", "vlaue": "smoke" }]);
    let err = v2_lib::transform::parse_ops(&typo).unwrap_err();
    assert!(err.contains("value"), "got {err}");

    // Clearing a field on purpose is a legitimate edit and still works.
    let clear = serde_json::json!([{ "op": "set_tags", "value": "" }]);
    assert!(v2_lib::transform::parse_ops(&clear).is_ok());

    let wrong_type = serde_json::json!([{ "op": "set_module", "value": 7 }]);
    assert!(v2_lib::transform::parse_ops(&wrong_type).is_err());
}
