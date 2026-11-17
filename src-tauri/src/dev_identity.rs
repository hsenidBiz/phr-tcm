//! A development build beside the installed app.
//!
//! `npm run tauri dev` can run while the installed app is open, without
//! either one touching the other's settings, cache, Auto Run data, accounts,
//! saved sign-in or database logins. A development build (`ai_tools::dev_build`)
//! has its own identity; a release build's is unchanged, byte for byte.
//!
//! Everything that is the app's own - the roaming data folder, the local data
//! folder (guide, npm logs, upload journal), the log folder, the WebView2
//! profile that holds the page's localStorage, and the single-instance lock -
//! is named by Tauri from the bundle identifier. So one change carries them
//! all: `apply` gives the development build the identifier with `.dev`
//! appended, before the app is built. What Tauri does not name from the
//! identifier is listed here, each with its own development value:
//!
//! - Windows Credential Manager entries (the kept sign-in, the database
//!   logins): `credential_target`.
//! - The file that tells `--mcp` proxies where the running app listens:
//!   `handshake_file_name`.
//! - The Start with Windows registry value: `autostart_app_name`.
//!
//! The window carries a marker in its title (`window_title`).

/// The identifier in `tauri.conf.json`: what a release build runs under.
pub const RELEASE_IDENTIFIER: &str = "com.avinalwis.testcasemanager.v2";

/// Appended to the identifier in a development build.
pub const DEV_IDENTIFIER_SUFFIX: &str = ".dev";

/// The marker a development build's window title ends with.
pub const DEV_TITLE_MARKER: &str = " (dev)";

/// Put in front of a Credential Manager target name in a development build.
/// It starts with `tcm-v2-dev`, not `tcm-v2/`, so no listing of the
/// installed app's entries (`tcm-v2/db/`) can ever see a development one.
pub const DEV_CREDENTIAL_PREFIX: &str = "tcm-v2-dev/";

/// The bundle identifier a build runs under.
pub fn identifier(dev: bool) -> String {
    if dev {
        format!("{RELEASE_IDENTIFIER}{DEV_IDENTIFIER_SUFFIX}")
    } else {
        RELEASE_IDENTIFIER.to_string()
    }
}

/// The single-instance key the plugin derives from the identifier
/// (`tauri-plugin-single-instance`: the mutex is `<identifier>-sim`). Two
/// builds with different identifiers can run at the same time.
pub fn single_instance_key(identifier: &str) -> String {
    format!("{identifier}-sim")
}

/// A window title with the development marker, or as it is in a release.
pub fn window_title(title: &str, dev: bool) -> String {
    if dev && !title.ends_with(DEV_TITLE_MARKER) {
        format!("{title}{DEV_TITLE_MARKER}")
    } else {
        title.to_string()
    }
}

/// The Credential Manager target a store name is kept under. A release
/// build keeps it as it is.
pub fn credential_target(name: &str, dev: bool) -> String {
    if dev {
        format!("{DEV_CREDENTIAL_PREFIX}{name}")
    } else {
        name.to_string()
    }
}

/// The store name behind a target found by listing, `None` when the target
/// is not one this build keeps (a development build never lists the
/// installed app's entries, nor the other way round).
pub fn store_name(target: &str, dev: bool) -> Option<String> {
    if dev {
        target.strip_prefix(DEV_CREDENTIAL_PREFIX).map(str::to_string)
    } else if target.starts_with(DEV_CREDENTIAL_PREFIX) {
        None
    } else {
        Some(target.to_string())
    }
}

/// Said when a development build is asked to change the AI tools' settings
/// (register, remove, auto-approve, commands, agents, allow files). Those
/// live in the person's home folder, under names shared with the installed
/// app (`tcm`, `~/.claude/commands/tcm`, `mcp__tcm__*`), so a development
/// build leaves them alone instead of replacing the installed app's.
pub const AI_TOOLS_REFUSED: &str =
    "This is a development build, which does not change the AI tools' settings - they are shared with the installed app. Do this in the installed app.";

/// `Some(AI_TOOLS_REFUSED)` for a development build, `None` for a release.
pub fn ai_tools_refusal(dev: bool) -> Option<&'static str> {
    dev.then_some(AI_TOOLS_REFUSED)
}

/// The folder a development build is told is its home for the AI tools'
/// settings: one that is never created, so no tool is found under it and
/// nothing is written to the real one.
pub fn no_ai_tools_home() -> String {
    std::env::temp_dir().join("tcm-dev-no-ai-tools").to_string_lossy().to_string()
}

/// The name of the file in the temp folder where the running app tells
/// `--mcp` proxies its port and token. A development build's own, so it
/// never replaces the installed app's.
pub fn handshake_file_name(dev: bool) -> &'static str {
    if dev {
        "tcm-v2-dev-mcp-bridge.json"
    } else {
        "tcm-v2-mcp-bridge.json"
    }
}

/// The Start with Windows registry value name. A development build's own,
/// so turning the switch on or off there never replaces or removes the
/// installed app's entry.
pub fn autostart_app_name(base: &str, dev: bool) -> String {
    if dev {
        format!("{base} (dev)")
    } else {
        base.to_string()
    }
}

/// Give a development build its identity: the identifier (and with it every
/// folder, the WebView2 profile and the single-instance key) and the marker
/// on every window's title. A release build's config is left exactly as it
/// is. Called once, on the config the app is built from.
pub fn apply(config: &mut tauri::Config, dev: bool) {
    if !dev {
        return;
    }
    config.identifier = identifier(true);
    for window in &mut config.app.windows {
        window.title = window_title(&window.title, true);
    }
}
