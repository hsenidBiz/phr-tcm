//! Crate root: module tree, the specta builder (typed IPC + events), and
//! the Tauri app wiring. All command handlers live under `commands/`,
//! grouped by domain; domain logic lives in the modules they call into.

pub mod ado;
pub mod ado_git;
pub mod ado_share;
pub mod ado_testplan;
pub mod applog;
pub mod autorun;
pub mod branchcheck;
pub mod browser;
pub mod bugreport;
pub mod ai_bridge;
pub mod ai_tools;
pub mod assigned_watch;
pub mod db_defaults;
pub mod audio;
pub mod auth;
pub mod backup;
pub mod capture;
pub mod commands;
pub mod events;
pub mod filewatch;
pub mod import_parser;
pub mod intake;
pub mod markdown;
pub mod mcp;
pub mod model;
pub mod note_server;
pub mod optimize;
pub mod pipelines;
pub mod refcache;
pub mod report;
pub mod speccov;
pub mod state;
pub mod steps_xml;
pub mod transform;
pub mod updater;
pub mod webtheme;
pub mod workspace;
pub mod work_board;

use std::sync::Mutex;
use tauri_specta::{collect_commands, collect_events, Builder};

// Pre-reorg public paths, kept resolving for tests and any external callers.
pub use commands::auth::AuthStatus;
pub use commands::bugs::{work_bug, RunAttachmentOut};
pub use commands::queue::{ImportResult, SubmitItemResult};
pub use commands::runs::{PointOutcome, ResultFailureDetail, RunAttachment};
pub use events::{CaseNoteSaved, SubmitProgress, SuiteScanProgress};
pub use state::SubmitCancel;

pub fn specta_builder() -> Builder<tauri::Wry> {
    use commands::{
        ai_bridge, ai_tools, auth, autorun, board, bugs, cases, discovery, misc, prs, queue, runs,
        testplan, workspace,
    };
    Builder::<tauri::Wry>::new()
        .events(collect_events![
            events::SubmitProgress,
            events::SuiteScanProgress,
            audio::AudioSpectrum,
            events::CaseNoteSaved,
            events::PlanCreated,
            events::SuiteNotCreated,
            events::WatchedFileChanged,
            events::DraftCommentSaved,
            events::DraftGeneralCommentSaved,
            events::WorkAssigned,
            events::IntakeOutputPath,
            events::UpdateProgress,
            events::SlowdownRequested
        ])
        .commands(collect_commands![
            misc::ping,
            misc::prepare_bug_report,
            auth::auth_status,
            auth::sign_in,
            discovery::list_projects,
            discovery::list_orgs,
            discovery::search_pbis,
            cases::pbi_test_cases,
            queue::parse_import_file,
            queue::file_stamp,
            queue::watch_file,
            queue::unwatch_file,
            queue::unwatch_all_files,
            queue::submit_queue,
            testplan::ensure_pbi_suite,
            runs::list_test_points,
            runs::result_failure_detail,
            runs::start_test_run,
            runs::record_result,
            runs::reset_test_points,
            runs::finish_test_run,
            board::fetch_board,
            board::move_board_item,
            board::work_item_history,
            misc::audio_capture_start,
            misc::audio_capture_stop,
            misc::check_update,
            misc::set_ado_rate_level,
            misc::app_logs,
            misc::log_ui,
            misc::app_log_dir,
            misc::apply_update,
            misc::watch_assigned_work,
            misc::export_app_backup,
            misc::import_app_backup,
            cases::list_test_case_fields,
            cases::pbi_test_cases_full,
            cases::update_test_case,
            queue::export_queue_json,
            testplan::list_plans_with_suites,
            runs::get_result_detail,
            bugs::capture_screens,
            bugs::file_bug,
            board::list_teams,
            board::list_team_members,
            board::work_item_detail,
            board::update_work_item,
            board::activity_values,
            board::work_item_comments,
            board::add_comment,
            board::update_comment,
            board::connected_user,
            board::avatar_b64,
            board::create_work_item,
            discovery::classification_paths,
            queue::cancel_submit,
            queue::share_queue,
            queue::fetch_shared_queue,
            queue::materialize_shared_draft,
            ai_tools::db_server_defaults,
            ai_tools::db_server_presets,
            autorun::auto_run_open_browser,
            autorun::auto_run_close_browser,
            autorun::auto_run_step,
            autorun::auto_run_load_script,
            autorun::auto_run_save_script,
            autorun::auto_run_import_scripts,
            autorun::auto_run_save_run,
            autorun::auto_run_list_runs,
            autorun::auto_run_new_id,
            queue::export_queue_html,
            discovery::list_project_tags,
            runs::result_screenshots,
            queue::view_queue_html,
            queue::view_draft_html,
            queue::read_general_comment,
            queue::save_general_comment,
            queue::save_draft_comment,
            queue::save_draft_cases,
            queue::refresh_draft_html,
            queue::refresh_queue_html,
            cases::test_cases_by_ids,
            cases::test_case_field_values,
            cases::can_delete_test_cases,
            cases::delete_test_cases,
            cases::relink_test_cases,
            testplan::find_pbi_suite,
            runs::run_history,
            runs::view_execution_report,
            bugs::read_file_b64,
            bugs::open_snip,
            prs::list_repos,
            prs::pr_overview,
            prs::repo_pull_requests,
            prs::board_pr_links,
            prs::pr_work_items,
            prs::pr_description,
            prs::pr_build_states,
            prs::pr_threads,
            prs::set_pr_thread_status,
            prs::pr_pipeline,
            prs::build_log,
            prs::pr_deployments,
            discovery::list_iterations,
            ai_bridge::bridge_status,
            ai_bridge::set_bridge_context,
            ai_tools::detect_ai_tools,
            ai_tools::register_ai_tool,
            ai_tools::unregister_ai_tool,
            ai_tools::retire_global_registrations,
            ai_tools::register_db_server,
            ai_tools::unregister_db_server,
            workspace::ensure_cases_dir,
            workspace::copy_into_cases
        ])
}

