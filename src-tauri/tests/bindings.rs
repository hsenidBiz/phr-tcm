//! Deterministic TS bindings generator: `cargo test --test bindings`.
//! Fails compilation if any command's types stop being specta-exportable.
//!
//! NOTE: all Rust tests in this crate live in tests/ (integration targets),
//! never as lib unit tests — see build.rs: only integration-test binaries get
//! the Common-Controls v6 manifest link args, and any test binary linking
//! tauri dies at startup without them (STATUS_ENTRYPOINT_NOT_FOUND). Every
//! test but these is a module of the one tests/suite/ binary; this file
//! stays a binary of its own because it is the generator.

/// No token may appear in any type exported over IPC.
///
/// Written to a file of its own, never ../src/bindings.ts: `export_bindings`
/// writes that one, tests run in parallel, and reading it while the other
/// test truncates it made this check read a half-written file.
#[test]
fn bindings_never_expose_a_token() {
    let path = std::env::temp_dir().join(format!("tcm-bindings-token-check-{}.ts", std::process::id()));
    v2_lib::specta_builder()
        .export(specta_typescript::Typescript::default(), &path)
        .expect("export failed");
    let ts = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(token_names_in(&ts), Vec::<&str>::new(), "generated bindings must not contain a token field");
}

/// Every token-shaped name in `text`, compared case-insensitively - so
/// `AccessToken` and `ACCESS_TOKEN` count as much as `accessToken`. The
/// id-token forms need a word boundary before them (start, a
/// non-alphanumeric, or a camel-case `Id`), because `invalidToken` and
/// `valid_token` contain them by accident.
fn token_names_in(text: &str) -> Vec<&'static str> {
    let lower = text.to_lowercase();
    let mut hits = Vec::new();
    // `requestverificationtoken` and `cookie`: an API template run's
    // anti-forgery token and session cookies stay in the page (design doc
    // "API templates" §10). Not `antiforgery`: the template's
    // `Antiforgery` block - the page to read a token from - is legitimate.
    for name in [
        "access_token",
        "accesstoken",
        "refresh_token",
        "refreshtoken",
        "bearer",
        "requestverificationtoken",
        "cookie",
    ] {
        if lower.contains(name) {
            hits.push(name);
        }
    }
    for name in ["id_token", "idtoken"] {
        // ASCII needles: a match offset in `lower` is a char boundary, and
        // lowercasing ASCII keeps offsets, so the same offset in `text`
        // (when it is ASCII there too) is the original letter.
        let at_boundary = lower.match_indices(name).any(|(i, _)| {
            let before = lower[..i].chars().next_back();
            let camel = text.get(i..i + 1) == Some("I");
            camel || before.is_none_or(|c| !c.is_alphanumeric())
        });
        if at_boundary {
            hits.push(name);
        }
    }
    hits
}

#[test]
fn the_token_check_ignores_case_but_not_word_boundaries() {
    assert_eq!(token_names_in("export type X = { AccessToken: string }"), vec!["accesstoken"]);
    assert_eq!(token_names_in("ACCESS_TOKEN"), vec!["access_token"]);
    assert_eq!(token_names_in("RefreshToken"), vec!["refreshtoken"]);
    assert_eq!(token_names_in("Authorization: Bearer x"), vec!["bearer"]);
    assert_eq!(token_names_in("{ idToken: string }"), vec!["idtoken"]);
    assert_eq!(token_names_in("{ IdToken: string }"), vec!["idtoken"]);
    assert_eq!(token_names_in("{ userIdToken: string }"), vec!["idtoken"]);
    assert_eq!(token_names_in("{ id_token: string }"), vec!["id_token"]);
    assert_eq!(token_names_in("{ invalidToken: boolean }"), Vec::<&str>::new());
    assert_eq!(token_names_in("{ valid_token: boolean }"), Vec::<&str>::new());
    assert_eq!(token_names_in("{ __RequestVerificationToken: string }"), vec!["requestverificationtoken"]);
    assert_eq!(token_names_in("{ sessionCookie: string }"), vec!["cookie"]);
    assert_eq!(token_names_in("export type Antiforgery = { page: string }"), Vec::<&str>::new());
}

