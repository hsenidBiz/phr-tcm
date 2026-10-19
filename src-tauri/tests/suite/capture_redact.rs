//! What the assistant reads of a captured request: secrets hidden by name in
//! every shape a body or query takes, and nothing else cut - no length cap,
//! no address shortened.

use serde_json::{json, Value};
use v2_lib::autorun::capture_redact::{redact_body, redact_path_query, secret_name};

/// The sign-in password every test must never see in a result.
const PASSWORD: &str = "Hunter2-Pa55w0rd!";
const R: &str = "[redacted]";

fn never_shows(out: &str) {
    assert!(!out.contains(PASSWORD), "the password showed: {out}");
    assert!(!out.contains("Hunter2"), "part of the password showed: {out}");
}

#[test]
fn the_sign_in_password_never_shows() {
    // A form body, as the identity provider's sign-in page posts it.
    let form = format!("Input.Username=avin&Input.Password={PASSWORD}&__RequestVerificationToken=CfDJ8abc&ReturnUrl=%2Fhome");
    let out = redact_body(Some("application/x-www-form-urlencoded; charset=UTF-8"), &form);
    never_shows(&out);
    assert!(out.contains("Input.Username=avin"), "{out}");
    assert!(out.contains("ReturnUrl=%2Fhome"), "{out}");
    assert!(out.contains(&format!("Input.Password={R}")), "{out}");
    assert!(!out.contains("CfDJ8abc"), "{out}");
    // The name is matched decoded.
    never_shows(&redact_body(Some("application/x-www-form-urlencoded"), &format!("Input%2EPass%77ord={PASSWORD}")));

    // A JSON body, the password at any depth and inside an array.
    let json = json!({
        "user": "avin",
        "credentials": { "login": "avin", "Password": PASSWORD },
        "attempts": [ { "pwd": PASSWORD }, { "ok": true } ],
        "nested": serde_json::to_string(&json!({ "password": PASSWORD })).unwrap()
    })
    .to_string();
    let out = redact_body(Some("application/json"), &json);
    never_shows(&out);
    let shown: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(shown["user"], "avin");
    assert_eq!(shown["attempts"][1]["ok"], true);

    // JSON the body was cut in the middle of.
    never_shows(&redact_body(Some("application/json"), &format!("{{\"a\":1,\"password\":\"{PASSWORD}")));

    // A multipart body, the field and its value on lines of their own.
    let multipart = format!(
        "--XyZ\r\nContent-Disposition: form-data; name=\"username\"\r\n\r\navin\r\n\
         --XyZ\r\nContent-Disposition: form-data; name=\"Password\"\r\n\r\n{PASSWORD}\r\n\
         --XyZ\r\nContent-Disposition: form-data; name=\"note\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--XyZ--\r\n"
    );
    for mime in [Some("multipart/form-data; boundary=XyZ"), Some("multipart/form-data"), None] {
        let out = redact_body(mime, &multipart);
        never_shows(&out);
        assert!(out.contains("avin") && out.contains("hello"), "{out}");
        assert!(out.contains(&format!("{R}\r\n--XyZ")), "{out}");
    }
    // A secret part of several lines is hidden whole.
    let long = format!("--B\r\nContent-Disposition: form-data; name=\"client_secret\"\r\n\r\nline one {PASSWORD}\r\nline two {PASSWORD}\r\n--B--\r\n");
    never_shows(&redact_body(Some("multipart/form-data; boundary=B"), &long));

    // A query, plain, encoded and nested inside another address.
    never_shows(&redact_path_query(&format!("/login?user=avin&Password={PASSWORD}")));
    let out = redact_path_query(&format!("/login?user=avin&password={PASSWORD}&x=1#access_token={PASSWORD}"));
    never_shows(&out);
    assert!(out.starts_with("/login?user=avin&password=[redacted]&x=1"), "{out}");
    let nested = format!(
        "/signin?ReturnUrl={}",
        urlencoding::encode(&format!("/connect/authorize/callback?client_id=hr&client_secret={PASSWORD}"))
    );
    let out = redact_path_query(&nested);
    never_shows(&out);
    assert!(out.contains("client_id"), "{out}");
    // A form value that holds JSON.
    let wrapped = format!("payload={}", urlencoding::encode(&format!("{{\"password\":\"{PASSWORD}\",\"n\":1}}")));
    never_shows(&redact_body(Some("application/x-www-form-urlencoded"), &wrapped));
    // A body with no content type, judged by its look.
    never_shows(&redact_body(None, &format!("a=1&password={PASSWORD}")));
    never_shows(&redact_body(None, &format!("{{\"password\":\"{PASSWORD}\"}}")));
}