/// Bring the running app's window forward.
///
/// Order matters: a minimized window cannot take focus, and this window is
/// created hidden (`visible: false` in tauri.conf.json) and shown by the
/// frontend, so it has to be restored and shown before `set_focus` has
/// anything to focus. Each step is best-effort - a window the user closed
/// out from under us is not worth failing over.
#[cfg(desktop)]
fn focus_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Step out of the install's `current\` directory before anything can
/// inherit it.
///
/// Velopack starts the app with cwd = `current\`. Every child started
/// without an explicit cwd - the browser behind "View in Browser" (the
/// opener plugin calls ShellExecute with no directory), Auto Run's Edge,
/// the `claude mcp add` shell, WebView2's own helpers - inherits that cwd,
/// and a process's cwd pins the directory against rename. Renaming
/// `current\` is the first thing Update.exe does when applying an update,
/// and it can only kill processes whose exe lives under the install root -
/// a browser's does not. On 2026-08-21 a 1.20.2 -> 1.20.5 update failed
/// three times over with "os error 32" because Edge, first opened from the
/// app that morning, was still sitting in `current\` hours later.
///
/// The temp dir is the one place guaranteed to exist, be writable, and
/// never be renamed by an installer. Best-effort: an app that cannot chdir
/// still runs - it just updates the way it did before.
pub fn leave_install_dir() {
    let _ = std::env::set_current_dir(std::env::temp_dir());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    leave_install_dir();
    let builder = specta_builder();
    #[allow(unused_mut)]
    let mut tauri_builder = tauri::Builder::default();

    // A second launch of the exe hands its arguments to the process that is
    // already running and then exits, so double-clicking the shortcut again
    // brings the app forward instead of starting a rival copy - which would
    // otherwise carry its own draft queue, its own file watches and its own
    // AI bridge port, and whichever copy the user typed into last would win.
    //
    // Registered before every other plugin: the second process is turned
    // away inside this plugin's setup, and anything registered ahead of it
    // would run in a process that is about to die.
    //
    // `--mcp` never reaches here (main() returns first), so an AI session
    // spawning the stdio proxy is not mistaken for a second window.
    #[cfg(desktop)]
    {
        tauri_builder = tauri_builder.plugin(tauri_plugin_single_instance::init(
            |app, _argv, _cwd| {
                applog::info("second launch refused - focusing the running window");
                focus_main_window(app);
            },
        ));
    }

    tauri_builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .manage(Mutex::new(auth::AuthState::default()))
        .manage(updater::UpdateState::default())
        .manage(SubmitCancel::default())
        .manage(commands::ai_bridge::BridgeHandle::default())
        .manage(filewatch::FileWatchState::default())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            // Registers the typed-event registry in Tauri state; without
            // this every specta Event::emit panics with "EventRegistry not
            // found in Tauri state".
            builder.mount_events(app);

            // The ADO pacer lives in a static module with no per-call
            // AppHandle - `note_server_delay` is reached from deep inside
            // the transport layer, not a command. Same problem as
            // `autorun::store::set_root` below, same fix: stash the handle
            // once, here, where `app` is in scope.
            ado::throttle::set_app_handle(app.handle().clone());

            // App log: file per day next to the OS's other app logs, plus
            // an in-memory tail Settings can show for bug reports.
            use tauri::Manager;
            if let Ok(dir) = app.path().app_log_dir() {
                applog::init(dir);
            }
            // Reference data (project tags) cached on disk and shared by the
            // UI and the AI bridge - see refcache.rs.
            if let Ok(dir) = app.path().app_data_dir() {
                refcache::init(dir.clone());
                // Auto Run scripts and local runs. The commands reach this
                // through their AppHandle; the AI bridge has no handle and
                // reads it from here, so a script an assistant saves lands
                // where the Auto Run screen actually looks.
                autorun::store::set_root(dir.join("autorun"));
            }
            applog::info(format!(
                "Test Case Manager {} started",
                app.package_info().version
            ));
            // A panic would otherwise vanish in a windowed release build.
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                applog::error(format!("panic: {info}"));
                previous(info);
            }));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
