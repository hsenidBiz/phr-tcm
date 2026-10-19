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
    // A 63-character run, a long path and plain words stay.
    let run63 = "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxy";
    assert_eq!(run63.len(), 61);
    let text = format!("stays: {run63}01");
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
