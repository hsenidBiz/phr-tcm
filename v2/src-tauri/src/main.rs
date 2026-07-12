// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Velopack install/update hooks must run before anything else touches
    // the process (first-run, obsolete-version cleanup, restart-after-update).
    velopack::VelopackApp::build().run();
    v2_lib::run()
}