#[test]
fn export_bindings() {
    v2_lib::specta_builder()
        .export(
            specta_typescript::Typescript::default(),
            "../src/bindings.ts",
        )
        .expect("failed to export typescript bindings");
}

/// tauri.conf.json's "version" is the app's real version - it names the
/// release, the update feed, and the What's-new entry. Cargo.toml's only
/// names the compile ("Compiling v2 v1.19.18" while shipping 1.20.0), but
/// a drifted one turns build logs and cargo metadata into misinformation.
/// The bump routine edits both; this makes forgetting one fail the gate.
#[test]
fn cargo_version_matches_tauri_conf() {
    let conf: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("tauri.conf.json").unwrap()).unwrap();
    let app = conf["version"].as_str().expect("tauri.conf.json has no version");
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        app,
        "Cargo.toml package.version must match tauri.conf.json - bump both together"
    );
}

/// Every identifier-shaped word in `text`.
fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect()
}

/// Whether `body` declares a field named exactly `password` that is not a
/// boolean (a flag that a field is a password field holds no password).
fn declares_password_field(body: &str) -> bool {
    body.match_indices("password").any(|(i, _)| {
        let before = body[..i].chars().next_back();
        let after = body[i + "password".len()..].trim_start_matches('?');
        before.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
            && after.starts_with(':')
            && !after[1..].trim_start().starts_with("boolean")
    })
}

/// One IPC call carries a password to the webview: `auto_run_list_accounts`
/// (CLAUDE.md). No other command's RESULT may contain, however deeply, a
/// field named `password` or the `Account` type. Inputs are not checked: a
/// save sends the list the person typed.
#[test]
fn only_the_account_list_carries_a_password_to_the_webview() {
    let path = std::env::temp_dir().join(format!("tcm-bindings-password-check-{}.ts", std::process::id()));
    v2_lib::specta_builder()
        .export(specta_typescript::Typescript::default(), &path)
        .expect("export failed");
    let ts = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);

    // Type name -> its body, with comment lines left out.
    let mut types: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for chunk in ts.split("export type ").skip(1) {
        let Some((name, rest)) = chunk.split_once(" =") else { continue };
        let name = name.trim().to_string();
        let end = rest.find("\nexport ").or_else(|| rest.find("\n// ")).unwrap_or(rest.len());
        let body: String = rest[..end]
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !(t.starts_with("/**") || t.starts_with('*') || t.starts_with("//"))
            })
            .collect::<Vec<_>>()
            .join("\n");
        types.insert(name, body);
    }
    assert!(types.contains_key("Account_Serialize"), "the Account type was not found in the bindings");

    let carries = |start: &str| -> bool {
        let mut seen = std::collections::HashSet::new();
        let mut todo: Vec<String> = words(start).into_iter().map(String::from).collect();
        while let Some(w) = todo.pop() {
            if w == "Account" || w.starts_with("Account_") {
                return true;
            }
            if !seen.insert(w.clone()) {
                continue;
            }
            if let Some(body) = types.get(&w) {
                if declares_password_field(body) {
                    return true;
                }
                todo.extend(words(body).into_iter().map(String::from));
            }
        }
        false
    };

    let mut offenders = vec![];
    let mut checked = 0;
    for line in ts.lines() {
        let Some(at) = line.find("typedError<") else { continue };
        let Some(end) = line.find("(__TAURI_INVOKE(\"") else { continue };
        let name = line[end + "(__TAURI_INVOKE(\"".len()..].split('"').next().unwrap_or("");
        let result = &line[at + "typedError<".len()..end];
        let result = result.rsplit_once(',').map_or(result, |(ok, _)| ok);
        checked += 1;
        if carries(result) {
            offenders.push(name.to_string());
        }
    }
    assert!(checked > 100, "the command lines were not recognised ({checked})");
    assert_eq!(
        offenders,
        ["auto_run_list_accounts"],
        "only auto_run_list_accounts may return a password to the webview"
    );
}
