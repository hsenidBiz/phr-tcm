//! Crate root: module tree, the specta builder (typed IPC + events), and
//! the Tauri app wiring. All command handlers live under `commands/`,
//! grouped by domain; domain logic lives in the modules they call into.

pub mod ado;
pub mod ado_git;
pub mod ado_share;
pub mod ado_testplan;
pub mod applog;
pub mod bugreport;
pub mod ai_bridge;
pub mod ai_tools;
pub mod assigned_watch;
pub mod audio;
pub mod auth;
pub mod capture;
pub mod commands;
pub mod events;
pub mod filewatch;
pub mod import_parser;
pub mod intake;
pub mod mcp;
pub mod model;
pub mod note_server;
pub mod optimize;
pub mod pipelines;
pub mod refcache;
pub mod report;
pub mod state;
pub mod steps_xml;
pub mod transform;
pub mod updater;
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
    use commands::{ai_bridge, ai_tools, auth, board, bugs, cases, discovery, misc, prs, queue, runs, testplan};
    Builder::<tauri::Wry>::new()
        .events(collect_events![
            events::SubmitProgress,
            events::SuiteScanProgress,
            audio::AudioSpectrum,
            events::CaseNoteSaved,
            events::PlanCreated,
            events::WatchedFileChanged,
            events::DraftCommentSaved,
            events::DraftGeneralCommentSaved,
            events::WorkAssigned
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
            runs::submit_test_run,
            board::fetch_board,
            board::move_board_item,
            board::work_item_history,
            misc::audio_capture_start,
            misc::audio_capture_stop,
            misc::check_update,
            misc::set_ado_rate_level,
            misc::app_logs,
            misc::app_log_dir,
            misc::apply_update,
            misc::watch_assigned_work,
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
            board::avatar_b64,
            board::create_work_item,
            discovery::classification_paths,
            queue::cancel_submit,
            queue::share_queue,
            queue::fetch_shared_queue,
            queue::export_queue_html,
            discovery::list_project_tags,
            runs::result_screenshots,
            queue::view_queue_html,
            queue::view_draft_html,
            queue::read_general_comment,
            queue::save_general_comment,
            queue::save_draft_comment,
            cases::test_cases_by_ids,
            cases::test_case_field_values,
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
            prs::pr_pipeline,
            prs::build_log,
            prs::pr_deployments,
            discovery::list_iterations,
            ai_bridge::bridge_status,
            ai_bridge::set_bridge_context,
            ai_tools::detect_ai_tools,
            ai_tools::register_ai_tool,
            ai_tools::unregister_ai_tool,
            ai_tools::register_db_server,
            ai_tools::unregister_db_server
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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

            // App log: file per day next to the OS's other app logs, plus
            // an in-memory tail Settings can show for bug reports.
            use tauri::Manager;
            if let Ok(dir) = app.path().app_log_dir() {
                applog::init(dir);
            }
            // Reference data (project tags) cached on disk and shared by the
            // UI and the AI bridge - see refcache.rs.
            if let Ok(dir) = app.path().app_data_dir() {
                refcache::init(dir);
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