#[test]
fn a_page_hides_its_secret_fields_and_scripts() {
    let page = format!(
        "<form><input type=\"hidden\" name=\"__RequestVerificationToken\" value=\"CfDJ8aaa\" />\
         <input type=\"text\" name=\"Username\" value=\"avin\"><input name=\"Pass\" type=\"password\" value=\"{PASSWORD}\">\
         <meta name=\"csrf-token\" content=\"{PASSWORD}\"></form>\
         <script>var apiKey = '{PASSWORD}'; var cfg = {{\"clientSecret\":\"{PASSWORD}\",\"home\":\"/hr\"}};</script>\
         <Password>{PASSWORD}</Password>"
    );
    let out = redact_body(Some("text/html; charset=utf-8"), &page);
    never_shows(&out);
    assert!(!out.contains("CfDJ8aaa"), "{out}");
    assert!(out.contains("value=\"avin\""), "{out}");
    assert!(out.contains("\"home\":\"/hr\""), "{out}");
}

#[test]
fn a_jwt_is_hidden_under_an_innocent_key() {
    let jwt = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    // The key is innocent: OIDC answers carry the identity under `id`.
    let out = redact_body(Some("application/json"), &json!({ "id": jwt, "name": "avin" }).to_string());
    assert!(!out.contains("eyJ"), "{out}");
    assert!(!out.contains("SflKx"), "{out}");
    assert!(out.contains("avin"), "{out}");
    // In a form, a query, a page and a header-like line.
    for shown in [
        redact_body(Some("application/x-www-form-urlencoded"), &format!("state={jwt}")),
        redact_path_query(&format!("/cb?hint={jwt}")),
        redact_path_query(&format!("/files/{jwt}/open")),
        redact_body(Some("text/html"), &format!("<p>{jwt}</p>")),
        redact_body(Some("text/plain"), &format!("Authorization: Bearer {jwt}")),
    ] {
        assert!(!shown.contains("eyJ") && !shown.contains("SflKx"), "{shown}");
    }
    // A token that is not a JOSE one but has the same shape is hidden too.
    let opaque = "Ab12Cd34Ef56Gh78Ij90.Kl12Mn34Op56Qr78St90.Uv12Wx34Yz56Ab78Cd90";
    assert!(!redact_body(Some("application/json"), &json!({ "n": opaque }).to_string()).contains("Kl12"));
    // Long dotted names and addresses that are not tokens stay.
    let dotted = json!({ "type": "Microsoft.AspNetCore.Mvc.ModelBinding.Validation.ValidationState", "v": "1.2.3" });
    assert_eq!(redact_body(Some("application/json"), &dotted.to_string()), dotted.to_string());
}

#[test]
fn addresses_inside_bodies_are_kept() {
    let body = json!({
        "next": "https://hr.example/api/employees?page=2&size=50",
        "self": "/api/employees/7",
        "links": [ "https://hr.example/hr/leave/requests?status=open" ]
    })
    .to_string();
    assert_eq!(redact_body(Some("application/json"), &body), body);
    // A secret parameter inside such an address is hidden; the address stays.
    let body = json!({ "next": format!("https://hr.example/cb?code=1&access_token={PASSWORD}") }).to_string();
    let out = redact_body(Some("application/json"), &body);
    never_shows(&out);
    assert!(out.contains("https://hr.example/cb?code=1&access_token=[redacted]"), "{out}");
}

