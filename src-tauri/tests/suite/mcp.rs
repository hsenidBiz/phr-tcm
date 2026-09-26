
/// A request carries an id, and JSON-RPC says every request gets exactly
/// one response. Returning None for anything unparseable meant the client
/// sat waiting on an id that was never going to come back - the stdio
/// transport has no timeout of its own to rescue it.
#[test]
fn a_request_always_gets_an_answer_even_when_it_makes_no_sense() {
    let call = |_: &str, _: &str, _: &str| Ok((200, "{}".to_string()));

    // Not JSON at all: the id is unknowable, so it is null - which is what
    // the protocol prescribes, and is still an answer.
    let r = v2_lib::mcp::handle_message("{not json", "1.0", &call).expect("must answer");
    assert!(r.contains("-32700"), "got {r}");
    assert!(r.contains("\"id\":null"), "got {r}");

    // JSON, has an id, but no method.
    let r = v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","id":7}"#, "1.0", &call)
        .expect("must answer");
    assert!(r.contains("-32600"), "got {r}");
    assert!(r.contains("\"id\":7"), "the answer must carry the id it was asked with: {r}");

    // A NOTIFICATION has no id and must still get no response - that part
    // was always right, and answering one would be a protocol error.
    assert!(v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","method":"x"}"#, "1.0", &call).is_none());
    // Even an unparseable notification cannot be told apart from a request,
    // so it gets the null-id parse error. That is the protocol's answer.
}
