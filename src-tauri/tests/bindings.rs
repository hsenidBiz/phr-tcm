//! Deterministic TS bindings generator: `cargo test --test bindings`.
//! Fails compilation if any command's types stop being specta-exportable.
//!
//! NOTE: all Rust tests in this crate live in tests/ (integration targets),
//! never as lib unit tests — see build.rs: only integration-test binaries get
//! the Common-Controls v6 manifest link args, and any test binary linking
//! tauri dies at startup without them (STATUS_ENTRYPOINT_NOT_FOUND).

/// Token must never appear in any type exported over IPC.
#[test]
fn bindings_never_expose_a_token() {
    v2_lib::specta_builder()
        .export(
            specta_typescript::Typescript::default(),
            "../src/bindings.ts",
        )
        .expect("export failed");
    let ts = std::fs::read_to_string("../src/bindings.ts").unwrap();
    let lower = ts.to_lowercase();
    assert!(
        !lower.contains("access_token") && !lower.contains("accesstoken"),
        "generated bindings must not contain token fields"
    );
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