#[test]
fn nothing_is_cut_and_a_clean_body_is_untouched() {
    // Far over api_checks' 500-character excerpt and its 64 KB judging cap.
    let rows: Vec<Value> = (0..4000).map(|i| json!({ "id": i, "name": format!("employee {i}") })).collect();
    let body = Value::Array(rows).to_string();
    assert!(body.len() > 100_000);
    assert_eq!(redact_body(Some("application/json"), &body), body);
    // The same with a secret in the middle: only that value goes.
    let mut with = json!({ "rows": serde_json::from_str::<Value>(&body).unwrap(), "token": PASSWORD });
    let out = redact_body(Some("application/json"), &with.to_string());
    never_shows(&out);
    with["token"] = json!(R);
    assert_eq!(out, with.to_string());
    // Formatting and key order survive when there is nothing to hide.
    let pretty = "{\n  \"b\": 1,\n  \"a\": [1, 2]\n}";
    assert_eq!(redact_body(Some("application/json"), pretty), pretty);
    let page = "<html><body><h1>Leave</h1><a href=\"/hr/leave?id=3\">3</a></body></html>";
    assert_eq!(redact_body(Some("text/html"), page), page);
    let form = "a=1&b=two+words&c=%2Fx%3Fy%3D1";
    assert_eq!(redact_body(Some("application/x-www-form-urlencoded"), form), form);
    assert_eq!(redact_path_query("/api/search?q=a%20b&page=2"), "/api/search?q=a%20b&page=2");
    assert_eq!(redact_path_query("/plain/path"), "/plain/path");
}

#[test]
fn one_list_of_words_decides_every_name() {
    // api_checks' own words, and the ones headers carry keys under.
    for name in [
        "access_token", "Password", "client_secret", "Authorization", "Set-Cookie", "sessionId", "x-api-key",
        "Credential", "x-signature", "id_jwt", "Bearer", "X-CSRF-Token", "xsrfToken", "passwd", "PWD", "passphrase",
    ] {
        assert!(secret_name(name), "{name}");
    }
    for name in ["user", "page", "name", "ReturnUrl", "q", "size", "content-type"] {
        assert!(!secret_name(name), "{name}");
    }
}

/// A body is judged by what it is, not by the mime it claims.
#[test]
fn a_body_is_judged_by_its_shape_not_its_mime() {
    let json = r#"{"UserName":"a","Password":"hunter2"}"#;
    for mime in ["application/x-www-form-urlencoded", "text/plain", "multipart/form-data; boundary=zz", "text/html", ""] {
        let out = redact_body(Some(mime), json);
        assert!(!out.contains("hunter2"), "{mime}: {out}");
        assert!(out.contains("\"UserName\":\"a\""), "{mime}: {out}");
    }
    for mime in ["text/plain", "application/json", "text/html", "application/octet-stream"] {
        let out = redact_body(Some(mime), "UserName=a&Password=hunter2");
        assert!(!out.contains("hunter2"), "{mime}: {out}");
        assert!(out.contains("UserName=a"), "{mime}: {out}");
    }
    // A multipart body under a JSON mime.
    let multipart = "--q\r\nContent-Disposition: form-data; name=\"Password\"\r\n\r\nhunter2\r\n--q--\r\n";
    assert!(!redact_body(Some("application/json"), multipart).contains("hunter2"));
    // Lines of name: value in plain text.
    let lines = "UserName=a\r\nPassword=hunter2\r\nnote: hi";
    let out = redact_body(Some("text/plain"), lines);
    assert!(!out.contains("hunter2") && out.contains("UserName=a") && out.contains("note: hi"), "{out}");
    // A BOM before JSON.
    let out = redact_body(Some("application/json"), "\u{feff}{\"password\":\"hunter2\"}");
    assert!(!out.contains("hunter2"), "{out}");
}

#[test]
fn multipart_preamble_epilogue_and_padded_delimiters_are_covered() {
    let body = "password=hunter2\r\n--x \r\nContent-Disposition: form-data; name=\"Password\"\r\n\r\nhunter2\r\n--x  \r\nContent-Disposition: form-data; name=\"n\"\r\n\r\nok\r\n--x--\r\ntoken: hunter2\r\n";
    for mime in [Some("multipart/form-data; boundary=x"), None] {
        let out = redact_body(mime, body);
        assert!(!out.contains("hunter2"), "{out}");
        assert!(out.contains("ok"), "{out}");
    }
    // RFC 5987 part name, plain and encoded.
    let ext = "--x\r\nContent-Disposition: form-data; name*=UTF-8''password\r\n\r\nhunter2\r\n--x--\r\n";
    assert!(!redact_body(Some("multipart/form-data; boundary=x"), ext).contains("hunter2"));
    let enc = "--x\r\nContent-Disposition: form-data; name*=UTF-8''pass%77ord\r\n\r\nhunter2\r\n--x--\r\n";
    assert!(!redact_body(None, enc).contains("hunter2"));
}

