// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The MCP stdio proxy is spawned per AI session - it must not run
    // installer hooks or touch the GUI, so this check comes before anything
    // else in main().
    if std::env::args().any(|a| a == "--mcp") {
        v2_lib::mcp::run_stdio_proxy();
        return;
    }
    // Velopack install/update hooks must run before anything else touches
    // the process (first-run, obsolete-version cleanup, restart-after-update).
    // Uninstalling also removes the Start with Windows entry, which would
    // otherwise point at an exe that no longer exists.
    #[cfg(windows)]
    velopack::VelopackApp::build()
        .on_before_uninstall_fast_callback(|_version| v2_lib::tray::remove_autostart_entry())
        .run();
    #[cfg(not(windows))]
    velopack::VelopackApp::build().run();
    v2_lib::run()
}
