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
    velopack::VelopackApp::build().run();
    v2_lib::run()
}