#[test]
fn values_that_hold_forms_urls_fragments_and_graphql_are_redacted_inside() {
    let cases: Vec<(&str, String)> = vec![
        ("application/json", r#"{"d":"grant_type=password&password=hunter2"}"#.to_string()),
        ("application/json", r#"{"r":"cb?token=hunter2"}"#.to_string()),
        ("application/x-www-form-urlencoded", "ReturnUrl=%2Fcb%23access_token%3Dhunter2".to_string()),
        ("application/json", r#"{"query":"mutation { login(password:\"hunter2\") { ok } }"}"#.to_string()),
        ("application/json", r#"{"q":"mutation { login(user: \"a\", password: \"hunter2\") { ok } }"}"#.to_string()),
        ("application/json", r#"{"query":"login(password:\"hunter2\"#.to_string()),
        ("text/html", "<a href=\"/cb#id_token=hunter2\">x</a>".to_string()),
    ];
    for (mime, body) in cases {
        let out = redact_body(Some(mime), &body);
        assert!(!out.contains("hunter2"), "{body} -> {out}");
    }
    let out = redact_path_query("/cb?code=hunter2&state=s1&x=1");
    assert!(!out.contains("hunter2") && out.contains("state=s1") && out.contains("x=1"), "{out}");
    let out = redact_body(Some("application/json"), r#"{"code":"hunter2","state":"s1","n":1}"#);
    assert!(!out.contains("hunter2") && out.contains("s1"), "{out}");
    // `code` alone is no secret.
    assert_eq!(redact_path_query("/p?code=ABC&x=1"), "/p?code=ABC&x=1");
}

#[test]
fn saml_oauth_and_opaque_values_are_hidden() {
    let out = redact_body(
        Some("application/x-www-form-urlencoded"),
        "SAMLResponse=hunter2&RelayState=hunter2&id_token=hunter2&refresh_token=hunter2&code_verifier=hunter2&client_secret=hunter2&SAMLRequest=hunter2",
    );
    assert!(!out.contains("hunter2"), "{out}");
    // An echoed session cookie, under an innocent name, in JSON, text and a query.
    let cookie = "CfDJ8Hk3n2LzQ7vXr0aB5cD9eF1gH4iJ6kL8mN0pQ2rS4tU6vW8xY0zA1bC3dE5fG7hI9jK1lM3nO5pQ7rS9tU";
    assert!(cookie.len() >= 64);
    for shown in [
        redact_body(Some("application/json"), &format!(r#"{{"echo":"{cookie}"}}"#)),
        redact_body(Some("text/plain"), &format!("seen {cookie} here")),
        redact_path_query(&format!("/x?v={cookie}")),
    ] {
        assert!(!shown.contains("CfDJ8Hk3") && shown.contains("(hidden)"), "{shown}");
    }
    // A 39-character run, a long path and plain words stay.
    let run39 = "abcdefghijklmnopqrstuvwxyz0123456789abc";
    assert_eq!(run39.len(), 39);
    let text = format!("stays: {run39}");
    assert_eq!(redact_body(Some("text/plain"), &text), text);
    let path = "/hr/leave/requests/pending/approvals/detail/2024/some-long-slug-here-for-the-test-case";
    assert_eq!(redact_path_query(path), path);
}

#[test]
fn over_redaction_is_limited_to_real_secret_words() {
    for name in ["author", "isAuthorized", "authorizedBy", "keyword", "sortKey", "monkey", "keyboard", "primaryKeyName"] {
        assert!(!secret_name(name), "{name}");
    }
    for name in [
        "apiKey", "api_key", "x-api-key", "authToken", "auth_token", "auth-token", "auth", "x-auth-user", "APIKey",
        "privateKey", "client_secret", "samlResponse", "RelayState", "code_verifier", "id_token", "key",
    ] {
        assert!(secret_name(name), "{name}");
    }
    let body = r#"{"author":"Bob","isAuthorized":true,"keyword":"leave","sortKey":"date","apiKey":"hunter2"}"#;
    let out = redact_body(Some("application/json"), body);
    assert!(!out.contains("hunter2") && out.contains("Bob") && out.contains("leave") && out.contains("date"), "{out}");
}

#[test]
fn odd_encodings_do_not_hide_a_name_or_a_token() {
    // Invalid UTF-8 in a percent-escape of the name.
    let out = redact_body(Some("application/x-www-form-urlencoded"), "Pass%77ord%FF=hunter2&u=a");
    assert!(!out.contains("hunter2") && out.contains("u=a"), "{out}");
    // An encoded space after Bearer.
    let out = redact_body(Some("text/plain"), "h=Bearer%20hunter2opaque");
    assert!(!out.contains("hunter2"), "{out}");
    assert!(!redact_path_query("/x?a=Bearer%20hunter2opaque").contains("hunter2"));
    // A JWT split across lines: dots at line ends or starts, a part wrapped.
    let jwt_a = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9";
    let jwt_b = "eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ";
    let jwt_c = "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    for shown in [
        redact_body(Some("text/plain"), &format!("{jwt_a}.\n{jwt_b}.\n{jwt_c}")),
        redact_body(Some("text/plain"), &format!("{jwt_a}\r\n.{jwt_b}\r\n.{jwt_c}")),
        redact_body(Some("text/plain"), &format!("{}\n{}.{}.{}", &jwt_a[..20], &jwt_a[20..], jwt_b, jwt_c)),
    ] {
        assert!(!shown.contains("hbGciOi") && !shown.contains("zdWIi") && !shown.contains("SflKx"), "{shown}");
    }
}

#[test]
fn free_text_headers_and_fields_are_redacted_as_text() {
    use v2_lib::autorun::capture_redact::{redact_field, redact_header, redact_text};
    assert_eq!(redact_header("x-api-key", "hunter2"), "(hidden)");
    assert_eq!(redact_header("authorization", "Basic aHVudGVyMg=="), "(hidden)");
    let out = redact_header("x-return-url", "https://hr.example/cb?token=hunter2&a=1");
    assert!(!out.contains("hunter2") && out.contains("a=1"), "{out}");
    assert_eq!(redact_header("content-type", "application/json; charset=utf-8"), "application/json; charset=utf-8");
    assert_eq!(redact_field("password", "hunter2"), "[redacted]");
    assert_eq!(redact_field("note", "hi"), "hi");
    assert!(!redact_text("net::ERR failed for /x?access_token=hunter2").contains("hunter2"));
}

const RESET: &str = "CfDJ8Abc1De2Fg3Hi4Jk5Lm6No7Pq8Rs9Tu0VwXyZ1aB2cD3eF4gH5iJ6kL7mN8oP9qR0sT1uV2wX3yZ4aB5cD6eF7gH8iJ9kL0mN1oP2qR3sT4";

#[test]
fn a_token_written_with_encoded_slashes_and_equals_is_hidden() {
    // Standard base64 as a query value is written with %2B, %2F and %3D.
    let token = "CfDJ8Abc1De2Fg3Hi4Jk5Lm%2Bx%2Fy6No7Pq8Rs9Tu0VwXyZ1aB2cD3eF4gH5iJ6kL7mN8oP9qR0sT1uV2wX3yZ4aB5%2Fc%3D%3D";
    let link = format!("/Account/ResetPassword?userId=7&code={token}");
    let out = redact_path_query(&link);
    assert!(!out.contains("CfDJ8") && !out.contains("sT1uV2"), "{out}");
    assert!(out.starts_with("/Account/ResetPassword?userId=7&code="), "{out}");
    // The same inside a JSON link, in a form value and in prose.
    let json = format!(r#"{{"link":"https://hr.example{link}"}}"#);
    for shown in [
        redact_body(Some("application/json"), &json),
        redact_body(Some("text/plain"), &format!("Open https://hr.example{link} to reset")),
        redact_body(Some("application/x-www-form-urlencoded"), &format!("next={}", urlencoding::encode(&link))),
    ] {
        assert!(!shown.contains("CfDJ8") && !shown.contains("sT1uV2"), "{shown}");
    }
    // A bare token as a JSON value starting with a slash, and as a path segment.
    for shown in [
        redact_body(Some("application/json"), &format!(r#"{{"t":"/{RESET}"}}"#)),
        redact_path_query(&format!("/reset/{RESET}")),
        redact_path_query(&format!("/reset/{RESET}/confirm")),
    ] {
        assert!(!shown.contains("CfDJ8") && !shown.contains("sT1uV2"), "{shown}");
        assert!(shown.starts_with('/') || shown.contains("{\"t\":\"/"), "{shown}");
    }
    assert_eq!(redact_path_query(&format!("/reset/{RESET}/confirm")), "/reset/(hidden)/confirm");
    // Addresses stay: a long encoded redirect, a path of short segments.
    let ret = "/signin?ReturnUrl=%2Fhr%2Fleave%2Frequests%2Fpending%2Fapprovals%2Fdetail%2Fall";
    assert_eq!(redact_path_query(ret), ret);
    let path = "/hr/leave/requests/pending/approvals/detail/2024/some-long-slug-here-for-the-test-case";
    assert_eq!(redact_path_query(path), path);
}

#[test]
fn a_long_code_is_hidden_without_a_state() {
    let long = "AbCd1234EfGh5678IjKl9012";
    let out = redact_path_query(&format!("/cb?code={long}&x=1"));
    assert!(!out.contains(long) && out.contains("x=1"), "{out}");
    let out = redact_body(Some("application/json"), &format!(r#"{{"code":"{long}","n":1}}"#));
    assert!(!out.contains(long), "{out}");
    // A short code stays.
    assert_eq!(redact_path_query("/cb?code=AB12&x=1"), "/cb?code=AB12&x=1");
    assert_eq!(redact_body(Some("application/json"), r#"{"code":"E101","n":1}"#), r#"{"code":"E101","n":1}"#);
}

#[test]
fn key_headers_of_azure_style_services_are_secret() {
    for name in ["subscription-key", "Ocp-Apim-Subscription-Key", "x-functions-key", "host_key", "master-key", "x-auth-key"] {
        assert!(secret_name(name), "{name}");
    }
    for name in ["sortKey", "primaryKey", "foreignKey", "keyword", "keys", "key-count", "cacheKeyName"] {
        assert!(!secret_name(name), "{name}");
    }
}

#[test]
fn text_that_is_not_json_still_loses_its_secret_members() {
    for body in [r#"{"password":"x",}"#, r#"{"a":1,"password":"x" "b":2}"#, r#"[{"token": "x"},]"#, "{'password':\"x\"}"] {
        let out = redact_body(Some("application/json"), body);
        assert!(!out.contains("\"x\"") && !out.contains(":\"x") && out.contains("[redacted]"), "{body} -> {out}");
    }
}

#[test]
fn a_short_session_cookie_echoed_in_a_page_is_hidden() {
    let sid = "5kbsz4dpbtzs1x4aeuvq2o4t";
    assert_eq!(sid.len(), 24);
    for page in [
        format!("<p>Your session: ASP.NET_SessionId={sid}</p>"),
        format!("Set-Cookie: ASP.NET_SessionId={sid}; path=/; HttpOnly"),
        format!("Cookie: theme=dark; ASP.NET_SessionId={sid}"),
        format!("document.cookie = '.ASPXAUTH={}'", "A1B2C3D4E5F6A7B8C9D0E1F2"),
        format!(".AspNetCore.Cookies = {sid}"),
    ] {
        let out = redact_body(Some("text/html"), &page);
        assert!(!out.contains(sid) && !out.contains("A1B2C3D4E5F6A7B8"), "{page} -> {out}");
    }
    // A label with ordinary words after it is left alone.
    let prose = "The ASP.NET_SessionId cookie improves internationalization of pages";
    assert_eq!(redact_body(Some("text/plain"), prose), prose);
}

#[test]
fn business_values_that_are_long_but_not_token_shaped_show() {
    let guid = "3f2504e0-4f89-11d3-9a0c-0305e82c3301";
    let guids = format!("{guid}-{}", "9b2f6a10-7c3d-4e8a-b1f5-2d4c6e8a0b13");
    let shown = [
        "LV-ANNUAL-2026-FULLDAY-APPROVAL-WORKFLOW-STEP-2".to_string(),
        "api/v2/employees/12345/leave-requests/pending-approval".to_string(),
        "/api/v2/employees/12345/leave-requests/pending-approval".to_string(),
        guid.to_string(),
        guids.clone(),
        "Company.Module.SubModule.SomeLongTypeName2026".to_string(),
        "Company.Module.SubModule.SomeLongTypeName2026.Another.Part".to_string(),
        "2026-10-19T04:00:01.123456+05:30".to_string(),
        "12345678901234567890123456789012345678901234567890".to_string(),
    ];
    for value in &shown {
        assert_eq!(redact_body(Some("application/json"), &format!(r#"{{"v":"{value}"}}"#)), format!(r#"{{"v":"{value}"}}"#), "{value}");
        assert_eq!(redact_body(Some("text/plain"), &format!("see {value} now")), format!("see {value} now"), "{value}");
        let query = format!("/x?v={value}&w=1");
        assert_eq!(redact_path_query(&query), query, "{value}");
    }
    assert_eq!(redact_path_query("/api/v2/employees/12345/leave-requests?type=LV-ANNUAL-2026-FULLDAY"),
        "/api/v2/employees/12345/leave-requests?type=LV-ANNUAL-2026-FULLDAY");
    // A business `code` shows; a token-shaped one does not.
    assert_eq!(redact_path_query("/cb?code=LV-ANNUAL-2026-FULLDAY&x=1"), "/cb?code=LV-ANNUAL-2026-FULLDAY&x=1");
    assert_eq!(redact_path_query("/cb?code=LEAVETYPE12345678901234"), "/cb?code=LEAVETYPE12345678901234");
    assert!(!redact_path_query("/cb?code=AbCd1234EfGh5678IjKl9012&x=1").contains("AbCd1234"));
}

#[test]
fn token_shaped_values_are_hidden_in_every_place() {
    let token = "CfDJ8AbCdEf1GhIjKl2MnOpQr3StUvWx4YzAbCd5EfGhIjKl6MnOpQr7StUvWx8Yz";
    let hex40 = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b";
    let hex32 = "9f86d081884c7d659a2feaa0c55ad015";
    for secret in [token, hex40, hex32] {
        for shown in [
            redact_body(Some("application/json"), &format!(r#"{{"v":"{secret}"}}"#)),
            redact_body(Some("text/plain"), &format!("see {secret} now")),
            redact_path_query(&format!("/x?v={secret}&w=1")),
            redact_path_query(&format!("/reset/{secret}/confirm")),
            redact_path_query(&format!("/Account/Confirm/{secret}")),
        ] {
            assert!(!shown.contains(&secret[..12]), "{secret}: {shown}");
        }
    }
    // The coordinator's encoded confirmation link, tokens split by %2F, ending in %3D%3D.
    let link = format!("/Account/Confirm/{}%2F{}%2F{}%3D%3D", &token[..24], &token[24..48], &token[48..]);
    let out = redact_path_query(&link);
    assert!(!out.contains("CfDJ8") && !out.contains("GhIjKl") && out.starts_with("/Account/Confirm/"), "{out}");
    // Mixed case and digits in a token with a literal base64 marker.
    let std = "Zm9vYmFy+YmF6cXV4/Zm9vYmFyYmF6cXV4Zm9vYmFyYmF6cXV4Zm9v==";
    assert!(!redact_body(Some("text/plain"), &format!("t={std}")).contains("Zm9vYmFy"));
    // The key's value and id.
    let out = redact_body(Some("application/json"), &format!(r#"{{"apiKeyValue":"{hex32}","apiKeyId":"{hex32}","sortKeyId":"k1","keyword":"leave"}}"#));
    assert!(!out.contains(hex32) && out.contains("\"k1\"") && out.contains("leave"), "{out}");
    assert!(secret_name("apiKeyValue") && secret_name("x-api-key-id") && secret_name("subscriptionKeyValue"));
    assert!(!secret_name("sortKeyValue") && !secret_name("keyId") && !secret_name("primaryKeyId"));
}

#[test]
fn authenticator_secrets_and_recovery_codes_are_hidden() {
    let base32 = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";
    // Under their own names, an array of codes included.
    let out = redact_body(Some("application/json"), &format!(r#"{{"sharedKey":"{base32}","user":"avin"}}"#));
    assert!(!out.contains("JBSWY3DP") && out.contains("avin"), "{out}");
    let out = redact_body(Some("application/json"), r#"{"recoveryCodes":["8f3k-29dk","x9q2-1ld0"],"user":"avin"}"#);
    assert!(!out.contains("8f3k-29dk") && !out.contains("x9q2-1ld0") && out.contains("avin"), "{out}");
    for name in ["sharedKey", "authenticatorKey", "authenticatorUri", "recoveryCodes", "totpSecret", "otp", "mfa_code",
        "twoFactorCode", "two_factor_key", "TwoFactorRecoveryCode"] {
        assert!(secret_name(name), "{name}");
    }
    assert!(!secret_name("footprint") && !secret_name("department") && !secret_name("score"));
    // As a form field and a query parameter.
    let out = redact_body(Some("application/x-www-form-urlencoded"), "otp=654321&user=avin");
    assert!(!out.contains("654321") && out.contains("user=avin"), "{out}");
    assert!(!redact_path_query("/mfa/verify?totp=482913&x=1").contains("482913"));

    // ASP.NET Identity's authenticator page shows the key in lower-case groups.
    let page = "<p>Enter this key <kbd>jbsw y3dp ehpk 3pxp</kbd> into your app.</p>";
    let out = redact_body(Some("text/html"), page);
    assert!(!out.contains("jbsw") && !out.contains("y3dp") && !out.contains("3pxp"), "{out}");
    assert!(out.contains("Enter this key") && out.contains("into your app."), "{out}");
    // Its 26-character key ends with a short group.
    let out = redact_body(Some("text/html"), "<kbd>jbsw y3dp ehpk 3pxp jbsw y3dp ehpk 3p</kbd>");
    assert!(!out.contains("y3dp") && !out.contains(" 3p<"), "{out}");

    // A base32 value under an innocent name, upper or lower case.
    for value in ["JBSWY3DPEHPK3PXP", "jbswy3dpehpk3pxp", base32] {
        let out = redact_body(Some("application/json"), &format!(r#"{{"value":"{value}"}}"#));
        assert!(!out.contains(&value[..8]), "{value}: {out}");
        let out = redact_body(Some("text/plain"), &format!("key: is {value} now"));
        assert!(!out.contains(&value[..8]), "{value}: {out}");
        assert!(!redact_path_query(&format!("/x?v={value}&w=1")).contains(&value[..8]), "{value}");
    }

    // An otpauth URI goes whole, raw or encoded.
    let uri = "otpauth://totp/App:user?secret=JBSWY3DPEHPK3PXP&issuer=App";
    let out = redact_body(Some("application/json"), &format!(r#"{{"qr":"{uri}","user":"avin"}}"#));
    assert!(!out.contains("otpauth") && !out.contains("JBSWY3DP") && out.contains("avin"), "{out}");
    let out = redact_body(Some("text/html"), &format!(r#"<div data-url="{uri}"></div><p>Scan it</p>"#));
    assert!(!out.contains("otpauth") && !out.contains("issuer") && out.contains("Scan it"), "{out}");
    let encoded = urlencoding::encode(uri);
    let out = redact_path_query(&format!("/qr?chl={encoded}&chs=200x200"));
    assert!(!out.contains("otpauth") && !out.contains("JBSWY3DP") && out.contains("chs=200x200"), "{out}");
}

#[test]
fn ordinary_text_is_not_taken_for_an_authenticator_secret() {
    for text in [
        "DEPARTMENT",
        "ADMINISTRATIONDEPARTMENT",
        "LV-ANNUAL-2026-FULLDAY",
        "Please enter the code from your authenticator app to continue.",
        "this will take some time to load from the server",
        "call 3456 7234 then wait",
        "Leave type ANNUALLEAVE2026FULL was approved on 2026-10-19.",
    ] {
        assert_eq!(redact_body(Some("text/plain"), text), text);
        let json = json!({ "v": text }).to_string();
        assert_eq!(redact_body(Some("application/json"), &json), json);
    }
    assert_eq!(redact_path_query("/x?type=LV-ANNUAL-2026-FULLDAY&dept=DEPARTMENT"),
        "/x?type=LV-ANNUAL-2026-FULLDAY&dept=DEPARTMENT");
}
