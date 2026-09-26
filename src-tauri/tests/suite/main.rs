//! Every Rust test in this crate except `tests/bindings.rs`, as ONE test
//! binary.
//!
//! Each file here was once its own integration target - its own binary -
//! and on Windows every one of those links the whole crate plus Tauri, with
//! link.exe generating a PDB each time. Eighty-odd links were most of the
//! wall-clock of a `cargo test`, so the files are modules of this one
//! target instead. A new test file goes in this folder and gets a `mod`
//! line below; it is still never a `#[cfg(test)]` module in src/ (see the
//! note at the top of tests/bindings.rs, and build.rs).
//!
//! One binary is also one PROCESS: statics, environment variables, the
//! working directory and the in-memory log are shared by every module now.
//! A test that sets or reads one of those takes its lock in `serial`, the
//! same lock whichever module it lives in.
//!
//! Run one module's tests with a path filter:
//!   cargo test --test suite throttle_backoff::

mod common;
mod serial;

mod ado;
mod ado_boards;
mod ado_git;
mod ado_network;
mod ado_share;
mod ado_testplan;
mod ai_bridge;
mod ai_tools;
mod applog;
mod assigned_watch;
mod audio;
mod auth;
mod autorun_accounts;
mod autorun_bridge;
mod autorun_commands;
mod autorun_edits;
mod autorun_failures;
mod autorun_floor;
mod autorun_guide;
mod autorun_nav;
mod autorun_publish;
mod autorun_quirks;
mod autorun_recipe;
mod autorun_recorder;
mod autorun_replay;
mod autorun_runner;
mod autorun_signin;
mod autorun_store;
mod backup;
mod branchcheck;
mod browser_actions;
mod browser_cdp;
mod browser_expect;
mod browser_input;
mod browser_launch;
mod browser_live;
mod browser_locator;
mod browser_page;
mod browser_session;
mod browser_snapshot;
mod bugreport;
mod cache;
mod comment_images;
mod db_credentials;
mod db_credentials_commands;
mod db_guard;
mod db_schema;
mod db_sqlcmd;
mod deletion;
mod draft_comments;
mod draft_merge;
mod extras;
mod filewatch;
mod help;
mod import_parser;
mod intake;
mod markdown;
mod mcp;
mod mentions;
mod note_server;
mod optimize;
mod permissions;
mod pipelines;
mod relink;
mod report;
mod run_order;
mod spec_pane;
mod speccov;
mod speccov_bridge;
mod specs_field;
mod startup_window;
mod steps_xml;
mod submit_mapping;
mod suite_manage;
mod tcm_mcp;
mod test_map;
mod throttle_backoff;
mod throttle_default;
mod transform;
mod updater;
mod upload_timeout;
mod webtheme;
mod wit_batch;
mod work_board;
mod work_history;
mod workspace;
