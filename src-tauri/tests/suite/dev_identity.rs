//! A development build runs beside the installed app with its own identity
//! and its own data (`dev_identity`). Release behaviour is unchanged.

use v2_lib::dev_identity::*;

fn conf() -> serde_json::Value {
    serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap()
}

#[test]
fn a_release_build_keeps_the_identifier_in_the_config() {
    assert_eq!(identifier(false), conf()["identifier"].as_str().unwrap());
    assert_eq!(identifier(false), RELEASE_IDENTIFIER);
}

#[test]
fn a_development_build_has_its_own_identifier_and_so_its_own_folders() {
    let dev = identifier(true);
    assert_ne!(dev, identifier(false));
    assert_eq!(dev, format!("{RELEASE_IDENTIFIER}.dev"));
    // Tauri puts the roaming, local and log folders and the WebView2 profile
    // under the identifier, so the folders differ with it.
    let roaming = || Some(std::ffi::OsString::from("C:/Roaming"));
    let release = v2_lib::modules::data_dir_for(roaming(), false).unwrap();
    let development = v2_lib::modules::data_dir_for(roaming(), true).unwrap();
    assert_ne!(release, development);
    assert!(!development.starts_with(&release), "the development folder is beside the release one, not inside it");
}

#[test]
fn the_single_instance_keys_differ() {
    assert_ne!(single_instance_key(&identifier(true)), single_instance_key(&identifier(false)));
}

#[test]
fn credential_targets_differ_and_never_list_each_others_entries() {
    let name = "tcm-v2/db/own";
    assert_eq!(credential_target(name, false), name, "a release build keeps its targets as they were");
    let dev = credential_target(name, true);
    assert_ne!(dev, name);
    assert!(!dev.starts_with("tcm-v2/"), "the installed app's listing of tcm-v2/ cannot reach it: {dev}");
    assert!(!credential_target("tcm-v2/auth/session", true).starts_with("tcm-v2/"));

    // Listing: each build sees only its own, under the names its callers use.
    assert_eq!(store_name(&dev, true).as_deref(), Some(name));
    assert_eq!(store_name(name, true), None);
    assert_eq!(store_name(name, false).as_deref(), Some(name));
    assert_eq!(store_name(&dev, false), None);
}

#[test]
fn the_handshake_file_and_the_startup_entry_differ() {
    assert_eq!(handshake_file_name(false), "tcm-v2-mcp-bridge.json");
    assert_ne!(handshake_file_name(true), handshake_file_name(false));
    assert_eq!(autostart_app_name("Test Case Manager V2", false), "Test Case Manager V2");
    assert_ne!(autostart_app_name("Test Case Manager V2", true), "Test Case Manager V2");
}

fn config(title: &str) -> tauri::Config {
    serde_json::from_value(serde_json::json!({
        "identifier": RELEASE_IDENTIFIER,
        "app": { "windows": [{ "title": title }] }
    }))
    .unwrap()
}

#[test]
fn applying_the_identity_marks_a_development_build_and_leaves_a_release_alone() {
    let mut release = config("Test Case Manager");
    apply(&mut release, false);
    assert_eq!(release.identifier, RELEASE_IDENTIFIER);
    assert_eq!(release.app.windows[0].title, "Test Case Manager");

    let mut development = config("Test Case Manager");
    apply(&mut development, true);
    assert_eq!(development.identifier, identifier(true));
    assert_eq!(development.app.windows[0].title, "Test Case Manager (dev)");
    // Applying twice does not stack the marker.
    apply(&mut development, true);
    assert_eq!(development.app.windows[0].title, "Test Case Manager (dev)");
}

/// The wiring: the config is changed before the app is built from it, and
/// the stores that name their own entries go through this module.
#[test]
fn the_app_is_built_from_the_config_the_identity_was_applied_to() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    let apply_at = lib.find("dev_identity::apply(context.config_mut()").expect("the identity is applied");
    let build_at = lib.find(".build(context)").expect("the app is built from that context");
    assert!(apply_at < build_at);
    assert!(!lib.contains("generate_context!())"), "no build bypasses the applied context");

    let creds = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/db/credentials.rs")).unwrap();
    for call in ["wincred::read(&os_target(", "wincred::write(&os_target(", "wincred::delete(&os_target("] {
        assert!(creds.contains(call), "{call}");
    }
}
